use super::*;
use crate::app_shell::render::ShellView;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn selects_daemon_package_only_for_available_idle_sessions() {
    for (command, source) in [
        ("update latest", DaemonUpdateSource::PublicStable),
        ("update from-cli", DaemonUpdateSource::ThisCli),
    ] {
        let mut shell = ShellState::snapshot_fixture();
        shell.resume_cwd_runtime.daemon_update_available = true;
        shell.active_turn_id = None;
        shell.run_daemon_command(command);
        assert_eq!(
            shell.pending_update_action,
            Some(UpdateAction::Daemon(source))
        );
    }
}

#[test]
fn active_or_queued_work_prevents_daemon_updates() {
    for queued in [false, true] {
        let mut shell = ShellState::snapshot_fixture();
        shell.resume_cwd_runtime.daemon_update_available = true;
        shell.active_turn_id = None;
        if queued {
            shell.composer.set_text("queued request");
            assert!(shell.composer.queue_current_message());
        } else {
            shell.active_turn_id = Some("active".to_string());
        }
        shell.run_daemon_command("update latest");
        assert_eq!(shell.pending_update_action, None);
        assert_eq!(
            shell.transcript.back().map(|line| line.text.as_str()),
            Some("finish active work before updating the background server"),
        );
    }
}

#[test]
fn unavailable_daemon_and_invalid_commands_do_not_schedule_updates() {
    let mut shell = ShellState::snapshot_fixture();
    shell.resume_cwd_runtime.daemon_update_available = false;
    shell.run_daemon_command("update latest");
    assert_eq!(shell.pending_update_action, None);
    assert_eq!(
        shell.transcript.back().map(|line| line.text.as_str()),
        Some("daemon updates require a connection to the local background server"),
    );
    shell.resume_cwd_runtime.daemon_update_available = true;
    shell.run_daemon_command("update unexpected");
    assert_eq!(shell.pending_update_action, None);
    assert_eq!(
        shell.transcript.back().map(|line| line.text.as_str()),
        Some("usage: /daemon [status|update latest|update from-cli]"),
    );
}

#[test]
fn daemon_status_shows_update_choices() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.resume_cwd_runtime.daemon_update_available = true;
    shell.run_daemon_command("status");
    assert_eq!(shell.pending_update_action, None);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 22,
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
