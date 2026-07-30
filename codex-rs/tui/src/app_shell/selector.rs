use super::settings::app_theme_label;
use super::settings::reasoning_effort_label;
use codex_app_server_protocol::AskForApproval;
use codex_config::types::TuiAppTheme;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelServiceTier;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::layout::Position;
use ratatui::layout::Rect;

#[path = "selector_view.rs"]
mod view;

use view::OPTION_HEIGHT;
use view::selector_geometry;

#[cfg(test)]
use super::design::palette;
#[cfg(test)]
use ratatui::buffer::Buffer;
#[cfg(test)]
use view::MAX_MODAL_HEIGHT;
#[cfg(test)]
use view::MAX_MODAL_WIDTH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ReasoningEffortValue {
    Default,
    Explicit(ReasoningEffort),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ServiceTierValue {
    Default,
    Explicit(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SelectorValue {
    Model(String),
    ReasoningEffort(ReasoningEffortValue),
    ServiceTier(ServiceTierValue),
    ApprovalPolicy(AskForApproval),
    AppTheme(TuiAppTheme),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SelectorOption<T> {
    value: T,
    label: String,
    detail: String,
    current: bool,
}

impl<T> SelectorOption<T> {
    pub(super) fn new(value: T, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            detail: detail.into(),
            current: false,
        }
    }

    pub(super) fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SelectorOutcome<T> {
    Pending,
    Cancelled,
    Selected(T),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SelectorState<T> {
    title: String,
    options: Vec<SelectorOption<T>>,
    selected: usize,
}

impl<T> SelectorState<T> {
    pub(super) fn new(title: impl Into<String>, options: Vec<SelectorOption<T>>) -> Self {
        let selected = options
            .iter()
            .position(|option| option.current)
            .unwrap_or_default();
        Self {
            title: title.into(),
            options,
            selected,
        }
    }

    pub(super) fn option_at(&self, area: Rect, position: Position) -> Option<usize> {
        let geometry = selector_geometry(area, self.options.len());
        if geometry.visible_options == 0 || !geometry.options.contains(position) {
            return None;
        }
        let visible_index =
            usize::from(position.y.saturating_sub(geometry.options.y) / OPTION_HEIGHT);
        if visible_index >= geometry.visible_options {
            return None;
        }
        let index = self
            .visible_scroll(geometry.visible_options)
            .saturating_add(visible_index);
        (index < self.options.len()).then_some(index)
    }

    fn set_selected(&mut self, selected: usize) {
        self.selected = selected.min(self.options.len().saturating_sub(1));
    }

    fn visible_scroll(&self, viewport_len: usize) -> usize {
        if self.options.is_empty() {
            return 0;
        }
        let viewport_len = viewport_len.max(1);
        let max_scroll = self.options.len().saturating_sub(viewport_len);
        self.selected
            .saturating_add(1)
            .saturating_sub(viewport_len)
            .min(max_scroll)
    }
}

impl<T: Clone> SelectorState<T> {
    pub(super) fn handle_key(&mut self, key: KeyEvent) -> SelectorOutcome<T> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            || !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return SelectorOutcome::Pending;
        }
        match key.code {
            KeyCode::Esc => SelectorOutcome::Cancelled,
            KeyCode::Enter => self.selected_value(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.set_selected(self.selected.saturating_sub(1));
                SelectorOutcome::Pending
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.set_selected(
                    self.selected
                        .saturating_add(1)
                        .min(self.options.len().saturating_sub(1)),
                );
                SelectorOutcome::Pending
            }
            KeyCode::PageUp => {
                self.set_selected(self.selected.saturating_sub(5));
                SelectorOutcome::Pending
            }
            KeyCode::PageDown => {
                self.set_selected(
                    self.selected
                        .saturating_add(5)
                        .min(self.options.len().saturating_sub(1)),
                );
                SelectorOutcome::Pending
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.set_selected(0);
                SelectorOutcome::Pending
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.set_selected(self.options.len().saturating_sub(1));
                SelectorOutcome::Pending
            }
            KeyCode::Char(digit @ '1'..='9') => {
                let index = digit.to_digit(10).unwrap_or_default() as usize - 1;
                if index < self.options.len() {
                    self.set_selected(index);
                    self.selected_value()
                } else {
                    SelectorOutcome::Pending
                }
            }
            _ => SelectorOutcome::Pending,
        }
    }

    pub(super) fn select_at(&mut self, area: Rect, position: Position) -> SelectorOutcome<T> {
        if !selector_geometry(area, self.options.len())
            .modal
            .contains(position)
        {
            return SelectorOutcome::Cancelled;
        }
        let Some(index) = self.option_at(area, position) else {
            return SelectorOutcome::Pending;
        };
        self.set_selected(index);
        self.selected_value()
    }

    fn selected_value(&self) -> SelectorOutcome<T> {
        self.options
            .get(self.selected)
            .map(|option| SelectorOutcome::Selected(option.value.clone()))
            .unwrap_or(SelectorOutcome::Pending)
    }
}

impl SelectorState<SelectorValue> {
    pub(super) fn models(current: &str, models: &[ModelPreset]) -> Self {
        let mut options = models
            .iter()
            .filter(|model| model.show_in_picker)
            .map(|model| {
                let label = if model.display_name.trim().is_empty() {
                    model.model.clone()
                } else {
                    model.display_name.clone()
                };
                let detail = if model.description.trim().is_empty() {
                    model.model.clone()
                } else {
                    format!("{} - {}", model.model, model.description)
                };
                SelectorOption::new(SelectorValue::Model(model.model.clone()), label, detail)
                    .current(model.model == current)
            })
            .collect::<Vec<_>>();
        if options.is_empty() && !current.trim().is_empty() {
            options.push(
                SelectorOption::new(
                    SelectorValue::Model(current.to_string()),
                    current,
                    "Model metadata is unavailable; keep the current model.",
                )
                .current(true),
            );
        }
        Self::new("Select model", options)
    }

    pub(super) fn reasoning_efforts(
        current: &ReasoningEffortValue,
        efforts: &[ReasoningEffortPreset],
    ) -> Self {
        let mut options = vec![
            SelectorOption::new(
                SelectorValue::ReasoningEffort(ReasoningEffortValue::Default),
                "Default",
                "Use the model's default reasoning effort.",
            )
            .current(current == &ReasoningEffortValue::Default),
        ];
        options.extend(efforts.iter().map(|preset| {
            let value = ReasoningEffortValue::Explicit(preset.effort.clone());
            SelectorOption::new(
                SelectorValue::ReasoningEffort(value.clone()),
                reasoning_effort_label(&preset.effort),
                preset.description.clone(),
            )
            .current(current == &value)
        }));
        Self::new("Select reasoning effort", options)
    }

    pub(super) fn service_tiers(current: &ServiceTierValue, tiers: &[ModelServiceTier]) -> Self {
        let mut options = vec![
            SelectorOption::new(
                SelectorValue::ServiceTier(ServiceTierValue::Default),
                "Default",
                "Use the model's default service tier.",
            )
            .current(current == &ServiceTierValue::Default),
        ];
        options.extend(tiers.iter().map(|tier| {
            let value = ServiceTierValue::Explicit(tier.id.clone());
            SelectorOption::new(
                SelectorValue::ServiceTier(value.clone()),
                tier.name.clone(),
                tier.description.clone(),
            )
            .current(current == &value)
        }));
        Self::new("Select service tier", options)
    }

    pub(super) fn approval_policies(current: AskForApproval) -> Self {
        let mut policies = vec![
            (
                AskForApproval::OnRequest,
                "On request",
                "Let Codex request approval when an action needs it.",
            ),
            (
                AskForApproval::UnlessTrusted,
                "Unless trusted",
                "Ask before commands that are not known to be safe.",
            ),
            (
                AskForApproval::Never,
                "Never",
                "Never pause the agent to request approval.",
            ),
        ];
        if matches!(current, AskForApproval::Granular { .. }) {
            policies.push((
                current,
                "Granular",
                "Keep the current fine-grained approval configuration.",
            ));
        }
        let options = policies
            .into_iter()
            .map(|(policy, label, detail)| {
                SelectorOption::new(SelectorValue::ApprovalPolicy(policy), label, detail)
                    .current(policy == current)
            })
            .collect();
        Self::new("Select approval policy", options)
    }

    pub(super) fn app_themes(current: TuiAppTheme) -> Self {
        let themes = [
            (
                TuiAppTheme::TokyoNight,
                "Better Codex's default cool blue and purple palette.",
            ),
            (
                TuiAppTheme::GruvboxDark,
                "A warm, retro palette with earthy contrast.",
            ),
            (
                TuiAppTheme::CatppuccinMocha,
                "A soft pastel palette on a dark base.",
            ),
        ];
        let options = themes
            .into_iter()
            .map(|(theme, detail)| {
                SelectorOption::new(
                    SelectorValue::AppTheme(theme),
                    app_theme_label(theme),
                    detail,
                )
                .current(theme == current)
            })
            .collect();
        Self::new("Select app theme", options)
    }
}

#[cfg(test)]
#[path = "selector_tests.rs"]
mod tests;
