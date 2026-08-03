use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_app_server_protocol::ApprovalsReviewer;
use codex_app_server_protocol::SandboxPolicy;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
struct FakeBackgroundRequestHandle {
    requests: Arc<Mutex<Vec<ObservedRequest>>>,
    fork_response: ThreadForkResponse,
    read_response: ThreadReadResponse,
}

#[derive(Debug, PartialEq)]
enum ObservedRequest {
    Fork(ThreadForkParams),
    Read(ThreadReadParams),
}

impl BackgroundRequestHandle for FakeBackgroundRequestHandle {
    fn send_thread_fork_request(
        &self,
        request: ClientRequest,
    ) -> impl std::future::Future<Output = Result<ThreadForkResponse, TypedRequestError>> + Send
    {
        let requests = Arc::clone(&self.requests);
        let response = self.fork_response.clone();
        async move {
            let ClientRequest::ThreadFork { params, .. } = request else {
                panic!("background fork should dispatch thread/fork");
            };
            requests
                .lock()
                .expect("requests mutex")
                .push(ObservedRequest::Fork(params));
            Ok(response)
        }
    }

    fn send_thread_read_request(
        &self,
        request: ClientRequest,
    ) -> impl std::future::Future<Output = Result<ThreadReadResponse, TypedRequestError>> + Send
    {
        let requests = Arc::clone(&self.requests);
        let response = self.read_response.clone();
        async move {
            let ClientRequest::ThreadRead { params, .. } = request else {
                panic!("fork title hydration should dispatch thread/read");
            };
            requests
                .lock()
                .expect("requests mutex")
                .push(ObservedRequest::Read(params));
            Ok(response)
        }
    }
}

fn thread(
    id: ThreadId,
    forked_from_id: Option<ThreadId>,
    name: Option<&str>,
    cwd: AbsolutePathBuf,
    turns: Vec<Turn>,
) -> Thread {
    Thread {
        id: id.to_string(),
        extra: None,
        session_id: ThreadId::new().to_string(),
        forked_from_id: forked_from_id.map(|thread_id| thread_id.to_string()),
        parent_thread_id: None,
        preview: "conversation preview".to_string(),
        ephemeral: false,
        history_mode: Default::default(),
        model_provider: "openai".to_string(),
        created_at: 1,
        updated_at: 2,
        recency_at: Some(2),
        status: ThreadStatus::Idle,
        path: None,
        cwd,
        cli_version: "0.0.0-test".to_string(),
        source: codex_app_server_protocol::SessionSource::Cli,
        thread_source: None,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: name.map(str::to_string),
        turns,
    }
}

#[tokio::test]
async fn background_fork_dispatches_boundary_and_hydrates_parent_title() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config = ConfigBuilder::default()
        .codex_home(temp_dir.path().to_path_buf())
        .build()
        .await
        .expect("config should build");
    let source_thread_id = ThreadId::new();
    let child_thread_id = ThreadId::new();
    let before_turn_id = "turn-before-branch";
    let turns = vec![Turn {
        id: "turn-kept".to_string(),
        items: Vec::new(),
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: Some(1),
        completed_at: Some(2),
        duration_ms: Some(1_000),
    }];
    let child_thread = thread(
        child_thread_id,
        Some(source_thread_id),
        Some("Forked conversation"),
        config.cwd.clone(),
        turns.clone(),
    );
    let parent_thread = thread(
        source_thread_id,
        None,
        Some("Source conversation"),
        config.cwd.clone(),
        Vec::new(),
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let request_handle = FakeBackgroundRequestHandle {
        requests: Arc::clone(&requests),
        fork_response: ThreadForkResponse {
            thread: child_thread,
            model: "gpt-5.4".to_string(),
            model_provider: "openai".to_string(),
            service_tier: None,
            cwd: config.cwd.clone(),
            runtime_workspace_roots: vec![config.cwd.clone()],
            instruction_sources: Vec::new(),
            approval_policy: AskForApproval::Never,
            approvals_reviewer: ApprovalsReviewer::User,
            sandbox: SandboxPolicy::ReadOnly {
                network_access: false,
            },
            active_permission_profile: None,
            reasoning_effort: None,
            multi_agent_mode: Default::default(),
        },
        read_response: ThreadReadResponse {
            thread: parent_thread,
        },
    };
    let expected_fork_params = ThreadForkParams {
        last_turn_id: None,
        before_turn_id: Some(before_turn_id.to_string()),
        defer_goal_continuation: true,
        ..thread_fork_params_from_config(
            config.clone(),
            source_thread_id,
            ThreadParamsMode::Embedded,
            /*remote_cwd_override*/ None,
        )
    };

    let started = fork_thread_before_turn(BackgroundThreadFork {
        request_handle,
        config: config.clone(),
        session_config: config.clone(),
        thread_id: source_thread_id,
        before_turn_id: before_turn_id.to_string(),
        goal_continuation: ForkGoalContinuation::DeferUntilNextTurn,
        thread_params_mode: ThreadParamsMode::Embedded,
        remote_cwd_override: None,
        request_id: RequestId::String("background-fork-test".to_string()),
    })
    .await
    .expect("background fork should complete");

    assert_eq!(
        *requests.lock().expect("requests mutex"),
        vec![
            ObservedRequest::Fork(expected_fork_params),
            ObservedRequest::Read(ThreadReadParams {
                thread_id: source_thread_id.to_string(),
                include_turns: false,
            }),
        ]
    );
    assert_eq!(started.session.thread_id, child_thread_id);
    assert_eq!(started.session.forked_from_id, Some(source_thread_id));
    assert_eq!(
        started.session.fork_parent_title.as_deref(),
        Some("Source conversation")
    );
    assert_eq!(
        started.session.thread_name.as_deref(),
        Some("Forked conversation")
    );
    assert_eq!(started.session.model, "gpt-5.4");
    assert_eq!(started.session.model_provider_id, "openai");
    assert_eq!(started.session.cwd, config.cwd);
    assert_eq!(started.turns, turns);
}
