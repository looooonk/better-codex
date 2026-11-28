use super::AgentPanel;
use super::Focus;
use super::Hit;
use crate::app_shell::agent_trace::Body;
use crate::app_shell::agent_trace::clock;
use crate::app_shell::design::palette;
use crate::app_shell::scrollback_view::ScrollbackGeometry;
use crate::app_shell::scrollback_view::render_scrollback_frame_at;
use crate::app_shell::scrollback_view::scrollback_geometry;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use ratatui::buffer::Buffer;
use ratatui::layout::Alignment;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const ROW_HEIGHT: u16 = 3;
const COLUMN_WIDTH: u16 = 13;

pub(super) struct Layout {
    pub(super) geometry: ScrollbackGeometry,
    first: usize,
    columns: usize,
    row: usize,
    page: usize,
}

impl Layout {
    pub(super) fn new(panel: &AgentPanel, screen: Rect) -> Self {
        let geometry = scrollback_geometry(screen, 136, 38);
        let columns =
            usize::from((geometry.body.width / COLUMN_WIDTH).max(1)).min(panel.trace.lanes.len());
        let page = usize::from(geometry.body.height / ROW_HEIGHT).max(1);
        let mut first = panel
            .first_lane
            .get()
            .min(panel.trace.lanes.len().saturating_sub(columns));
        let mut row = panel
            .first_row
            .get()
            .min(panel.trace.messages.len().saturating_sub(page));
        if panel.focus == Focus::Lane && panel.lane < panel.trace.lanes.len() {
            first = first.clamp(
                panel.lane.saturating_sub(columns.saturating_sub(1)),
                panel.lane,
            );
        }
        if panel.focus == Focus::Message && panel.message < panel.trace.messages.len() {
            row = row.clamp(panel.message.saturating_sub(page - 1), panel.message);
        }
        Self {
            geometry,
            columns,
            first,
            row,
            page,
        }
    }

    fn slot(&self, lane: usize) -> Rect {
        let header = self.geometry.header;
        let width = usize::from(header.width);
        let index = lane - self.first;
        let left = index * width / self.columns;
        let right = (index + 1) * width / self.columns;
        Rect::new(
            header.x + left as u16,
            header.y,
            (right - left) as u16,
            header.height.min(1),
        )
    }

    fn center(&self, lane: usize) -> u16 {
        let slot = self.slot(lane);
        slot.x + slot.width / 2
    }

    fn x(&self, lane: usize) -> u16 {
        if lane < self.first {
            self.geometry.body.x
        } else if lane >= self.first + self.columns {
            self.geometry.body.right().saturating_sub(1)
        } else {
            self.center(lane)
        }
    }

    fn arrow(&self, panel: &AgentPanel, index: usize) -> Option<(u16, u16)> {
        let msg = panel.trace.messages.get(index)?;
        if msg.from.min(msg.to) >= self.first + self.columns || msg.from.max(msg.to) < self.first {
            return None;
        }
        Some((self.x(msg.from), self.x(msg.to)))
    }

    pub(super) fn hit(&self, panel: &AgentPanel, p: Position) -> Option<Hit> {
        let g = self.geometry;
        if g.header.contains(p) && p.y == g.header.y {
            if self.first > 0 && p.x == g.header.x {
                return Some(Hit::EarlierLanes);
            }
            if self.first + self.columns < panel.trace.lanes.len() && p.x + 1 == g.header.right() {
                return Some(Hit::LaterLanes);
            }
            return (self.first..self.first + self.columns)
                .find(|i| self.slot(*i).contains(p))
                .map(Hit::Lane);
        }
        if !g.body.contains(p) {
            return None;
        }
        let dy = p.y - g.body.y;
        let index = self.row + usize::from(dy / ROW_HEIGHT);
        if index >= self.row + self.page {
            return None;
        }
        if panel.trace.messages.is_empty() && dy < ROW_HEIGHT {
            return (self.first..self.first + self.columns)
                .find(|i| self.center(*i) == p.x)
                .map(Hit::Lane);
        }
        let msg = panel.trace.messages.get(index)?;
        if dy % ROW_HEIGHT == 1
            && let Some((a, b)) = self.arrow(panel, index)
            && (a.min(b)..=a.max(b)).contains(&p.x)
        {
            return Some(Hit::Message(index));
        }
        (self.first..self.first + self.columns)
            .find(|i| self.center(*i) == p.x && panel.trace.lanes[*i].span_at(msg.at).is_some())
            .map(Hit::Lane)
    }
}

pub(in crate::app_shell) fn render(
    panel: &AgentPanel,
    screen: Rect,
    hover: Option<Position>,
    buf: &mut Buffer,
) {
    let layout = Layout::new(panel, screen);
    let g = layout.geometry;
    if let Some(detail) = &panel.detail {
        render_scrollback_frame_at(screen, g, format!(" {} ", detail.route), buf);
        Line::from(format!(
            "{}  ·  {}",
            detail.message.kind,
            clock(detail.message.at)
        ))
        .fg(palette::muted())
        .render(g.header, buf);
        let lines = detail
            .message
            .body
            .text()
            .lines()
            .map(|line| Line::from(line.to_owned()))
            .collect::<Vec<_>>();
        let lines = word_wrap_lines(lines, RtOptions::new(usize::from(g.body.width.max(1))));
        let max = lines.len().saturating_sub(usize::from(g.body.height));
        detail.max_scroll.set(max);
        let lines = lines
            .into_iter()
            .skip(detail.scroll.min(max))
            .take(g.body.height.into())
            .collect::<Vec<_>>();
        let color = if matches!(*detail.message.body, Body::Text(_)) {
            palette::text()
        } else {
            palette::muted()
        };
        Paragraph::new(lines).fg(color).render(g.body, buf);
        Line::from(if max > 0 {
            "↑↓ scroll   Esc back"
        } else {
            "Esc back"
        })
        .fg(palette::muted())
        .render(g.footer, buf);
        return;
    }
    render_scrollback_frame_at(screen, g, " Agent communications ", buf);
    if g.body.is_empty() {
        return;
    }
    panel.page.set(layout.page);
    panel.columns.set(layout.columns.max(1));
    panel.first_lane.set(layout.first);
    panel.first_row.set(layout.row);
    let hovered = hover.and_then(|p| layout.hit(panel, p));
    for i in layout.first..layout.first + layout.columns {
        let lane = &panel.trace.lanes[i];
        let slot = layout.slot(i);
        let line = truncate_line_with_ellipsis_if_overflow(
            Line::from(lane.target.display_name.clone()),
            usize::from(slot.width.saturating_sub(2)),
        );
        let active = hovered == Some(Hit::Lane(i)) || panel.focus == Focus::Lane && panel.lane == i;
        let label = Paragraph::new(line).alignment(Alignment::Center).bold();
        if active {
            label.underlined().fg(palette::cyan())
        } else {
            label.fg(palette::text())
        }
        .render(slot, buf);
    }
    if g.header.width > 0 && g.header.height > 0 {
        if layout.first > 0 {
            buf[(g.header.x, g.header.y)]
                .set_symbol("‹")
                .set_fg(palette::cyan());
        }
        if layout.first + layout.columns < panel.trace.lanes.len() {
            buf[(g.header.right() - 1, g.header.y)]
                .set_symbol("›")
                .set_fg(palette::cyan());
        }
    }
    if panel.trace.messages.is_empty() {
        let label = if panel.error {
            "History unavailable"
        } else if panel.task.is_some() && panel.trace.lanes.is_empty() {
            "Loading…"
        } else {
            "No messages"
        };
        for i in layout.first..layout.first + layout.columns {
            let end = if panel.trace.lanes[i].running() {
                "│"
            } else {
                "╵"
            };
            for (y, symbol) in (g.body.y..g.body.bottom()).zip(["╷", "│", end]) {
                buf[(layout.center(i), y)]
                    .set_symbol(symbol)
                    .set_fg(palette::muted());
            }
        }
        let mut empty = g.body;
        if layout.columns > 0 && empty.height > ROW_HEIGHT {
            empty.y += ROW_HEIGHT;
            empty.height -= ROW_HEIGHT;
        }
        Paragraph::new(label)
            .centered()
            .fg(palette::muted())
            .render(empty, buf);
    }
    let end = (layout.row + layout.page).min(panel.trace.messages.len());
    for index in layout.row..end {
        let msg = &panel.trace.messages[index];
        let y = g.body.y + (index - layout.row) as u16 * ROW_HEIGHT;
        for i in layout.first..layout.first + layout.columns {
            let lane = &panel.trace.lanes[i];
            let Some(span) = lane.span_at(msg.at) else {
                continue;
            };
            let before = index
                .checked_sub(1)
                .and_then(|i| panel.trace.messages.get(i))
                .and_then(|m| lane.span_at(m.at));
            let after = panel
                .trace
                .messages
                .get(index + 1)
                .and_then(|m| lane.span_at(m.at));
            let top = if before == Some(span) { "│" } else { "╷" };
            let bottom = if after == Some(span)
                || index + 1 == panel.trace.messages.len() && lane.span_is_open(span)
            {
                "│"
            } else {
                "╵"
            };
            let color =
                if hovered == Some(Hit::Lane(i)) || panel.focus == Focus::Lane && panel.lane == i {
                    palette::cyan()
                } else {
                    palette::muted()
                };
            for (dy, symbol) in [top, "│", bottom].into_iter().enumerate() {
                if y + (dy as u16) < g.body.bottom() {
                    buf[(layout.center(i), y + dy as u16)]
                        .set_symbol(symbol)
                        .set_fg(color);
                }
            }
        }
        if y + 1 >= g.body.bottom() {
            continue;
        }
        if panel.focus == Focus::Message && index == panel.message && g.body.width > 0 {
            buf[(g.body.x, y + 1)]
                .set_symbol("›")
                .set_fg(palette::cyan());
        }
        if let Some((from, to)) = layout.arrow(panel, index) {
            let color = if hovered == Some(Hit::Message(index))
                || panel.focus == Focus::Message && panel.message == index
            {
                palette::cyan()
            } else {
                palette::text()
            };
            for x in from.min(to)..=from.max(to) {
                let cell = &mut buf[(x, y + 1)];
                let symbol = if x == to {
                    if msg.from < msg.to { "▶" } else { "◀" }
                } else if x == from
                    && (layout.first..layout.first + layout.columns).contains(&msg.from)
                {
                    "●"
                } else if cell.symbol() == "│" {
                    "┼"
                } else {
                    "─"
                };
                cell.set_symbol(symbol).set_fg(color);
            }
        }
    }
    let range = if end > 0 {
        format!("{}–{end}/{}", layout.row + 1, panel.trace.messages.len())
    } else {
        String::new()
    };
    let status = if panel.error {
        "Refresh failed"
    } else if panel.trace.partial {
        "Partial history"
    } else {
        ""
    };
    let hint = if g.footer.width >= 56 {
        "↑↓ messages   ←→ agents   Enter open   Esc"
    } else {
        "↑↓  ←→  Enter  Esc"
    };
    let footer = [hint, status, &range]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("   ");
    Line::from(footer)
        .fg(palette::muted())
        .render(g.footer, buf);
}
