use super::*;
use crate::test_support::PathBufExt;
use crate::test_support::test_path_buf;
use codex_app_server_protocol::HookEventName;
use codex_app_server_protocol::HookHandlerType;
use codex_app_server_protocol::HookMetadata;
use codex_app_server_protocol::HookSource;
use codex_app_server_protocol::HookTrustStatus;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn hook(key: &str, trust_status: HookTrustStatus) -> HookMetadata {
    HookMetadata {
        key: key.to_string(),
        event_name: HookEventName::PreToolUse,
        handler_type: HookHandlerType::Command,
        is_managed: false,
        matcher: Some("Bash".to_string()),
        command: Some("/tmp/hook.sh".to_string()),
        timeout_sec: 30,
        status_message: None,
        source_path: test_path_buf("/tmp/hooks.json").abs(),
        source: HookSource::User,
        plugin_id: None,
        display_order: 0,
        enabled: false,
        current_hash: format!("sha256:{key}"),
        trust_status,
    }
}

fn entry() -> HooksListEntry {
    HooksListEntry {
        cwd: test_path_buf("/tmp"),
        hooks: vec![
            hook("path:new", HookTrustStatus::Untrusted),
            hook("path:changed", HookTrustStatus::Modified),
        ],
        warnings: Vec::new(),
        errors: Vec::new(),
    }
}

fn press(code: KeyCode, state: &mut StartupHooksReviewState) -> StartupHooksReviewKeyAction {
    handle_startup_hooks_key(
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
        state,
    )
}

#[test]
fn bypass_hook_trust_suppresses_startup_review() {
    assert!(!review_is_needed(/*bypass_hook_trust*/ true, &entry()));
}

#[test]
fn untrusted_hooks_need_review_without_bypass() {
    assert!(review_is_needed(/*bypass_hook_trust*/ false, &entry()));
}

#[test]
fn startup_hook_review_keys_move_between_choices() {
    let mut state = StartupHooksReviewState::new(entry());

    assert_eq!(
        press(KeyCode::Down, &mut state),
        StartupHooksReviewKeyAction::Redraw
    );
    assert_eq!(
        state.selected(),
        StartupHooksReviewSelection::TrustAllAndContinue
    );
    assert_eq!(
        press(KeyCode::Char('3'), &mut state),
        StartupHooksReviewKeyAction::Redraw
    );
    assert_eq!(
        state.selected(),
        StartupHooksReviewSelection::ContinueWithoutTrusting
    );
    assert_eq!(
        press(KeyCode::Enter, &mut state),
        StartupHooksReviewKeyAction::Continue
    );
}

#[test]
fn startup_hooks_review_view_renders_native_choices() {
    let state = StartupHooksReviewState::new(entry());
    let backend = TestBackend::new(/*width*/ 100, /*height*/ 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    terminal
        .draw(|frame| {
            StartupHooksReviewView { state: &state }.render(frame.area(), frame.buffer_mut());
        })
        .expect("draw startup hooks review");
    insta::assert_snapshot!(terminal.backend().to_string());
}
