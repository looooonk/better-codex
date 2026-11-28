use super::AgentPanel;
use super::Focus;
use super::Hit;
use crate::app_shell::agent_activity::AgentActivityState;
use crate::app_shell::agent_trace::clock;
use crate::app_shell::design::palette;
use crate::app_shell::scrollback_view::ScrollbackGeometry;
use crate::app_shell::scrollback_view::render_scrollback_frame_at;
use crate::app_shell::scrollback_view::scrollback_geometry;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::style::Color;

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
    plot: Rect,
    first: usize,
    columns: usize,
    row: usize,
    page: usize,
}

impl Layout {
    pub(super) fn new(panel: &AgentPanel, screen: Rect) -> Self {
        let geometry = scrollback_geometry(screen, 136, 38);
        let mut plot = geometry.body;
        let digits = panel.trace.messages.len().max(1).ilog10() as u16 + 1;
        let gutter = (digits + 2).min(plot.width.saturating_sub(1));
        plot.x += gutter;
        plot.width -= gutter;
        let columns = usize::from((plot.width / COLUMN_WIDTH).max(1)).min(panel.trace.lanes.len());
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
        if let Some(index) = panel.message.filter(|_| panel.focus == Focus::Message) {
            row = row.clamp(index.saturating_sub(page - 1), index);
        }
        Self {
            geometry,
            plot,
            columns,
            first,
            row,
            page,
        }
    }

    fn slot(&self, lane: usize) -> Rect {
        let header = self.geometry.header;
        let width = usize::from(self.plot.width);
        let index = lane - self.first;
        let left = index * width / self.columns;
        let right = (index + 1) * width / self.columns;
        Rect::new(
            self.plot.x + left as u16,
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
            self.plot.x
        } else if lane >= self.first + self.columns {
            self.plot.right().saturating_sub(1)
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
        let lane = (self.first..self.first + self.columns)
            .find(|i| self.center(*i) == p.x && panel.trace.lanes[*i].span_at(msg.at).is_some());
        if dy % ROW_HEIGHT == 1
            && let Some((a, b)) = self.arrow(panel, index)
            && (a.min(b)..=a.max(b)).contains(&p.x)
        {
            if let Some(i) = lane
                && i != msg.from
                && i != msg.to
            {
                return Some(Hit::Lane(i));
            }
            return msg.readable().then_some(Hit::Message(index));
        }
        lane.map(Hit::Lane)
    }
}

pub(in crate::app_shell) fn render(
    panel: &AgentPanel,
    agents: &AgentActivityState,
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
        let mut rows = detail.rows.borrow_mut();
        if rows.width != g.body.width {
            rows.lines = crate::app_shell::agent_text::markdown(
                detail.message.body.text(),
                &detail.cwd,
                Some(usize::from(g.body.width.max(1))),
            );
            rows.width = g.body.width;
        }
        let max = rows.lines.len().saturating_sub(usize::from(g.body.height));
        detail.max_scroll.set(max);
        detail
            .links
            .render(&rows.lines[detail.scroll.min(max)..], g.body, buf);
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
        let color = if i == 0 {
            Color::White
        } else {
            agents.color(&lane.target.thread_id)
        };
        if active { label.underlined() } else { label }
            .fg(color)
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
                "┃"
            } else {
                "╹"
            };
            for (y, symbol) in (g.body.y..g.body.bottom()).zip(["╻", "┃", end]) {
                buf[(layout.center(i), y)]
                    .set_symbol(symbol)
                    .set_fg(if i == 0 {
                        Color::White
                    } else {
                        agents.color(&panel.trace.lanes[i].target.thread_id)
                    });
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
            let top = if before == Some(span) { "┃" } else { "╻" };
            let bottom = if after == Some(span)
                || index + 1 == panel.trace.messages.len() && lane.span_is_open(span)
            {
                "┃"
            } else {
                "╹"
            };
            let color = if i == 0 {
                Color::White
            } else {
                agents.color(&lane.target.thread_id)
            };
            for (dy, symbol) in [top, "┃", bottom].into_iter().enumerate() {
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
        let chosen = msg.readable()
            && (hovered == Some(Hit::Message(index))
                || panel.focus == Focus::Message && panel.message == Some(index));
        let gutter = Rect::new(g.body.x, y + 1, layout.plot.x - g.body.x, 1);
        let index_label = format!("{}{} ", index + 1, if chosen { "›" } else { " " });
        Paragraph::new(index_label)
            .right_aligned()
            .fg(Color::DarkGray)
            .render(gutter, buf);
        if let Some((from, to)) = layout.arrow(panel, index) {
            let color = if msg.readable() {
                Color::White
            } else {
                Color::DarkGray
            };
            // The lane passes in front. Taper both ends of the gap so it cannot
            // be mistaken for an endpoint or a three-way junction.
            let crossings: Vec<_> = (layout.first..layout.first + layout.columns)
                .filter(|i| {
                    (msg.from.min(msg.to) + 1..msg.from.max(msg.to)).contains(i)
                        && panel.trace.lanes[*i].span_at(msg.at).is_some()
                })
                .map(|i| layout.center(i))
                .collect();
            for x in from.min(to)..=from.max(to) {
                let cell = &mut buf[(x, y + 1)];
                let symbol = if x == to {
                    if msg.from < msg.to { "▶" } else { "◀" }
                } else if x == from
                    && (layout.first..layout.first + layout.columns).contains(&msg.from)
                {
                    "●"
                } else if crossings.contains(&x) {
                    continue;
                } else if crossings.contains(&x.saturating_add(1)) {
                    "╴"
                } else if crossings.contains(&x.saturating_sub(1)) {
                    "╶"
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
