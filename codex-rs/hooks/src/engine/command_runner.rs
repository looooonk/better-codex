use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::Span;

use super::CommandShell;
use super::ConfiguredHandler;
use super::dispatcher::hook_event_name_label;
use super::dispatcher::hook_execution_mode_label;
use super::dispatcher::hook_handler_type_label;
use super::dispatcher::hook_scope_label;
use super::dispatcher::hook_source_label;
use super::dispatcher::scope_for_event;
use codex_protocol::protocol::HookExecutionMode;
use codex_protocol::protocol::HookHandlerType;

const MAX_HOOK_OUTPUT_BYTES_PER_STREAM: usize = 1024 * 1024;

#[derive(Debug)]
pub(crate) struct CommandRunResult {
    pub started_at: i64,
    pub completed_at: i64,
    pub duration_ms: i64,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

#[tracing::instrument(
    name = "codex.hooks.command",
    level = "trace",
    skip_all,
    fields(
        hook.event_name = hook_event_name_label(handler.event_name),
        hook.handler_type = hook_handler_type_label(HookHandlerType::Command),
        hook.execution_mode = hook_execution_mode_label(HookExecutionMode::Sync),
        hook.scope = hook_scope_label(scope_for_event(handler.event_name)),
        hook.source = hook_source_label(handler.source),
        hook.display_order = handler.display_order,
        hook.configured_order = configured_order,
        hook.timeout_sec = handler.timeout_sec,
        hook.command_outcome = tracing::field::Empty,
    )
)]
pub(crate) async fn run_command(
    shell: &CommandShell,
    handler: &ConfiguredHandler,
    configured_order: usize,
    input_json: &str,
    cwd: &Path,
) -> CommandRunResult {
    let started_at = chrono::Utc::now().timestamp();
    let started = Instant::now();

    let mut command = build_command(shell, handler);
    command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return finish_command_run(
                started_at,
                started,
                CommandRunCompletion {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(err.to_string()),
                    outcome: "spawn_error",
                },
            );
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && let Err(err) = stdin.write_all(input_json.as_bytes()).await
    {
        let _ = child.kill().await;
        return finish_command_run(
            started_at,
            started,
            CommandRunCompletion {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("failed to write hook stdin: {err}")),
                outcome: "stdin_error",
            },
        );
    }

    let Some(stdout) = child.stdout.take() else {
        unreachable!("hook stdout is configured as piped");
    };
    let Some(stderr) = child.stderr.take() else {
        unreachable!("hook stderr is configured as piped");
    };
    let wait = async {
        let (status, stdout, stderr) =
            tokio::join!(child.wait(), capture_output(stdout), capture_output(stderr));
        Ok::<_, std::io::Error>((status?, stdout?, stderr?))
    };
    let timeout_duration = Duration::from_secs(handler.timeout_sec);
    match timeout(timeout_duration, wait).await {
        Ok(Ok((status, stdout, stderr))) => {
            let exceeded_streams = match (stdout.truncated, stderr.truncated) {
                (false, false) => None,
                (true, false) => Some("stdout"),
                (false, true) => Some("stderr"),
                (true, true) => Some("stdout and stderr"),
            };
            finish_command_run(
                started_at,
                started,
                CommandRunCompletion {
                    exit_code: status.code(),
                    stdout: String::from_utf8_lossy(&stdout.bytes).to_string(),
                    stderr: String::from_utf8_lossy(&stderr.bytes).to_string(),
                    error: exceeded_streams.map(|streams| {
                        format!(
                            "hook {streams} exceeded the {MAX_HOOK_OUTPUT_BYTES_PER_STREAM}-byte capture limit"
                        )
                    }),
                    outcome: if exceeded_streams.is_some() {
                        "output_limit"
                    } else {
                        "completed"
                    },
                },
            )
        }
        Ok(Err(err)) => finish_command_run(
            started_at,
            started,
            CommandRunCompletion {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(err.to_string()),
                outcome: "wait_error",
            },
        ),
        Err(_) => finish_command_run(
            started_at,
            started,
            CommandRunCompletion {
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("hook timed out after {}s", handler.timeout_sec)),
                outcome: "timeout",
            },
        ),
    }
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn capture_output(mut reader: impl AsyncRead + Unpin) -> std::io::Result<CapturedOutput> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_HOOK_OUTPUT_BYTES_PER_STREAM.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..read.min(remaining)]);
        truncated |= read > remaining;
    }
    Ok(CapturedOutput { bytes, truncated })
}

struct CommandRunCompletion {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
    outcome: &'static str,
}

fn finish_command_run(
    started_at: i64,
    started: Instant,
    completion: CommandRunCompletion,
) -> CommandRunResult {
    Span::current().record("hook.command_outcome", completion.outcome);
    CommandRunResult {
        started_at,
        completed_at: chrono::Utc::now().timestamp(),
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
        exit_code: completion.exit_code,
        stdout: completion.stdout,
        stderr: completion.stderr,
        error: completion.error,
    }
}

fn build_command(shell: &CommandShell, handler: &ConfiguredHandler) -> Command {
    let mut command = if shell.program.is_empty() {
        default_shell_command()
    } else {
        Command::new(&shell.program)
    };
    if shell.program.is_empty() {
        command.arg(&handler.command);
    } else {
        command.args(&shell.args);
        command.arg(&handler.command);
    }
    command.envs(&handler.env);
    command
}

fn default_shell_command() -> Command {
    #[cfg(windows)]
    {
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        let mut command = Command::new(comspec);
        command.arg("/C");
        command
    }

    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut command = Command::new(shell);
        command.arg("-lc");
        command
    }
}

#[cfg(test)]
#[path = "command_runner_tests.rs"]
mod tests;
