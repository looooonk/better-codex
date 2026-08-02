use super::Tui;
use super::disable_raw_mode;
use super::flush_terminal_input_buffer;
use super::restore_for_interactive_child;
use super::set_modes;
use super::terminal_stderr;
use std::future::Future;
use std::io::Result;

struct RestoredTerminalGuard<'a> {
    tui: &'a mut Tui,
    was_alt_screen: bool,
    active: bool,
}

impl<'a> RestoredTerminalGuard<'a> {
    fn acquire(tui: &'a mut Tui) -> Result<Self> {
        tui.event_broker.pause_events();
        let was_alt_screen = tui.is_alt_screen_active();
        let guard = Self {
            tui,
            was_alt_screen,
            active: true,
        };
        if was_alt_screen {
            guard.tui.leave_alt_screen()?;
        }
        restore_for_interactive_child()?;
        terminal_stderr::pause()?;
        Ok(guard)
    }

    fn restore(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let mut first_error = terminal_stderr::resume().err();
        if let Err(err) = disable_raw_mode() {
            first_error.get_or_insert(err);
        }
        if let Err(err) = set_modes() {
            first_error.get_or_insert(err);
        }
        flush_terminal_input_buffer();
        if self.was_alt_screen
            && let Err(err) = self.tui.enter_alt_screen()
        {
            first_error.get_or_insert(err);
        }
        self.tui.event_broker.resume_events();
        self.active = false;
        match first_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

impl Drop for RestoredTerminalGuard<'_> {
    fn drop(&mut self) {
        if let Err(err) = self.restore() {
            tracing::warn!("failed to restore terminal after interactive child: {err}");
        }
    }
}

impl Tui {
    /// Temporarily release the terminal to an interactive child process.
    ///
    /// Crossterm's event reader must be dropped before the child starts so it cannot consume the
    /// child's input. The Better Codex alternate screen and terminal modes are restored after the
    /// child exits, then input polling resumes with a fresh event reader.
    pub(crate) async fn with_restored_terminal<R, F, Fut>(&mut self, run: F) -> Result<R>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = R>,
    {
        let mut guard = RestoredTerminalGuard::acquire(self)?;
        let output = run().await;
        guard.restore()?;
        Ok(output)
    }
}

#[cfg(unix)]
#[doc(hidden)]
#[expect(
    clippy::print_stderr,
    reason = "test helper reports terminal handoff failures after restoring the PTY"
)]
pub fn run_handoff_helper_for_tests() -> ! {
    use crate::custom_terminal::Terminal as CustomTerminal;
    use crate::tui::TuiEvent;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEventKind;
    use ratatui::backend::CrosstermBackend;
    use ratatui::layout::Position;
    use std::io::Write;
    use std::io::stdout;
    use std::process::Stdio;
    use std::task::Poll;
    use std::time::Duration;
    use tokio::process::Command;
    use tokio_stream::StreamExt;

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to build interactive child handoff runtime: {err}");
            std::process::exit(120);
        }
    };
    let result: Result<()> = runtime.block_on(async {
        let cancellation_mode = std::env::var("CODEX_TUI_INTERACTIVE_CHILD_HANDOFF_HELPER")
            .is_ok_and(|mode| mode == "cancel");
        set_modes()?;
        let backend = CrosstermBackend::new(stdout());
        let terminal = CustomTerminal::with_options_and_cursor_position(
            backend,
            Position { x: 0, y: 0 },
        )?;
        let stderr_guard = terminal_stderr::TerminalStderrGuard::install()?;
        let mut tui = Tui::new(
            terminal,
            /*enhanced_keys_supported*/ false,
            stderr_guard,
        );
        tui.enter_alt_screen()?;
        let mut events = tui.event_stream();
        std::future::poll_fn(|cx| loop {
            match events.as_mut().poll_next(cx) {
                Poll::Ready(Some(_)) => {}
                Poll::Ready(None) | Poll::Pending => return Poll::Ready(()),
            }
        })
        .await;

        let mut output = stdout();
        let resumed_key = if cancellation_mode {
            let cancellation = tokio::time::timeout(
                Duration::from_millis(/*millis*/ 100),
                tui.with_restored_terminal(|| async {
                    let mut output = stdout();
                    writeln!(output, "CANCEL_CHILD_READY")?;
                    output.flush()?;
                    std::future::pending::<Result<()>>().await
                }),
            )
            .await;
            if cancellation.is_ok() {
                return Err(std::io::Error::other(
                    "interactive child cancellation helper unexpectedly completed",
                ));
            }
            writeln!(output, "TUI_CANCEL_RESUMED")?;
            'c'
        } else {
            let status = tui
                .with_restored_terminal(|| {
                    Command::new("/bin/sh")
                        .arg("-c")
                        .arg(
                            r#"printf 'CHILD_READY\n'; IFS= read -r line; printf 'CHILD_GOT:%s\n' "$line""#,
                        )
                        .stdin(Stdio::inherit())
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .kill_on_drop(true)
                        .status()
                })
                .await??;
            if !status.success() {
                return Err(std::io::Error::other(format!(
                    "interactive child exited with {status}"
                )));
            }
            writeln!(output, "TUI_RESUMED")?;
            'r'
        };
        output.flush()?;
        loop {
            match events.next().await {
                Some(TuiEvent::Key(key))
                    if key.kind == KeyEventKind::Press
                        && key.code == KeyCode::Char(resumed_key) =>
                {
                    writeln!(output, "TUI_GOT:{resumed_key}")?;
                    output.flush()?;
                    break;
                }
                Some(_) => {}
                None => return Err(std::io::Error::other("terminal event stream ended")),
            }
        }

        drop(events);
        tui.leave_alt_screen()?;
        drop(tui);
        super::restore_after_exit()
    });

    if let Err(err) = result {
        let _ = super::restore_after_exit();
        eprintln!("interactive child handoff helper failed: {err}");
        std::process::exit(120);
    }
    std::process::exit(0)
}
