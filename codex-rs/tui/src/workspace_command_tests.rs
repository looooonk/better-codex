use super::*;

use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use pretty_assertions::assert_eq;
use tokio::sync::Semaphore;

struct RecordingCancellableExecutor {
    output: WorkspaceCommandOutput,
    run_process_ids: Mutex<Vec<String>>,
    terminate_process_ids: Mutex<Vec<String>>,
    run_started: Arc<Semaphore>,
    finish_run: Arc<Semaphore>,
    terminate_called: Arc<Semaphore>,
    transient_terminate_failures: AtomicUsize,
    termination_behavior: RecordedTerminationBehavior,
}

struct DefaultOnlyExecutor {
    output: WorkspaceCommandOutput,
    run_started: Arc<Semaphore>,
    finish_run: Arc<Semaphore>,
}

#[derive(Clone, Copy)]
enum RecordedTerminationBehavior {
    Complete,
    Stall,
}

impl DefaultOnlyExecutor {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            output: WorkspaceCommandOutput {
                exit_code: 0,
                stdout: "default runner done\n".to_string(),
                stderr: String::new(),
            },
            run_started: Arc::new(Semaphore::new(/*permits*/ 0)),
            finish_run: Arc::new(Semaphore::new(/*permits*/ 0)),
        })
    }

    async fn wait_until_started(&self) {
        tokio::time::timeout(Duration::from_secs(/*secs*/ 1), self.run_started.acquire())
            .await
            .expect("default workspace command should start")
            .expect("start semaphore should remain open")
            .forget();
    }
}

impl WorkspaceCommandExecutor for DefaultOnlyExecutor {
    fn run(
        &self,
        _command: WorkspaceCommand,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        let output = self.output.clone();
        let run_started = Arc::clone(&self.run_started);
        let finish_run = Arc::clone(&self.finish_run);
        Box::pin(async move {
            run_started.add_permits(/*n*/ 1);
            let permit = finish_run
                .acquire_owned()
                .await
                .expect("finish semaphore should remain open");
            permit.forget();
            Ok(output)
        })
    }
}

impl RecordingCancellableExecutor {
    fn new(transient_terminate_failures: usize) -> Arc<Self> {
        Self::with_termination_behavior(
            transient_terminate_failures,
            RecordedTerminationBehavior::Complete,
        )
    }

    fn with_stalled_termination() -> Arc<Self> {
        Self::with_termination_behavior(
            /*transient_terminate_failures*/ 0,
            RecordedTerminationBehavior::Stall,
        )
    }

    fn with_termination_behavior(
        transient_terminate_failures: usize,
        termination_behavior: RecordedTerminationBehavior,
    ) -> Arc<Self> {
        Arc::new(Self {
            output: WorkspaceCommandOutput {
                exit_code: 0,
                stdout: "done\n".to_string(),
                stderr: String::new(),
            },
            run_process_ids: Mutex::new(Vec::new()),
            terminate_process_ids: Mutex::new(Vec::new()),
            run_started: Arc::new(Semaphore::new(/*permits*/ 0)),
            finish_run: Arc::new(Semaphore::new(/*permits*/ 0)),
            terminate_called: Arc::new(Semaphore::new(/*permits*/ 0)),
            transient_terminate_failures: AtomicUsize::new(transient_terminate_failures),
            termination_behavior,
        })
    }

    async fn wait_until_started(&self) {
        tokio::time::timeout(Duration::from_secs(/*secs*/ 1), self.run_started.acquire())
            .await
            .expect("workspace command should start")
            .expect("start semaphore should remain open")
            .forget();
    }

    async fn wait_until_terminate_called(&self) {
        tokio::time::timeout(
            Duration::from_secs(/*secs*/ 1),
            self.terminate_called.acquire(),
        )
        .await
        .expect("workspace command termination should be requested")
        .expect("termination semaphore should remain open")
        .forget();
    }

    fn run_process_ids(&self) -> Vec<String> {
        self.run_process_ids
            .lock()
            .expect("run process ids should lock")
            .clone()
    }

    fn terminate_process_ids(&self) -> Vec<String> {
        self.terminate_process_ids
            .lock()
            .expect("terminate process ids should lock")
            .clone()
    }
}

impl WorkspaceCommandExecutor for RecordingCancellableExecutor {
    fn run(
        &self,
        _command: WorkspaceCommand,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        let output = self.output.clone();
        Box::pin(async move { Ok(output) })
    }

    fn run_cancellable(
        &self,
        _command: WorkspaceCommand,
        process_id: String,
    ) -> Pin<
        Box<dyn Future<Output = Result<WorkspaceCommandOutput, WorkspaceCommandError>> + Send + '_>,
    > {
        self.run_process_ids
            .lock()
            .expect("run process ids should lock")
            .push(process_id);
        let output = self.output.clone();
        let run_started = Arc::clone(&self.run_started);
        let finish_run = Arc::clone(&self.finish_run);
        Box::pin(async move {
            run_started.add_permits(/*n*/ 1);
            let permit = finish_run
                .acquire_owned()
                .await
                .expect("finish semaphore should remain open");
            permit.forget();
            Ok(output)
        })
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
        self.terminate_process_ids
            .lock()
            .expect("terminate process ids should lock")
            .push(process_id);
        let finish_run = Arc::clone(&self.finish_run);
        let terminate_called = Arc::clone(&self.terminate_called);
        let termination_behavior = self.termination_behavior;
        let should_fail = self
            .transient_terminate_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok();
        Box::pin(async move {
            terminate_called.add_permits(/*n*/ 1);
            if matches!(termination_behavior, RecordedTerminationBehavior::Stall) {
                std::future::pending().await
            }
            if should_fail {
                return Err(WorkspaceCommandError::new("process is not active yet"));
            }
            finish_run.add_permits(/*n*/ 1);
            Ok(WorkspaceCommandTermination::Requested)
        })
    }
}

#[tokio::test]
async fn background_execution_returns_while_command_is_running() {
    let runner = RecordingCancellableExecutor::new(/*transient_terminate_failures*/ 0);
    let execution =
        WorkspaceCommandExecution::start(runner.clone(), WorkspaceCommand::new(["long-running"]));

    runner.wait_until_started().await;
    assert!(!execution.is_finished());

    let wait_task = tokio::spawn(execution.wait());
    tokio::task::yield_now().await;
    assert!(!wait_task.is_finished());
    runner.finish_run.add_permits(/*n*/ 1);
    let output = tokio::time::timeout(Duration::from_secs(/*secs*/ 1), wait_task)
        .await
        .expect("workspace command should finish")
        .expect("workspace command task should not panic")
        .expect("workspace command should succeed");
    assert_eq!(output, runner.output);
}

#[tokio::test]
async fn cancellation_retries_start_race_and_uses_the_same_process_id() {
    let runner = RecordingCancellableExecutor::new(/*transient_terminate_failures*/ 1);
    let mut execution =
        WorkspaceCommandExecution::start(runner.clone(), WorkspaceCommand::new(["long-running"]));
    runner.wait_until_started().await;

    assert!(execution.cancel());
    assert!(!execution.cancel());
    let err = tokio::time::timeout(Duration::from_secs(/*secs*/ 1), execution.wait())
        .await
        .expect("workspace command cancellation should finish")
        .expect_err("workspace command should be cancelled");

    assert!(err.is_cancelled());
    let run_process_ids = runner.run_process_ids();
    let terminate_process_ids = runner.terminate_process_ids();
    assert_eq!(run_process_ids.len(), 1);
    assert_eq!(terminate_process_ids.len(), 2);
    assert!(
        terminate_process_ids
            .iter()
            .all(|process_id| process_id == &run_process_ids[0])
    );
}

#[tokio::test]
async fn dropping_execution_requests_remote_termination() {
    let runner = RecordingCancellableExecutor::new(/*transient_terminate_failures*/ 0);
    let execution =
        WorkspaceCommandExecution::start(runner.clone(), WorkspaceCommand::new(["long-running"]));
    runner.wait_until_started().await;

    drop(execution);
    runner.wait_until_terminate_called().await;

    assert_eq!(runner.terminate_process_ids().len(), 1);
}

#[tokio::test]
async fn unsupported_cancellation_preserves_the_running_future() {
    let runner = DefaultOnlyExecutor::new();
    let mut execution =
        WorkspaceCommandExecution::start(runner.clone(), WorkspaceCommand::new(["long-running"]));
    runner.wait_until_started().await;

    assert!(execution.cancel());
    let wait_task = tokio::spawn(execution.wait());
    tokio::task::yield_now().await;
    assert!(!wait_task.is_finished());

    runner.finish_run.add_permits(/*n*/ 1);
    let output = wait_task
        .await
        .expect("workspace command task should not panic")
        .expect("unsupported cancellation should preserve the command result");
    assert_eq!(output, runner.output);
}

#[tokio::test(start_paused = true)]
async fn stalled_remote_termination_is_bounded() {
    let runner = RecordingCancellableExecutor::with_stalled_termination();
    let mut execution =
        WorkspaceCommandExecution::start(runner.clone(), WorkspaceCommand::new(["long-running"]));
    runner.wait_until_started().await;

    assert!(execution.cancel());
    let wait_task = tokio::spawn(execution.wait());
    runner.wait_until_terminate_called().await;
    tokio::time::advance(TERMINATE_RETRY_TIMEOUT + Duration::from_secs(/*secs*/ 1)).await;
    let err = wait_task
        .await
        .expect("workspace command task should not panic")
        .expect_err("stalled termination should fail");

    assert!(!err.is_cancelled());
    assert_eq!(err.to_string(), "workspace command cancellation timed out");
}
