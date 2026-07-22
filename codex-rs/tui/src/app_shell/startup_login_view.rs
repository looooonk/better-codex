use super::super::design::fill_rect;
use super::super::design::palette;
use super::LoginMode;
use super::LoginOnboardingState;
use super::modal_view;
use crate::terminal_hyperlinks::mark_url_hyperlink;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) struct LoginOnboardingView<'a> {
    pub(super) state: &'a LoginOnboardingState,
}

impl LoginOnboardingView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_visible(area, buf);
        if let Some(url) = self.state.active_url() {
            mark_url_hyperlink(buf, area, url);
        }
    }

    pub(super) fn render_visible(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, palette::BASE);
        let width = usize::from(modal_view::modal_body_width(area));
        modal_view::render_modal(area, "Account login", login_lines(self.state, width), buf);
    }
}

pub(super) fn login_lines(state: &LoginOnboardingState, width: usize) -> Vec<Line<'static>> {
    let mut lines = match &state.mode {
        LoginMode::Select => {
            let mut lines = vec!["Choose how to sign in.".into(), "".into()];
            for (index, selection) in state.choices().into_iter().enumerate() {
                let marker = if index == state.selected { ">" } else { " " };
                lines.push(
                    vec![
                        format!("{marker} {}. ", index + 1).fg(palette::FOCUS),
                        selection.label().to_string().bold(),
                    ]
                    .into(),
                );
                lines.push(format!("    {}", selection.description()).dim().into());
            }
            lines.extend([
                "".into(),
                "↑↓ navigate   Enter select   Esc exit".dim().into(),
            ]);
            lines
        }
        LoginMode::ApiKeyEntry => {
            let value = state
                .api_key_draft
                .masked_text_with_cursor_window(width.saturating_sub(2).max(1));
            vec![
                "Enter an OpenAI API key.".into(),
                "The key is hidden and will not be added to the transcript."
                    .dim()
                    .into(),
                "".into(),
                value.into(),
                "".into(),
                "Enter save   Esc back".dim().into(),
            ]
        }
        LoginMode::DeviceCode {
            verification_url: Some(verification_url),
            user_code: Some(user_code),
            ..
        } => vec![
            "Finish signing in with a device code.".into(),
            "".into(),
            "1. Open this link and sign in:".into(),
            verification_url.clone().cyan().underlined().into(),
            "".into(),
            "2. Enter this one-time code:".into(),
            user_code
                .clone()
                .fg(palette::FOCUS)
                .bold()
                .underlined()
                .into(),
            "".into(),
            "Only continue if you started this login in Better Codex."
                .dim()
                .into(),
            "Enter open link   C copy code   Esc cancel and exit"
                .dim()
                .into(),
        ],
        LoginMode::DeviceCode { .. } => vec![
            "Requesting a one-time code from ChatGPT...".dim().into(),
            "".into(),
            "Esc cancel and exit".dim().into(),
        ],
    };
    if let Some(notice) = &state.notice {
        lines.extend(["".into(), notice.clone().green().into()]);
    }
    if let Some(error) = &state.error {
        lines.extend(["".into(), error.clone().red().into()]);
    }
    lines
}
