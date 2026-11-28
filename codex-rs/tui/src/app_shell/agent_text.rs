//! Agent popups use the transcript's Markdown renderer and clickable terminal rows.
use super::design::markdown_styles;
use crate::markdown_render::render_markdown_lines_with_width_cwd_and_styles;
use crate::terminal_hyperlinks::ClickTarget;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::TerminalHyperlink;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use std::cell::RefCell;
use std::path::Path;

pub(super) fn markdown(text: &str, cwd: &Path, width: Option<usize>) -> Vec<HyperlinkLine> {
    render_markdown_lines_with_width_cwd_and_styles(text, width, Some(cwd), markdown_styles())
}

#[derive(Default)]
pub(super) struct AgentTextLinks(RefCell<Vec<(Rect, TerminalHyperlink)>>);

impl AgentTextLinks {
    pub(super) fn render(&self, lines: &[HyperlinkLine], body: Rect, buf: &mut Buffer) {
        let mut regions = self.0.borrow_mut();
        regions.clear();
        for (y, line) in (body.y..body.bottom()).zip(lines) {
            line.render_fitted(Rect::new(body.x, y, body.width, 1), buf);
            for link in &line.hyperlinks {
                let start = link.columns.start.min(usize::from(body.width)) as u16;
                let end = link.columns.end.min(usize::from(body.width)) as u16;
                if start < end {
                    regions.push((Rect::new(body.x + start, y, end - start, 1), link.clone()));
                }
            }
        }
    }

    pub(super) fn at(&self, position: Position) -> Option<ClickTarget> {
        self.0
            .borrow()
            .iter()
            .find(|(area, _)| area.contains(position))
            .and_then(|(_, link)| link.click_target())
    }
}
