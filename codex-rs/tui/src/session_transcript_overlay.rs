//! Full-screen transcript overlay used by the standalone session picker.

use std::io::Result;

use crate::key_hint::KeyBinding;
use crate::key_hint::KeyBindingListExt;
use crate::keymap::PagerKeymap;
use crate::tui;
use crate::tui::MouseScrollDirection;
use crate::tui::TuiEvent;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;

pub(crate) struct SessionTranscriptOverlay {
    lines: Vec<Line<'static>>,
    keymap: PagerKeymap,
    scroll_offset: usize,
    last_content_height: usize,
    is_done: bool,
}

impl SessionTranscriptOverlay {
    pub(crate) fn new(lines: Vec<Line<'static>>, keymap: PagerKeymap) -> Self {
        Self {
            lines,
            keymap,
            scroll_offset: usize::MAX,
            last_content_height: 0,
            is_done: false,
        }
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(key) => self.handle_key(tui, key),
            TuiEvent::Draw | TuiEvent::Resize => {
                tui.draw(u16::MAX, |frame| self.render(frame.area(), frame.buffer))?;
                Ok(())
            }
            TuiEvent::MouseScroll { direction, .. } => {
                let code = match direction {
                    MouseScrollDirection::Up => KeyCode::PageUp,
                    MouseScrollDirection::Down => KeyCode::PageDown,
                };
                self.handle_key(tui, KeyEvent::new(code, KeyModifiers::NONE))
            }
            TuiEvent::MouseClick(_)
            | TuiEvent::MouseDrag(_)
            | TuiEvent::MouseRelease(_)
            | TuiEvent::MouseMove(_)
            | TuiEvent::Paste(_) => Ok(()),
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }

    fn handle_key(&mut self, tui: &mut tui::Tui, key: KeyEvent) -> Result<()> {
        if self.keymap.close.is_pressed(key) || self.keymap.close_transcript.is_pressed(key) {
            self.is_done = true;
            return Ok(());
        }

        let page_height = self.last_content_height.max(1);
        if self.keymap.scroll_up.is_pressed(key) {
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        } else if self.keymap.scroll_down.is_pressed(key) {
            self.scroll_offset = self.scroll_offset.saturating_add(1);
        } else if self.keymap.page_up.is_pressed(key) {
            self.scroll_offset = self.scroll_offset.saturating_sub(page_height);
        } else if self.keymap.page_down.is_pressed(key) {
            self.scroll_offset = self.scroll_offset.saturating_add(page_height);
        } else if self.keymap.half_page_up.is_pressed(key) {
            self.scroll_offset = self
                .scroll_offset
                .saturating_sub(page_height.saturating_add(1) / 2);
        } else if self.keymap.half_page_down.is_pressed(key) {
            self.scroll_offset = self
                .scroll_offset
                .saturating_add(page_height.saturating_add(1) / 2);
        } else if self.keymap.jump_top.is_pressed(key) {
            self.scroll_offset = 0;
        } else if self.keymap.jump_bottom.is_pressed(key) {
            self.scroll_offset = usize::MAX;
        } else {
            return Ok(());
        }

        tui.frame_requester()
            .schedule_frame_in(crate::tui::TARGET_FRAME_INTERVAL);
        Ok(())
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let content = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(4),
        );
        self.last_content_height = usize::from(content.height);

        "T R A N S C R I P T"
            .dim()
            .render_ref(Rect::new(area.x, area.y, area.width, 1), buf);

        let paragraph = Paragraph::new(Text::from(self.lines.clone())).wrap(Wrap { trim: false });
        let max_scroll = paragraph
            .line_count(content.width)
            .saturating_sub(usize::from(content.height));
        self.scroll_offset = self.scroll_offset.min(max_scroll);
        paragraph
            .scroll((u16::try_from(self.scroll_offset).unwrap_or(u16::MAX), 0))
            .render(content, buf);

        let hints = Rect::new(
            area.x,
            content.bottom(),
            area.width,
            area.bottom().saturating_sub(content.bottom()),
        );
        self.render_hints(hints, buf);
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let first =
            |bindings: &[KeyBinding]| bindings.first().copied().into_iter().collect::<Vec<_>>();
        render_key_hints(
            Rect::new(area.x, area.y, area.width, 1),
            buf,
            &[
                (
                    first(&self.keymap.scroll_up)
                        .into_iter()
                        .chain(first(&self.keymap.scroll_down))
                        .collect(),
                    "to scroll",
                ),
                (
                    first(&self.keymap.page_up)
                        .into_iter()
                        .chain(first(&self.keymap.page_down))
                        .collect(),
                    "to page",
                ),
                (
                    first(&self.keymap.jump_top)
                        .into_iter()
                        .chain(first(&self.keymap.jump_bottom))
                        .collect(),
                    "to jump",
                ),
            ],
        );
        render_key_hints(
            Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
            buf,
            &[(first(&self.keymap.close), "to close")],
        );
    }
}

fn render_key_hints(area: Rect, buf: &mut Buffer, pairs: &[(Vec<KeyBinding>, &str)]) {
    let mut spans: Vec<Span<'static>> = vec![" ".into()];
    for (index, (keys, description)) in pairs.iter().enumerate() {
        if index > 0 {
            spans.push("   ".into());
        }
        for (key_index, key) in keys.iter().enumerate() {
            if key_index > 0 {
                spans.push("/".into());
            }
            spans.push(Span::from(key));
        }
        spans.push(" ".into());
        spans.push((*description).to_string().into());
    }
    Paragraph::new(Line::from(spans).dim()).render_ref(area, buf);
}
