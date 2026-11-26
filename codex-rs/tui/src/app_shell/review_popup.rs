use super::ShellState;
use super::approval::json_detail;
use super::approval::protocol_label;
use super::design::palette;
use super::guardian_activity::ToolApproval;
use super::guardian_activity::action_summary;
use super::scrollback_view::ScrollbackGeometry;
use super::scrollback_view::render_scrollback_frame_at;
use super::scrollback_view::scrollback_geometry;
use crate::wrapping::word_wrap_lines;
use codex_app_server_protocol::GuardianApprovalReviewAction;
use codex_app_server_protocol::GuardianApprovalReviewStatus;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::cell::Cell;

pub(super) struct ReviewPopup {
    pub(super) approval: ToolApproval,
    offset: Cell<usize>,
    page: Cell<usize>,
    max_offset: Cell<usize>,
}

impl ShellState {
    pub(super) fn open_tool_review(&mut self, item_index: usize, review_index: usize) {
        let Some(approval) = self
            .transcript
            .get(item_index)
            .and_then(|item| item.approvals.get(review_index))
            .cloned()
        else {
            return;
        };
        self.clear_transcript_selection();
        self.close_tool_output();
        self.tool_review = Some(ReviewPopup {
            approval,
            offset: Cell::new(0),
            page: Cell::new(1),
            max_offset: Cell::new(0),
        });
    }
}

impl ReviewPopup {
    fn details(&self) -> Vec<Line<'static>> {
        let review = &self.approval.review;
        let action = &self.approval.action;
        let mut lines = Vec::new();
        if let Some(reason) = review
            .rationale
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            lines.extend(
                reason
                    .lines()
                    .map(|line| Line::from(line.to_owned().fg(palette::text()))),
            );
            lines.push(Line::default());
        }
        let mut field = |label: &str, value: &str| {
            for (index, line) in value.lines().enumerate() {
                let mut row = Line::default();
                if index == 0 {
                    row.push_span(format!("{label}  ").fg(palette::muted()));
                }
                row.push_span(line.to_owned().fg(palette::text()));
                lines.push(row);
            }
        };
        match action {
            GuardianApprovalReviewAction::Command { command, cwd, .. } => {
                field("Command", command);
                field("Working directory", &cwd.to_string());
            }
            GuardianApprovalReviewAction::Execve {
                program, argv, cwd, ..
            } => {
                field("Command", &action_summary(action));
                if argv.first().is_some_and(|first| first != program) {
                    field("Executable", program);
                }
                field("Working directory", &cwd.display().to_string());
            }
            GuardianApprovalReviewAction::WriteStdin {
                process_id,
                stdin,
                cwd,
                ..
            } => {
                field("Process", process_id);
                field("Input", &format!("{stdin:?}"));
                field("Working directory", &cwd.to_string());
            }
            GuardianApprovalReviewAction::ApplyPatch { files, cwd } => {
                for file in files {
                    field("File", &file.to_string());
                }
                field("Working directory", &cwd.to_string());
            }
            GuardianApprovalReviewAction::NetworkAccess {
                target,
                host,
                protocol,
                port,
            } => {
                field("Network target", target);
                field(
                    "Endpoint",
                    &format!("{}://{host}:{port}", protocol_label(*protocol)),
                );
            }
            GuardianApprovalReviewAction::McpToolCall {
                server,
                tool_name,
                connector_name,
                connector_id,
                ..
            } => {
                field("Tool", &format!("{server}/{tool_name}"));
                if let Some(name) = connector_name.as_ref().or(connector_id.as_ref()) {
                    field("Connector", name);
                }
            }
            GuardianApprovalReviewAction::RequestPermissions {
                reason,
                permissions,
            } => {
                if let Some(reason) = reason {
                    field("Request", reason);
                }
                lines.push(Line::from(
                    json_detail("Permissions", permissions).fg(palette::text()),
                ));
            }
        }
        if review.status == GuardianApprovalReviewStatus::Denied {
            lines.push(Line::from(
                format!("Review ID  {}", self.approval.id).fg(palette::muted()),
            ));
        }
        lines
    }

    fn layout(&self, screen: Rect) -> (ScrollbackGeometry, Vec<Line<'static>>) {
        let frame = scrollback_geometry(screen, 80, 32);
        let rows = word_wrap_lines(self.details(), usize::from(frame.body.width.max(1)));
        // Two borders, a two-line header and a footer surround the scrollable details.
        let height = u16::try_from(rows.len())
            .unwrap_or(u16::MAX)
            .saturating_add(5)
            .min(32);
        (scrollback_geometry(screen, 80, height), rows)
    }

    pub(super) fn panel(&self, screen: Rect) -> Rect {
        self.layout(screen).0.modal
    }

    pub(super) fn scroll(&self, key: KeyCode) {
        let offset = self.offset.get();
        let max = self.max_offset.get();
        let next = match key {
            KeyCode::Up | KeyCode::Char('k') => offset.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => offset.saturating_add(1),
            KeyCode::PageDown => offset.saturating_add(self.page.get()),
            KeyCode::PageUp => offset.saturating_sub(self.page.get()),
            KeyCode::End | KeyCode::Char('G') => max,
            KeyCode::Home | KeyCode::Char('g') => 0,
            _ => return,
        };
        self.offset.set(next.min(max));
    }

    pub(super) fn render(&self, screen: Rect, buf: &mut Buffer) {
        let (geometry, rows) = self.layout(screen);
        render_scrollback_frame_at(screen, geometry, " Auto-review ", buf);
        let (status, color) = self.approval.status();
        let mut header = Line::from(status.fg(color).bold());
        if let Some(risk) = self.approval.review.risk_level {
            header.push_span(
                format!("  ·  {risk:?} risk")
                    .to_lowercase()
                    .fg(palette::muted()),
            );
        }
        if let Some(auth) = self.approval.review.user_authorization {
            header.push_span(
                format!("  ·  {auth:?} authorization")
                    .to_lowercase()
                    .fg(palette::muted()),
            );
        }
        Paragraph::new(header)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .render(geometry.header, buf);
        let page = usize::from(geometry.body.height);
        let max = rows.len().saturating_sub(page);
        self.page.set(page);
        self.max_offset.set(max);
        let offset = self.offset.get().min(max);
        self.offset.set(offset);
        let visible = rows.into_iter().skip(offset).take(page).collect::<Vec<_>>();
        Paragraph::new(visible).render(geometry.body, buf);
        let footer = if max == 0 {
            "Esc close"
        } else {
            "↑↓ scroll   Esc close"
        };
        Paragraph::new(footer.fg(palette::muted())).render(geometry.footer, buf);
    }
}
