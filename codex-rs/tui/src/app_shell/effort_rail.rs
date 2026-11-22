//! A terminal cell is roughly twice as tall as it is wide. Half blocks let both
//! pills share a round outline while leaving a full cell for each centered dot.

use crate::app_shell::design::palette;
use crate::color::blend;
use crate::terminal_palette::rgb_color;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

pub(super) const RAINBOW_FRAMES: u16 = 40;

pub(super) struct EffortRail {
    pub(super) area: Rect,
    stops: usize,
    first: u16,
    last: u16,
}

impl EffortRail {
    pub(super) fn new(panel: Rect, stops: usize) -> Self {
        let mut area = Rect::new(panel.x + 2, panel.y + 3, panel.width.saturating_sub(4), 3)
            .intersection(panel);
        if stops <= 1 {
            let width = if stops == 0 { 0 } else { area.width.min(5) };
            area.x += (area.width - width) / 2;
            area.width = width;
        }
        let inset = 2.min(area.width.saturating_sub(1) / 2);
        let first = area.x + inset;
        let last = area.right().saturating_sub(1 + inset).max(first);
        Self {
            area,
            stops,
            first,
            last,
        }
    }

    fn center(&self, stop: usize) -> u16 {
        let intervals = self.stops.saturating_sub(1).max(1);
        let span = usize::from(self.last - self.first);
        self.first + ((stop.saturating_sub(1) * span + intervals / 2) / intervals) as u16
    }

    pub(super) fn nearest_stop(&self, x: u16) -> usize {
        (1..=self.stops)
            .min_by_key(|&stop| self.center(stop).abs_diff(x))
            .unwrap_or_default()
    }

    pub(super) fn render(&self, selected: usize, rainbow_frame: Option<u16>, buf: &mut Buffer) {
        let thumb = self.center(selected);
        let track = palette::border();
        let panel = palette::elevated();
        for x in self.area.left()..self.area.right() {
            let fill = rainbow_frame.map_or_else(palette::purple, |frame| {
                let position = f32::from(x - self.area.x) / f32::from(self.area.width);
                rainbow_color(position - f32::from(frame) / f32::from(RAINBOW_FRAMES))
            });
            // Cap coverage in a two-column radius. Each rim half-row has less
            // coverage than the middle row at the same distance from the cap center.
            let cap_distance = |end: u16| self.first.saturating_sub(x).max(x.saturating_sub(end));
            let strip = |coverage: [f32; 3]| {
                let outer = coverage
                    .get(usize::from(cap_distance(self.last)))
                    .copied()
                    .unwrap_or_default();
                let filled = coverage
                    .get(usize::from(cap_distance(thumb)))
                    .copied()
                    .unwrap_or_default();
                if outer == 0.0 {
                    return panel;
                }
                mix(panel, mix(track, fill, filled / outer), outer)
            };
            let rim = strip([1.0, 0.8, 0.06]);
            let middle = strip([1.0, 1.0, 0.43]);
            let center_row = self.area.y + 1;
            buf[(x, self.area.y)]
                .set_symbol("▄")
                .set_fg(rim)
                .set_bg(panel);
            buf[(x, self.area.y + 2)]
                .set_symbol("▀")
                .set_fg(rim)
                .set_bg(panel);
            let cell = &mut buf[(x, center_row)];
            cell.set_symbol(" ").set_bg(middle);
            if x == thumb {
                cell.set_symbol("●").set_fg(palette::text());
            } else if (1..=self.stops).any(|stop| self.center(stop) == x) {
                cell.set_symbol("•").set_fg(if x < thumb {
                    palette::dark()
                } else {
                    palette::muted()
                });
            }
        }
    }
}

fn rainbow_color(phase: f32) -> Color {
    let colors = [
        palette::error(),
        palette::warning(),
        palette::success(),
        palette::cyan(),
        palette::focus(),
        palette::purple(),
    ];
    let position = phase.rem_euclid(1.0) * colors.len() as f32;
    let index = position as usize % colors.len();
    mix(
        colors[index],
        colors[(index + 1) % colors.len()],
        position.fract(),
    )
}

fn mix(base: Color, tint: Color, weight: f32) -> Color {
    match (base, tint) {
        (Color::Rgb(r, g, b), Color::Rgb(tr, tg, tb)) => {
            rgb_color(blend((tr, tg, tb), (r, g, b), weight))
        }
        _ if weight >= 0.5 => tint,
        _ => base,
    }
}

#[cfg(test)]
#[path = "effort_rail_tests.rs"]
mod tests;
