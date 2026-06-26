use super::ShellState;
use super::TranscriptLine;
use crate::tui;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

pub(super) fn draw_shell(tui: &mut tui::Tui, shell: &ShellState) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        ShellView { shell }.render(frame.area(), frame.buffer);
    })
}

pub(super) struct ShellView<'a> {
    pub(super) shell: &'a ShellState,
}

impl ShellView<'_> {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(48), Constraint::Length(30)])
            .split(area);
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(4),
            ])
            .split(horizontal[0]);

        self.render_header(main[0], buf);
        self.render_transcript(main[1], buf);
        self.render_input(main[2], buf);
        self.render_dashboard(horizontal[1], buf);
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let title = vec![
            "Better Codex".magenta().bold(),
            "  ".into(),
            self.shell.status.clone().cyan(),
        ];
        Paragraph::new(Line::from(title))
            .block(Block::default().borders(Borders::BOTTOM))
            .render(area, buf);
    }

    fn render_transcript(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();
        for line in &self.shell.transcript {
            lines.extend(transcript_lines(
                line,
                area.width.saturating_sub(2) as usize,
            ));
        }
        if !self.shell.streaming_assistant.is_empty() {
            lines.extend(transcript_lines(
                &TranscriptLine::Assistant(self.shell.streaming_assistant.clone()),
                area.width.saturating_sub(2) as usize,
            ));
        }
        let visible_count = area.height.saturating_sub(2) as usize;
        let visible_from = lines.len().saturating_sub(visible_count);
        Paragraph::new(lines.into_iter().skip(visible_from).collect::<Vec<_>>())
            .block(Block::default().borders(Borders::ALL).title("Conversation"))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        let text = if self.shell.input.is_empty() {
            "Type a message and press Enter".dim()
        } else {
            self.shell.input.clone().into()
        };
        Paragraph::new(Line::from(vec!["> ".cyan(), text]))
            .block(Block::default().borders(Borders::TOP).title("Composer"))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        let context = self
            .shell
            .model_context_window
            .map(|window| window.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let lines = vec![
            Line::from("Status".bold()),
            Line::from(self.shell.status.clone()),
            Line::from(""),
            Line::from("Model".bold()),
            Line::from(self.shell.model.clone()),
            Line::from(""),
            Line::from("Thread".bold()),
            Line::from(self.shell.thread_id.to_string()),
            Line::from(""),
            Line::from("Tokens".bold()),
            Line::from(format!("total {}", self.shell.token_usage.total_tokens)),
            Line::from(format!("input {}", self.shell.token_usage.input_tokens)),
            Line::from(format!("output {}", self.shell.token_usage.output_tokens)),
            Line::from(format!("context {context}")),
            Line::from(""),
            Line::from("Permissions".bold()),
            Line::from(format!("{:?}", self.shell.permission_profile)),
            Line::from(""),
            Line::from("Plan".bold()),
            Line::from("stage 1 shell"),
            Line::from("dashboard pending".dim()),
            Line::from(""),
            Line::from("Keys".bold()),
            Line::from("Enter send"),
            Line::from("Esc exit"),
        ];
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title("Dashboard"))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

fn transcript_lines(line: &TranscriptLine, width: usize) -> Vec<Line<'static>> {
    let width = width.max(12);
    let (label, text, style): (&str, &str, LineStyle) = match line {
        TranscriptLine::System(text) => ("system", text, LineStyle::Dim),
        TranscriptLine::User(text) => ("you", text, LineStyle::Cyan),
        TranscriptLine::Assistant(text) => ("codex", text, LineStyle::Magenta),
        TranscriptLine::Status(text) => ("status", text, LineStyle::Dim),
        TranscriptLine::Error(text) => ("error", text, LineStyle::Red),
    };
    let subsequent_indent = " ".repeat(label.len() + 2);
    let options = textwrap::Options::new(width)
        .initial_indent("")
        .subsequent_indent(&subsequent_indent);
    textwrap::wrap(text, options)
        .into_iter()
        .map(|wrapped| {
            Line::from(vec![
                style.label(label),
                ": ".dim(),
                style.text(wrapped.into_owned()),
            ])
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum LineStyle {
    Cyan,
    Dim,
    Magenta,
    Red,
}

impl LineStyle {
    fn label(self, text: &str) -> Span<'static> {
        match self {
            Self::Cyan => text.to_string().cyan().bold(),
            Self::Dim => text.to_string().dim().bold(),
            Self::Magenta => text.to_string().magenta().bold(),
            Self::Red => text.to_string().red().bold(),
        }
    }

    fn text(self, text: String) -> Span<'static> {
        match self {
            Self::Cyan => text.into(),
            Self::Dim => text.dim(),
            Self::Magenta => text.into(),
            Self::Red => text.red(),
        }
    }
}
