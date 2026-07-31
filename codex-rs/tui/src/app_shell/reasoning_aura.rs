use super::design::palette;
use crate::color::blend;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::time::Duration;
use std::time::Instant;

const AURA_DURATION: Duration = Duration::from_millis(/*millis*/ 800);
const OUTER_GLOW_ALPHA: f32 = 0.42;
const INNER_GLOW_ALPHA: f32 = 0.18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReasoningAuraTone {
    Max,
    Ultra,
}

impl ReasoningAuraTone {
    pub(super) fn for_transition(
        current: Option<&ReasoningEffort>,
        target: Option<&ReasoningEffort>,
    ) -> Option<Self> {
        if current == target {
            return None;
        }
        match target {
            Some(ReasoningEffort::Max) => Some(Self::Max),
            Some(ReasoningEffort::Ultra) => Some(Self::Ultra),
            Some(
                ReasoningEffort::None
                | ReasoningEffort::Minimal
                | ReasoningEffort::Low
                | ReasoningEffort::Medium
                | ReasoningEffort::High
                | ReasoningEffort::XHigh
                | ReasoningEffort::Custom(_),
            )
            | None => None,
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Max => palette::warning(),
            Self::Ultra => palette::purple(),
        }
    }
}

#[derive(Debug)]
pub(super) struct ReasoningAura {
    tone: ReasoningAuraTone,
    expires_at: Instant,
}

impl ReasoningAura {
    pub(super) fn new(tone: ReasoningAuraTone, now: Instant) -> Self {
        Self {
            tone,
            expires_at: now + AURA_DURATION,
        }
    }

    pub(super) fn expires_at(&self) -> Instant {
        self.expires_at
    }

    pub(super) fn render(&self, area: Rect, buf: &mut Buffer, now: Instant) {
        if now >= self.expires_at {
            return;
        }
        let Color::Rgb(aura_red, aura_green, aura_blue) = self.tone.color() else {
            return;
        };
        let Color::Rgb(base_red, base_green, base_blue) = palette::base() else {
            return;
        };
        let aura_rgb = (aura_red, aura_green, aura_blue);
        let base_rgb = (base_red, base_green, base_blue);

        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let alpha = match distance_from_edge(area, x, y) {
                    0 => OUTER_GLOW_ALPHA,
                    1 => INNER_GLOW_ALPHA,
                    _ => continue,
                };
                let Some(cell) = buf.cell_mut((x, y)) else {
                    continue;
                };
                let background_rgb = match cell.style().bg {
                    Some(Color::Rgb(red, green, blue)) => (red, green, blue),
                    Some(
                        Color::Reset
                        | Color::Black
                        | Color::Red
                        | Color::Green
                        | Color::Yellow
                        | Color::Blue
                        | Color::Magenta
                        | Color::Cyan
                        | Color::Gray
                        | Color::DarkGray
                        | Color::LightRed
                        | Color::LightGreen
                        | Color::LightYellow
                        | Color::LightBlue
                        | Color::LightMagenta
                        | Color::LightCyan
                        | Color::White
                        | Color::Indexed(_),
                    )
                    | None => base_rgb,
                };
                let (red, green, blue) = blend(aura_rgb, background_rgb, alpha);
                cell.set_bg(Color::Rgb(red, green, blue));
            }
        }
    }
}

fn distance_from_edge(area: Rect, x: u16, y: u16) -> u16 {
    let right = area.right().saturating_sub(1);
    let bottom = area.bottom().saturating_sub(1);
    x.saturating_sub(area.x)
        .min(right.saturating_sub(x))
        .min(y.saturating_sub(area.y))
        .min(bottom.saturating_sub(y))
}

#[cfg(test)]
#[path = "reasoning_aura_tests.rs"]
mod tests;
