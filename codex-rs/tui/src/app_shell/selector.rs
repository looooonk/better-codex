use super::design::fill_rect;
use super::design::palette;
use super::design::pane_style;
use super::settings::reasoning_effort_label;
use codex_app_server_protocol::AskForApproval;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelServiceTier;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const MODAL_MARGIN: u16 = 2;
const MAX_MODAL_WIDTH: u16 = 68;
const MAX_MODAL_HEIGHT: u16 = 24;
const OPTION_HEIGHT: u16 = 2;
const OPTION_PREFIX_WIDTH: usize = 6;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SelectorGeometry {
    pub(super) modal: Rect,
    pub(super) options: Rect,
    pub(super) footer: Rect,
    pub(super) visible_options: usize,
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

    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        let geometry = selector_geometry(area, self.options.len());
        buf.set_style(area, Style::new().add_modifier(Modifier::DIM));
        Clear.render(geometry.modal, buf);
        fill_rect(buf, geometry.modal, palette::SURFACE);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::FOCUS))
            .style(pane_style(palette::SURFACE))
            .title(Line::from(format!(" {} ", self.title)).bold());
        block.render(geometry.modal, buf);

        let scroll = self.visible_scroll(geometry.visible_options);
        for (visible_index, option) in self
            .options
            .iter()
            .skip(scroll)
            .take(geometry.visible_options)
            .enumerate()
        {
            let index = scroll + visible_index;
            let area = Rect::new(
                geometry.options.x,
                geometry.options.y.saturating_add(
                    u16::try_from(visible_index).unwrap_or(u16::MAX) * OPTION_HEIGHT,
                ),
                geometry.options.width,
                OPTION_HEIGHT,
            );
            self.render_option(area, index, option, buf);
        }
        Paragraph::new(Line::from(vec![
            " j/k ".fg(palette::FOCUS).bold(),
            "move  ".fg(palette::MUTED),
            " enter ".fg(palette::FOCUS).bold(),
            "select  ".fg(palette::MUTED),
            " esc ".fg(palette::FOCUS).bold(),
            "cancel  ".fg(palette::MUTED),
            format!(
                "{}/{}",
                self.selected.saturating_add(1).min(self.options.len()),
                self.options.len()
            )
            .fg(palette::PURPLE)
            .bold(),
        ]))
        .style(pane_style(palette::SURFACE))
        .render(geometry.footer, buf);
    }

    fn render_option(
        &self,
        area: Rect,
        index: usize,
        option: &SelectorOption<T>,
        buf: &mut Buffer,
    ) {
        let selected = index == self.selected;
        let background = if selected {
            palette::ELEVATED
        } else {
            palette::SURFACE
        };
        fill_rect(buf, area, background);
        let current_label = if option.current { "  current" } else { "" };
        let label_width = usize::from(area.width)
            .saturating_sub(OPTION_PREFIX_WIDTH + current_label.chars().count())
            .max(1);
        let label = first_wrapped_line(&option.label, label_width);
        let detail_width = usize::from(area.width)
            .saturating_sub(OPTION_PREFIX_WIDTH)
            .max(1);
        let detail = first_wrapped_line(&option.detail, detail_width);
        let pointer = if selected {
            "›".fg(palette::FOCUS).bold()
        } else {
            " ".into()
        };
        let shortcut = if index < 9 {
            (index + 1).to_string().fg(palette::MUTED)
        } else {
            "·".fg(palette::MUTED)
        };
        let current = if option.current {
            "●".fg(palette::SUCCESS)
        } else {
            "○".fg(palette::MUTED)
        };
        let label = if selected {
            label.fg(palette::TEXT).bold()
        } else {
            label.fg(palette::TEXT)
        };
        let current_label = if option.current {
            current_label.fg(palette::SUCCESS)
        } else {
            "".into()
        };
        let lines = vec![
            Line::from(vec![
                pointer,
                " ".into(),
                shortcut,
                " ".into(),
                current,
                " ".into(),
                label,
                current_label,
            ]),
            Line::from(vec![
                " ".repeat(OPTION_PREFIX_WIDTH).into(),
                detail.fg(palette::MUTED),
            ]),
        ];
        Paragraph::new(lines)
            .style(pane_style(background))
            .render(area, buf);
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
}

pub(super) fn selector_geometry(area: Rect, option_count: usize) -> SelectorGeometry {
    let available_width = area.width.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let available_height = area.height.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let width = available_width.min(MAX_MODAL_WIDTH);
    let option_count = u16::try_from(option_count.max(1)).unwrap_or(u16::MAX);
    let desired_height = option_count.saturating_mul(OPTION_HEIGHT).saturating_add(3);
    let height = desired_height
        .min(MAX_MODAL_HEIGHT)
        .min(available_height)
        .max(available_height.min(5));
    let modal = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    );
    let inner = Rect::new(
        modal.x.saturating_add(u16::from(modal.width > 1)),
        modal.y.saturating_add(u16::from(modal.height > 1)),
        modal.width.saturating_sub(2),
        modal.height.saturating_sub(2),
    );
    let horizontal_padding = u16::from(inner.width > 2);
    let content = Rect::new(
        inner.x.saturating_add(horizontal_padding),
        inner.y,
        inner
            .width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        inner.height,
    );
    let footer_height = u16::from(content.height > OPTION_HEIGHT);
    let option_height = content.height.saturating_sub(footer_height);
    let options = Rect::new(content.x, content.y, content.width, option_height);
    let footer = Rect::new(
        content.x,
        content.y.saturating_add(option_height),
        content.width,
        footer_height,
    );
    SelectorGeometry {
        modal,
        options,
        footer,
        visible_options: usize::from(options.height / OPTION_HEIGHT),
    }
}

fn first_wrapped_line(text: &str, width: usize) -> String {
    textwrap::wrap(text, textwrap::Options::new(width.max(1)))
        .first()
        .map_or_else(String::new, |line| line.to_string())
}

#[cfg(test)]
#[path = "selector_tests.rs"]
mod tests;
