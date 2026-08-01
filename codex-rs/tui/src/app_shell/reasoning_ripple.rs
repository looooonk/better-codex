use super::design::palette;
use crate::color::blend;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::time::Duration;
use std::time::Instant;

const RIPPLE_WAVE_DURATION: Duration = Duration::from_millis(/*millis*/ 750);
const FOLLOWING_WAVE_DELAY: Duration = Duration::from_millis(/*millis*/ 150);
const RIPPLE_DURATION: Duration = RIPPLE_WAVE_DURATION.saturating_add(FOLLOWING_WAVE_DELAY);
pub(super) const FRAME_INTERVAL: Duration = Duration::from_millis(/*millis*/ 33);
const RING_GRADIENT_WIDTH: f32 = 7.0;
const VERTICAL_DISTANCE_SCALE: f32 = 8.0;
const MAX_TRAVEL_FRACTION: f32 = 0.7;
const MAX_TRAVEL_DISTANCE: f32 = 42.0;
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
        let wave_progress = |delay| {
            let elapsed = elapsed.checked_sub(delay)?;
            if elapsed >= RIPPLE_WAVE_DURATION {
                return None;
            }
            let sampled_elapsed = (elapsed.as_secs_f32() / FRAME_INTERVAL.as_secs_f32()).floor()
                * FRAME_INTERVAL.as_secs_f32();
            Some((sampled_elapsed / RIPPLE_WAVE_DURATION.as_secs_f32()).clamp(0.0, 1.0))
        };
        Some(ReasoningRippleFrame {
            tone: self.tone,
            wave_progresses: [Duration::ZERO, FOLLOWING_WAVE_DELAY].map(wave_progress),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ReasoningRippleFrame {
    tone: ReasoningRippleTone,
    wave_progresses: [Option<f32>; 2],
}

impl ReasoningRippleFrame {
    pub(super) fn render(
        self,
        area: Rect,
        origin: Rect,
        paint_areas: impl IntoIterator<Item = Rect>,
        buf: &mut Buffer,
    ) {
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
        let center_x = f32::from(origin.x) + (f32::from(origin.width) - 1.0) / 2.0;
        let center_y = f32::from(origin.y) + (f32::from(origin.height) - 1.0) / 2.0;
        let left_distance = center_x - f32::from(area.x);
        let right_distance = f32::from(area.right().saturating_sub(1)) - center_x;
        let far_edge_distance = left_distance.max(right_distance).max(1.0);
        let fade_radius = (far_edge_distance * MAX_TRAVEL_FRACTION).clamp(1.0, MAX_TRAVEL_DISTANCE);

        for paint_area in paint_areas {
            let paint_area = paint_area.intersection(area);
            for (x, y) in paint_area
                .positions()
                .map(|position| (position.x, position.y))
            {
                let horizontal_distance = f32::from(x) - center_x;
                let vertical_distance = (f32::from(y) - center_y) * VERTICAL_DISTANCE_SCALE;
                let distance = horizontal_distance.hypot(vertical_distance);
                let distance_ratio = (distance / fade_radius).clamp(0.0, 1.0);
                let distance_alpha =
                    1.0 - distance_ratio * distance_ratio * (3.0 - 2.0 * distance_ratio);
                let alpha = self
                    .wave_progresses
                    .into_iter()
                    .flatten()
                    .map(|progress| {
                        let radius = progress * (fade_radius + RING_GRADIENT_WIDTH);
                        let gradient =
                            (1.0 - (distance - radius).abs() / RING_GRADIENT_WIDTH).clamp(0.0, 1.0);
                        PEAK_ALPHA * gradient * gradient * distance_alpha
                    })
                    .fold(0.0_f32, f32::max);
                if alpha == 0.0 {
                    continue;
                }
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

#[cfg(test)]
#[path = "reasoning_ripple_tests.rs"]
mod tests;
