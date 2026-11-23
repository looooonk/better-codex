//! The effort picker previews a model-catalog value until Enter or pointer release.
//! Option zero restores the model default. The rail contains only explicit effort levels.

use super::Presentation;
use super::SelectorOutcome;
use super::SelectorState;
use crate::app_shell::design::fill_rect;
use crate::app_shell::design::palette;
use crate::app_shell::design::pane_style;
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

#[path = "effort_rail.rs"]
mod effort_rail;

use effort_rail::EffortRail;
use effort_rail::RAINBOW_FRAMES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EffortSlider {
    model: String,
    default_stop: usize,
    ultra_stop: Option<usize>,
    pub(super) dragging: bool,
    rainbow_frame: u16,
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
            rainbow_frame: 0,
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
        if panel.height < 7 || panel.width < 8 {
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
        Paragraph::new(selector.save_error.as_deref().unwrap_or(&self.model))
            .centered()
            .fg(if selector.save_error.is_some() {
                palette::error()
            } else {
                palette::muted()
            })
            .render(
                Rect {
                    y: inner.y + 1,
                    ..inner
                },
                buf,
            );
        let rail = selector.effort_rail(panel);
        let rainbow = (self.ultra_stop == Some(stop)).then_some(self.rainbow_frame);
        rail.render(stop, rainbow, buf);
    }
}

fn reset_area(panel: Rect) -> Rect {
    Rect::new(panel.right().saturating_sub(4), panel.y + 1, 3, 1).intersection(panel)
}

impl<T> SelectorState<T> {
    pub(in crate::app_shell) fn is_effort_slider(&self) -> bool {
        matches!(self.presentation, Presentation::Slider(_))
    }

    pub(in crate::app_shell) fn shows_ultra(&self) -> bool {
        matches!(&self.presentation, Presentation::Slider(s) if s.ultra_stop == Some(s.active_stop(self.selected)))
    }

    pub(in crate::app_shell) fn advance_rainbow(&mut self) {
        if let Presentation::Slider(slider) = &mut self.presentation {
            slider.rainbow_frame = (slider.rainbow_frame + 1) % RAINBOW_FRAMES;
        }
    }

    fn effort_rail(&self, panel: Rect) -> EffortRail {
        EffortRail::new(panel, self.options.len().saturating_sub(1))
    }

    pub(super) fn slider_stop_at(&self, panel: Rect, position: Position) -> Option<usize> {
        let rail = self.effort_rail(panel);
        rail.area
            .contains(position)
            .then(|| rail.nearest_stop(position.x))
    }

    pub(in crate::app_shell) fn drag_slider(&mut self, panel: Rect, position: Position) {
        if matches!(&self.presentation, Presentation::Slider(slider) if slider.dragging) {
            self.selected = self.effort_rail(panel).nearest_stop(position.x);
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
            && let Presentation::Slider(slider) = &mut self.presentation
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
        let Presentation::Slider(slider) = &mut self.presentation else {
            return None;
        };
        if !std::mem::take(&mut slider.dragging) {
            return None;
        }
        self.selected = self.effort_rail(panel).nearest_stop(position.x);
        Some(self.selected_value())
    }
}
