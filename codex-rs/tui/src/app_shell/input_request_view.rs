use super::PendingApproval;
use super::PendingElicitation;
use super::PendingUserInput;
use super::composer::ComposerState;
use super::design::palette;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) fn approval_lines(pending: &PendingApproval) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        "? ".fg(palette::WARNING).bold(),
        pending.title().to_string().fg(palette::TEXT).bold(),
    ])];
    lines.extend(
        pending
            .details()
            .iter()
            .map(|detail| Line::from(vec!["  ".into(), detail.clone().fg(palette::MUTED)])),
    );
    lines.extend(pending.options().map(|(index, label)| {
        let marker = if index == 0 { "> " } else { "  " };
        Line::from(vec![
            marker.fg(palette::FOCUS).bold(),
            format!("{} ", index + 1).fg(palette::SUCCESS).bold(),
            label.to_string().fg(palette::TEXT),
        ])
    }));
    lines.push(Line::from(vec![
        "  ".into(),
        " e Edit ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
        " ".into(),
        " ? Explain ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
    ]));
    lines
}

pub(super) fn user_input_lines(
    pending: &PendingUserInput,
    composer: &ComposerState,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let (current, total) = pending.question_position();
    lines.push(Line::from(vec![
        "? ".cyan().bold(),
        format!("{} ({current}/{total})", pending.title()).bold(),
    ]));

    if let Some(question) = pending.current_question() {
        lines.push(Line::from(vec![
            "  ".into(),
            question.header.clone().bold(),
            ": ".dim(),
            question.question.clone().into(),
        ]));
        if let Some(options) = question
            .options
            .as_deref()
            .filter(|options| !options.is_empty())
        {
            lines.extend(options.iter().enumerate().map(|(index, option)| {
                Line::from(vec![
                    "  ".into(),
                    format!("{} ", index + 1).green().bold(),
                    option.label.clone().into(),
                    " - ".dim(),
                    option.description.clone().dim(),
                ])
            }));
            if question.is_other {
                lines.push(Line::from(vec![
                    "  ".into(),
                    "Other (free-form)".into(),
                    " - Type a custom answer below.".dim(),
                ]));
            }
        }
    }

    if let Some(delay_ms) = pending.auto_resolution_ms() {
        let delay = if delay_ms.is_multiple_of(1_000) {
            format!("{}s", delay_ms / 1_000)
        } else {
            format!("{delay_ms}ms")
        };
        lines.push(Line::from(vec![
            "  ".into(),
            format!("Auto-continue after {delay} if unanswered").dim(),
        ]));
    }

    let secret = pending
        .current_question()
        .is_some_and(|question| question.is_secret);
    let answer_width = usize::from(width).saturating_sub(2).max(1);
    let answer = if composer.is_empty() {
        "▏answer".dim()
    } else if secret {
        composer.masked_text_with_cursor_window(answer_width).dim()
    } else {
        composer.text_with_cursor_window(answer_width).into()
    };
    lines.push(Line::from(vec!["> ".cyan().bold(), answer]));
    lines
}

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
            .fg(palette::DARK)
            .bg(palette::SUCCESS)
            .bold(),
        " ".into(),
    ];
    let (decline, cancel) = if editing {
        (" Decline ^D ", " Cancel Esc ")
    } else {
        (" Decline d ", " Cancel c ")
    };
    action_line.extend([
        decline.fg(palette::TEXT).bg(palette::ERROR).bold(),
        " ".into(),
        cancel.fg(palette::TEXT).bg(palette::ELEVATED).bold(),
    ]);

    let mut lines = vec![
        Line::from(vec!["? ".cyan().bold(), pending.title().to_string().bold()]),
        Line::from(vec!["  ".into(), pending.message().to_string().dim()]),
    ];
    if let Some(url) = pending.url() {
        lines.push(Line::from(vec![
            "  ".into(),
            "URL: ".bold(),
            url.to_string().cyan().underlined(),
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
        lines.push(Line::from(vec!["> ".cyan().bold(), answer]));
    }
    lines.push(Line::from(action_line));
    lines
}
