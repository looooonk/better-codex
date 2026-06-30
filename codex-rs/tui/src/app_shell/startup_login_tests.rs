use super::*;
use codex_app_server_protocol::AccountLoginCompletedNotification;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn press(code: KeyCode, state: &mut LoginOnboardingState) -> LoginKeyAction {
    handle_login_key(KeyEvent::new(code, KeyModifiers::NONE), state)
}

#[test]
fn login_selection_respects_forced_login_method() {
    let chatgpt_state = LoginOnboardingState::new(Some(ForcedLoginMethod::Chatgpt));
    assert_eq!(
        chatgpt_state.choices(),
        vec![LoginSelection::ChatGptDeviceCode, LoginSelection::Exit]
    );

    let api_state = LoginOnboardingState::new(Some(ForcedLoginMethod::Api));
    assert_eq!(
        api_state.choices(),
        vec![LoginSelection::ApiKey, LoginSelection::Exit]
    );
}

#[test]
fn login_keys_open_api_entry_and_capture_secret_text() {
    let mut state = LoginOnboardingState::new(None);

    assert_eq!(press(KeyCode::Down, &mut state), LoginKeyAction::Redraw);
    assert_eq!(state.selected(), LoginSelection::ApiKey);
    assert_eq!(press(KeyCode::Enter, &mut state), LoginKeyAction::Redraw);
    assert!(matches!(state.mode, LoginMode::ApiKeyEntry));

    for ch in "sk-test".chars() {
        assert_eq!(press(KeyCode::Char(ch), &mut state), LoginKeyAction::Redraw);
    }
    assert_eq!(state.api_key_draft, "sk-test");
    assert_eq!(
        press(KeyCode::Backspace, &mut state),
        LoginKeyAction::Redraw
    );
    assert_eq!(state.api_key_draft, "sk-tes");
    assert_eq!(
        press(KeyCode::Enter, &mut state),
        LoginKeyAction::SubmitApiKey
    );
}

#[test]
fn device_code_completion_matches_active_login() {
    let mut state = LoginOnboardingState::new(None);
    state.mode = LoginMode::DeviceCode {
        login_id: Some("login-1".to_string()),
        verification_url: Some("https://auth.example.test/device".to_string()),
        user_code: Some("ABCD-EFGH".to_string()),
    };

    assert_eq!(
        state.receive_login_completed(AccountLoginCompletedNotification {
            login_id: Some("other".to_string()),
            success: true,
            error: None,
        }),
        None
    );
    assert_eq!(
        state.receive_login_completed(AccountLoginCompletedNotification {
            login_id: Some("login-1".to_string()),
            success: true,
            error: None,
        }),
        Some(LoginOnboardingOutcome::Continue)
    );
}

#[test]
fn login_onboarding_view_renders_native_auth_choices() {
    let state = LoginOnboardingState::new(None);
    let backend = TestBackend::new(/*width*/ 100, /*height*/ 28);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    terminal
        .draw(|frame| {
            LoginOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut());
        })
        .expect("draw login onboarding");
    insta::assert_snapshot!(terminal.backend().to_string());
}
