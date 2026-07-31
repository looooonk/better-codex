use super::design::palette;
use crate::color::blend;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::time::Duration;
use std::time::Instant;

const RIPPLE_DURATION: Duration = Duration::from_millis(/*millis*/ 850);
pub(super) const FRAME_INTERVAL: Duration = Duration::from_millis(/*millis*/ 33);
const LEADING_GRADIENT_WIDTH: f32 = 3.0;
const TRAILING_GRADIENT_WIDTH: f32 = 12.0;
const PEAK_ALPHA: f32 = 0.48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReasoningRippleTone {
    Max,
    Ultra,
}

impl ReasoningRippleTone {
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
pub(super) struct ReasoningRipple {
    tone: ReasoningRippleTone,
    started_at: Instant,
    expires_at: Instant,
}

impl ReasoningRipple {
    pub(super) fn new(tone: ReasoningRippleTone, now: Instant) -> Self {
        Self {
            tone,
            started_at: now,
            expires_at: now + RIPPLE_DURATION,
        }
    }

    pub(super) fn is_expired(&self, now: Instant) -> bool {
        now >= self.expires_at
    }

    pub(super) fn frame(&self, now: Instant) -> Option<ReasoningRippleFrame> {
        if self.is_expired(now) {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.started_at);
        let sampled_elapsed = (elapsed.as_secs_f32() / FRAME_INTERVAL.as_secs_f32()).floor()
            * FRAME_INTERVAL.as_secs_f32();
        let progress = sampled_elapsed / RIPPLE_DURATION.as_secs_f32();
        Some(ReasoningRippleFrame {
            tone: self.tone,
            progress: progress.clamp(0.0, 1.0),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ReasoningRippleFrame {
    tone: ReasoningRippleTone,
    progress: f32,
}

impl ReasoningRippleFrame {
    pub(super) fn render(self, area: Rect, origin: Rect, buf: &mut Buffer) {
        if area.is_empty() || origin.is_empty() {
            return;
        }
        let Color::Rgb(ripple_red, ripple_green, ripple_blue) = self.tone.color() else {
            return;
        };
        let Color::Rgb(base_red, base_green, base_blue) = palette::dark() else {
            return;
        };
        let ripple_rgb = (ripple_red, ripple_green, ripple_blue);
        let base_rgb = (base_red, base_green, base_blue);
        let left_distance = origin.x.saturating_sub(area.x);
        let right_distance = area.right().saturating_sub(origin.right());
        let max_distance = f32::from(left_distance.max(right_distance));
        let radius = self.progress * (max_distance + TRAILING_GRADIENT_WIDTH);
        let lifetime_alpha = 1.0 - self.progress * 0.25;

        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let distance = f32::from(horizontal_distance_from(origin, x));
                let wave_offset = distance - radius;
                let gradient = if wave_offset >= 0.0 {
                    1.0 - wave_offset / LEADING_GRADIENT_WIDTH
                } else {
                    1.0 + wave_offset / TRAILING_GRADIENT_WIDTH
                }
                .clamp(0.0, 1.0);
                if gradient == 0.0 {
                    continue;
                }
                let vertical_distance = f32::from(y.abs_diff(origin.y));
                let vertical_alpha = 1.0 / (1.0 + vertical_distance * 0.7);
                let alpha = PEAK_ALPHA * gradient * gradient * vertical_alpha * lifetime_alpha;
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
                let (red, green, blue) = blend(ripple_rgb, background_rgb, alpha);
                cell.set_bg(Color::Rgb(red, green, blue));
            }
        }
    }
}

fn horizontal_distance_from(origin: Rect, x: u16) -> u16 {
    if x < origin.x {
        origin.x - x
    } else if x >= origin.right() {
        x.saturating_sub(origin.right()).saturating_add(1)
    } else {
        0
    }
}

#[cfg(test)]
#[path = "reasoning_ripple_tests.rs"]
mod tests;
