use super::ShellState;
use super::ToolActivity;
use super::TranscriptKind;
use super::TranscriptLine;
use crate::tui;
use codex_app_server_protocol::TurnPlanStepStatus;
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
        if !self.shell.streaming_plan.is_empty() {
            lines.extend(transcript_lines(
                &TranscriptLine::new(TranscriptKind::Plan, self.shell.streaming_plan.clone()),
                area.width.saturating_sub(2) as usize,
            ));
        }
        if !self.shell.streaming_assistant.is_empty() {
            lines.extend(transcript_lines(
                &TranscriptLine::new(
                    TranscriptKind::Assistant,
                    self.shell.streaming_assistant.clone(),
                ),
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
        let context_window = self
            .shell
            .model_context_window
            .map(|window| window.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let mut lines = vec![
            Line::from("Status".bold()),
            status_line(&self.shell.status),
            Line::from(""),
            Line::from("Model".bold()),
            Line::from(self.shell.model.clone()),
            Line::from(""),
            Line::from("Thread".bold()),
            Line::from(short_thread_id(&self.shell.thread_id.to_string())),
            Line::from(""),
            Line::from("Tokens".bold()),
            Line::from(format!("total {}", self.shell.token_usage.total_tokens)),
            Line::from(format!("input {}", self.shell.token_usage.input_tokens)),
            Line::from(format!("output {}", self.shell.token_usage.output_tokens)),
            Line::from(format!("context {context_window}")),
        ];

        lines.push(Line::from(""));
        lines.push(Line::from("Diff".bold()));
        if let Some(diff) = &self.shell.latest_diff {
            lines.push(Line::from(format!(
                "{} files +{} -{}",
                diff.files, diff.additions, diff.removals
            )));
        } else {
            lines.push(Line::from("no changes".dim()));
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Plan".bold()));
        if let Some(explanation) = &self.shell.plan_explanation {
            lines.push(Line::from(explanation.clone().dim()));
        }
        if self.shell.plan_steps.is_empty() {
            lines.push(Line::from("no active plan".dim()));
        } else {
            for step in self.shell.plan_steps.iter().take(5) {
                lines.push(plan_step_line(step.status, &step.step));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Tools".bold()));
        if self.shell.tool_activity.is_empty() {
            lines.push(Line::from("idle".dim()));
        } else {
            for activity in self.shell.tool_activity.iter().rev().take(4).rev() {
                lines.push(tool_activity_line(activity));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from("Workspace".bold()));
        lines.push(Line::from(self.shell.cwd.clone()));
        lines.push(Line::from(format!("{:?}", self.shell.permission_profile)));
        lines.push(Line::from(""));
        lines.push(Line::from("Keys".bold()));
        lines.push(Line::from("Enter send"));
        lines.push(Line::from("Esc exit"));
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::LEFT).title("Dashboard"))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

fn transcript_lines(line: &TranscriptLine, width: usize) -> Vec<Line<'static>> {
    let width = width.max(12);
    let (label, style): (&str, LineStyle) = match line.kind {
        TranscriptKind::System => ("system", LineStyle::Dim),
        TranscriptKind::User => ("you", LineStyle::Cyan),
        TranscriptKind::Assistant => ("codex", LineStyle::Magenta),
        TranscriptKind::Plan => ("plan", LineStyle::Green),
        TranscriptKind::Tool => ("tool", LineStyle::Cyan),
        TranscriptKind::Diff => ("diff", LineStyle::Green),
        TranscriptKind::Output => ("output", LineStyle::Dim),
        TranscriptKind::Status => ("status", LineStyle::Dim),
        TranscriptKind::Error => ("error", LineStyle::Red),
    };
    let subsequent_indent = " ".repeat(label.len() + 2);
    let options = textwrap::Options::new(width)
        .initial_indent("")
        .subsequent_indent(&subsequent_indent);
    textwrap::wrap(&line.text, options)
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
    Green,
    Magenta,
    Red,
}

impl LineStyle {
    fn label(self, text: &str) -> Span<'static> {
        match self {
            Self::Cyan => text.to_string().cyan().bold(),
            Self::Dim => text.to_string().dim().bold(),
            Self::Green => text.to_string().green().bold(),
            Self::Magenta => text.to_string().magenta().bold(),
            Self::Red => text.to_string().red().bold(),
        }
    }

    fn text(self, text: String) -> Span<'static> {
        match self {
            Self::Cyan => text.into(),
            Self::Dim => text.dim(),
            Self::Green => text.green(),
            Self::Magenta => text.into(),
            Self::Red => text.red(),
        }
    }
}

fn status_line(status: &str) -> Line<'static> {
    match status {
        "ready" => Line::from(status.to_string().green()),
        "failed" | "error" | "disconnected" => Line::from(status.to_string().red()),
        "thinking" | "reasoning" | "retrying" => Line::from(status.to_string().cyan()),
        "interrupted" => Line::from(status.to_string().magenta()),
        _ => Line::from(status.to_string()),
    }
}

fn plan_step_line(status: TurnPlanStepStatus, step: &str) -> Line<'static> {
    let marker = match status {
        TurnPlanStepStatus::Pending => "-".dim(),
        TurnPlanStepStatus::InProgress => ">".cyan().bold(),
        TurnPlanStepStatus::Completed => "x".green(),
    };
    Line::from(vec![marker, " ".dim(), step.to_string().into()])
}

fn tool_activity_line(activity: &ToolActivity) -> Line<'static> {
    let status = match activity.status.as_str() {
        "completed" => activity.status.clone().green(),
        "failed" | "declined" => activity.status.clone().red(),
        "in progress" | "inprogress" => activity.status.clone().cyan(),
        _ => activity.status.clone().dim(),
    };
    Line::from(vec![
        status,
        " ".dim(),
        compact_dashboard_text(&activity.title).into(),
    ])
}

fn short_thread_id(thread_id: &str) -> String {
    thread_id
        .get(..8)
        .map(|prefix| format!("{prefix}..."))
        .unwrap_or_else(|| thread_id.to_string())
}

fn compact_dashboard_text(text: &str) -> String {
    const MAX_CHARS: usize = 24;
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let mut compact = text.chars().take(MAX_CHARS).collect::<String>();
    compact.push_str("...");
    compact
}
