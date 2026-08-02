use super::BACKEND_ACTION_POLL_INTERVAL;
use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::composer::MAX_COMPOSER_BYTES;
use super::composer::input_too_large_message;
use codex_protocol::ThreadId;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use std::fs;
use std::future::Future;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use tempfile::Builder;
use tokio::process::Command;

const VIM_ACTION_ENV: &str = "BETTER_CODEX_VIM_ACTION";
const VIM_INPUT_ENV: &str = "BETTER_CODEX_VIM_INPUT";
pub(super) const MAX_CONSECUTIVE_APP_SERVER_EVENTS: usize = 32;
const VIM_BRIDGE: &str = r#"set nomodeline
silent execute 'edit ' . fnameescape($BETTER_CODEX_VIM_INPUT)
setlocal noswapfile nobackup nowritebackup noundofile nomodeline fileformat=unix
function! s:CodexSubmit()
  try
    silent write
    call writefile(['submit'], $BETTER_CODEX_VIM_ACTION)
    qall!
  catch
    call delete($BETTER_CODEX_VIM_ACTION)
    echoerr v:exception
  endtry
endfunction
command! Codex call <SID>CodexSubmit()
nnoremap <silent><buffer> <C-J> :Codex<CR>
echo 'Ctrl-J or :Codex sends this buffer to Better Codex'
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VimInputRequest {
    seed: String,
    thread_id: ThreadId,
}

impl VimInputRequest {
    pub(super) fn empty(thread_id: ThreadId) -> Self {
        Self {
            seed: String::new(),
            thread_id,
        }
    }

    pub(super) fn thread_id(&self) -> ThreadId {
        self.thread_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum VimInputOutcome {
    Submit(String),
    ReturnDraft(String),
    Cancelled,
}

pub(super) enum VimInputWaitOutcome {
    Completed(Result<VimInputOutcome>),
    AppServerDisconnected,
}

pub(super) async fn run(request: VimInputRequest) -> Result<VimInputOutcome> {
    let program = resolve_vim_program()?;
    run_with_program(request, &program).await
}

fn resolve_vim_program() -> Result<PathBuf> {
    for candidate in ["vim", "nvim"] {
        #[cfg(unix)]
        if let Some(program) = std::env::var_os("PATH").and_then(|path| {
            use std::os::unix::fs::PermissionsExt;

            std::env::split_paths(&path)
                .map(|directory| directory.join(candidate))
                .find(|program| {
                    program.metadata().is_ok_and(|metadata| {
                        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                    })
                })
        }) {
            return Ok(program);
        }
        #[cfg(windows)]
        if let Ok(program) = which::which(candidate) {
            return Ok(program);
        }
    }
    Err(eyre!(
        "neither `vim` nor `nvim` was found on PATH; install one before running /vim"
    ))
}

pub(super) async fn wait_while_processing_events<S, F>(
    shell: &mut ShellState,
    app_server: &mut S,
    editor: F,
) -> VimInputWaitOutcome
where
    S: AppShellBackend,
    F: Future<Output = Result<VimInputOutcome>>,
{
    tokio::pin!(editor);
    let mut backend_action_poll = tokio::time::interval(BACKEND_ACTION_POLL_INTERVAL);
    backend_action_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut consecutive_app_server_events = 0;
    loop {
        let user_input_auto_resolution_deadline =
            shell.pending_user_input_auto_resolution_deadline();
        let yield_after_event_burst =
            consecutive_app_server_events >= MAX_CONSECUTIVE_APP_SERVER_EVENTS;
        tokio::select! {
            biased;
            event = app_server.next_event(), if !yield_after_event_burst => {
                let Some(event) = event else {
                    return VimInputWaitOutcome::AppServerDisconnected;
                };
                consecutive_app_server_events += 1;
                if let Err(err) = shell.handle_app_server_event(app_server, event).await {
                    shell.report_action_error("failed to handle app-server event", err);
                }
            }
            _ = tokio::time::sleep_until(
                user_input_auto_resolution_deadline.unwrap_or_else(tokio::time::Instant::now)
            ), if user_input_auto_resolution_deadline.is_some() => {
                consecutive_app_server_events = 0;
                shell.start_expired_user_input_resolution(app_server);
            }
            result = &mut editor => return VimInputWaitOutcome::Completed(result),
            _ = backend_action_poll.tick(), if shell.has_pending_backend_actions() => {
                consecutive_app_server_events = 0;
                shell.poll_backend_actions(app_server).await;
            }
            _ = tokio::task::yield_now(), if yield_after_event_burst => {
                consecutive_app_server_events = 0;
            }
        }
    }
}

async fn run_with_program(request: VimInputRequest, program: &Path) -> Result<VimInputOutcome> {
    let temp_dir = Builder::new()
        .prefix("better-codex-vim-")
        .tempdir()
        .wrap_err("failed to create Vim input directory")?;
    let input_path = temp_dir.path().join("input.md");
    let submit_path = temp_dir.path().join("submit");
    let bridge_path = temp_dir.path().join("bridge.vim");
    fs::write(&input_path, request.seed).wrap_err("failed to seed Vim input")?;
    fs::write(&bridge_path, VIM_BRIDGE).wrap_err("failed to create Vim bridge")?;

    let mut command = Command::new(program);
    command
        .arg("-n")
        .arg("-i")
        .arg("NONE")
        .arg("-S")
        .arg(&bridge_path)
        .env(VIM_ACTION_ENV, &submit_path)
        .env(VIM_INPUT_ENV, &input_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let status = command
        .status()
        .await
        .wrap_err_with(|| format!("failed to launch {}", program.display()))?;

    if !status.success() {
        return Ok(VimInputOutcome::Cancelled);
    }

    let text = read_vim_input(&input_path)?;
    if is_submit_marker(&submit_path)? {
        Ok(VimInputOutcome::Submit(text))
    } else {
        Ok(VimInputOutcome::ReturnDraft(text))
    }
}

fn is_submit_marker(path: &Path) -> Result<bool> {
    match fs::read_to_string(path) {
        Ok(marker) => Ok(matches!(marker.as_str(), "submit" | "submit\n")),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).wrap_err("failed to read Vim submission marker"),
    }
}

fn read_vim_input(path: &Path) -> Result<String> {
    let mut bytes = Vec::with_capacity(MAX_COMPOSER_BYTES.saturating_add(1));
    fs::File::open(path)
        .wrap_err("failed to open Vim input")?
        .take(u64::try_from(MAX_COMPOSER_BYTES.saturating_add(1)).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .wrap_err("failed to read Vim input")?;
    if bytes.len() > MAX_COMPOSER_BYTES {
        return Err(eyre!(input_too_large_message(bytes.len())));
    }
    let mut text = String::from_utf8(bytes)
        .wrap_err("Vim input was not valid UTF-8")?
        .replace("\r\n", "\n");
    if text.ends_with('\n') {
        text.pop();
    }
    Ok(text)
}

impl ShellState {
    async fn submit_prompt_preserving_draft<S>(
        &mut self,
        app_server: &mut S,
        prompt: String,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let composer_draft = self.composer.submission_text();
        self.composer.set_text(prompt.clone());
        let result = if self.active_turn_id.is_some() {
            self.steer_active_turn(app_server, prompt).await
        } else {
            self.submit_prompt(app_server, prompt);
            Ok(())
        };
        if !composer_draft.is_empty() {
            self.composer.restore_failed_submission(&composer_draft);
        }
        result
    }

    pub(super) async fn dispatch_pending_prompt_submission<S>(
        &mut self,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.has_pending_backend_action(ActionGroup::TurnStart) {
            return Ok(());
        }
        let Some(prompt) = self.pending_prompt_submission.take() else {
            return Ok(());
        };
        self.submit_prompt_preserving_draft(app_server, prompt)
            .await
    }

    pub(super) fn request_vim_input(&mut self) {
        if self.has_pending_backend_action(ActionGroup::SessionSwitch) {
            self.push_status("wait for the pending session switch to finish");
            return;
        }
        if self.pending_prompt_submission.is_some() {
            self.push_status("wait for the pending Vim input to send");
            return;
        }
        self.pending_vim_input = Some(VimInputRequest::empty(self.thread_id));
        self.push_status("opening Vim input");
    }

    pub(super) fn take_vim_input_request(&mut self) -> Option<VimInputRequest> {
        self.pending_vim_input.take()
    }

    pub(super) async fn complete_vim_input<S>(
        &mut self,
        originating_thread_id: ThreadId,
        result: Result<VimInputOutcome>,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let session_changed = originating_thread_id != self.thread_id;
        let result = match result {
            Ok(VimInputOutcome::Submit(text)) if session_changed => {
                Ok(VimInputOutcome::ReturnDraft(text))
            }
            result => result,
        };
        match result {
            Ok(VimInputOutcome::Submit(text)) => {
                if text.trim().is_empty() {
                    self.push_status("Vim input was empty");
                    return Ok(());
                }
                if self.reject_oversized_input(text.len()) {
                    return Ok(());
                }
                if self.has_pending_backend_action(ActionGroup::TurnStart) {
                    self.pending_prompt_submission = Some(text);
                    self.push_status("Vim input will send after the pending turn starts");
                    return Ok(());
                }
                self.submit_prompt_preserving_draft(app_server, text)
                    .await?;
            }
            Ok(VimInputOutcome::ReturnDraft(text)) => {
                if self.reject_oversized_input(text.len()) {
                    return Ok(());
                }
                let composer_draft = self.composer.submission_text();
                self.composer.set_text(text);
                if !composer_draft.is_empty() {
                    self.composer.restore_failed_submission(&composer_draft);
                }
                self.push_status(if session_changed {
                    "session changed; Vim input returned without sending"
                } else {
                    "Vim input returned to composer"
                });
            }
            Ok(VimInputOutcome::Cancelled) => self.push_status("Vim input cancelled"),
            Err(err) => self.report_action_error("failed to open Vim input", err),
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "vim_input_tests.rs"]
mod tests;
