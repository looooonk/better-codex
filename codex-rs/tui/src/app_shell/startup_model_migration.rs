use super::backend::AppShellBackend;
use super::design::MOCHA_BASE;
use super::design::MOCHA_MANTLE;
use super::design::MOCHA_SURFACE0;
use super::design::fill_rect;
use super::design::pane_content_rect;
use super::design::pane_style;
use crate::app_server_session::AppServerSession;
use crate::config_update::build_model_migration_seen_edit;
use crate::config_update::build_model_selection_edits;
use crate::legacy_core::config::Config;
use crate::model_migration::ModelMigrationCopy;
use crate::model_migration::migration_copy_for_models;
use crate::tui;
use crate::tui::TuiEvent;
use codex_models_manager::model_presets::HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG;
use codex_models_manager::model_presets::HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffort;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use tokio_stream::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelMigrationOnboardingOutcome {
    Continue,
    Exit,
}

impl ModelMigrationOnboardingOutcome {
    pub(crate) fn is_exit(self) -> bool {
        matches!(self, Self::Exit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelMigrationSelection {
    TryNewModel,
    KeepCurrentModel,
    Exit,
}

impl ModelMigrationSelection {
    fn label(self, can_opt_out: bool) -> &'static str {
        match self {
            Self::TryNewModel => "Try new model",
            Self::KeepCurrentModel => {
                if can_opt_out {
                    "Keep current model"
                } else {
                    "Exit"
                }
            }
            Self::Exit => "Exit",
        }
    }

    fn description(self, can_opt_out: bool) -> &'static str {
        match self {
            Self::TryNewModel => "Update config and start this session with the recommended model.",
            Self::KeepCurrentModel => {
                if can_opt_out {
                    "Acknowledge the migration and keep the current model for this workspace."
                } else {
                    "Return to the terminal without starting a thread."
                }
            }
            Self::Exit => "Return to the terminal without starting a thread.",
        }
    }
}

#[derive(Clone)]
struct ModelMigrationPromptData {
    from_model: String,
    target_model: String,
    target_default_effort: ReasoningEffort,
    target_display_name: String,
    copy: ModelMigrationCopy,
}

#[derive(Clone)]
struct ModelMigrationOnboardingState {
    prompt: ModelMigrationPromptData,
    selected: usize,
    error: Option<String>,
}

impl ModelMigrationOnboardingState {
    fn new(prompt: ModelMigrationPromptData) -> Self {
        Self {
            prompt,
            selected: 0,
            error: None,
        }
    }

    fn choices(&self) -> Vec<ModelMigrationSelection> {
        if self.prompt.copy.can_opt_out {
            vec![
                ModelMigrationSelection::TryNewModel,
                ModelMigrationSelection::KeepCurrentModel,
                ModelMigrationSelection::Exit,
            ]
        } else {
            vec![
                ModelMigrationSelection::TryNewModel,
                ModelMigrationSelection::KeepCurrentModel,
            ]
        }
    }

    fn selected(&self) -> ModelMigrationSelection {
        let choices = self.choices();
        choices[self.selected.min(choices.len().saturating_sub(1))]
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.choices().len().saturating_sub(1));
    }

    fn select_number(&mut self, number: char) {
        let next = match number {
            '1' => Some(0),
            '2' => Some(1),
            '3' if self.prompt.copy.can_opt_out => Some(2),
            _ => None,
        };
        if let Some(next) = next {
            self.selected = next;
        }
    }
}

pub(crate) async fn run_model_migration_onboarding(
    tui: &mut tui::Tui,
    app_server: &mut AppServerSession,
    config: &mut Config,
    model: &str,
    available_models: &[ModelPreset],
) -> Result<ModelMigrationOnboardingOutcome> {
    let Some(prompt) = model_migration_prompt_data(config, model, available_models) else {
        return Ok(ModelMigrationOnboardingOutcome::Continue);
    };

    let mut state = ModelMigrationOnboardingState::new(prompt);
    let mut tui_events = tui.event_stream();
    tui.frame_requester().schedule_frame();

    loop {
        let Some(event) = tui_events.next().await else {
            return Ok(ModelMigrationOnboardingOutcome::Continue);
        };
        match event {
            TuiEvent::Key(key) => match handle_model_migration_key(key, &mut state) {
                ModelMigrationKeyAction::Accept => {
                    match persist_accepted_model_migration(app_server, config, &state.prompt).await
                    {
                        Ok(()) => return Ok(ModelMigrationOnboardingOutcome::Continue),
                        Err(err) => {
                            state.error = Some(crate::config_update::format_config_error(&err));
                            tui.frame_requester().schedule_frame();
                        }
                    }
                }
                ModelMigrationKeyAction::Reject => {
                    match persist_rejected_model_migration(app_server, config, &state.prompt).await
                    {
                        Ok(()) => return Ok(ModelMigrationOnboardingOutcome::Continue),
                        Err(err) => {
                            state.error = Some(crate::config_update::format_config_error(&err));
                            tui.frame_requester().schedule_frame();
                        }
                    }
                }
                ModelMigrationKeyAction::Exit => {
                    return Ok(ModelMigrationOnboardingOutcome::Exit);
                }
                ModelMigrationKeyAction::Redraw => {
                    tui.frame_requester().schedule_frame();
                }
                ModelMigrationKeyAction::Ignored => {}
            },
            TuiEvent::Paste(_) => {}
            TuiEvent::Resize | TuiEvent::Draw => {
                draw_model_migration_onboarding(tui, &state)?;
            }
        }
    }
}

fn model_migration_prompt_data(
    config: &Config,
    model: &str,
    available_models: &[ModelPreset],
) -> Option<ModelMigrationPromptData> {
    let current_preset = available_models
        .iter()
        .find(|preset| preset.model == model)?;
    let upgrade = current_preset.upgrade.as_ref()?;
    if migration_prompt_hidden(config, upgrade.migration_config_key.as_str()) {
        return None;
    }

    let target_model = upgrade.id.as_str();
    if !should_show_model_migration_prompt(
        model,
        target_model,
        &config.notices.model_migrations,
        available_models,
    ) {
        return None;
    }

    let target_preset = target_preset_for_upgrade(available_models, target_model)?;
    let target_display_name = target_preset.display_name.clone();
    let heading_label = if target_display_name == model {
        target_model.to_string()
    } else {
        target_display_name.clone()
    };
    let target_description =
        (!target_preset.description.is_empty()).then(|| target_preset.description.clone());
    Some(ModelMigrationPromptData {
        from_model: model.to_string(),
        target_model: target_model.to_string(),
        target_default_effort: target_preset.default_reasoning_effort.clone(),
        target_display_name,
        copy: migration_copy_for_models(
            model,
            target_model,
            upgrade.model_link.clone(),
            upgrade.upgrade_copy.clone(),
            upgrade.migration_markdown.clone(),
            heading_label,
            target_description,
            /*can_opt_out*/ true,
        ),
    })
}

fn migration_prompt_hidden(config: &Config, migration_config_key: &str) -> bool {
    match migration_config_key {
        HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG => config
            .notices
            .hide_gpt_5_1_codex_max_migration_prompt
            .unwrap_or(false),
        HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG => {
            config.notices.hide_gpt5_1_migration_prompt.unwrap_or(false)
        }
        _ => false,
    }
}

fn should_show_model_migration_prompt(
    current_model: &str,
    target_model: &str,
    seen_migrations: &std::collections::BTreeMap<String, String>,
    available_models: &[ModelPreset],
) -> bool {
    if target_model == current_model {
        return false;
    }

    if let Some(seen_target) = seen_migrations.get(current_model)
        && seen_target == target_model
    {
        return false;
    }

    available_models
        .iter()
        .any(|preset| preset.model == target_model && preset.show_in_picker)
}

fn target_preset_for_upgrade<'a>(
    available_models: &'a [ModelPreset],
    target_model: &str,
) -> Option<&'a ModelPreset> {
    available_models
        .iter()
        .find(|preset| preset.model == target_model && preset.show_in_picker)
}

async fn persist_accepted_model_migration(
    app_server: &mut AppServerSession,
    config: &mut Config,
    prompt: &ModelMigrationPromptData,
) -> Result<()> {
    let mut edits =
        build_model_selection_edits(&prompt.target_model, Some(&prompt.target_default_effort));
    edits.push(build_model_migration_seen_edit(
        &prompt.from_model,
        &prompt.target_model,
    ));
    app_server.write_config(edits).await?;
    config.model = Some(prompt.target_model.clone());
    config.model_reasoning_effort = Some(prompt.target_default_effort.clone());
    config
        .notices
        .model_migrations
        .insert(prompt.from_model.clone(), prompt.target_model.clone());
    Ok(())
}

async fn persist_rejected_model_migration(
    app_server: &mut AppServerSession,
    config: &mut Config,
    prompt: &ModelMigrationPromptData,
) -> Result<()> {
    app_server
        .write_config(vec![build_model_migration_seen_edit(
            &prompt.from_model,
            &prompt.target_model,
        )])
        .await?;
    config
        .notices
        .model_migrations
        .insert(prompt.from_model.clone(), prompt.target_model.clone());
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelMigrationKeyAction {
    Accept,
    Reject,
    Exit,
    Redraw,
    Ignored,
}

fn handle_model_migration_key(
    key: KeyEvent,
    state: &mut ModelMigrationOnboardingState,
) -> ModelMigrationKeyAction {
    if key.kind != KeyEventKind::Press {
        return ModelMigrationKeyAction::Ignored;
    }
    match key.code {
        KeyCode::Up => {
            state.move_up();
            ModelMigrationKeyAction::Redraw
        }
        KeyCode::Down => {
            state.move_down();
            ModelMigrationKeyAction::Redraw
        }
        KeyCode::Char(number @ ('1' | '2' | '3')) => {
            state.select_number(number);
            ModelMigrationKeyAction::Redraw
        }
        KeyCode::Enter => match state.selected() {
            ModelMigrationSelection::TryNewModel => ModelMigrationKeyAction::Accept,
            ModelMigrationSelection::KeepCurrentModel if state.prompt.copy.can_opt_out => {
                ModelMigrationKeyAction::Reject
            }
            ModelMigrationSelection::KeepCurrentModel | ModelMigrationSelection::Exit => {
                ModelMigrationKeyAction::Exit
            }
        },
        KeyCode::Esc => ModelMigrationKeyAction::Exit,
        KeyCode::Char(_)
        | KeyCode::Backspace
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => ModelMigrationKeyAction::Ignored,
    }
}

fn draw_model_migration_onboarding(
    tui: &mut tui::Tui,
    state: &ModelMigrationOnboardingState,
) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        ModelMigrationOnboardingView { state }.render(frame.area(), frame.buffer);
    })
}

struct ModelMigrationOnboardingView<'a> {
    state: &'a ModelMigrationOnboardingState,
}

impl ModelMigrationOnboardingView<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);
        self.render_main(horizontal[0], buf);
        self.render_dashboard(horizontal[1], buf);
    }

    fn render_main(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(5),
            ])
            .split(area);
        fill_rect(buf, vertical[0], MOCHA_MANTLE);
        Paragraph::new(Line::from("Better Codex".magenta().bold()))
            .style(pane_style(MOCHA_MANTLE))
            .render(pane_content_rect(vertical[0]), buf);

        let content = pane_content_rect(vertical[1]);
        let mut lines = vec![Line::from("Model migration".bold()), Line::from("")];
        lines.extend(model_migration_copy_lines(
            &self.state.prompt.copy,
            usize::from(content.width),
        ));
        lines.push(Line::from(""));
        for (index, selection) in self.state.choices().into_iter().enumerate() {
            lines.push(model_migration_selection_line(
                index,
                selection,
                self.state.prompt.copy.can_opt_out,
                index == self.state.selected,
            ));
            lines.extend(
                wrapped_lines_with_indent(
                    selection.description(self.state.prompt.copy.can_opt_out),
                    usize::from(content.width),
                    "  ",
                )
                .into_iter()
                .map(ratatui::prelude::Stylize::dim),
            );
        }
        if let Some(error) = &self.state.error {
            lines.push(Line::from(""));
            lines.extend(
                wrapped_lines(error, usize::from(content.width))
                    .into_iter()
                    .map(ratatui::prelude::Stylize::red),
            );
        }
        Paragraph::new(lines)
            .style(pane_style(MOCHA_BASE))
            .render(content, buf);

        fill_rect(buf, vertical[2], MOCHA_SURFACE0);
        Paragraph::new(vec![
            Line::from("Enter continue  Up/Down choose  1/2/3 jump  Esc exit".dim()),
            Line::from("The decision is saved through app-server config.".dim()),
        ])
        .style(pane_style(MOCHA_SURFACE0))
        .render(pane_content_rect(vertical[2]), buf);
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_SURFACE0);
        let content = pane_content_rect(area);
        let mut lines = vec![
            Line::from("Startup".bold()),
            Line::from(""),
            Line::from("current model".dim()),
        ];
        lines.extend(wrapped_lines(
            &self.state.prompt.from_model,
            usize::from(content.width),
        ));
        lines.push(Line::from(""));
        lines.push(Line::from("recommended model".dim()));
        lines.extend(wrapped_lines(
            &self.state.prompt.target_model,
            usize::from(content.width),
        ));
        if self.state.prompt.target_display_name != self.state.prompt.target_model {
            lines.push(Line::from(""));
            lines.push(Line::from("display name".dim()));
            lines.extend(wrapped_lines(
                &self.state.prompt.target_display_name,
                usize::from(content.width),
            ));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            "selected ".dim(),
            self.state
                .selected()
                .label(self.state.prompt.copy.can_opt_out)
                .cyan()
                .bold(),
        ]));
        Paragraph::new(lines)
            .style(pane_style(MOCHA_SURFACE0))
            .render(content, buf);
    }
}

fn model_migration_selection_line(
    index: usize,
    selection: ModelMigrationSelection,
    can_opt_out: bool,
    selected: bool,
) -> Line<'static> {
    let marker = if selected {
        ">".cyan().bold()
    } else {
        " ".dim()
    };
    let label = format!("{}. {}", index + 1, selection.label(can_opt_out));
    Line::from(vec![marker, " ".dim(), label.into()])
}

fn model_migration_copy_lines(copy: &ModelMigrationCopy, width: usize) -> Vec<Line<'static>> {
    if let Some(markdown) = &copy.markdown {
        return markdown
            .lines()
            .flat_map(|line| wrapped_lines(line, width))
            .collect();
    }

    let mut lines = Vec::new();
    let heading = copy
        .heading
        .iter()
        .map(span_to_plain_text)
        .collect::<String>();
    if !heading.is_empty() {
        lines.extend(
            wrapped_lines(&heading, width)
                .into_iter()
                .map(ratatui::prelude::Stylize::bold),
        );
        lines.push(Line::from(""));
    }
    for line in &copy.content {
        let text = line_to_plain_text(line);
        if text.is_empty() {
            lines.push(Line::from(""));
        } else {
            lines.extend(wrapped_lines(&text, width));
        }
    }
    lines
}

fn line_to_plain_text(line: &Line<'_>) -> String {
    line.spans.iter().map(span_to_plain_text).collect()
}

fn span_to_plain_text(span: &Span<'_>) -> String {
    span.content.to_string()
}

fn wrapped_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    textwrap::wrap(text, width)
        .into_iter()
        .map(|line| Line::from(line.into_owned()))
        .collect()
}

fn wrapped_lines_with_indent(text: &str, width: usize, indent: &'static str) -> Vec<Line<'static>> {
    let options = textwrap::Options::new(width.max(indent.len() + 1))
        .initial_indent(indent)
        .subsequent_indent(indent);
    textwrap::wrap(text, options)
        .into_iter()
        .map(|line| Line::from(line.into_owned()))
        .collect()
}

#[cfg(test)]
#[path = "startup_model_migration_tests.rs"]
mod tests;
