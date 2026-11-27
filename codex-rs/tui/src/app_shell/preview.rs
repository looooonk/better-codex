//! Read-only overlays share keyboard, mouse, and focus handling.
use super::ShellState;
use super::design::palette;
use super::scrollback_view::ScrollbackGeometry;
use super::scrollback_view::render_scrollback_frame_at;
use super::scrollback_view::scrollback_geometry;
use super::scrollback_view::scrollback_panel_area;
use super::source_preview::SourcePreview;
use crate::wrapping::word_wrap_lines;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::cell::Cell;
use std::cell::RefCell;

pub(super) enum Preview {
    Source(SourcePreview),
    Command(CommandReport),
}

impl Preview {
    pub(super) fn panel(&self, screen: Rect) -> Rect {
        match self {
            Self::Source(_) => scrollback_panel_area(screen),
            Self::Command(report) => report.layout(screen).modal,
        }
    }

    pub(super) fn scroll(&mut self, key: KeyCode) {
        match self {
            Self::Source(source) => source.scroll(key),
            Self::Command(report) => report.scroll(key),
        }
    }

    pub(super) fn render(&self, screen: Rect, buf: &mut Buffer) {
        match self {
            Self::Source(source) => source.render(screen, buf),
            Self::Command(report) => report.render(screen, buf),
        }
    }
}

pub(super) struct CommandReport {
    title: &'static str,
    body: String,
    wrapped: RefCell<Vec<Line<'static>>>,
    width: Cell<u16>,
    page: Cell<usize>,
    offset: Cell<usize>,
    copy_status: Option<String>,
}

impl ShellState {
    pub(super) fn show_command_report(&mut self, title: &'static str, body: impl Into<String>) {
        self.input_hint = None;
        self.preview = Some(Preview::Command(CommandReport {
            title,
            body: body.into(),
            wrapped: RefCell::default(),
            width: Cell::default(),
            page: Cell::default(),
            offset: Cell::default(),
            copy_status: None,
        }));
    }

    pub(super) fn report_command_error(&mut self, context: &str, error: color_eyre::Report) {
        self.status = "action failed".to_string();
        self.show_command_report("Command failed", format!("{context}: {error:#}"));
    }

    pub(super) fn command_feedback(&mut self, title: &'static str, message: impl Into<String>) {
        let message = message.into();
        let width = self.composer_layout.borrow().width;
        let hint_width = if width == 0 {
            56
        } else {
            width.saturating_sub(2).min(72)
        };
        if message.contains('\n')
            || unicode_width::UnicodeWidthStr::width(message.as_str()) > hint_width
        {
            self.show_command_report(title, message);
        } else {
            self.show_hint(message);
        }
    }
}

impl CommandReport {
    pub(super) fn copy(&mut self, lease: &mut Option<crate::clipboard_copy::ClipboardLease>) {
        self.copy_status = Some(match crate::clipboard_copy::copy_to_clipboard(&self.body) {
            Ok(result) => {
                *lease = result;
                "Copied".to_owned()
            }
            Err(error) => format!("Copy failed: {error}"),
        });
    }

    fn layout(&self, screen: Rect) -> ScrollbackGeometry {
        let width = scrollback_geometry(screen, 76, 30).body.width.max(1);
        if self.width.replace(width) != width {
            *self.wrapped.borrow_mut() = word_wrap_lines(self.body.lines(), usize::from(width));
        }
        let height = u16::try_from(self.wrapped.borrow().len()).unwrap_or(u16::MAX);
        scrollback_geometry(screen, 76, height.saturating_add(5).min(30))
    }

    fn scroll(&self, key: KeyCode) {
        let offset = self.offset.get();
        let page = self.page.get();
        let max = self.wrapped.borrow().len().saturating_sub(page);
        let next = match key {
            KeyCode::Up | KeyCode::Char('k') => offset.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => offset.saturating_add(1),
            KeyCode::PageUp => offset.saturating_sub(page),
            KeyCode::PageDown => offset.saturating_add(page),
            KeyCode::Home | KeyCode::Char('g') => 0,
            KeyCode::End | KeyCode::Char('G') => max,
            _ => return,
        };
        self.offset.set(next.min(max));
    }

    fn render(&self, screen: Rect, buf: &mut Buffer) {
        let frame = self.layout(screen);
        render_scrollback_frame_at(screen, frame, format!(" {} ", self.title), buf);
        // Use the frame's header as top padding; the title sits in its border.
        let body = Rect::new(
            frame.body.x,
            frame.header.y.saturating_add(1),
            frame.body.width,
            frame
                .body
                .height
                .saturating_add(frame.header.height.saturating_sub(1)),
        );
        let rows = self.wrapped.borrow();
        let page = usize::from(body.height);
        let offset = self.offset.get().min(rows.len().saturating_sub(page));
        self.page.set(page);
        self.offset.set(offset);
        let end = (offset + page).min(rows.len());
        Paragraph::new(rows[offset..end].to_vec()).render(body, buf);
        let copy = self.copy_status.as_deref().unwrap_or("y copy");
        let mut footer = format!("Esc close   {copy}");
        if rows.len() > page {
            footer.push_str(&format!("   ↑↓ {}–{end}/{}", offset + 1, rows.len()));
        }
        Paragraph::new(footer.fg(palette::muted())).render(frame.footer, buf);
    }
}
