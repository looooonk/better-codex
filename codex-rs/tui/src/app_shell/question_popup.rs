use super::PendingUserInput;
use super::composer_layout::ComposerLayout;
use super::composer_layout::composer_viewport;
use super::composer_render::composer_lines;
use super::composer_render::viewport_cursor_position;
use super::design::palette;
use super::input_request_layout::RequestPanelSegment;
use super::input_request_layout::wrapped_segments;
use super::request_popup;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuestionHit {
    Option(usize),
    Editor,
    Back,
    Submit,
}

struct FooterButton {
    label: Span<'static>,
    area: Rect,
    action: QuestionHit,
}

/// Keep wrapped button rows, editor viewport, and mouse targets on the same geometry.
pub(super) struct QuestionPopup {
    pub(super) panel: Rect,
    content: Rect,
    segments: Vec<RequestPanelSegment>,
    options: Range<usize>,
    pub(super) editor: Rect,
    buttons: [Option<FooterButton>; 2],
    scroll_max: usize,
    editor_layout: ComposerLayout,
}

impl QuestionPopup {
    pub(super) fn new(pending: &PendingUserInput, transcript: Rect) -> Self {
        let width = request_popup::width(transcript).saturating_sub(4);
        let question = pending.current_question();
        let mut lines = vec![];
        if let Some(question) = question {
            lines.extend(
                question
                    .question
                    .lines()
                    .map(|line| Line::from(line.to_owned()).bold()),
            );
        }
        lines.push(Line::default());
        let start = lines.len();
        for (index, option) in pending.options().iter().enumerate() {
            let selected = index == pending.draft().selected;
            let marker = if selected { "● " } else { "○ " };
            let label = format!("{marker}{}", option.label);
            let label = if selected {
                label.fg(palette::focus()).bold()
            } else {
                label.fg(palette::text())
            };
            let mut spans = vec![label];
            if !option.description.is_empty() {
                spans.push(format!("  {}", option.description).fg(palette::muted()));
            }
            lines.push(spans.into());
        }
        let options = start..lines.len();
        if let Some(delay) = pending.auto_resolution_ms() {
            lines.push(
                Line::from(format!("Continues after {}s if unanswered", delay / 1000))
                    .fg(palette::muted()),
            );
        }
        if let Some(error) = &pending.error {
            lines.push(Line::from(error.clone()).fg(palette::error()));
        }
        let mut segments = wrapped_segments(&lines, width);
        let display = pending.draft().editor.display();
        let editor_layout = ComposerLayout::new(display.text(), width.into());
        let editor_rows = if pending.allows_text() {
            if pending.editing() {
                editor_layout
                    .cursor(display.cursor())
                    .line_count
                    .clamp(1, 6)
            } else {
                1
            }
        } else {
            0
        };
        let (current, total) = pending.question_position();
        let footer = current > 1 || pending.editing();
        let panel = request_popup::area(
            transcript,
            segments.len() + editor_rows + usize::from(footer),
        );
        let body = request_popup::body(panel);
        let footer_rows = u16::from(footer && body.height > 2);
        let editor_height = (editor_rows as u16).min(body.height.saturating_sub(footer_rows + 1));
        let content = Rect {
            height: body.height.saturating_sub(editor_height + footer_rows),
            ..body
        };
        let editor = Rect::new(body.x, content.bottom(), width, editor_height);
        let mut x = body.x;
        let mut button = |label: Span<'static>, action| {
            let width = (label.width() as u16).min(body.right().saturating_sub(x));
            let area = Rect::new(x, editor.bottom(), width, footer_rows);
            x = area.right().saturating_add(2);
            FooterButton {
                label,
                area,
                action,
            }
        };
        let buttons = [
            (current > 1).then(|| button("Previous".fg(palette::muted()), QuestionHit::Back)),
            pending.editing().then(|| {
                let label = if current < total { "Next" } else { "Submit" };
                button(label.fg(palette::focus()), QuestionHit::Submit)
            }),
        ];
        let scroll_max = segments.len().saturating_sub(content.height.into());
        let mut offset = pending.scroll.get().min(scroll_max);
        if pending.reveal_selection.replace(false) {
            let line = options.start + pending.draft().selected;
            if let Some(row) = segments.iter().position(|s| s.logical_line == line) {
                offset = offset
                    .min(row)
                    .max((row + 1).saturating_sub(content.height.into()));
            }
        }
        pending.scroll.set(offset);
        segments = segments
            .into_iter()
            .skip(offset)
            .take(content.height.into())
            .collect();
        Self {
            panel,
            content,
            segments,
            options,
            editor,
            buttons,
            scroll_max,
            editor_layout,
        }
    }

    pub(super) fn scroll(&self, pending: &PendingUserInput, amount: isize) {
        pending.scroll.set(
            pending
                .scroll
                .get()
                .saturating_add_signed(amount)
                .min(self.scroll_max),
        );
    }

    pub(super) fn hit(&self, pos: Position) -> Option<QuestionHit> {
        if self.editor.contains(pos) {
            return Some(QuestionHit::Editor);
        }
        for button in self.buttons.iter().flatten() {
            if button.area.contains(pos) {
                return Some(button.action);
            }
        }
        if !self.content.contains(pos) {
            return None;
        }
        let line = self
            .segments
            .get(usize::from(pos.y - self.content.y))?
            .logical_line;
        self.options
            .contains(&line)
            .then(|| QuestionHit::Option(line - self.options.start))
    }

    pub(super) fn cursor(&self, pending: &PendingUserInput) -> Option<Position> {
        if !pending.editing() {
            return None;
        }
        let display = pending.draft().editor.display();
        viewport_cursor_position(&composer_viewport(
            self.editor,
            self.editor_layout.cursor(display.cursor()),
        )?)
    }

    pub(super) fn render(
        &self,
        pending: &PendingUserInput,
        pointer: Option<Position>,
        buf: &mut Buffer,
    ) {
        let (current, total) = pending.question_position();
        let mut title = " Question".to_string();
        if total > 1 {
            title.push_str(&format!(" {current}/{total}"));
        }
        if let Some(q) = pending.current_question().filter(|q| !q.header.is_empty()) {
            title.push_str(&format!(" · {}", q.header));
        }
        title.push(' ');
        request_popup::render_frame(self.panel, &title, buf);
        if self.scroll_max > 0 {
            let border = Rect::new(
                self.content.x,
                self.panel.bottom().saturating_sub(1),
                self.content.width,
                1,
            );
            Line::from(" PgUp/PgDn ".fg(palette::muted()))
                .right_aligned()
                .render(border, buf);
        }
        let hovered = pointer.and_then(|pos| self.hit(pos));
        for (row, segment) in self.content.rows().zip(&self.segments) {
            segment.content.clone().render(row, buf);
            if matches!(hovered, Some(QuestionHit::Option(i)) if self.options.start + i == segment.logical_line)
            {
                buf.set_style(row, Style::new().bg(palette::border()));
            }
        }
        let draft = &pending.draft().editor;
        if pending.editing() && !draft.is_empty() {
            let display = draft.display();
            let cursor = self.editor_layout.cursor(display.cursor());
            if let Some(view) = composer_viewport(self.editor, cursor) {
                let end = cursor
                    .line_count
                    .min(view.visible_start + view.visible_height);
                let mut lines = composer_lines(
                    display.text(),
                    false,
                    &self.editor_layout,
                    display.selection_range(),
                    view.visible_start..end,
                );
                if pending.current_question().is_some_and(|q| q.is_secret) {
                    for line in &mut lines {
                        for span in line.spans.iter_mut().skip(1) {
                            span.content = "*".repeat(span.width()).into();
                        }
                    }
                }
                Paragraph::new(lines).render(self.editor, buf);
            }
        } else {
            let label = if pending.editing() {
                "> Type an answer"
            } else {
                "○ Write an answer…"
            };
            Line::from(label.fg(palette::muted())).render(self.editor, buf);
        }
        if matches!(hovered, Some(QuestionHit::Editor)) && !pending.editing() {
            buf.set_style(self.editor, Style::new().bg(palette::border()));
        }
        for button in self.buttons.iter().flatten() {
            (&button.label).render(button.area, buf);
            if hovered == Some(button.action) {
                buf.set_style(button.area, Style::new().bg(palette::border()));
            }
        }
    }
}
