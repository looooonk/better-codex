use super::super::design::palette;
use super::super::terminal_output;
use crate::session_transcript::TranscriptLines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use ratatui::style::Stylize;
use ratatui::text::Line;

pub(super) fn document_lines(title: &str, output: &str, width: usize) -> TranscriptLines {
    let mut lines = vec![Line::from("Tool call").fg(palette::cyan()).bold()];
    lines.extend(
        word_wrap_lines(
            codex_ansi_escape::ansi_escape(&terminal_output::normalize(title)).lines,
            RtOptions::new(width)
                .initial_indent(Line::from("│ ".fg(palette::cyan())))
                .subsequent_indent(Line::from("│ ".fg(palette::cyan()))),
        )
        .into_iter()
        .map(|line| line.bg(palette::elevated())),
    );
    lines.push(Line::default());
    lines.push(Line::from("Output").fg(palette::cyan()).bold());
    if output.is_empty() {
        lines.push(Line::from("No output captured yet.").fg(palette::muted()));
    } else {
        for line in codex_ansi_escape::ansi_escape(output).lines {
            lines.extend(word_wrap_lines(
                [line],
                RtOptions::new(width).subsequent_indent(Line::from("↪ ".fg(palette::muted()))),
            ));
        }
    }
    lines
}
