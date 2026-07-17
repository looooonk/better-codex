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
    let mut state = LoginOnboardingState::new(/*forced_login_method*/ None);

    assert_eq!(press(KeyCode::Down, &mut state), LoginKeyAction::Redraw);
    assert_eq!(state.selected(), LoginSelection::ApiKey);
    assert_eq!(press(KeyCode::Enter, &mut state), LoginKeyAction::Redraw);
    assert!(matches!(state.mode, LoginMode::ApiKeyEntry));

    for ch in "sk-test".chars() {
        assert_eq!(press(KeyCode::Char(ch), &mut state), LoginKeyAction::Redraw);
    }
    assert_eq!(state.api_key_draft.text(), "sk-test");
    assert_eq!(
        press(KeyCode::Backspace, &mut state),
        LoginKeyAction::Redraw
    );
    assert_eq!(state.api_key_draft.text(), "sk-tes");
    assert_eq!(
        press(KeyCode::Enter, &mut state),
        LoginKeyAction::SubmitApiKey
    );
}

#[test]
fn api_key_entry_uses_shared_cursor_shortcuts() {
    let mut state = LoginOnboardingState::new(/*forced_login_method*/ None);
    state.mode = LoginMode::ApiKeyEntry;
    state.api_key_draft.set_text("sk-alpha-beta");

    assert_eq!(
        handle_login_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT), &mut state),
        LoginKeyAction::Redraw
    );
    assert_eq!(
        press(KeyCode::Char('X'), &mut state),
        LoginKeyAction::Redraw
    );

    assert_eq!(state.api_key_draft.text(), "sk-alpha-Xbeta");

    let backend = TestBackend::new(/*width*/ 100, /*height*/ 28);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| {
            LoginOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut())
        })
        .expect("draw API key entry");
    insta::assert_snapshot!("api_key_entry_with_cursor", terminal.backend().to_string());

    state
        .api_key_draft
        .set_text(format!("sk-{}", "secret".repeat(30)));
    let narrow_backend = TestBackend::new(/*width*/ 52, /*height*/ 24);
    let mut narrow_terminal = Terminal::new(narrow_backend).expect("create narrow terminal");
    narrow_terminal
        .draw(|frame| {
            LoginOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut())
        })
        .expect("draw long API key entry");
    insta::assert_snapshot!(
        "api_key_entry_long_cursor",
        narrow_terminal.backend().to_string()
    );

    state.api_key_draft.clear();
    terminal
        .draw(|frame| {
            LoginOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut())
        })
        .expect("draw empty API key entry");
    insta::assert_snapshot!("api_key_entry_empty_cursor", terminal.backend().to_string());
}

#[test]
fn device_code_completion_matches_active_login() {
    let mut state = LoginOnboardingState::new(/*forced_login_method*/ None);
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
    let state = LoginOnboardingState::new(/*forced_login_method*/ None);
    let backend = TestBackend::new(/*width*/ 100, /*height*/ 28);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    terminal
        .draw(|frame| {
            LoginOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut());
        })
        .expect("draw login onboarding");
    insta::assert_snapshot!(terminal.backend().to_string());
}

#[test]
fn login_onboarding_view_renders_device_code_phishing_warning() {
    let mut state = LoginOnboardingState::new(/*forced_login_method*/ None);
    state.mode = LoginMode::DeviceCode {
        login_id: Some("login-1".to_string()),
        verification_url: Some("https://auth.example.test/device".to_string()),
        user_code: Some("ABCD-EFGH".to_string()),
    };
    let backend = TestBackend::new(/*width*/ 100, /*height*/ 28);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    terminal
        .draw(|frame| {
            LoginOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut());
        })
        .expect("draw login onboarding");
    insta::assert_snapshot!(terminal.backend().to_string());
}
