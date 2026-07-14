use super::PendingApproval;
use super::PendingElicitation;
use super::PendingUserInput;
use super::design::palette;
use crate::text_formatting::truncate_text;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) fn approval_lines(pending: &PendingApproval) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "? ".fg(palette::WARNING).bold(),
            pending.title().to_string().fg(palette::TEXT).bold(),
        ]),
        Line::from(vec![
            "  ".into(),
            pending.detail().to_string().fg(palette::MUTED),
        ]),
        Line::from(vec![
            "  ".into(),
            " Approve ↵ ".fg(palette::DARK).bg(palette::SUCCESS).bold(),
            " ".into(),
            " Deny n ".fg(palette::TEXT).bg(palette::ERROR).bold(),
            " ".into(),
            " Edit e ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
            " ".into(),
            " Explain ? ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
        ]),
    ]
}

pub(super) fn user_input_lines(
    pending: &PendingUserInput,
    composer_text: &str,
    is_empty: bool,
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
    }

    let secret = pending
        .current_question()
        .is_some_and(|question| question.is_secret);
    let answer = if is_empty {
        "answer".dim()
    } else if secret {
        "[hidden]".dim()
    } else {
        composer_text.to_string().into()
    };
    let mut answer_line = vec!["> ".cyan().bold(), answer];
    if let Some(question) = pending.current_question()
        && let Some(options) = question.options.as_ref()
    {
        answer_line.push("  ".dim());
        answer_line.extend(
            options
                .iter()
                .take(3)
                .enumerate()
                .flat_map(|(index, option)| {
                    vec![
                        format!("{} ", index + 1).green().bold(),
                        option.label.clone().dim(),
                        "  ".dim(),
                    ]
                }),
        );
    }
    lines.push(Line::from(answer_line));
    lines
}

pub(super) fn elicitation_lines(pending: &PendingElicitation) -> Vec<Line<'static>> {
    let mut action_line = vec!["  ".into()];
    if pending.can_accept() {
        action_line.extend([
            " Accept ↵ ".fg(palette::DARK).bg(palette::SUCCESS).bold(),
            " ".into(),
        ]);
    }
    action_line.extend([
        " Decline d ".fg(palette::TEXT).bg(palette::ERROR).bold(),
        " ".into(),
        " Cancel c ".fg(palette::TEXT).bg(palette::ELEVATED).bold(),
    ]);

    vec![
        Line::from(vec!["? ".cyan().bold(), pending.title().to_string().bold()]),
        Line::from(vec![
            "  ".into(),
            truncate_text(pending.detail(), /*max_graphemes*/ 42).dim(),
        ]),
        Line::from(action_line),
    ]
}
