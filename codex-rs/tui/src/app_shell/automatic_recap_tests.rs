use super::*;
use pretty_assertions::assert_eq;

#[test]
fn automatic_recap_requires_focus_loss_three_turns_and_a_quiet_half_hour() {
    let now = Instant::now();
    let mut state = AutomaticRecap::default();
    for _ in 0..3 {
        state.note_turn_finished(&TurnStatus::Completed, now);
    }
    assert!(!state.ready(now + RECAP_DELAY));
    state.note_focus(/*focused*/ false, now);
    state.note_focus(
        /*focused*/ false,
        now + Duration::from_secs(/*secs*/ 10),
    );
    assert!(!state.ready(now + RECAP_DELAY - Duration::from_secs(/*secs*/ 1)));
    assert!(state.ready(now + RECAP_DELAY));
    state.note_turn_finished(&TurnStatus::Failed, now + RECAP_DELAY);
    assert!(!state.ready(now + RECAP_DELAY));
    assert!(state.ready(now + RECAP_DELAY * 2));
    state.note_focus(/*focused*/ true, now + RECAP_DELAY * 2);
    assert!(!state.ready(now + RECAP_DELAY * 3));
}

#[test]
fn automatic_recaps_retry_once_and_require_two_more_completed_turns() {
    let now = Instant::now();
    let mut state = AutomaticRecap::default();
    state.note_focus(/*focused*/ false, now);
    for _ in 0..3 {
        state.note_turn_finished(&TurnStatus::Completed, now);
    }
    let due = now + RECAP_DELAY;
    state.mark_started();
    state.mark_failed(due);
    assert!(!state.ready(due));
    assert!(state.ready(due + RETRY_DELAY));
    state.mark_started();
    state.mark_failed(due + RETRY_DELAY);
    assert!(!state.ready(due + RETRY_DELAY * 2));
    state.mark_recapped();
    state.note_turn_finished(&TurnStatus::Completed, due);
    assert!(!state.ready(due + RECAP_DELAY));
    state.note_turn_finished(&TurnStatus::Completed, due);
    assert!(state.ready(due + RECAP_DELAY));
    state.enabled = false;
    assert!(!state.ready(due + RECAP_DELAY));
    state.reset();
    assert_eq!(
        (state.enabled, state.completed_turns, state.revision),
        (false, 0, 0)
    );
}

#[test]
fn disabling_automatic_recaps_keeps_manual_command_available() {
    use crate::app_shell::render::ShellView;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.transcript.clear();
    shell.complete_workspace_request(
        shell.thread_id,
        Ok(WorkspaceResponse::AutomaticRecap(false)),
    );
    assert!(!shell.automatic_recap.enabled);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let screen = buffer
        .content
        .chunks(100)
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("automatic_recap_disabled", screen);
}
