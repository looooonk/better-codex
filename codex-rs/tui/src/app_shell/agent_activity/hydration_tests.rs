use super::*;
use crate::app_shell::agent_activity::AgentChildEvent;
use crate::app_shell::agent_activity::AgentLifecycleStatus;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SessionSource;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadSource;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnError;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::AgentPath;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SubAgentSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn nested_thread_histories_restore_hierarchy_timeline_and_lifecycle() {
    let root_id = thread_id("01900000-0000-7000-8000-000000000001");
    let alpha_id = thread_id("01900000-0000-7000-8000-000000000002");
    let child_id = thread_id("01900000-0000-7000-8000-000000000003");
    let session_id = root_id.to_string();
    let mut state = AgentActivityState::default();
    state.reduce_completed(&activity("root-spawn", alpha_id, "/root/alpha"));

    state.hydrate_threads(vec![
        thread(
            alpha_id,
            root_id,
            &session_id,
            "/root/alpha",
            turn(
                "alpha-turn",
                vec![
                    message("alpha-message", "delegating nested work"),
                    activity("alpha-spawn", child_id, "/root/alpha/child"),
                ],
                TurnStatus::Completed,
                /*error*/ None,
            ),
        ),
        thread(
            child_id,
            alpha_id,
            &session_id,
            "/root/alpha/child",
            turn(
                "child-turn",
                vec![message("child-message", "nested work complete")],
                TurnStatus::Completed,
                /*error*/ None,
            ),
        ),
    ]);

    let agents = state.ordered_agents();
    assert_eq!(
        agents
            .iter()
            .map(|agent| (
                agent.thread_id.clone(),
                agent.path.as_ref().map(|path| String::from(path.clone())),
                agent.status,
                agent.latest_message.clone(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                alpha_id.to_string(),
                Some("/root/alpha".to_string()),
                AgentLifecycleStatus::Completed,
                Some("delegating nested work".to_string()),
            ),
            (
                child_id.to_string(),
                Some("/root/alpha/child".to_string()),
                AgentLifecycleStatus::Completed,
                Some("nested work complete".to_string()),
            ),
        ]
    );
    assert_eq!(
        state
            .agent(&child_id.to_string())
            .expect("child should be hydrated")
            .timeline
            .iter()
            .map(super::super::timeline::AgentTimelineEntry::label)
            .collect::<Vec<_>>(),
        vec![
            "agent started",
            "message completed: nested work complete",
            "agent completed",
        ]
    );
}

#[test]
fn failed_turn_preserves_error_detail() {
    let root_id = thread_id("01900000-0000-7000-8000-000000000011");
    let child_id = thread_id("01900000-0000-7000-8000-000000000012");
    let mut state = AgentActivityState::default();

    state.hydrate_threads(vec![thread(
        child_id,
        root_id,
        &root_id.to_string(),
        "/root/failing",
        turn(
            "failed-turn",
            Vec::new(),
            TurnStatus::Failed,
            Some("backend failed"),
        ),
    )]);

    let agent = state
        .agent(&child_id.to_string())
        .expect("agent should exist");
    assert_eq!(agent.status, AgentLifecycleStatus::Errored);
    assert_eq!(agent.latest_message.as_deref(), Some("backend failed"));
    assert_eq!(
        agent
            .timeline
            .iter()
            .map(super::super::timeline::AgentTimelineEntry::label)
            .collect::<Vec<_>>(),
        vec!["agent failed: backend failed"]
    );
}

#[test]
fn collaboration_snapshot_remains_authoritative_over_replayed_history() {
    let root_id = thread_id("01900000-0000-7000-8000-000000000021");
    let child_id = thread_id("01900000-0000-7000-8000-000000000022");
    let mut state = AgentActivityState::default();
    state.reduce_completed(&ThreadItem::CollabAgentToolCall {
        id: "root-status".to_string(),
        tool: CollabAgentTool::Wait,
        status: CollabAgentToolCallStatus::Completed,
        sender_thread_id: root_id.to_string(),
        receiver_thread_ids: vec![child_id.to_string()],
        prompt: None,
        model: None,
        reasoning_effort: None,
        agents_states: HashMap::from([(
            child_id.to_string(),
            CollabAgentState {
                status: CollabAgentStatus::Running,
                message: Some("still processing".to_string()),
            },
        )]),
    });

    state.hydrate_threads(vec![thread(
        child_id,
        root_id,
        &root_id.to_string(),
        "/root/active",
        turn(
            "historical-turn",
            vec![message("old-message", "older completed result")],
            TurnStatus::Completed,
            /*error*/ None,
        ),
    )]);

    let agent = state
        .agent(&child_id.to_string())
        .expect("agent should exist");
    assert_eq!(agent.status, AgentLifecycleStatus::Running);
    assert_eq!(agent.latest_message.as_deref(), Some("still processing"));
}

#[test]
fn late_history_keeps_newer_live_state_and_timeline_at_the_end() {
    let root_id = thread_id("01900000-0000-7000-8000-000000000031");
    let child_id = thread_id("01900000-0000-7000-8000-000000000032");
    let historical = thread(
        child_id,
        root_id,
        &root_id.to_string(),
        "/root/live",
        turn(
            "historical-turn",
            vec![message("old-message", "older completed result")],
            TurnStatus::Completed,
            /*error*/ None,
        ),
    );
    let mut metadata = historical.clone();
    metadata.turns.clear();
    let mut state = AgentActivityState::default();
    state.hydrate_threads(vec![metadata]);
    state.record_child_progress(
        &child_id.to_string(),
        "old-message",
        AgentChildEvent::Message,
        "new live result",
    );
    state.mark_live_thread(&child_id.to_string());

    state.hydrate_threads(vec![historical]);

    let agent = state
        .agent(&child_id.to_string())
        .expect("agent should remain hydrated");
    assert_eq!(agent.status, AgentLifecycleStatus::Running);
    assert_eq!(agent.latest_message.as_deref(), Some("new live result"));
    assert_eq!(
        agent
            .timeline
            .iter()
            .map(|entry| entry.item_id.as_str())
            .collect::<Vec<_>>(),
        vec!["historical-turn", "old-message"]
    );
}

#[test]
fn parent_history_does_not_overwrite_a_live_nested_child() {
    let root_id = thread_id("01900000-0000-7000-8000-000000000041");
    let parent_id = thread_id("01900000-0000-7000-8000-000000000042");
    let child_id = thread_id("01900000-0000-7000-8000-000000000043");
    let mut state = AgentActivityState::default();
    state.reduce_completed(&activity("child-start", child_id, "/root/parent/child"));
    state.record_child_progress(
        &child_id.to_string(),
        "live-message",
        AgentChildEvent::Message,
        "new live result",
    );
    state.mark_live_thread(&child_id.to_string());
    let live_child = state
        .agent(&child_id.to_string())
        .expect("live child should exist")
        .clone();
    let historical_collaboration = ThreadItem::CollabAgentToolCall {
        id: "historical-wait".to_string(),
        tool: CollabAgentTool::Wait,
        status: CollabAgentToolCallStatus::Completed,
        sender_thread_id: parent_id.to_string(),
        receiver_thread_ids: vec![child_id.to_string()],
        prompt: None,
        model: None,
        reasoning_effort: None,
        agents_states: HashMap::from([(
            child_id.to_string(),
            CollabAgentState {
                status: CollabAgentStatus::Completed,
                message: Some("old result".to_string()),
            },
        )]),
    };

    state.hydrate_threads(vec![thread(
        parent_id,
        root_id,
        &root_id.to_string(),
        "/root/parent",
        turn(
            "parent-turn",
            vec![historical_collaboration],
            TurnStatus::Completed,
            /*error*/ None,
        ),
    )]);

    assert_eq!(
        state
            .agent(&child_id.to_string())
            .expect("live child should remain"),
        &live_child
    );
}

fn thread(id: ThreadId, parent_id: ThreadId, session_id: &str, path: &str, turn: Turn) -> Thread {
    Thread {
        id: id.to_string(),
        extra: None,
        session_id: session_id.to_string(),
        forked_from_id: None,
        parent_thread_id: Some(parent_id.to_string()),
        preview: String::new(),
        ephemeral: false,
        history_mode: ThreadHistoryMode::Legacy,
        model_provider: "openai".to_string(),
        created_at: 1,
        updated_at: 2,
        recency_at: Some(2),
        status: ThreadStatus::NotLoaded,
        path: None,
        cwd: AbsolutePathBuf::from_absolute_path_checked("/workspace")
            .expect("workspace path should be absolute"),
        cli_version: "test".to_string(),
        source: SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id: parent_id,
            depth: 1,
            agent_path: Some(AgentPath::try_from(path).expect("path should be valid")),
            agent_nickname: None,
            agent_role: None,
        }),
        thread_source: Some(ThreadSource::Subagent),
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns: vec![turn],
    }
}

fn turn(id: &str, items: Vec<ThreadItem>, status: TurnStatus, error: Option<&str>) -> Turn {
    Turn {
        id: id.to_string(),
        items,
        items_view: TurnItemsView::default(),
        status,
        error: error.map(|message| TurnError {
            message: message.to_string(),
            codex_error_info: None,
            additional_details: None,
        }),
        started_at: Some(1),
        completed_at: Some(2),
        duration_ms: Some(1),
    }
}

fn activity(id: &str, thread_id: ThreadId, path: &str) -> ThreadItem {
    ThreadItem::SubAgentActivity {
        id: id.to_string(),
        kind: SubAgentActivityKind::Started,
        agent_thread_id: thread_id.to_string(),
        agent_path: path.to_string(),
    }
}

fn message(id: &str, text: &str) -> ThreadItem {
    ThreadItem::AgentMessage {
        id: id.to_string(),
        text: text.to_string(),
        phase: None,
        memory_citation: None,
    }
}

fn thread_id(id: &str) -> ThreadId {
    ThreadId::from_string(id).expect("thread id should be valid")
}
