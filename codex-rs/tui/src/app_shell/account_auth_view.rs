use super::super::design::palette;
use super::super::modal_view;
use super::AccountAuthMode;
use super::AccountAuthState;
use crate::terminal_hyperlinks::mark_url_hyperlink;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) fn lines(state: &AccountAuthState, width: usize) -> Vec<Line<'static>> {
    match &state.mode {
        AccountAuthMode::Choose => choice_lines(state),
        AccountAuthMode::ApiKey => api_key_lines(state, width),
        AccountAuthMode::Browser { auth_url, .. } => vec![
            "Finish signing in with ChatGPT in your browser.".into(),
            "".into(),
            auth_url.clone().cyan().underlined().into(),
            "".into(),
            "Esc cancel".dim().into(),
        ],
        AccountAuthMode::DeviceCode {
            verification_url,
            user_code,
            ..
        } => vec![
            "Finish signing in with a device code.".into(),
            "".into(),
            "1. Open this link and sign in:".into(),
            verification_url.clone().cyan().underlined().into(),
            "".into(),
            "2. Enter this one-time code:".into(),
            user_code.clone().fg(palette::FOCUS).bold().into(),
            "".into(),
            "Only continue if you started this login in Better Codex."
                .dim()
                .into(),
            "Esc cancel".dim().into(),
        ],
        AccountAuthMode::Success => vec![
            "Signed in successfully.".green().bold().into(),
            "".into(),
            "Better Codex will reload account configuration on the next launch.".into(),
            "".into(),
            "Enter exit".dim().into(),
        ],
    }
}

fn choice_lines(state: &AccountAuthState) -> Vec<Line<'static>> {
    let mut lines = vec!["Choose how to sign in.".into(), "".into()];
    for (index, choice) in state.choices().into_iter().enumerate() {
        let marker = if index == state.selected { ">" } else { " " };
        lines.push(
            vec![
                format!("{marker} {}. ", index + 1).fg(palette::FOCUS),
                choice.label().to_string().bold(),
            ]
            .into(),
        );
        lines.push(format!("    {}", choice.description()).dim().into());
    }
    if let Some(error) = &state.error {
        lines.extend(["".into(), error.clone().red().into()]);
    }
    lines.extend([
        "".into(),
        "↑↓ / j k navigate   Enter select   Esc close".dim().into(),
    ]);
    lines
}

fn api_key_lines(state: &AccountAuthState, width: usize) -> Vec<Line<'static>> {
    let value = state
        .api_key
        .masked_text_with_cursor_window(width.saturating_sub(2).max(1));
    let mut lines = vec![
        "Enter an OpenAI API key.".into(),
        "The key is hidden and will not be added to the transcript."
            .dim()
            .into(),
        "".into(),
        value.into(),
    ];
    if let Some(error) = &state.error {
        lines.extend(["".into(), error.clone().red().into()]);
    }
    lines.extend(["".into(), "Enter save   Esc back".dim().into()]);
    lines
}

pub(super) fn render(state: &AccountAuthState, area: Rect, buf: &mut Buffer) {
    let width = usize::from(modal_view::modal_body_width(area));
    modal_view::render_modal(area, "Account login", lines(state, width), buf);
    if let Some(url) = state.active_url() {
        mark_url_hyperlink(buf, area, url);
    }
}
