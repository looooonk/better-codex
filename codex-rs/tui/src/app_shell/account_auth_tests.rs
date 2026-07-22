use super::super::modal_view;
use super::*;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn render_state(state: &AccountAuthState) -> String {
    let backend = TestBackend::new(/*width*/ 88, /*height*/ 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| {
            let area = frame.area();
            let width = usize::from(modal_view::modal_body_width(area));
            modal_view::render_modal(
                area,
                "Account login",
                state.lines(width),
                frame.buffer_mut(),
            );
        })
        .expect("render account login");
    terminal.backend().to_string()
}

#[test]
fn forced_login_method_filters_choices() {
    let chatgpt = AccountAuthState::new(Some(ForcedLoginMethod::Chatgpt));
    assert_eq!(
        chatgpt.choices(),
        vec![
            AccountAuthChoice::ChatGptBrowser,
            AccountAuthChoice::ChatGptDeviceCode,
            AccountAuthChoice::Cancel,
        ]
    );

    let api = AccountAuthState::new(Some(ForcedLoginMethod::Api));
    assert_eq!(
        api.choices(),
        vec![AccountAuthChoice::ApiKey, AccountAuthChoice::Cancel]
    );
}

#[test]
fn completion_only_updates_the_matching_login() {
    let mut state = AccountAuthState::new(/*forced_login_method*/ None);
    state.mode = AccountAuthMode::DeviceCode {
        login_id: "login-1".to_string(),
        verification_url: "https://auth.example.test/device".to_string(),
        user_code: "ABCD-EFGH".to_string(),
    };

    state.receive_login_completed(AccountLoginCompletedNotification {
        login_id: Some("other".to_string()),
        success: true,
        error: None,
    });
    assert!(matches!(state.mode, AccountAuthMode::DeviceCode { .. }));

    state.receive_login_completed(AccountLoginCompletedNotification {
        login_id: Some("login-1".to_string()),
        success: true,
        error: None,
    });
    assert_eq!(state.mode, AccountAuthMode::Success);
}

#[test]
fn account_login_prompts_map_clicks_to_url_and_copy_actions() {
    let mut state = AccountAuthState::new(/*forced_login_method*/ None);
    state.mode = AccountAuthMode::Browser {
        login_id: "login-browser".to_string(),
        auth_url: "https://auth.example.test/browser".to_string(),
    };
    assert_eq!(state.click_key_at(/*line*/ 2), Some(KeyCode::Enter));

    state.mode = AccountAuthMode::DeviceCode {
        login_id: "login-device".to_string(),
        verification_url: "https://auth.example.test/device".to_string(),
        user_code: "ABCD-EFGH".to_string(),
    };
    assert_eq!(state.click_key_at(/*line*/ 3), Some(KeyCode::Enter));
    assert_eq!(state.click_key_at(/*line*/ 6), Some(KeyCode::Char('c')));
}

#[test]
fn account_login_choices_snapshot() {
    let state = AccountAuthState::new(/*forced_login_method*/ None);
    insta::assert_snapshot!("account_login_choices", render_state(&state));
}

#[test]
fn account_login_device_code_snapshot() {
    let mut state = AccountAuthState::new(/*forced_login_method*/ None);
    state.mode = AccountAuthMode::DeviceCode {
        login_id: "login-1".to_string(),
        verification_url: "https://auth.example.test/device".to_string(),
        user_code: "ABCD-EFGH".to_string(),
    };
    insta::assert_snapshot!("account_login_device_code", render_state(&state));
}

#[test]
fn account_login_browser_snapshot() {
    let mut state = AccountAuthState::new(/*forced_login_method*/ None);
    state.mode = AccountAuthMode::Browser {
        login_id: "login-1".to_string(),
        auth_url: "https://auth.example.test/browser".to_string(),
    };
    insta::assert_snapshot!("account_login_browser", render_state(&state));
}

#[test]
fn account_login_api_key_snapshot() {
    let mut state = AccountAuthState::new(/*forced_login_method*/ None);
    state.mode = AccountAuthMode::ApiKey;
    state.api_key.set_text("sk-secret-value");
    let rendered = render_state(&state);
    assert!(!rendered.contains("sk-secret-value"));
    insta::assert_snapshot!("account_login_api_key", rendered);
}

#[test]
fn account_login_success_snapshot() {
    let mut state = AccountAuthState::new(/*forced_login_method*/ None);
    state.mode = AccountAuthMode::Success;
    insta::assert_snapshot!("account_login_success", render_state(&state));
}
