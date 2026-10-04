use super::*;
use crate::app_shell::render::ShellView;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn worktree_creation_requires_an_available_idle_session() {
    let mut shell = ShellState::snapshot_fixture();
    assert!(shell.check_worktree_idle().is_err());
    shell.composer.set_text("");
    assert!(shell.check_worktree_idle().is_ok());
    shell.active_turn_id = Some("running-turn".to_string());
    assert!(shell.check_worktree_idle().is_err());
    shell.active_turn_id = None;
    shell.can_accept_direct_input = false;
    assert!(shell.check_worktree_idle().is_err());
}

#[tokio::test]
async fn preparing_a_steer_blocks_worktree_and_session_transitions() {
    let mut shell = ShellState::snapshot_fixture();
    shell.composer.clear();
    shell.start_backend_action(
        ActionGroup::TurnSteer,
        "reading IDE context",
        std::future::pending::<super::super::backend_actions::BackendActionResult>(),
    );
    assert!(shell.check_worktree_idle().is_err());
    assert!(shell.block_session_switch_if_busy());
}

#[test]
fn pending_draft_explains_why_worktree_creation_is_blocked() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.dashboard_visible = false;
    let error = shell.check_worktree_idle().unwrap_err();
    shell.push_error(error.to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 25,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let lines = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    insta::assert_snapshot!(lines.join("\n"));
}
