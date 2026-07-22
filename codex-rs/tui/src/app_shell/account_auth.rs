use super::ShellState;
use super::backend::AppShellBackend;
use super::is_unmodified_action_key;
use crate::text_input::EditableText;
use crate::text_input::text_input_action_from_key;
use codex_app_server_protocol::AccountLoginCompletedNotification;
use codex_app_server_protocol::LoginAccountParams;
use codex_app_server_protocol::LoginAccountResponse;
use codex_protocol::config_types::ForcedLoginMethod;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountAuthChoice {
    ChatGptBrowser,
    ChatGptDeviceCode,
    ApiKey,
    Cancel,
}

impl AccountAuthChoice {
    fn label(self) -> &'static str {
        match self {
            Self::ChatGptBrowser => "Sign in with ChatGPT",
            Self::ChatGptDeviceCode => "Use a device code",
            Self::ApiKey => "Use an API key",
            Self::Cancel => "Cancel",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ChatGptBrowser => "Open the standard ChatGPT login flow in your browser.",
            Self::ChatGptDeviceCode => "Enter a one-time code on another device or browser.",
            Self::ApiKey => "Store an OpenAI API key through app-server auth.",
            Self::Cancel => "Keep the current account and return to the session.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AccountAuthMode {
    Choose,
    ApiKey,
    Browser {
        login_id: String,
        auth_url: String,
    },
    DeviceCode {
        login_id: String,
        verification_url: String,
        user_code: String,
    },
    Success,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct AccountAuthState {
    forced_login_method: Option<ForcedLoginMethod>,
    selected: usize,
    mode: AccountAuthMode,
    api_key: EditableText,
    error: Option<String>,
}

impl AccountAuthState {
    pub(super) fn new(forced_login_method: Option<ForcedLoginMethod>) -> Self {
        Self {
            forced_login_method,
            selected: 0,
            mode: AccountAuthMode::Choose,
            api_key: EditableText::default(),
            error: None,
        }
    }

    fn choices(&self) -> Vec<AccountAuthChoice> {
        match self.forced_login_method {
            Some(ForcedLoginMethod::Chatgpt) => vec![
                AccountAuthChoice::ChatGptBrowser,
                AccountAuthChoice::ChatGptDeviceCode,
                AccountAuthChoice::Cancel,
            ],
            Some(ForcedLoginMethod::Api) => {
                vec![AccountAuthChoice::ApiKey, AccountAuthChoice::Cancel]
            }
            None => vec![
                AccountAuthChoice::ChatGptBrowser,
                AccountAuthChoice::ChatGptDeviceCode,
                AccountAuthChoice::ApiKey,
                AccountAuthChoice::Cancel,
            ],
        }
    }

    fn selected(&self) -> AccountAuthChoice {
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

    pub(super) fn select_line(&mut self, line: usize) -> bool {
        let Some(index) = line.checked_sub(/*choice_start*/ 2).map(|line| line / 2) else {
            return false;
        };
        if matches!(self.mode, AccountAuthMode::Choose) && index < self.choices().len() {
            self.selected = index;
            true
        } else {
            false
        }
    }

    fn active_login_id(&self) -> Option<&str> {
        match &self.mode {
            AccountAuthMode::Browser { login_id, .. }
            | AccountAuthMode::DeviceCode { login_id, .. } => Some(login_id),
            AccountAuthMode::Choose | AccountAuthMode::ApiKey | AccountAuthMode::Success => None,
        }
    }

    fn active_url(&self) -> Option<&str> {
        match &self.mode {
            AccountAuthMode::Browser { auth_url, .. } => Some(auth_url),
            AccountAuthMode::DeviceCode {
                verification_url, ..
            } => Some(verification_url),
            AccountAuthMode::Choose | AccountAuthMode::ApiKey | AccountAuthMode::Success => None,
        }
    }

    pub(super) fn editing(&self) -> bool {
        matches!(self.mode, AccountAuthMode::ApiKey)
    }

    fn receive_login_completed(&mut self, notification: AccountLoginCompletedNotification) {
        let Some(login_id) = notification.login_id else {
            return;
        };
        if self.active_login_id() != Some(login_id.as_str()) {
            return;
        }
        if notification.success {
            self.mode = AccountAuthMode::Success;
            self.error = None;
        } else {
            self.mode = AccountAuthMode::Choose;
            self.error = Some(
                notification
                    .error
                    .unwrap_or_else(|| "ChatGPT login was not completed".to_string()),
            );
        }
    }

    pub(super) fn insert_paste(&mut self, text: &str) {
        if matches!(self.mode, AccountAuthMode::ApiKey) {
            self.api_key.insert_str(text.trim());
        }
    }

    pub(super) fn lines(&self, width: usize) -> Vec<Line<'static>> {
        view::lines(self, width)
    }
}

impl ShellState {
    pub(super) fn open_account_auth(&mut self, forced_login_method: Option<ForcedLoginMethod>) {
        self.close_agent_log();
        self.close_tool_output();
        self.close_diff_view();
        self.pending_account_auth = Some(AccountAuthState::new(forced_login_method));
    }

    pub(super) async fn handle_account_auth_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        let Some(mode) = self
            .pending_account_auth
            .as_ref()
            .map(|state| state.mode.clone())
        else {
            return Ok(false);
        };
        if matches!(mode, AccountAuthMode::ApiKey)
            && let Some(action) = text_input_action_from_key(key)
        {
            if let Some(state) = &mut self.pending_account_auth {
                state.api_key.apply(action);
            }
            return Ok(false);
        }
        if !(is_unmodified_action_key(key)
            || matches!(mode, AccountAuthMode::ApiKey)
                && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT))
        {
            return Ok(false);
        }
        match mode {
            AccountAuthMode::Choose => match key.code {
                KeyCode::Esc => self.pending_account_auth = None,
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(state) = &mut self.pending_account_auth {
                        state.move_up();
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(state) = &mut self.pending_account_auth {
                        state.move_down();
                    }
                }
                KeyCode::Char(number @ ('1' | '2' | '3' | '4')) => {
                    if let Some(state) = &mut self.pending_account_auth {
                        state.select_number(number);
                    }
                }
                KeyCode::Enter => self.start_selected_account_auth(app_server).await,
                _ => {}
            },
            AccountAuthMode::ApiKey => match key.code {
                KeyCode::Esc => {
                    if let Some(state) = &mut self.pending_account_auth {
                        state.api_key.clear();
                        state.error = None;
                        state.mode = AccountAuthMode::Choose;
                    }
                }
                KeyCode::Enter => self.submit_account_api_key(app_server).await,
                KeyCode::Char(ch) => {
                    if let Some(state) = &mut self.pending_account_auth {
                        state.api_key.insert_char(ch);
                    }
                }
                _ => {}
            },
            AccountAuthMode::Browser { .. } | AccountAuthMode::DeviceCode { .. } => {
                if matches!(key.code, KeyCode::Esc) {
                    self.cancel_pending_account_login(app_server).await;
                }
            }
            AccountAuthMode::Success => {
                if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    async fn start_selected_account_auth<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let Some(choice) = self
            .pending_account_auth
            .as_ref()
            .map(AccountAuthState::selected)
        else {
            return;
        };
        match choice {
            AccountAuthChoice::ChatGptBrowser => self.start_browser_login(app_server).await,
            AccountAuthChoice::ChatGptDeviceCode => self.start_device_code_login(app_server).await,
            AccountAuthChoice::ApiKey => {
                if let Some(state) = &mut self.pending_account_auth {
                    state.mode = AccountAuthMode::ApiKey;
                    state.error = None;
                }
            }
            AccountAuthChoice::Cancel => self.pending_account_auth = None,
        }
    }

    async fn start_browser_login<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        match app_server
            .login_account(LoginAccountParams::Chatgpt {
                codex_streamlined_login: false,
                use_hosted_login_success_page: false,
                app_brand: None,
            })
            .await
        {
            Ok(LoginAccountResponse::Chatgpt { login_id, auth_url }) => {
                if app_server.uses_embedded_app_server()
                    && let Err(err) = webbrowser::open(&auth_url)
                {
                    tracing::warn!("failed to open browser for login URL: {err}");
                }
                if let Some(state) = &mut self.pending_account_auth {
                    state.mode = AccountAuthMode::Browser { login_id, auth_url };
                    state.error = None;
                }
            }
            Ok(other) => self.report_account_login_response(other),
            Err(err) => self.report_account_login_error(err.to_string()),
        }
    }

    async fn start_device_code_login<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        match app_server
            .login_account(LoginAccountParams::ChatgptDeviceCode)
            .await
        {
            Ok(LoginAccountResponse::ChatgptDeviceCode {
                login_id,
                verification_url,
                user_code,
            }) => {
                if let Some(state) = &mut self.pending_account_auth {
                    state.mode = AccountAuthMode::DeviceCode {
                        login_id,
                        verification_url,
                        user_code,
                    };
                    state.error = None;
                }
            }
            Ok(other) => self.report_account_login_response(other),
            Err(err) => self.report_account_login_error(err.to_string()),
        }
    }

    async fn submit_account_api_key<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let Some(api_key) = self
            .pending_account_auth
            .as_mut()
            .map(|state| std::mem::take(&mut state.api_key).into_text())
        else {
            return;
        };
        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            self.report_api_key_login_error("Enter an API key before continuing.".to_string());
            return;
        }
        match app_server
            .login_account(LoginAccountParams::ApiKey { api_key })
            .await
        {
            Ok(LoginAccountResponse::ApiKey {}) => {
                if let Some(state) = &mut self.pending_account_auth {
                    state.mode = AccountAuthMode::Success;
                    state.error = None;
                }
            }
            Ok(other) => self.report_api_key_login_error(format!(
                "Unexpected account/login/start response: {other:?}"
            )),
            Err(err) => self.report_api_key_login_error(err.to_string()),
        }
    }

    fn report_account_login_response(&mut self, response: LoginAccountResponse) {
        self.report_account_login_error(format!(
            "Unexpected account/login/start response: {response:?}"
        ));
    }

    fn report_account_login_error(&mut self, error: String) {
        if let Some(state) = &mut self.pending_account_auth {
            state.mode = AccountAuthMode::Choose;
            state.error = Some(error);
        }
    }

    fn report_api_key_login_error(&mut self, error: String) {
        if let Some(state) = &mut self.pending_account_auth {
            state.mode = AccountAuthMode::ApiKey;
            state.error = Some(error);
        }
    }

    pub(super) fn receive_account_login_completed(
        &mut self,
        notification: AccountLoginCompletedNotification,
    ) {
        if let Some(state) = &mut self.pending_account_auth {
            state.receive_login_completed(notification);
        }
    }

    async fn cancel_pending_account_login<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let login_id = self
            .pending_account_auth
            .as_ref()
            .and_then(AccountAuthState::active_login_id)
            .map(str::to_string);
        if let Some(login_id) = login_id
            && let Err(err) = app_server.cancel_login_account(login_id).await
        {
            self.report_account_login_error(format!("Failed to cancel login: {err}"));
            return;
        }
        if let Some(state) = &mut self.pending_account_auth {
            state.mode = AccountAuthMode::Choose;
            state.error = None;
        }
    }

    pub(super) async fn close_account_auth<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        let login_id = self
            .pending_account_auth
            .as_ref()
            .and_then(AccountAuthState::active_login_id)
            .map(str::to_string);
        if let Some(login_id) = login_id {
            let _ = app_server.cancel_login_account(login_id).await;
        }
        self.pending_account_auth = None;
    }
}

pub(super) fn render(state: &AccountAuthState, area: Rect, buf: &mut Buffer) {
    view::render(state, area, buf);
}

#[path = "account_auth_view.rs"]
mod view;

#[cfg(test)]
#[path = "account_auth_tests.rs"]
mod tests;
