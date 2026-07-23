use super::modal_view;
use crate::LoginStatus;
use crate::app_server_session::AppServerSession;
use crate::clipboard_copy::ClipboardLease;
use crate::legacy_core::config::Config;
use crate::text_input::EditableText;
use crate::text_input::text_input_action_from_key;
use crate::tui;
use crate::tui::TuiEvent;
use codex_app_server_client::AppServerEvent;
use codex_app_server_protocol::AccountLoginCompletedNotification;
use codex_app_server_protocol::LoginAccountParams;
use codex_app_server_protocol::LoginAccountResponse;
use codex_app_server_protocol::ServerNotification;
use codex_protocol::config_types::ForcedLoginMethod;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use tokio::select;
use tokio_stream::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginOnboardingOutcome {
    Continue,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginSelection {
    ChatGptDeviceCode,
    ApiKey,
    Exit,
}

impl LoginSelection {
    fn label(self) -> &'static str {
        match self {
            Self::ChatGptDeviceCode => "Sign in with ChatGPT",
            Self::ApiKey => "Use API key",
            Self::Exit => "Exit",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ChatGptDeviceCode => "Get a one-time code and finish sign-in in your browser.",
            Self::ApiKey => "Paste an OpenAI API key and store it through app-server auth.",
            Self::Exit => "Return to the terminal without starting a thread.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LoginMode {
    Select,
    ApiKeyEntry,
    DeviceCode {
        login_id: Option<String>,
        verification_url: Option<String>,
        user_code: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoginOnboardingState {
    forced_login_method: Option<ForcedLoginMethod>,
    selected: usize,
    mode: LoginMode,
    api_key_draft: EditableText,
    notice: Option<String>,
    error: Option<String>,
}

impl LoginOnboardingState {
    fn new(forced_login_method: Option<ForcedLoginMethod>) -> Self {
        Self {
            forced_login_method,
            selected: 0,
            mode: LoginMode::Select,
            api_key_draft: EditableText::default(),
            notice: None,
            error: None,
        }
    }

    fn choices(&self) -> Vec<LoginSelection> {
        match self.forced_login_method {
            Some(ForcedLoginMethod::Chatgpt) => {
                vec![LoginSelection::ChatGptDeviceCode, LoginSelection::Exit]
            }
            Some(ForcedLoginMethod::Api) => vec![LoginSelection::ApiKey, LoginSelection::Exit],
            None => vec![
                LoginSelection::ChatGptDeviceCode,
                LoginSelection::ApiKey,
                LoginSelection::Exit,
            ],
        }
    }

    fn selected(&self) -> LoginSelection {
        let choices = self.choices();
        choices[self.selected.min(choices.len().saturating_sub(1))]
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.choices().len().saturating_sub(1));
    }

    fn select_number(&mut self, number: char) {
        let Some(index) = number.to_digit(10).and_then(|number| number.checked_sub(1)) else {
            return;
        };
        let index = index as usize;
        if index < self.choices().len() {
            self.selected = index;
        }
    }

    fn active_login_id(&self) -> Option<&str> {
        match &self.mode {
            LoginMode::DeviceCode {
                login_id: Some(login_id),
                ..
            } => Some(login_id),
            LoginMode::Select | LoginMode::ApiKeyEntry | LoginMode::DeviceCode { .. } => None,
        }
    }

    fn active_url(&self) -> Option<&str> {
        match &self.mode {
            LoginMode::DeviceCode {
                verification_url: Some(verification_url),
                ..
            } => Some(verification_url),
            LoginMode::Select | LoginMode::ApiKeyEntry | LoginMode::DeviceCode { .. } => None,
        }
    }

    fn active_code(&self) -> Option<&str> {
        match &self.mode {
            LoginMode::DeviceCode {
                user_code: Some(user_code),
                ..
            } => Some(user_code),
            LoginMode::Select | LoginMode::ApiKeyEntry | LoginMode::DeviceCode { .. } => None,
        }
    }

    fn select_line(&mut self, line: usize) -> bool {
        let Some(index) = line.checked_sub(/*choice_start*/ 2).map(|line| line / 2) else {
            return false;
        };
        if matches!(self.mode, LoginMode::Select) && index < self.choices().len() {
            self.selected = index;
            true
        } else {
            false
        }
    }

    fn receive_login_completed(
        &mut self,
        notification: AccountLoginCompletedNotification,
    ) -> Option<LoginOnboardingOutcome> {
        let login_id = notification.login_id?;
        if self.active_login_id() != Some(login_id.as_str()) {
            return None;
        }
        if notification.success {
            Some(LoginOnboardingOutcome::Continue)
        } else {
            self.mode = LoginMode::Select;
            self.notice = None;
            self.error = Some(
                notification
                    .error
                    .unwrap_or_else(|| "ChatGPT login was not completed".to_string()),
            );
            None
        }
    }
}

pub(crate) async fn run_login_onboarding(
    tui: &mut tui::Tui,
    app_server: &mut AppServerSession,
    config: &Config,
    login_status: LoginStatus,
) -> Result<LoginOnboardingOutcome> {
    if !matches!(login_status, LoginStatus::NotAuthenticated) {
        return Ok(LoginOnboardingOutcome::Continue);
    }

    tui.enter_alt_screen()
        .wrap_err("failed to enter login setup screen")?;
    tui.frame_requester().schedule_frame();

    let mut state = LoginOnboardingState::new(config.forced_login_method);
    let mut clipboard_lease: Option<ClipboardLease> = None;
    let mut tui_events = tui.event_stream();

    loop {
        select! {
            event = tui_events.next() => {
                let Some(event) = event else {
                    cancel_active_login(app_server, &mut state).await;
                    return Ok(LoginOnboardingOutcome::Exit);
                };
                match event {
                    TuiEvent::Key(key) => {
                        let action = handle_login_key(key, &mut state);
                        if let Some(outcome) = apply_login_action(
                            action,
                            app_server,
                            &mut state,
                            &mut clipboard_lease,
                        ).await {
                            return Ok(outcome);
                        }
                        if action != LoginKeyAction::Ignored {
                            tui.frame_requester().schedule_frame();
                        }
                    }
                    TuiEvent::Paste(text) => {
                        if matches!(state.mode, LoginMode::ApiKeyEntry) {
                            state.api_key_draft.insert_str(text.trim());
                            tui.frame_requester().schedule_frame();
                        }
                    }
                    TuiEvent::MouseClick(position) => {
                        let size = tui.terminal.size()?;
                        let area = Rect::new(
                            /*x*/ 0,
                            /*y*/ 0,
                            size.width,
                            size.height,
                        );
                        let action = handle_login_click(area, position, &mut state);
                        if let Some(outcome) = apply_login_action(
                            action,
                            app_server,
                            &mut state,
                            &mut clipboard_lease,
                        ).await {
                            return Ok(outcome);
                        }
                        if action != LoginKeyAction::Ignored {
                            tui.frame_requester().schedule_frame();
                        }
                    }
                    TuiEvent::MouseMove(_)
                    | TuiEvent::MouseDrag(_)
                    | TuiEvent::MouseRelease(_)
                    | TuiEvent::MouseScroll { .. } => {}
                    TuiEvent::Resize | TuiEvent::Draw => {
                        draw_login_onboarding(tui, &state)?;
                    }
                }
            }
            event = app_server.next_event() => {
                let Some(event) = event else {
                    return Ok(LoginOnboardingOutcome::Exit);
                };
                match event {
                    AppServerEvent::ServerNotification(ServerNotification::AccountLoginCompleted(notification)) => {
                        if let Some(outcome) = state.receive_login_completed(notification) {
                            return Ok(outcome);
                        }
                        tui.frame_requester().schedule_frame();
                    }
                    AppServerEvent::Disconnected { message } => {
                        return Err(color_eyre::eyre::eyre!(message));
                    }
                    AppServerEvent::Lagged { .. }
                    | AppServerEvent::ServerNotification(_)
                    | AppServerEvent::ServerRequest(_) => {}
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginKeyAction {
    StartDeviceCode,
    SubmitApiKey,
    OpenUrl,
    CopyCode,
    Exit,
    Redraw,
    Ignored,
}

fn handle_login_key(key: KeyEvent, state: &mut LoginOnboardingState) -> LoginKeyAction {
    if key.kind != KeyEventKind::Press {
        return LoginKeyAction::Ignored;
    }
    match state.mode {
        LoginMode::Select => handle_select_key(key, state),
        LoginMode::ApiKeyEntry => handle_api_key_key(key, state),
        LoginMode::DeviceCode { .. } => handle_device_code_key(key),
    }
}

fn handle_select_key(key: KeyEvent, state: &mut LoginOnboardingState) -> LoginKeyAction {
    match key.code {
        KeyCode::Up => {
            state.move_up();
            LoginKeyAction::Redraw
        }
        KeyCode::Down => {
            state.move_down();
            LoginKeyAction::Redraw
        }
        KeyCode::Char(number @ ('1' | '2' | '3')) => {
            state.select_number(number);
            LoginKeyAction::Redraw
        }
        KeyCode::Enter => match state.selected() {
            LoginSelection::ChatGptDeviceCode => LoginKeyAction::StartDeviceCode,
            LoginSelection::ApiKey => {
                state.mode = LoginMode::ApiKeyEntry;
                state.notice = None;
                state.error = None;
                LoginKeyAction::Redraw
            }
            LoginSelection::Exit => LoginKeyAction::Exit,
        },
        KeyCode::Esc => LoginKeyAction::Exit,
        _ => LoginKeyAction::Ignored,
    }
}

fn handle_api_key_key(key: KeyEvent, state: &mut LoginOnboardingState) -> LoginKeyAction {
    if let Some(action) = text_input_action_from_key(key) {
        state.api_key_draft.apply(action);
        return LoginKeyAction::Redraw;
    }
    match key.code {
        KeyCode::Esc => {
            state.mode = LoginMode::Select;
            state.api_key_draft.clear();
            state.notice = None;
            state.error = None;
            LoginKeyAction::Redraw
        }
        KeyCode::Enter => LoginKeyAction::SubmitApiKey,
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.api_key_draft.insert_char(ch);
            LoginKeyAction::Redraw
        }
        _ => LoginKeyAction::Ignored,
    }
}

fn handle_device_code_key(key: KeyEvent) -> LoginKeyAction {
    match key.code {
        KeyCode::Enter => LoginKeyAction::OpenUrl,
        KeyCode::Char('c') if key.modifiers == KeyModifiers::NONE => LoginKeyAction::CopyCode,
        KeyCode::Esc => LoginKeyAction::Exit,
        _ => LoginKeyAction::Ignored,
    }
}

fn handle_login_click(
    area: Rect,
    position: Position,
    state: &mut LoginOnboardingState,
) -> LoginKeyAction {
    let width = usize::from(modal_view::modal_body_width(area));
    let lines = login_lines(state, width);
    let Some(hit) = modal_view::modal_hit(area, position, &lines) else {
        return LoginKeyAction::Ignored;
    };
    if matches!(state.mode, LoginMode::Select) && state.select_line(hit.line) {
        return handle_select_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), state);
    }
    match &state.mode {
        LoginMode::DeviceCode { .. } if hit.line == 3 => LoginKeyAction::OpenUrl,
        LoginMode::DeviceCode { .. } if hit.line == 6 => LoginKeyAction::CopyCode,
        LoginMode::Select | LoginMode::ApiKeyEntry | LoginMode::DeviceCode { .. } => {
            LoginKeyAction::Ignored
        }
    }
}

async fn apply_login_action(
    action: LoginKeyAction,
    app_server: &mut AppServerSession,
    state: &mut LoginOnboardingState,
    clipboard_lease: &mut Option<ClipboardLease>,
) -> Option<LoginOnboardingOutcome> {
    match action {
        LoginKeyAction::StartDeviceCode => start_device_code_login(app_server, state).await,
        LoginKeyAction::SubmitApiKey => return submit_api_key(app_server, state).await,
        LoginKeyAction::OpenUrl => open_login_url(state),
        LoginKeyAction::CopyCode => copy_login_code(state, clipboard_lease),
        LoginKeyAction::Exit => {
            cancel_active_login(app_server, state).await;
            return Some(LoginOnboardingOutcome::Exit);
        }
        LoginKeyAction::Redraw | LoginKeyAction::Ignored => {}
    }
    None
}

fn open_login_url(state: &mut LoginOnboardingState) {
    let url = state.active_url().map(str::to_string);
    let Some(url) = url else {
        return;
    };
    match webbrowser::open(&url) {
        Ok(()) => {
            state.notice = Some("Opened the sign-in link in your browser.".to_string());
            state.error = None;
        }
        Err(error) => {
            tracing::warn!("failed to open browser for login URL: {error}");
            state.notice = None;
            state.error = Some(format!("Could not open the sign-in link: {error}"));
        }
    }
}

fn copy_login_code(state: &mut LoginOnboardingState, clipboard_lease: &mut Option<ClipboardLease>) {
    let code = state.active_code().map(str::to_string);
    let Some(code) = code else {
        return;
    };
    match crate::clipboard_copy::copy_to_clipboard(&code) {
        Ok(lease) => {
            *clipboard_lease = lease;
            state.notice = Some("Copied the one-time code.".to_string());
            state.error = None;
        }
        Err(error) => {
            state.notice = None;
            state.error = Some(format!("Copy failed: {error}"));
        }
    }
}

async fn start_device_code_login(
    app_server: &mut AppServerSession,
    state: &mut LoginOnboardingState,
) {
    state.mode = LoginMode::DeviceCode {
        login_id: None,
        verification_url: None,
        user_code: None,
    };
    state.notice = None;
    state.error = None;
    match app_server
        .login_account(LoginAccountParams::ChatgptDeviceCode)
        .await
    {
        Ok(LoginAccountResponse::ChatgptDeviceCode {
            login_id,
            verification_url,
            user_code,
        }) => {
            state.mode = LoginMode::DeviceCode {
                login_id: Some(login_id),
                verification_url: Some(verification_url),
                user_code: Some(user_code),
            };
        }
        Ok(other) => {
            state.mode = LoginMode::Select;
            state.notice = None;
            state.error = Some(format!(
                "Unexpected account/login/start response: {other:?}"
            ));
        }
        Err(err) => {
            state.mode = LoginMode::Select;
            state.notice = None;
            state.error = Some(err.to_string());
        }
    }
}

async fn submit_api_key(
    app_server: &mut AppServerSession,
    state: &mut LoginOnboardingState,
) -> Option<LoginOnboardingOutcome> {
    let api_key = state.api_key_draft.text().trim().to_string();
    if api_key.is_empty() {
        state.notice = None;
        state.error = Some("Enter an API key before continuing.".to_string());
        return None;
    }
    state.notice = None;
    state.error = None;
    match app_server
        .login_account(LoginAccountParams::ApiKey { api_key })
        .await
    {
        Ok(LoginAccountResponse::ApiKey {}) => Some(LoginOnboardingOutcome::Continue),
        Ok(other) => {
            state.error = Some(format!(
                "Unexpected account/login/start response: {other:?}"
            ));
            None
        }
        Err(err) => {
            state.error = Some(err.to_string());
            None
        }
    }
}

async fn cancel_active_login(app_server: &mut AppServerSession, state: &mut LoginOnboardingState) {
    let Some(login_id) = state.active_login_id().map(str::to_string) else {
        return;
    };
    let _ = app_server.cancel_login_account(login_id).await;
}

fn draw_login_onboarding(tui: &mut tui::Tui, state: &LoginOnboardingState) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        LoginOnboardingView { state }.render(frame.area(), frame.buffer);
    })
}

use view::LoginOnboardingView;
use view::login_lines;

#[path = "startup_login_view.rs"]
mod view;

#[cfg(test)]
#[path = "startup_login_tests.rs"]
mod tests;
