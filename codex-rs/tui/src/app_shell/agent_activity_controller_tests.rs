use super::*;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadItem;
use pretty_assertions::assert_eq;

#[test]
fn navigation_requires_the_visible_focused_agents_route() {
    let mut shell = agent_shell();
    shell.agents_focused = false;
    assert!(!shell.handle_agent_activity_key(key(KeyCode::Down)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-a"));

    shell.agents_focused = true;
    shell.dashboard_visible = false;
    assert!(!shell.handle_agent_activity_key(key(KeyCode::Down)));
    shell.dashboard_visible = true;
    shell.dashboard_route = DashboardRoute::Sessions;
    assert!(!shell.handle_agent_activity_key(key(KeyCode::Down)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-a"));
}

#[test]
fn escape_returns_focus_to_the_composer() {
    let mut shell = agent_shell();

    assert!(shell.handle_agent_activity_key(key(KeyCode::Esc)));
    assert!(!shell.agents_focused);
    assert_eq!(shell.dashboard_route, DashboardRoute::Agents);
    assert!(shell.dashboard_visible);
}

#[test]
fn arrows_and_vim_keys_move_and_clamp_selection() {
    let mut shell = agent_shell();

    assert!(shell.handle_agent_activity_key(key(KeyCode::Down)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-b"));
    assert!(shell.handle_agent_activity_key(key(KeyCode::Char('j'))));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-c"));
    assert!(shell.handle_agent_activity_key(key(KeyCode::Down)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-c"));

    assert!(shell.handle_agent_activity_key(key(KeyCode::Up)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-b"));
    assert!(shell.handle_agent_activity_key(key(KeyCode::Char('k'))));
    assert!(shell.handle_agent_activity_key(key(KeyCode::Up)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-a"));
}

#[test]
fn home_end_and_vim_edges_select_first_and_last() {
    let mut shell = agent_shell();

    assert!(shell.handle_agent_activity_key(key(KeyCode::End)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-c"));
    assert!(shell.handle_agent_activity_key(key(KeyCode::Home)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-a"));
    assert!(
        shell.handle_agent_activity_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT,))
    );
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-c"));
    assert!(shell.handle_agent_activity_key(key(KeyCode::Char('g'))));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-a"));
}

#[test]
fn page_keys_move_multiple_agents_for_mouse_wheel_navigation() {
    let mut shell = agent_shell();

    assert!(shell.handle_agent_activity_key(key(KeyCode::PageDown)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-c"));
    assert!(shell.handle_agent_activity_key(key(KeyCode::PageUp)));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-a"));
}

#[test]
fn unrelated_or_modified_keys_are_left_for_other_controllers() {
    let mut shell = agent_shell();

    assert!(!shell.handle_agent_activity_key(key(KeyCode::Char('x'))));
    assert!(
        !shell.handle_agent_activity_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL,))
    );
    assert!(!shell.handle_agent_activity_key(KeyEvent::new_with_kind(
        KeyCode::Down,
        KeyModifiers::NONE,
        KeyEventKind::Release,
    )));
    assert_eq!(shell.agent_activity.selected_thread_id(), Some("agent-a"));
    assert!(shell.agents_focused);
}

fn agent_shell() -> ShellState {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = true;
    shell.dashboard_route = DashboardRoute::Agents;
    shell.agents_focused = true;
    shell.agent_activity = Default::default();
    for (thread_id, path) in [
        ("agent-c", "/root/charlie"),
        ("agent-b", "/root/bravo"),
        ("agent-a", "/root/alpha"),
    ] {
        shell
            .agent_activity
            .reduce_completed(&ThreadItem::SubAgentActivity {
                id: format!("activity-{thread_id}"),
                kind: SubAgentActivityKind::Started,
                agent_thread_id: thread_id.to_string(),
                agent_path: path.to_string(),
            });
    }
    shell.agent_activity.select_thread("agent-a");
    shell
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
