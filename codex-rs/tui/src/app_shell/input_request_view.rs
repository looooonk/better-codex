use super::PendingElicitation;
use super::composer::ComposerState;
use super::design::palette;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) fn elicitation_lines(
    pending: &PendingElicitation,
    composer: &ComposerState,
    width: u16,
) -> Vec<Line<'static>> {
    let editing = pending.editing();
    let primary = pending.primary_action_label();
    let mut action_line = vec![
        "  ".into(),
        format!(" {primary} ↵ ")
            .fg(palette::dark())
            .bg(palette::success())
            .bold(),
        " ".into(),
    ];
    let (decline, cancel) = if editing {
        (" Decline ^D ", " Cancel Esc ")
    } else {
        (" Decline d ", " Cancel c ")
    };
    action_line.extend([
        decline.fg(palette::text()).bg(palette::error()).bold(),
        " ".into(),
        cancel.fg(palette::text()).bg(palette::elevated()).bold(),
    ]);

    let mut lines = vec![
        Line::from(vec![
            "? ".fg(palette::cyan()).bold(),
            pending.title().to_string().bold(),
        ]),
        Line::from(vec!["  ".into(), pending.message().to_string().dim()]),
    ];
    if let Some(url) = pending.url() {
        lines.push(Line::from(vec![
            "  ".into(),
            "URL: ".bold(),
            url.to_string().fg(palette::cyan()).underlined(),
        ]));
    }
    if let Some(field) = pending.field_view() {
        let required = if field.required { " *" } else { "" };
        let label = format!(
            "{}/{} {}{required}",
            field.position, field.total, field.label
        );
        lines.push(Line::from(vec![
            "  ".into(),
            label.bold(),
            " - ".dim(),
            field.detail.dim(),
        ]));
        let answer_width = usize::from(width).saturating_sub(2).max(1);
        let answer = if composer.is_empty() {
            format!("▏{}", field.input_hint).dim()
        } else {
            composer.text_with_cursor_window(answer_width).into()
        };
        lines.push(Line::from(vec!["> ".fg(palette::cyan()).bold(), answer]));
    }
    lines.push(Line::from(action_line));
    lines
}
