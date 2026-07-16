use super::*;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadItem;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

#[test]
fn renders_bounded_agent_hierarchy_and_selected_inspector() {
    let state = hierarchy_fixture();
    let mut lines = agent_activity_overview_lines(&state, /*width*/ 72);
    lines.extend(agent_activity_inspector_lines(
        &state, /*width*/ 72, /*line_budget*/ 20,
    ));
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(rendered, @"
    Agents 4  ● 1 active  ✓ 1 done  ! 1 interrupted  × 1 failed
      ● running  research
        └ ✓ completed  audit
      ! interrupted  test
    ›   └ × errored  failure
    Inspector  failure  × errored  · Unit tests failed
    Path  /root/test/failure
    Task  Inspect the TUI flow.
    Runtime  gpt-5-codex · high reasoning
    Latest  Unit tests failed
    Recent
      • agent spawned
      • agent started
    ");
}

#[test]
fn width_and_line_budget_bound_every_row() {
    let lines = agent_activity_inspector_lines(
        &hierarchy_fixture(),
        /*width*/ 24,
        /*line_budget*/ 7,
    );

    assert_eq!(lines.len(), 7);
    assert!(
        lines
            .iter()
            .all(|line| crate::line_truncation::line_width(line) <= 24)
    );
}

fn hierarchy_fixture() -> AgentActivityState {
    let mut state = AgentActivityState::default();
    for (id, path) in [
        ("running", "/root/research"),
        ("completed", "/root/research/audit"),
        ("interrupted", "/root/test"),
        ("errored", "/root/test/failure"),
    ] {
        state.reduce_completed(&ThreadItem::SubAgentActivity {
            id: format!("started-{id}"),
            kind: SubAgentActivityKind::Started,
            agent_thread_id: id.to_string(),
            agent_path: path.to_string(),
        });
    }
    let states = [
        ("running", CollabAgentStatus::Running, "Reviewing layouts"),
        ("completed", CollabAgentStatus::Completed, "Audit complete"),
        (
            "interrupted",
            CollabAgentStatus::Interrupted,
            "Stopped by user",
        ),
        ("errored", CollabAgentStatus::Errored, "Unit tests failed"),
    ]
    .into_iter()
    .map(|(id, status, message)| {
        (
            id.to_string(),
            CollabAgentState {
                status,
                message: Some(message.to_string()),
            },
        )
    })
    .collect::<HashMap<_, _>>();
    state.reduce_completed(&ThreadItem::CollabAgentToolCall {
        id: "spawn-agents".to_string(),
        tool: CollabAgentTool::SpawnAgent,
        status: CollabAgentToolCallStatus::Completed,
        sender_thread_id: "root-thread".to_string(),
        receiver_thread_ids: states.keys().cloned().collect(),
        prompt: Some("Inspect the TUI flow.".to_string()),
        model: Some("gpt-5-codex".to_string()),
        reasoning_effort: Some(ReasoningEffort::High),
        agents_states: states,
    });
    assert!(state.select_thread("errored"));
    state
}
