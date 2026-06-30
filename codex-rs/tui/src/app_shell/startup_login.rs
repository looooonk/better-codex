use super::design::MOCHA_BASE;
use super::design::MOCHA_MANTLE;
use super::design::MOCHA_SURFACE0;
use super::design::fill_rect;
use super::design::pane_content_rect;
use super::design::pane_style;
use crate::LoginStatus;
use crate::app_server_session::AppServerSession;
use crate::legacy_core::config::Config;
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
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
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
    api_key_draft: String,
    error: Option<String>,
}

impl LoginOnboardingState {
    fn new(forced_login_method: Option<ForcedLoginMethod>) -> Self {
        Self {
            forced_login_method,
            selected: 0,
            mode: LoginMode::Select,
            api_key_draft: String::new(),
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
    let mut tui_events = tui.event_stream();

    loop {
        select! {
            event = tui_events.next() => {
                let Some(event) = event else {
                    cancel_active_login(app_server, &mut state).await;
                    return Ok(LoginOnboardingOutcome::Exit);
                };
                match event {
                    TuiEvent::Key(key) => match handle_login_key(key, &mut state) {
                        LoginKeyAction::StartDeviceCode => {
                            start_device_code_login(app_server, &mut state).await;
                            tui.frame_requester().schedule_frame();
                        }
                        LoginKeyAction::SubmitApiKey => {
                            match submit_api_key(app_server, &mut state).await {
                                Some(outcome) => return Ok(outcome),
                                None => tui.frame_requester().schedule_frame(),
                            }
                        }
                        LoginKeyAction::Exit => {
                            cancel_active_login(app_server, &mut state).await;
                            return Ok(LoginOnboardingOutcome::Exit);
                        }
                        LoginKeyAction::Redraw => {
                            tui.frame_requester().schedule_frame();
                        }
                        LoginKeyAction::Ignored => {}
                    },
                    TuiEvent::Paste(text) => {
                        if matches!(state.mode, LoginMode::ApiKeyEntry) {
                            state.api_key_draft.push_str(text.trim());
                            tui.frame_requester().schedule_frame();
                        }
                    }
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
    match key.code {
        KeyCode::Esc => {
            state.mode = LoginMode::Select;
            state.api_key_draft.clear();
            LoginKeyAction::Redraw
        }
        KeyCode::Enter => LoginKeyAction::SubmitApiKey,
        KeyCode::Backspace => {
            state.api_key_draft.pop();
            LoginKeyAction::Redraw
        }
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            state.api_key_draft.push(ch);
            LoginKeyAction::Redraw
        }
        _ => LoginKeyAction::Ignored,
    }
}

fn handle_device_code_key(key: KeyEvent) -> LoginKeyAction {
    match key.code {
        KeyCode::Esc => LoginKeyAction::Exit,
        _ => LoginKeyAction::Ignored,
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
            state.error = Some(format!(
                "Unexpected account/login/start response: {other:?}"
            ));
        }
        Err(err) => {
            state.mode = LoginMode::Select;
            state.error = Some(err.to_string());
        }
    }
}

async fn submit_api_key(
    app_server: &mut AppServerSession,
    state: &mut LoginOnboardingState,
) -> Option<LoginOnboardingOutcome> {
    let api_key = state.api_key_draft.trim().to_string();
    if api_key.is_empty() {
        state.error = Some("Enter an API key before continuing.".to_string());
        return None;
    }
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

struct LoginOnboardingView<'a> {
    state: &'a LoginOnboardingState,
}

impl LoginOnboardingView<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);
        self.render_main(horizontal[0], buf);
        self.render_dashboard(horizontal[1], buf);
    }

    fn render_main(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(5),
            ])
            .split(area);
        fill_rect(buf, vertical[0], MOCHA_MANTLE);
        Paragraph::new(Line::from("Better Codex".magenta().bold()))
            .style(pane_style(MOCHA_MANTLE))
            .render(pane_content_rect(vertical[0]), buf);

        let content = pane_content_rect(vertical[1]);
        let mut lines = vec![
            Line::from("Sign in".bold()),
            Line::from(""),
            Line::from(
                "Better Codex needs an authenticated OpenAI account for this provider.".dim(),
            ),
            Line::from(""),
        ];
        match &self.state.mode {
            LoginMode::Select => self.push_selection_lines(&mut lines, usize::from(content.width)),
            LoginMode::ApiKeyEntry => self.push_api_key_lines(&mut lines),
            LoginMode::DeviceCode {
                verification_url,
                user_code,
                ..
            } => self.push_device_code_lines(&mut lines, verification_url, user_code),
        }
        if let Some(error) = &self.state.error {
            lines.push(Line::from(""));
            lines.extend(
                wrapped_lines(error, usize::from(content.width))
                    .into_iter()
                    .map(ratatui::prelude::Stylize::red),
            );
        }
        Paragraph::new(lines)
            .style(pane_style(MOCHA_BASE))
            .render(content, buf);

        fill_rect(buf, vertical[2], MOCHA_SURFACE0);
        Paragraph::new(self.footer_lines())
            .style(pane_style(MOCHA_SURFACE0))
            .render(pane_content_rect(vertical[2]), buf);
    }

    fn push_selection_lines(&self, lines: &mut Vec<Line<'static>>, width: usize) {
        for (index, selection) in self.state.choices().into_iter().enumerate() {
            lines.push(selection_line(
                index,
                selection,
                index == self.state.selected,
            ));
            lines.extend(
                wrapped_lines_with_indent(selection.description(), width, "  ")
                    .into_iter()
                    .map(ratatui::prelude::Stylize::dim),
            );
        }
    }

    fn push_api_key_lines(&self, lines: &mut Vec<Line<'static>>) {
        lines.push(
            "Paste your API key below. It is hidden while you type."
                .dim()
                .into(),
        );
        lines.push(Line::from(""));
        let mask = if self.state.api_key_draft.is_empty() {
            "<empty>".dim()
        } else {
            "*".repeat(self.state.api_key_draft.chars().count()).cyan()
        };
        lines.push(Line::from(vec!["> ".cyan().bold(), mask]));
    }

    fn push_device_code_lines(
        &self,
        lines: &mut Vec<Line<'static>>,
        verification_url: &Option<String>,
        user_code: &Option<String>,
    ) {
        match (verification_url, user_code) {
            (Some(verification_url), Some(user_code)) => {
                lines.push("Open this link in your browser and sign in:".into());
                lines.push(Line::from(""));
                lines.push(verification_url.clone().cyan().underlined().into());
                lines.push(Line::from(""));
                lines.push("Then enter this one-time code:".into());
                lines.push(Line::from(""));
                lines.push(user_code.clone().cyan().bold().into());
                lines.push(Line::from(""));
                lines.push("Never share this device code with anyone.".dim().into());
            }
            _ => {
                lines.push("Requesting a one-time code from ChatGPT...".dim().into());
            }
        }
    }

    fn footer_lines(&self) -> Vec<Line<'static>> {
        match self.state.mode {
            LoginMode::Select => vec![
                "Enter continue  Up/Down choose  1/2/3 jump  Esc exit"
                    .dim()
                    .into(),
                "Authentication is handled through app-server account APIs."
                    .dim()
                    .into(),
            ],
            LoginMode::ApiKeyEntry => vec![
                "Enter save  Esc back  Paste insert key".dim().into(),
                "The key is stored by the app-server auth layer."
                    .dim()
                    .into(),
            ],
            LoginMode::DeviceCode { .. } => vec![
                "Esc cancel and exit".dim().into(),
                "Waiting for app-server to report login completion."
                    .dim()
                    .into(),
            ],
        }
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_SURFACE0);
        let content = pane_content_rect(area);
        let mut lines = vec![Line::from("Startup".bold())];
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            "mode ".dim(),
            match self.state.mode {
                LoginMode::Select => "select".cyan().bold(),
                LoginMode::ApiKeyEntry => "api key".cyan().bold(),
                LoginMode::DeviceCode { .. } => "device code".cyan().bold(),
            },
        ]));
        Paragraph::new(lines)
            .style(pane_style(MOCHA_SURFACE0))
            .render(content, buf);
    }
}

fn selection_line(index: usize, selection: LoginSelection, selected: bool) -> Line<'static> {
    let marker = if selected {
        ">".cyan().bold()
    } else {
        " ".dim()
    };
    let label = format!("{}. {}", index + 1, selection.label());
    Line::from(vec![marker, " ".dim(), label.into()])
}

fn wrapped_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    textwrap::wrap(text, width)
        .into_iter()
        .map(|line| Line::from(line.into_owned()))
        .collect()
}

fn wrapped_lines_with_indent(text: &str, width: usize, indent: &'static str) -> Vec<Line<'static>> {
    let options = textwrap::Options::new(width.max(indent.len() + 1))
        .initial_indent(indent)
        .subsequent_indent(indent);
    textwrap::wrap(text, options)
        .into_iter()
        .map(|line| Line::from(line.into_owned()))
        .collect()
}

#[cfg(test)]
#[path = "startup_login_tests.rs"]
mod tests;
