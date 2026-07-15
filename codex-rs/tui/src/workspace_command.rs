//! App-server-backed workspace command execution for TUI-owned background lookups.
//!
//! This module is the TUI boundary for non-interactive commands that need to run wherever
//! the active workspace lives. Callers describe a command in terms of argv, cwd, environment
//! overrides, timeout, and output cap; the runner translates that request to app-server
//! `command/exec`. Keeping this as a TUI-local abstraction lets status surfaces avoid knowing
//! whether the current app-server is embedded or remote.
//!
//! Commands sent through this path should not prompt for stdin. Most callers should keep output
//! bounded so metadata refreshes cannot grow into unbounded background processes; callers that own a
//! full user-visible payload, such as `/diff`, can explicitly opt out of output capping.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::CommandExecParams;
use codex_app_server_protocol::CommandExecResponse;
use codex_app_server_protocol::CommandExecTerminateParams;
use codex_app_server_protocol::CommandExecTerminateResponse;
use codex_app_server_protocol::RequestId;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

const TERMINATE_RETRY_INTERVAL: Duration = Duration::from_millis(/*millis*/ 25);
const TERMINATE_RETRY_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);
const TERMINATE_SETTLE_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 5);

/// Shared handle for running workspace commands from TUI components.
pub(crate) type WorkspaceCommandRunner = Arc<dyn WorkspaceCommandExecutor>;

/// A workspace command that runs independently of the TUI event loop.
///
/// Dropping the handle requests cancellation. App-server-backed commands use a client-supplied
/// process id, so cancellation reaches the process on the machine that owns the workspace rather
/// than merely dropping the local response future.
pub(crate) struct WorkspaceCommandExecution {
    cancel_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<WorkspaceCommandOutput, WorkspaceCommandError>>>,
}

impl WorkspaceCommandExecution {
    /// Starts a command on the supplied workspace runner and returns immediately.
    pub(crate) fn start(runner: WorkspaceCommandRunner, command: WorkspaceCommand) -> Self {
        let process_id = format!("workspace-command-{}", Uuid::new_v4());
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let task = tokio::spawn(run_background_command(
            runner, command, process_id, cancel_rx,
        ));
        Self {
            cancel_tx: Some(cancel_tx),
            task: Some(task),
        }
    }

    /// Requests cancellation without waiting for the remote process to terminate.
    ///
    /// Returns `true` only for the first cancellation request.
    pub(crate) fn cancel(&mut self) -> bool {
        self.cancel_tx
            .take()
            .is_some_and(|cancel_tx| cancel_tx.send(()).is_ok())
    }

    /// Returns whether the background task has produced its final result.
    pub(crate) fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Waits for the final command result.
    pub(crate) async fn wait(mut self) -> Result<WorkspaceCommandOutput, WorkspaceCommandError> {
        let Some(task) = self.task.take() else {
            return Err(WorkspaceCommandError::new(
                "workspace command task result was already consumed",
            ));
        };
        let result = task.await.map_err(|err| {
            WorkspaceCommandError::new(format!("workspace command task failed: {err}"))
        })?;
        let _ = self.cancel_tx.take();
        result
    }
}

impl Drop for WorkspaceCommandExecution {
    fn drop(&mut self) {
        if let Some(cancel_tx) = self.cancel_tx.take() {
            let _ = cancel_tx.send(());
        }
    }
}

/// Describes a bounded non-interactive command to execute in the active workspace.
///
/// The command is intentionally argv-based rather than shell-based so callers do not need to quote
/// user or repository data. `cwd` is interpreted by app-server relative to the workspace rules for
/// the active session, which is what makes the same request shape work for embedded and remote
/// app-server instances.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceCommand {
    /// Program and arguments to execute without shell interpolation.
    pub(crate) argv: Vec<String>,
    /// Working directory for the command, if different from app-server's session cwd.
    pub(crate) cwd: Option<PathBuf>,
    /// Environment overrides where `None` removes a variable.
    pub(crate) env: HashMap<String, Option<String>>,
    /// Maximum wall-clock duration before app-server cancels the command.
    pub(crate) timeout: Duration,
    /// Maximum captured stdout/stderr bytes returned by app-server.
    pub(crate) output_bytes_cap: usize,
    /// Whether app-server should return uncapped stdout/stderr.
    pub(crate) disable_output_cap: bool,
}

impl WorkspaceCommand {
    /// Creates a workspace command with conservative defaults for metadata probes.
    pub(crate) fn new(argv: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            cwd: None,
            env: HashMap::new(),
            timeout: Duration::from_secs(/*secs*/ 5),
            output_bytes_cap: 64 * 1024,
            disable_output_cap: false,
        }
    }

    /// Sets the command working directory.
    pub(crate) fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Adds or replaces one environment variable override.
    pub(crate) fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the maximum wall-clock duration before app-server cancels the command.
    pub(crate) fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Requests uncapped stdout/stderr capture from app-server.
    pub(crate) fn disable_output_cap(mut self) -> Self {
        self.disable_output_cap = true;
        self
    }
}

/// Captured result from a completed workspace command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceCommandOutput {
    /// Process exit status code reported by app-server.
    pub(crate) exit_code: i32,
    /// Captured stdout after app-server output capping.
    pub(crate) stdout: String,
    /// Captured stderr after app-server output capping.
    pub(crate) stderr: String,
}

impl WorkspaceCommandOutput {
    /// Returns whether the process exited successfully.
    pub(crate) fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Failure before a command result was available.
///
/// Non-zero process exits are represented as `WorkspaceCommandOutput` so callers can distinguish
/// a normal probe miss from an app-server request failure or explicit cancellation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceCommandError {
    message: String,
    cancelled: bool,
}

impl WorkspaceCommandError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cancelled: false,
        }
    }

    fn cancelled() -> Self {
        Self {
            message: "workspace command cancelled".to_string(),
            cancelled: true,
        }
    }

    /// Returns whether the command ended because cancellation was requested.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

impl std::fmt::Display for WorkspaceCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WorkspaceCommandError {}

/// Executes non-interactive workspace commands through the active TUI app-server session.
///
/// Implementations decide where the workspace lives. Callers provide argv/cwd/env and should not
/// branch on local versus remote execution.
pub(crate) trait WorkspaceCommandExecutor: Send + Sync {
    /// Runs a workspace command and returns captured output or an app-server request error.
    ///
    /// Callers should treat errors as infrastructure failures and should treat successful output
    /// with a non-zero exit code as ordinary command failure. Returning a boxed future keeps the
    /// trait object-safe.
    fn run(
        &self,
        command: WorkspaceCommand,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    >;

    /// Runs a command under a caller-supplied process id that can later be terminated.
    ///
    /// Implementations that cannot address individual processes may use the default, which keeps
    /// running through [`Self::run`]. Cancellation cannot stop those commands, so the background
    /// handle continues waiting for their actual result.
    fn run_cancellable(
        &self,
        command: WorkspaceCommand,
        _process_id: String,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        self.run(command)
    }

    /// Requests termination of a command previously started by [`Self::run_cancellable`].
    fn terminate(
        &self,
        _process_id: String,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<WorkspaceCommandTermination, WorkspaceCommandError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async { Ok(WorkspaceCommandTermination::Unsupported) })
    }
}

/// Whether an executor accepted a request to terminate a workspace command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceCommandTermination {
    /// The executor forwarded termination to the process owner.
    Requested,
    /// This executor cannot address individual processes.
    Unsupported,
}

enum WorkspaceCommandProcess {
    ServerGenerated,
    Client(String),
}

/// Workspace command runner that forwards every request to the active app-server.
#[derive(Clone)]
pub(crate) struct AppServerWorkspaceCommandRunner {
    request_handle: AppServerRequestHandle,
}

impl AppServerWorkspaceCommandRunner {
    /// Creates a runner from an app-server request handle owned by the current TUI session.
    pub(crate) fn new(request_handle: AppServerRequestHandle) -> Self {
        Self { request_handle }
    }
}

impl WorkspaceCommandExecutor for AppServerWorkspaceCommandRunner {
    /// Sends the command as a one-off app-server `command/exec` request.
    ///
    /// The request is non-tty, does not stream stdin/stdout/stderr, and uses the caller's timeout
    /// and output cap. It leaves sandbox and permission profile selection to app-server so the same
    /// runner follows the active session's embedded or remote execution policy.
    fn run(
        &self,
        command: WorkspaceCommand,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        self.run_with_process(command, WorkspaceCommandProcess::ServerGenerated)
    }

    fn run_cancellable(
        &self,
        command: WorkspaceCommand,
        process_id: String,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        self.run_with_process(command, WorkspaceCommandProcess::Client(process_id))
    }

    fn terminate(
        &self,
        process_id: String,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<WorkspaceCommandTermination, WorkspaceCommandError>>
                + Send
                + '_,
        >,
    > {
        let request_handle = self.request_handle.clone();
        Box::pin(async move {
            let _: CommandExecTerminateResponse = request_handle
                .request_typed(ClientRequest::CommandExecTerminate {
                    request_id: RequestId::String(format!(
                        "workspace-command-terminate-{}",
                        Uuid::new_v4()
                    )),
                    params: CommandExecTerminateParams { process_id },
                })
                .await
                .map_err(|err| WorkspaceCommandError::new(err.to_string()))?;
            Ok(WorkspaceCommandTermination::Requested)
        })
    }
}

impl AppServerWorkspaceCommandRunner {
    fn run_with_process(
        &self,
        command: WorkspaceCommand,
        process: WorkspaceCommandProcess,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        let request_handle = self.request_handle.clone();
        Box::pin(async move {
            let process_id = match process {
                WorkspaceCommandProcess::ServerGenerated => None,
                WorkspaceCommandProcess::Client(process_id) => Some(process_id),
            };
            let timeout_ms = i64::try_from(command.timeout.as_millis()).unwrap_or(i64::MAX);
            let env = if command.env.is_empty() {
                None
            } else {
                Some(command.env)
            };
            let response: CommandExecResponse = request_handle
                .request_typed(ClientRequest::OneOffCommandExec {
                    request_id: RequestId::String(format!("workspace-command-{}", Uuid::new_v4())),
                    params: CommandExecParams {
                        command: command.argv,
                        process_id,
                        tty: false,
                        stream_stdin: false,
                        stream_stdout_stderr: false,
                        output_bytes_cap: (!command.disable_output_cap)
                            .then_some(command.output_bytes_cap),
                        disable_output_cap: command.disable_output_cap,
                        disable_timeout: false,
                        timeout_ms: Some(timeout_ms),
                        cwd: command.cwd,
                        env,
                        size: None,
                        sandbox_policy: None,
                        permission_profile: None,
                    },
                })
                .await
                .map_err(|err| WorkspaceCommandError::new(err.to_string()))?;

            Ok(WorkspaceCommandOutput {
                exit_code: response.exit_code,
                stdout: response.stdout,
                stderr: response.stderr,
            })
        })
    }
}

async fn run_background_command(
    runner: WorkspaceCommandRunner,
    command: WorkspaceCommand,
    process_id: String,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<WorkspaceCommandOutput, WorkspaceCommandError> {
    let execution = runner.run_cancellable(command, process_id.clone());
    tokio::pin!(execution);

    let cancellation = tokio::select! {
        result = &mut execution => return result,
        cancellation = &mut cancel_rx => cancellation,
    };
    if cancellation.is_err() {
        return execution.await;
    }

    let retry_deadline = Instant::now() + TERMINATE_RETRY_TIMEOUT;
    loop {
        let terminate = runner.terminate(process_id.clone());
        let termination = tokio::select! {
            result = &mut execution => return result,
            termination = terminate => Some(termination),
            () = tokio::time::sleep_until(retry_deadline) => None,
        };
        let Some(termination) = termination else {
            return Err(WorkspaceCommandError::new(
                "workspace command cancellation timed out",
            ));
        };
        match termination {
            Ok(WorkspaceCommandTermination::Requested) => {
                return tokio::select! {
                    _ = &mut execution => Err(WorkspaceCommandError::cancelled()),
                    () = tokio::time::sleep(TERMINATE_SETTLE_TIMEOUT) => Err(
                        WorkspaceCommandError::new(
                            "workspace command termination was accepted but did not finish",
                        )
                    ),
                };
            }
            Ok(WorkspaceCommandTermination::Unsupported) => {
                return execution.await;
            }
            Err(err) if Instant::now() >= retry_deadline => {
                return Err(WorkspaceCommandError::new(format!(
                    "workspace command cancellation failed: {err}"
                )));
            }
            Err(_) => {
                let retry_at = (Instant::now() + TERMINATE_RETRY_INTERVAL).min(retry_deadline);
                tokio::select! {
                    result = &mut execution => return result,
                    () = tokio::time::sleep_until(retry_at) => {}
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "workspace_command_tests.rs"]
mod tests;
