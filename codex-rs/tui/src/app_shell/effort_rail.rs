//! The effort track and its tick marks use the same positions as pointer selection.

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
}

impl EffortRail {
    pub(super) fn new(panel: Rect, stops: usize) -> Self {
        let mut area = Rect::new(panel.x + 2, panel.y + 3, panel.width.saturating_sub(4), 2)
            .intersection(panel);
        if stops <= 1 {
            let width = if stops == 0 { 0 } else { area.width.min(5) };
            area.x += (area.width - width) / 2;
            area.width = width;
        }
        Self { area, stops }
    }

    fn stop_x(&self, stop: usize) -> u16 {
        let span = usize::from(self.area.width.saturating_sub(1));
        let intervals = self.stops.saturating_sub(1);
        let offset = if intervals == 0 {
            span / 2
        } else {
            ((stop - 1) * span + intervals / 2) / intervals
        };
        self.area.x + offset as u16
    }

    pub(super) fn nearest_stop(&self, x: u16) -> usize {
        (1..=self.stops)
            .min_by_key(|&stop| self.stop_x(stop).abs_diff(x))
            .unwrap_or_default()
    }

    pub(super) fn render(&self, selected: usize, rainbow_frame: Option<u16>, buf: &mut Buffer) {
        if self.area.height < 2 || self.area.width == 0 {
            return;
        }
        let selected_x = (selected > 0).then(|| self.stop_x(selected));
        let fill_end = if selected == self.stops {
            self.area.right()
        } else {
            selected_x.map_or(self.area.x, |x| x + 1)
        };
        let bar_y = self.area.y;
        for x in self.area.left()..self.area.right() {
            let color = if x >= fill_end {
                palette::border()
            } else if let Some(frame) = rainbow_frame {
                let position = f32::from(x - self.area.x) / f32::from(self.area.width);
                rainbow_color(position - f32::from(frame) / f32::from(RAINBOW_FRAMES))
            } else {
                palette::purple()
            };
            buf[(x, bar_y)].set_symbol("█").set_fg(color).set_bg(color);
        }
        for stop in 1..=self.stops {
            let x = self.stop_x(stop);
            let (symbol, color) = if Some(x) == selected_x {
                ("▴", palette::text())
            } else {
                ("╵", palette::muted())
            };
            buf[(x, bar_y + 1)].set_symbol(symbol).set_fg(color);
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
    let base = colors[index];
    let tint = colors[(index + 1) % colors.len()];
    let weight = position.fract();
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
