//! The effort picker previews a model-catalog value until Enter or pointer release.
//! Option zero restores the model default. The rail contains only explicit effort levels.

use super::SelectorOutcome;
use super::SelectorState;
use crate::app_shell::design::fill_rect;
use crate::app_shell::design::palette;
use crate::app_shell::design::pane_style;
use crate::app_shell::reasoning_ripple::ultra_color;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EffortSlider {
    model: String,
    default_stop: usize,
    ultra_stop: Option<usize>,
    pub(super) dragging: bool,
}

impl EffortSlider {
    pub(super) fn new(preset: &ModelPreset) -> Self {
        let stop = |effort: &ReasoningEffort| {
            preset
                .supported_reasoning_efforts
                .iter()
                .position(|level| &level.effort == effort)
                .map(|index| index + 1)
        };
        Self {
            model: preset.display_name.clone(),
            default_stop: stop(&preset.default_reasoning_effort).unwrap_or_default(),
            ultra_stop: stop(&ReasoningEffort::Ultra),
            dragging: false,
        }
    }

    pub(super) fn active_stop(&self, selected: usize) -> usize {
        if selected == 0 {
            self.default_stop
        } else {
            selected
        }
    }

    pub(super) fn render<T>(&self, panel: Rect, selector: &SelectorState<T>, buf: &mut Buffer) {
        if panel.height < 6 || panel.width < 8 {
            return;
        }
        fill_rect(buf, panel, palette::elevated());
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::border()))
            .style(pane_style(palette::elevated()))
            .render(panel, buf);
        let stop = self.active_stop(selector.selected);
        let label = &selector.options[stop].label;
        let title = if selector.selected == 0 && stop > 0 {
            format!("{label} (default)")
        } else {
            label.clone()
        };
        let inner = Rect::new(panel.x + 4, panel.y + 1, panel.width.saturating_sub(8), 1);
        Paragraph::new(title)
            .centered()
            .bold()
            .fg(palette::purple())
            .render(inner, buf);
        Paragraph::new("↺")
            .centered()
            .fg(palette::muted())
            .render(reset_area(panel), buf);
        Paragraph::new(self.model.as_str())
            .centered()
            .fg(palette::muted())
            .render(
                Rect {
                    y: inner.y + 1,
                    ..inner
                },
                buf,
            );
        let track = track_area(panel);
        let stops = selector.options.len().saturating_sub(1);
        let span = usize::from(track.width.saturating_sub(1));
        let intervals = stops.saturating_sub(1).max(1);
        let offset = |index: usize| (index.saturating_sub(1) * span + intervals / 2) / intervals;
        let thumb = offset(stop);
        for x in 0..usize::from(track.width) {
            let fill = if self.ultra_stop == Some(stop) {
                ultra_color(x, span)
            } else {
                palette::purple()
            };
            let (symbol, fg) = if x == thumb {
                ("●", palette::text())
            } else if (1..=stops).any(|index| offset(index) == x) {
                (
                    "•",
                    if x < thumb {
                        palette::dark()
                    } else {
                        palette::muted()
                    },
                )
            } else {
                (" ", palette::muted())
            };
            buf[(track.x + x as u16, track.y)]
                .set_symbol(symbol)
                .set_fg(fg)
                .set_bg(if x <= thumb { fill } else { palette::border() });
        }
        let hints = selector
            .key_hints
            .as_deref()
            .unwrap_or("← → adjust   Enter apply   Esc close");
        Paragraph::new(hints)
            .centered()
            .fg(palette::muted())
            .render(
                Rect::new(panel.x + 1, panel.bottom() - 2, panel.width - 2, 1),
                buf,
            );
    }
}

fn track_area(panel: Rect) -> Rect {
    Rect::new(panel.x + 2, panel.y + 3, panel.width.saturating_sub(4), 1).intersection(panel)
}

fn reset_area(panel: Rect) -> Rect {
    Rect::new(panel.right().saturating_sub(4), panel.y + 1, 3, 1).intersection(panel)
}

impl<T> SelectorState<T> {
    pub(in crate::app_shell) fn is_effort_slider(&self) -> bool {
        self.slider.is_some()
    }

    fn nearest_stop(&self, track: Rect, x: u16) -> usize {
        let stops = self.options.len().saturating_sub(1);
        let span = usize::from(track.width.saturating_sub(1)).max(1);
        let x = usize::from(x.saturating_sub(track.x)).min(span);
        (1 + (x * stops.saturating_sub(1) + span / 2) / span).min(stops)
    }

    pub(super) fn slider_stop_at(&self, panel: Rect, position: Position) -> Option<usize> {
        let track = track_area(panel);
        track
            .contains(position)
            .then(|| self.nearest_stop(track, position.x))
    }

    pub(in crate::app_shell) fn drag_slider(&mut self, panel: Rect, position: Position) {
        if self.slider.as_ref().is_some_and(|slider| slider.dragging) {
            self.selected = self.nearest_stop(track_area(panel), position.x);
        }
    }
}

impl<T: Clone> SelectorState<T> {
    pub(super) fn press_slider(&mut self, panel: Rect, position: Position) -> SelectorOutcome<T> {
        if !panel.contains(position) {
            return SelectorOutcome::Cancelled;
        }
        if reset_area(panel).contains(position) {
            self.selected = 0;
            return self.selected_value();
        }
        if let Some(stop) = self.slider_stop_at(panel, position)
            && let Some(slider) = &mut self.slider
        {
            slider.dragging = true;
            self.selected = stop;
        }
        SelectorOutcome::Pending
    }

    pub(in crate::app_shell) fn release_slider(
        &mut self,
        panel: Rect,
        position: Position,
    ) -> Option<SelectorOutcome<T>> {
        let slider = self.slider.as_mut()?;
        if !std::mem::take(&mut slider.dragging) {
            return None;
        }
        self.selected = self.nearest_stop(track_area(panel), position.x);
        Some(self.selected_value())
    }
}
