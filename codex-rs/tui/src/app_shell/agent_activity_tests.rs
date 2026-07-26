use super::*;
use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::TurnStatus;
use codex_utils_path_uri::LegacyAppPathString;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn subagent_signals_set_hierarchy_and_lifecycle() {
    let mut state = AgentActivityState::default();
    for (id, kind) in [
        ("signal-1", SubAgentActivityKind::Started),
        ("signal-2", SubAgentActivityKind::Interacted),
        ("signal-3", SubAgentActivityKind::Interrupted),
    ] {
        assert!(state.reduce_completed(&subagent_item(
            id,
            kind,
            "agent-1",
            "/root/researcher/worker",
        )));
    }

    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(agent.thread_id, "agent-1");
    assert_eq!(agent.display_name(), "worker");
    assert_eq!(agent.path.as_deref(), Some("/root/researcher/worker"));
    assert_eq!(agent.parent_path.as_deref(), Some("/root/researcher"));
    assert_eq!(agent.depth, Some(2));
    assert_eq!(agent.status, AgentLifecycleStatus::Interrupted);
    assert_eq!(
        agent
            .timeline
            .iter()
            .map(AgentTimelineEntry::label)
            .collect::<Vec<_>>(),
        vec!["agent started", "agent interacted", "agent interrupted"]
    );
}

#[test]
fn collab_spawn_retains_metadata_and_upserts_phase() {
    let mut state = AgentActivityState::default();
    let started = collab_item(
        "spawn-1",
        CollabAgentTool::SpawnAgent,
        CollabAgentToolCallStatus::InProgress,
        &["agent-1"],
        Some("  Audit   the UI flow.\nThen report.  "),
        Some("gpt-5-codex"),
        Some(ReasoningEffort::High),
        HashMap::new(),
    );
    assert!(state.reduce_started(&started));

    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(agent.status, AgentLifecycleStatus::PendingInit);
    assert_eq!(
        agent.task_summary.as_deref(),
        Some("Audit the UI flow. Then report.")
    );
    assert_eq!(agent.model.as_deref(), Some("gpt-5-codex"));
    assert_eq!(
        agent.reasoning_effort.as_ref(),
        Some(&ReasoningEffort::High)
    );
    assert_eq!(
        agent
            .timeline
            .iter()
            .map(AgentTimelineEntry::label)
            .collect::<Vec<_>>(),
        vec!["spawning agent"]
    );

    let completed = collab_item(
        "spawn-1",
        CollabAgentTool::SpawnAgent,
        CollabAgentToolCallStatus::Completed,
        &["agent-1"],
        Some("Audit the UI flow. Then report."),
        Some("gpt-5-codex"),
        Some(ReasoningEffort::High),
        states(&[(
            "agent-1",
            CollabAgentStatus::Running,
            Some("Inspecting interaction paths"),
        )]),
    );
    assert!(state.reduce_completed(&completed));

    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(agent.status, AgentLifecycleStatus::Running);
    assert_eq!(
        agent.latest_message.as_deref(),
        Some("Inspecting interaction paths")
    );
    let timeline = agent.timeline.iter().collect::<Vec<_>>();
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].label(), "agent spawned");
    assert_eq!(
        &timeline[0].event,
        &AgentTimelineEvent::Collaboration {
            tool: CollabAgentTool::SpawnAgent,
            phase: AgentItemPhase::Completed,
            status: CollabAgentToolCallStatus::Completed,
        }
    );
}

#[test]
fn every_collab_agent_state_maps_without_losing_counts() {
    use AgentLifecycleStatus as Lifecycle;
    use CollabAgentStatus as ProtocolStatus;

    let mappings = [
        (
            "agent-7",
            ProtocolStatus::PendingInit,
            Lifecycle::PendingInit,
        ),
        ("agent-6", ProtocolStatus::Running, Lifecycle::Running),
        (
            "agent-5",
            ProtocolStatus::Interrupted,
            Lifecycle::Interrupted,
        ),
        ("agent-4", ProtocolStatus::Completed, Lifecycle::Completed),
        ("agent-3", ProtocolStatus::Errored, Lifecycle::Errored),
        ("agent-2", ProtocolStatus::Shutdown, Lifecycle::Shutdown),
        ("agent-1", ProtocolStatus::NotFound, Lifecycle::NotFound),
    ];
    let agent_states = mappings
        .iter()
        .map(|(id, status, _)| {
            (
                (*id).to_string(),
                CollabAgentState {
                    status: status.clone(),
                    message: None,
                },
            )
        })
        .collect();
    let item = collab_item(
        "wait-1",
        CollabAgentTool::Wait,
        CollabAgentToolCallStatus::Completed,
        &[],
        None,
        None,
        None,
        agent_states,
    );
    let mut state = AgentActivityState::default();
    assert!(state.reduce_completed(&item));

    assert_eq!(
        state
            .ordered_agents()
            .into_iter()
            .map(|agent| (agent.thread_id.as_str(), agent.status))
            .collect::<Vec<_>>(),
        mappings
            .iter()
            .rev()
            .map(|(id, _, status)| (*id, *status))
            .collect::<Vec<_>>()
    );
    let counts = AgentActivityCounts {
        total: 7,
        active: 2,
        interrupted: 1,
        completed: 2,
        failed: 2,
    };
    assert_eq!(state.counts(), counts);
}

#[test]
fn path_order_and_selection_are_deterministic() {
    let mut state = AgentActivityState::default();
    for (id, path) in [
        ("z-thread", "/root/zeta"),
        ("child-thread", "/root/alpha/child"),
        ("a-thread", "/root/alpha"),
    ] {
        state.reduce_completed(&subagent_item(
            &format!("signal-{id}"),
            SubAgentActivityKind::Started,
            id,
            path,
        ));
    }

    assert_eq!(
        state
            .ordered_agents()
            .into_iter()
            .map(|agent| agent.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["a-thread", "child-thread", "z-thread"]
    );
    assert_eq!(state.selected_thread_id(), Some("z-thread"));
    state.move_selection_up();
    assert_eq!(state.selected_thread_id(), Some("child-thread"));
    state.move_selection_up();
    state.move_selection_up();
    assert_eq!(state.selected_thread_id(), Some("a-thread"));
    state.move_selection_down();
    assert_eq!(state.selected_thread_id(), Some("child-thread"));
    assert!(state.select_thread("z-thread"));
    assert_eq!(state.selected_thread_id(), Some("z-thread"));
    assert!(!state.select_thread("missing"));
}

#[test]
fn root_thread_is_excluded_from_agent_activity() {
    let mut state = AgentActivityState::for_root("root-thread");
    assert!(state.reduce_completed(&collab_item(
        "wait-1",
        CollabAgentTool::Wait,
        CollabAgentToolCallStatus::Completed,
        &["root-thread", "child-thread"],
        /*prompt*/ None,
        /*model*/ None,
        /*effort*/ None,
        HashMap::new(),
    )));
    assert!(state.reduce_completed(&subagent_item(
        "root-activity",
        SubAgentActivityKind::Interacted,
        "root-thread",
        "/root",
    )));

    assert_eq!(
        state
            .ordered_agents()
            .into_iter()
            .map(|agent| agent.thread_id.as_str())
            .collect::<Vec<_>>(),
        vec!["child-thread"]
    );
    assert_eq!(state.counts().total, 1);
}

#[test]
fn timelines_and_agent_collection_are_bounded() {
    let mut timeline_state = AgentActivityState::default();
    for index in 0..MAX_AGENT_TIMELINE_ENTRIES + 3 {
        timeline_state.reduce_completed(&subagent_item(
            &format!("signal-{index}"),
            SubAgentActivityKind::Interacted,
            "timeline-agent",
            "/root/timeline_agent",
        ));
    }
    let timeline = timeline_state
        .agent("timeline-agent")
        .expect("agent should be tracked")
        .timeline
        .iter()
        .map(|entry| entry.item_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(timeline.len(), MAX_AGENT_TIMELINE_ENTRIES);
    assert_eq!(timeline.first().copied(), Some("signal-3"));

    let mut agent_state = AgentActivityState::default();
    for index in 0..MAX_TRACKED_AGENTS + 3 {
        agent_state.reduce_completed(&subagent_item(
            &format!("signal-{index}"),
            SubAgentActivityKind::Started,
            &format!("thread-{index:03}"),
            &format!("/root/agent_{index}"),
        ));
    }
    assert_eq!(agent_state.counts().total, MAX_TRACKED_AGENTS);
    assert!(agent_state.agent("thread-000").is_none());
    assert!(agent_state.agent("thread-003").is_some());
    assert!(
        agent_state
            .agent(&format!("thread-{:03}", MAX_TRACKED_AGENTS + 2))
            .is_some()
    );
}

#[test]
fn known_child_message_and_reasoning_updates_upsert_typed_activity() {
    let mut state = tracked_agent();
    assert!(state.is_known_thread("agent-1"));
    assert!(!state.is_known_thread("missing"));
    assert!(!state.record_child_progress(
        "missing",
        "message-1",
        AgentChildEvent::Message,
        "ignored",
    ));

    assert!(state.record_child_progress(
        "agent-1",
        "message-1",
        AgentChildEvent::Message,
        "Inspecting ",
    ));
    assert!(state.record_child_progress(
        "agent-1",
        "message-1",
        AgentChildEvent::Message,
        "the layout",
    ));
    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(
        agent.latest_message.as_deref(),
        Some("Inspecting the layout")
    );
    assert_eq!(
        agent.timeline.back().map(AgentTimelineEntry::label),
        Some("message: Inspecting the layout".to_string())
    );

    assert!(state.record_child_item(
        "agent-1",
        &ThreadItem::AgentMessage {
            id: "message-1".to_string(),
            text: "Layout audit complete".to_string(),
            phase: None,
            memory_citation: None,
        },
        AgentItemPhase::Completed,
    ));
    assert!(state.record_child_progress(
        "agent-1",
        "reasoning-1",
        AgentChildEvent::Reasoning,
        "Checking focus ownership",
    ));
    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(
        agent.latest_message.as_deref(),
        Some("Checking focus ownership")
    );
    assert_eq!(
        &agent.timeline[1].event,
        &AgentTimelineEvent::ChildItem {
            event: AgentChildEvent::Message,
            phase: AgentItemPhase::Completed,
        }
    );
    assert_eq!(agent.timeline.len(), 3);
}

#[test]
fn child_command_and_output_summaries_replace_in_place() {
    let mut state = tracked_agent();
    let started = command_item(CommandExecutionStatus::InProgress, /*output*/ None);
    assert!(state.record_child_item("agent-1", &started, AgentItemPhase::Started));
    assert!(state.record_child_progress(
        "agent-1",
        "exec-1",
        AgentChildEvent::Output,
        "tests 50%\n",
    ));
    assert!(state.record_child_progress(
        "agent-1",
        "exec-1",
        AgentChildEvent::Output,
        "tests 100%",
    ));

    let completed = command_item(CommandExecutionStatus::Completed, Some("12 tests passed\n"));
    assert!(state.record_child_item("agent-1", &completed, AgentItemPhase::Completed));
    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(agent.latest_message.as_deref(), Some("12 tests passed"));
    assert_eq!(agent.timeline.len(), 2);
    assert_eq!(
        agent.timeline.back().map(AgentTimelineEntry::label),
        Some("command completed: 12 tests passed".to_string())
    );
}

#[test]
fn child_progress_text_has_a_hard_cap() {
    let mut state = tracked_agent();
    state.record_child_progress(
        "agent-1",
        "output-1",
        AgentChildEvent::Output,
        &"x".repeat(MAX_LATEST_MESSAGE_CHARS + 20),
    );
    state.record_child_progress("agent-1", "output-1", AgentChildEvent::Output, "tail");

    let latest = state
        .agent("agent-1")
        .and_then(|agent| agent.latest_message.as_deref())
        .expect("progress should update latest message");
    assert!(latest.chars().count() <= MAX_LATEST_MESSAGE_CHARS);
    assert!(latest.ends_with("tail"));
}

#[test]
fn child_turns_and_errors_update_typed_lifecycle_activity() {
    let mut state = tracked_agent();

    assert!(state.record_child_error("agent-1", "turn-1", "temporary failure", true));
    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(agent.status, AgentLifecycleStatus::Running);
    assert_eq!(
        agent.timeline.back().map(AgentTimelineEntry::label),
        Some("agent retrying: temporary failure".to_string())
    );

    assert!(state.record_child_turn("agent-1", "turn-1", &TurnStatus::Interrupted));
    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(agent.status, AgentLifecycleStatus::Interrupted);
    assert_eq!(
        agent.timeline.back().map(AgentTimelineEntry::label),
        Some("agent interrupted".to_string())
    );

    assert!(state.record_child_error("agent-1", "turn-2", "fatal failure", false));
    let agent = state.agent("agent-1").expect("agent should be tracked");
    assert_eq!(agent.status, AgentLifecycleStatus::Errored);
    assert_eq!(agent.latest_message.as_deref(), Some("fatal failure"));
}

fn subagent_item(id: &str, kind: SubAgentActivityKind, thread_id: &str, path: &str) -> ThreadItem {
    ThreadItem::SubAgentActivity {
        id: id.to_string(),
        kind,
        agent_thread_id: thread_id.to_string(),
        agent_path: path.to_string(),
    }
}

fn tracked_agent() -> AgentActivityState {
    let mut state = AgentActivityState::default();
    state.reduce_completed(&subagent_item(
        "activity-1",
        SubAgentActivityKind::Started,
        "agent-1",
        "/root/worker",
    ));
    state
}

fn command_item(status: CommandExecutionStatus, output: Option<&str>) -> ThreadItem {
    ThreadItem::CommandExecution {
        id: "exec-1".to_string(),
        command: "just test -p codex-tui".to_string(),
        cwd: LegacyAppPathString::from_path(Path::new("/workspace")),
        process_id: None,
        source: CommandExecutionSource::Agent,
        status,
        command_actions: Vec::new(),
        aggregated_output: output.map(ToString::to_string),
        exit_code: None,
        duration_ms: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn collab_item(
    id: &str,
    tool: CollabAgentTool,
    status: CollabAgentToolCallStatus,
    receiver_ids: &[&str],
    prompt: Option<&str>,
    model: Option<&str>,
    effort: Option<ReasoningEffort>,
    agents_states: HashMap<String, CollabAgentState>,
) -> ThreadItem {
    ThreadItem::CollabAgentToolCall {
        id: id.to_string(),
        tool,
        status,
        sender_thread_id: "root-thread".to_string(),
        receiver_thread_ids: receiver_ids.iter().map(ToString::to_string).collect(),
        prompt: prompt.map(ToString::to_string),
        model: model.map(ToString::to_string),
        reasoning_effort: effort,
        agents_states,
    }
}

fn states(
    entries: &[(&str, CollabAgentStatus, Option<&str>)],
) -> HashMap<String, CollabAgentState> {
    entries
        .iter()
        .map(|(id, status, message)| {
            (
                (*id).to_string(),
                CollabAgentState {
                    status: status.clone(),
                    message: message.map(ToString::to_string),
                },
            )
        })
        .collect()
}
