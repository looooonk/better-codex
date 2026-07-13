use std::fmt;
use std::io::IsTerminal;
use std::io::Result;
use std::io::Stdout;
use std::io::Write;
use std::io::stdin;
use std::io::stdout;
use std::panic;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crossterm::Command;
use crossterm::SynchronizedUpdate;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::DisableBracketedPaste;
use crossterm::event::DisableFocusChange;
use crossterm::event::DisableMouseCapture;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::EnableFocusChange;
use crossterm::event::EnableMouseCapture;
use crossterm::event::KeyEvent;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
#[cfg(not(unix))]
use crossterm::terminal::supports_keyboard_enhancement;
use ratatui::backend::Backend;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::disable_raw_mode;
use ratatui::crossterm::terminal::enable_raw_mode;
use ratatui::layout::Offset;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use tokio::sync::broadcast;
use tokio_stream::Stream;

pub use self::frame_requester::FrameRequester;
use crate::custom_terminal;
use crate::custom_terminal::Terminal as CustomTerminal;
use crate::tui::event_stream::EventBroker;
use crate::tui::event_stream::TuiEventStream;
#[cfg(unix)]
use crate::tui::job_control::SuspendContext;

mod event_stream;
mod frame_rate_limiter;
mod frame_requester;
#[cfg(unix)]
mod job_control;
mod keyboard_modes;
mod terminal_stderr;
#[cfg(test)]
pub(crate) mod test_support;

/// Target frame interval for UI redraw scheduling.
pub(crate) const TARGET_FRAME_INTERVAL: Duration = frame_rate_limiter::MIN_FRAME_INTERVAL;

/// A type alias for the terminal type used in this application
pub type Terminal = CustomTerminal<CrosstermBackend<Stdout>>;

pub(crate) struct InitializedTerminal {
    pub(crate) terminal: Terminal,
    pub(crate) enhanced_keys_supported: bool,
    pub(crate) stderr_guard: terminal_stderr::TerminalStderrGuard,
}

pub(crate) fn running_in_vscode_terminal() -> bool {
    keyboard_modes::running_in_vscode_terminal()
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.leave_alt_screen();
    }
}

#[cfg(test)]
mod tests {
    use crossterm::Command;
    use std::io::Write as _;

    use super::DisableAlternateScroll;
    use super::clear_for_viewport_change;
    use super::write_exit_alt_screen_restore;
    use crate::custom_terminal::Terminal as CustomTerminal;
    use crate::test_backend::VT100Backend;
    use crossterm::terminal::LeaveAlternateScreen;
    use ratatui::layout::Position;
    use ratatui::layout::Rect;

    #[test]
    fn first_viewport_change_clears_from_new_viewport_when_old_viewport_is_empty() {
        let width = 12;
        let height = 4;
        let backend = VT100Backend::new(width, height);
        let mut terminal =
            CustomTerminal::with_options_and_cursor_position(backend, Position { x: 0, y: 1 })
                .expect("terminal");
        write!(
            terminal.backend_mut(),
            "shell line\r\nstale cells\r\nmore stale"
        )
        .expect("prefill terminal");

        clear_for_viewport_change(
            &mut terminal,
            Rect::new(
                /*x*/ 0,
                /*y*/ 1,
                /*width*/ width,
                /*height*/ height - 1,
            ),
        )
        .expect("clear transition");

        let rows: Vec<String> = terminal
            .backend()
            .vt100()
            .screen()
            .rows(/*start*/ 0, width)
            .collect();
        assert!(
            rows[0].contains("shell line"),
            "expected content before the viewport to remain visible, rows: {rows:?}"
        );
        assert!(
            !rows.iter().skip(1).any(|row| row.contains("stale")),
            "expected stale cells inside the new viewport to be cleared, rows: {rows:?}"
        );
    }

    #[test]
    fn exit_restore_leaves_alternate_screen_after_disabling_alt_scroll() {
        let mut output = Vec::new();
        write_exit_alt_screen_restore(&mut output).expect("write exit alt-screen restore");

        let mut expected = String::new();
        DisableAlternateScroll
            .write_ansi(&mut expected)
            .expect("disable alternate scroll ansi");
        LeaveAlternateScreen
            .write_ansi(&mut expected)
            .expect("leave alternate screen ansi");

        assert_eq!(
            String::from_utf8(output).expect("utf8 restore sequence"),
            expected
        );
    }

    #[cfg(windows)]
    #[test]
    fn alternate_scroll_commands_force_ansi_on_windows() {
        use super::EnableAlternateScroll;

        assert!(EnableAlternateScroll.is_ansi_code_supported());
        assert!(DisableAlternateScroll.is_ansi_code_supported());
    }
}

pub fn set_modes() -> Result<()> {
    ensure_virtual_terminal_processing()?;

    let mut cleanup_guard = TerminalModeCleanupGuard::new();
    execute!(stdout(), EnableBracketedPaste)?;

    enable_raw_mode()?;
    // Enable keyboard enhancement flags so modifiers for keys like Enter are disambiguated.
    // chat_composer.rs is using a keyboard event listener to enter for any modified keys
    // to create a new line that require this.
    // Some terminals (notably legacy Windows consoles) do not support
    // keyboard enhancement flags. Attempt to enable them, but continue
    // gracefully if unsupported.
    keyboard_modes::enable_keyboard_enhancement();

    let _ = execute!(stdout(), EnableFocusChange);
    cleanup_guard.disarm();
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableAlternateScroll;

impl Command for EnableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<()> {
        Err(std::io::Error::other(
            "tried to execute EnableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableAlternateScroll;

impl Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<()> {
        Err(std::io::Error::other(
            "tried to execute DisableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyboardRestore {
    PopStack,
    ResetAfterExit,
}

struct TerminalModeCleanupGuard {
    active: bool,
}

impl TerminalModeCleanupGuard {
    fn new() -> Self {
        Self { active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for TerminalModeCleanupGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = restore_terminal_modes_after_exit();
        }
    }
}

fn restore_common(keyboard_restore: KeyboardRestore) -> Result<()> {
    let mut first_error = ensure_virtual_terminal_processing().err();

    match keyboard_restore {
        KeyboardRestore::PopStack => keyboard_modes::restore_keyboard_enhancement_stack(),
        KeyboardRestore::ResetAfterExit => keyboard_modes::reset_keyboard_reporting_after_exit(),
    }

    if let Err(err) = execute!(stdout(), DisableBracketedPaste) {
        first_error.get_or_insert(err);
    }
    let _ = execute!(stdout(), DisableFocusChange);
    let _ = execute!(stdout(), DisableMouseCapture);
    if let Err(err) = disable_raw_mode() {
        first_error.get_or_insert(err);
    }
    if let Err(err) = execute!(
        stdout(),
        SetCursorStyle::DefaultUserShape,
        crossterm::cursor::Show
    ) {
        first_error.get_or_insert(err);
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn restore_terminal_modes_after_exit() -> Result<()> {
    let mut first_error = restore_common(KeyboardRestore::ResetAfterExit).err();
    if let Err(err) = write_exit_alt_screen_restore(&mut stdout()) {
        first_error.get_or_insert(err);
    }
    keyboard_modes::reset_keyboard_reporting_after_exit();

    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn write_exit_alt_screen_restore(writer: &mut impl Write) -> Result<()> {
    execute!(writer, DisableAlternateScroll, LeaveAlternateScreen)
}

/// Restore the terminal to its original state.
/// Inverse of `set_modes`.
pub fn restore() -> Result<()> {
    restore_common(KeyboardRestore::PopStack)
}

/// Force crossterm's cached raw-mode state back in sync with the terminal after `fg`.
///
/// A shell may restore the job's saved termios after the process receives `SIGCONT`. When that
/// races with [`set_modes`], crossterm still believes raw mode is enabled even though the terminal
/// has returned to canonical, echoing mode. Clearing crossterm's saved state before enabling raw
/// mode again makes the kernel state authoritative once the shell has completed its handoff.
#[cfg(unix)]
pub(super) fn reapply_raw_mode_after_resume() -> Result<()> {
    disable_raw_mode()?;
    enable_raw_mode()
}

/// Restore the terminal after Codex is exiting.
///
/// Uses a stronger keyboard reset than [`restore`] so the parent shell recovers even if a
/// terminal missed the stack pop that normally pairs with [`set_modes`].
pub fn restore_after_exit() -> Result<()> {
    let mut first_error = restore_terminal_modes_after_exit().err();
    if let Err(err) = terminal_stderr::finish() {
        first_error.get_or_insert(err);
    }

    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[cfg(unix)]
#[doc(hidden)]
pub fn run_terminal_restore_panic_helper_for_tests() -> ! {
    set_panic_hook();
    enter_terminal_restore_helper_modes("panic helper");

    panic!("intentional panic for terminal restore regression");
}

#[cfg(unix)]
#[doc(hidden)]
#[expect(
    clippy::print_stderr,
    reason = "test helper mirrors the binary's fatal-exit message after terminal restore"
)]
pub fn run_terminal_restore_fatal_disconnect_helper_for_tests() -> ! {
    enter_terminal_restore_helper_modes("fatal-disconnect helper");
    if let Err(err) = restore_after_exit() {
        eprintln!("failed to restore terminal after fatal disconnect: {err}");
        std::process::exit(120);
    }
    eprintln!("ERROR: app-server disconnected");
    std::process::exit(1);
}

#[cfg(unix)]
fn enter_terminal_restore_helper_modes(label: &str) {
    if let Err(err) = set_modes() {
        panic!("set terminal modes for {label}: {err}");
    }
    if let Err(err) = execute!(
        stdout(),
        EnterAlternateScreen,
        EnableAlternateScroll,
        EnableMouseCapture,
        crossterm::cursor::Hide,
        SetCursorStyle::SteadyBar,
    ) {
        panic!("enter terminal modes for {label}: {err}");
    }
}

/// Flush the underlying stdin buffer to clear any input that may be buffered at the terminal level.
/// For example, clears any user input that occurred while the crossterm EventStream was dropped.
#[cfg(unix)]
fn flush_terminal_input_buffer() {
    // Safety: flushing the stdin queue is safe and does not move ownership.
    let result = unsafe { libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH) };
    if result != 0 {
        let err = std::io::Error::last_os_error();
        tracing::warn!("failed to tcflush stdin: {err}");
    }
}

/// Flush the underlying stdin buffer to clear any input that may be buffered at the terminal level.
/// For example, clears any user input that occurred while the crossterm EventStream was dropped.
#[cfg(windows)]
fn flush_terminal_input_buffer() {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::FlushConsoleInputBuffer;
    use windows_sys::Win32::System::Console::GetStdHandle;
    use windows_sys::Win32::System::Console::STD_INPUT_HANDLE;

    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if handle == INVALID_HANDLE_VALUE || handle == 0 {
        let err = unsafe { GetLastError() };
        tracing::warn!("failed to get stdin handle for flush: error {err}");
        return;
    }

    let result = unsafe { FlushConsoleInputBuffer(handle) };
    if result == 0 {
        let err = unsafe { GetLastError() };
        tracing::warn!("failed to flush stdin buffer: error {err}");
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn flush_terminal_input_buffer() {}

/// Initialize the terminal (inline viewport; history stays in normal scrollback)
pub(crate) fn init() -> Result<InitializedTerminal> {
    if !stdin().is_terminal() {
        return Err(std::io::Error::other("stdin is not a terminal"));
    }
    if !stdout().is_terminal() {
        return Err(std::io::Error::other("stdout is not a terminal"));
    }
    set_modes()?;
    let mut cleanup_guard = TerminalModeCleanupGuard::new();

    flush_terminal_input_buffer();

    set_panic_hook();

    #[cfg(unix)]
    let backend = CrosstermBackend::new(stdout());

    #[cfg(unix)]
    let startup_probe = {
        use crate::terminal_probe::StartupKeyboardEnhancementProbe;

        let started_at = std::time::Instant::now();
        let keyboard_probe = if keyboard_modes::keyboard_enhancement_disabled() {
            StartupKeyboardEnhancementProbe::Skip
        } else {
            StartupKeyboardEnhancementProbe::Query
        };
        match crate::terminal_probe::startup(crate::terminal_probe::DEFAULT_TIMEOUT, keyboard_probe)
        {
            Ok(probe) => {
                tracing::info!(
                    duration_ms = %started_at.elapsed().as_millis(),
                    cursor_position = probe.cursor_position.is_some(),
                    default_colors = probe.default_colors.is_some(),
                    keyboard_enhancement_supported = ?probe.keyboard_enhancement_supported,
                    "terminal startup probes completed"
                );
                probe
            }
            Err(err) => {
                tracing::warn!(
                    duration_ms = %started_at.elapsed().as_millis(),
                    "terminal startup probes failed: {err}"
                );
                crate::terminal_probe::StartupProbe {
                    cursor_position: None,
                    default_colors: None,
                    keyboard_enhancement_supported: None,
                }
            }
        }
    };

    #[cfg(unix)]
    crate::terminal_palette::set_default_colors_from_startup_probe(startup_probe.default_colors);

    #[cfg(unix)]
    let cursor_pos = match startup_probe.cursor_position {
        Some(pos) => pos,
        None => {
            tracing::warn!("initial cursor position probe timed out; defaulting to origin");
            Position { x: 0, y: 0 }
        }
    };

    #[cfg(unix)]
    let enhanced_keys_supported = startup_probe
        .keyboard_enhancement_supported
        .unwrap_or(/*default*/ false);

    #[cfg(not(unix))]
    let mut backend = CrosstermBackend::new(stdout());

    #[cfg(not(unix))]
    let cursor_pos = cursor_position_with_crossterm(&mut backend);

    #[cfg(not(unix))]
    let enhanced_keys_supported =
        !keyboard_modes::keyboard_enhancement_disabled() && detect_keyboard_enhancement_supported();

    #[cfg(windows)]
    probe_windows_default_colors();

    let tui = CustomTerminal::with_options_and_cursor_position(backend, cursor_pos)?;
    let stderr_guard = terminal_stderr::TerminalStderrGuard::install()?;
    cleanup_guard.disarm();
    Ok(InitializedTerminal {
        terminal: tui,
        enhanced_keys_supported,
        stderr_guard,
    })
}

#[cfg(not(unix))]
fn cursor_position_with_crossterm(backend: &mut CrosstermBackend<Stdout>) -> Position {
    backend.get_cursor_position().unwrap_or_else(|err| {
        tracing::warn!("failed to read initial cursor position; defaulting to origin: {err}");
        Position { x: 0, y: 0 }
    })
}

#[cfg(not(unix))]
fn detect_keyboard_enhancement_supported() -> bool {
    // Non-Unix startup keeps the existing crossterm keyboard probe path because it already knows
    // how to interpret platform-specific event sources.
    supports_keyboard_enhancement().unwrap_or(/*default*/ false)
}

#[cfg(windows)]
fn probe_windows_default_colors() {
    let started_at = std::time::Instant::now();
    match crate::terminal_probe::default_colors(crate::terminal_probe::DEFAULT_TIMEOUT) {
        Ok(colors) => {
            tracing::info!(
                duration_ms = %started_at.elapsed().as_millis(),
                default_colors = colors.is_some(),
                "terminal default color probe completed"
            );
            crate::terminal_palette::set_default_colors_from_startup_probe(colors);
        }
        Err(err) => {
            tracing::warn!(
                duration_ms = %started_at.elapsed().as_millis(),
                "terminal default color probe failed: {err}"
            );
            crate::terminal_palette::set_default_colors_from_startup_probe(/*colors*/ None);
        }
    }
}

fn set_panic_hook() {
    let hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_after_exit(); // ignore any errors as we are already failing
        hook(panic_info);
    }));
}

#[derive(Clone, Debug)]
pub enum TuiEvent {
    /// A terminal key event after focus, paste, and protocol bookkeeping has been handled.
    Key(KeyEvent),
    /// A bracketed paste payload normalized by the app layer before it reaches the composer.
    Paste(String),
    /// A terminal size notification that should be handled as resize-sensitive draw work.
    ///
    /// Resize is separate from `Draw` so the app can run feature-gated pre-render logic without
    /// changing the default draw path for scheduled frames.
    Resize,
    /// A scheduled repaint that does not necessarily correspond to a terminal size change.
    Draw,
}

pub struct Tui {
    frame_requester: FrameRequester,
    draw_tx: broadcast::Sender<()>,
    event_broker: Arc<EventBroker>,
    pub(crate) terminal: Terminal,
    alt_saved_viewport: Option<ratatui::layout::Rect>,
    #[cfg(unix)]
    suspend_context: SuspendContext,
    // True when overlay alt-screen UI is active
    alt_screen_active: Arc<AtomicBool>,
    // True when terminal/tab is focused; updated internally from crossterm events
    terminal_focused: Arc<AtomicBool>,
    // When false, enter_alt_screen() becomes a no-op.
    alt_screen_enabled: bool,
    // Keeps unmanaged process stderr writes out of the inline viewport.
    _stderr_guard: terminal_stderr::TerminalStderrGuard,
}

fn clear_for_viewport_change<B>(terminal: &mut CustomTerminal<B>, new_area: Rect) -> Result<()>
where
    B: Backend + Write,
{
    let clear_position = if terminal.viewport_area.is_empty() {
        new_area.as_position()
    } else {
        terminal.viewport_area.as_position()
    };
    terminal.clear_after_position(clear_position)
}

impl Tui {
    pub(crate) fn new(
        terminal: Terminal,
        _enhanced_keys_supported: bool,
        stderr_guard: terminal_stderr::TerminalStderrGuard,
    ) -> Self {
        let (draw_tx, _) = broadcast::channel(1);
        let frame_requester = FrameRequester::new(draw_tx.clone());

        // Cache this to avoid contention with the event reader.
        supports_color::on_cached(supports_color::Stream::Stdout);
        let _ = crate::terminal_palette::default_colors();
        Self {
            frame_requester,
            draw_tx,
            event_broker: Arc::new(EventBroker::new()),
            terminal,
            alt_saved_viewport: None,
            #[cfg(unix)]
            suspend_context: SuspendContext::new(),
            alt_screen_active: Arc::new(AtomicBool::new(false)),
            terminal_focused: Arc::new(AtomicBool::new(true)),
            alt_screen_enabled: true,
            _stderr_guard: stderr_guard,
        }
    }

    /// Set whether alternate screen is enabled. When false, enter_alt_screen() becomes a no-op.
    pub fn set_alt_screen_enabled(&mut self, enabled: bool) {
        self.alt_screen_enabled = enabled;
    }

    pub fn frame_requester(&self) -> FrameRequester {
        self.frame_requester.clone()
    }

    pub fn is_alt_screen_active(&self) -> bool {
        self.alt_screen_active.load(Ordering::Relaxed)
    }

    pub fn event_stream(&self) -> Pin<Box<dyn Stream<Item = TuiEvent> + Send + 'static>> {
        #[cfg(unix)]
        let stream = TuiEventStream::new(
            self.event_broker.clone(),
            self.draw_tx.subscribe(),
            self.terminal_focused.clone(),
            self.suspend_context.clone(),
            self.alt_screen_active.clone(),
        );
        #[cfg(not(unix))]
        let stream = TuiEventStream::new(
            self.event_broker.clone(),
            self.draw_tx.subscribe(),
            self.terminal_focused.clone(),
        );
        Box::pin(stream)
    }

    /// Enter alternate screen and expand the viewport to full terminal size, saving the current
    /// inline viewport for restoration when leaving.
    pub fn enter_alt_screen(&mut self) -> Result<()> {
        if !self.alt_screen_enabled {
            return Ok(());
        }
        let was_alt_screen = self.is_alt_screen_active();
        let _ = execute!(self.terminal.backend_mut(), EnterAlternateScreen);
        if !was_alt_screen {
            keyboard_modes::enable_keyboard_enhancement();
        }
        // Keep wheel input available in alternate screen.
        let _ = execute!(self.terminal.backend_mut(), EnableAlternateScroll);
        let _ = execute!(self.terminal.backend_mut(), EnableMouseCapture);
        if let Ok(size) = self.terminal.size() {
            self.alt_saved_viewport = Some(self.terminal.viewport_area);
            self.terminal.set_viewport_area(ratatui::layout::Rect::new(
                0,
                0,
                size.width,
                size.height,
            ));
            let _ = self.terminal.clear();
        }
        self.alt_screen_active.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Leave alternate screen and restore the previously saved inline viewport, if any.
    pub fn leave_alt_screen(&mut self) -> Result<()> {
        if !self.alt_screen_enabled {
            return Ok(());
        }
        if self.is_alt_screen_active() {
            keyboard_modes::restore_keyboard_enhancement_stack();
        }
        // Disable alternate scroll when leaving alt-screen
        let _ = execute!(self.terminal.backend_mut(), DisableMouseCapture);
        let _ = execute!(self.terminal.backend_mut(), DisableAlternateScroll);
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        if let Some(saved) = self.alt_saved_viewport.take() {
            self.terminal.set_viewport_area(saved);
        }
        self.alt_screen_active.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub fn draw(
        &mut self,
        height: u16,
        draw_fn: impl FnOnce(&mut custom_terminal::Frame),
    ) -> Result<()> {
        // If we are resuming from ^Z, we need to prepare the resume action now so we can apply it
        // in the synchronized update.
        #[cfg(unix)]
        let mut prepared_resume = self
            .suspend_context
            .prepare_resume_action(&mut self.alt_saved_viewport);

        // Precompute any viewport updates that need a cursor-position query before entering
        // the synchronized update, to avoid racing with the event reader.
        let mut pending_viewport_area = self.pending_viewport_area()?;

        ensure_virtual_terminal_processing()?;

        stdout().sync_update(|_| {
            #[cfg(unix)]
            if let Some(prepared) = prepared_resume.take() {
                prepared.apply(&mut self.terminal)?;
            }

            let terminal = &mut self.terminal;
            if let Some(new_area) = pending_viewport_area.take() {
                terminal.set_viewport_area(new_area);
                terminal.clear()?;
            }

            let size = terminal.size()?;

            let mut area = terminal.viewport_area;
            area.height = height.min(size.height);
            area.width = size.width;
            // If the viewport has expanded, scroll everything else up to make room.
            if area.bottom() > size.height {
                terminal
                    .backend_mut()
                    .scroll_region_up(0..area.top(), area.bottom() - size.height)?;
                area.y = size.height - area.height;
            }
            if area != terminal.viewport_area {
                // On startup, the old viewport can still be empty. Clear from the
                // new viewport top so stale shell cells do not show through spaces.
                clear_for_viewport_change(terminal, area)?;
                terminal.set_viewport_area(area);
            }

            // Update the y position for suspending so Ctrl-Z can place the cursor correctly.
            #[cfg(unix)]
            {
                let area = terminal.viewport_area;
                let inline_area_bottom = if self.alt_screen_active.load(Ordering::Relaxed) {
                    self.alt_saved_viewport
                        .map(|r| r.bottom().saturating_sub(1))
                        .unwrap_or_else(|| area.bottom().saturating_sub(1))
                } else {
                    area.bottom().saturating_sub(1)
                };
                self.suspend_context.set_cursor_y(inline_area_bottom);
            }

            terminal.draw(|frame| {
                draw_fn(frame);
            })
        })?
    }

    fn pending_viewport_area(&mut self) -> Result<Option<Rect>> {
        let terminal = &mut self.terminal;
        let screen_size = terminal.size()?;
        let last_known_screen_size = terminal.last_known_screen_size;
        if screen_size != last_known_screen_size
            && let Ok(cursor_pos) = terminal.get_cursor_position()
        {
            let last_known_cursor_pos = terminal.last_known_cursor_pos;
            // If we resized AND the cursor moved, we adjust the viewport area to keep the
            // cursor in the same position. This is a heuristic that seems to work well
            // at least in iTerm2.
            if cursor_pos.y != last_known_cursor_pos.y {
                let offset = Offset {
                    x: 0,
                    y: cursor_pos.y as i32 - last_known_cursor_pos.y as i32,
                };
                return Ok(Some(terminal.viewport_area.offset(offset)));
            }
        }
        Ok(None)
    }
}

#[cfg(windows)]
fn ensure_virtual_terminal_processing() -> Result<()> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::ENABLE_PROCESSED_OUTPUT;
    use windows_sys::Win32::System::Console::ENABLE_VIRTUAL_TERMINAL_PROCESSING;
    use windows_sys::Win32::System::Console::GetConsoleMode;
    use windows_sys::Win32::System::Console::GetStdHandle;
    use windows_sys::Win32::System::Console::STD_ERROR_HANDLE;
    use windows_sys::Win32::System::Console::STD_OUTPUT_HANDLE;
    use windows_sys::Win32::System::Console::SetConsoleMode;

    fn enable_for_handle(handle: HANDLE) -> Result<()> {
        if handle == INVALID_HANDLE_VALUE || handle == 0 {
            return Ok(());
        }

        let mut mode = 0;
        if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
            return Ok(());
        }

        let requested = ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
        if mode & requested == requested {
            return Ok(());
        }

        if unsafe { SetConsoleMode(handle, mode | requested) } == 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(())
    }

    let stdout_handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    enable_for_handle(stdout_handle)?;

    let stderr_handle = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    enable_for_handle(stderr_handle)?;

    Ok(())
}

#[cfg(not(windows))]
fn ensure_virtual_terminal_processing() -> Result<()> {
    Ok(())
}
