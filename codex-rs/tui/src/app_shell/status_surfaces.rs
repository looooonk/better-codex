use super::ShellState;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::selector::SelectorOption;
use super::selector::SelectorState;
use super::selector::SelectorValue;
use super::status_surface_items::StatusLineItem;
use super::status_surface_items::TerminalTitleItem;
use crate::legacy_core::config::Config;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use codex_config::ConfigLayerStack;
use codex_config::config_toml::ConfigToml;
use color_eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::sync::Arc;
use std::sync::Mutex;
use strum::IntoEnumIterator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Surface {
    StatusLine,
    Title,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SurfaceChange {
    Items(Surface, Vec<String>),
    Colors(bool),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Preferences {
    pub(super) status: Vec<StatusLineItem>,
    pub(super) title: Vec<TerminalTitleItem>,
    pub(super) colors: bool,
}

#[derive(Clone, Default)]
pub(super) struct StatusSurfaces {
    pub(super) preferences: Preferences,
    layers: Option<ConfigLayerStack>,
    title: Arc<Mutex<ManagedTitle>>,
    pub(super) metadata: super::status_surface_metadata::StatusMetadata,
}

#[derive(Default)]
struct ManagedTitle {
    last: Option<String>,
    revision: u64,
}

impl Drop for ManagedTitle {
    fn drop(&mut self) {
        if self.last.as_ref().is_some_and(|title| !title.is_empty()) {
            crate::terminal_title::clear_managed_terminal_title();
        }
    }
}

impl StatusSurfaces {
    pub(super) fn configure(&mut self, config: &Config) -> Vec<String> {
        self.layers = Some(config.config_layer_stack.clone());
        let (preferences, invalid) = Preferences::parse(
            config.tui_status_line.as_deref().unwrap_or_default(),
            config.tui_terminal_title.as_deref().unwrap_or_default(),
            config.tui_status_line_use_colors,
        );
        self.preferences = preferences;
        invalid
    }

    pub(super) fn status_visible(&self) -> bool {
        !self.preferences.status.is_empty()
    }
}

impl Preferences {
    fn parse(status: &[String], title: &[String], colors: bool) -> (Self, Vec<String>) {
        let mut invalid = Vec::new();
        let mut status_items = Vec::new();
        for id in status {
            match id.parse() {
                Ok(item) if !status_items.contains(&item) => status_items.push(item),
                Ok(_) => {}
                Err(_) => invalid.push(id.clone()),
            }
        }
        let mut title_items = Vec::new();
        for id in title {
            match id.parse() {
                Ok(item) if !title_items.contains(&item) => title_items.push(item),
                Ok(_) => {}
                Err(_) => invalid.push(id.clone()),
            }
        }
        (
            Self {
                status: status_items,
                title: title_items,
                colors,
            },
            invalid,
        )
    }

    fn from_layers(layers: &ConfigLayerStack) -> Result<Self> {
        let config: ConfigToml = layers.effective_config().try_into()?;
        let tui = config.tui.unwrap_or_default();
        Ok(Self::parse(
            tui.status_line.as_deref().unwrap_or_default(),
            tui.terminal_title.as_deref().unwrap_or_default(),
            tui.status_line_use_colors,
        )
        .0)
    }
}

impl Surface {
    fn label(self) -> &'static str {
        match self {
            Self::StatusLine => "STATUS LINE",
            Self::Title => "TERMINAL TITLE",
        }
    }
    fn defaults(self) -> Vec<String> {
        match self {
            Self::StatusLine => ["model-with-reasoning", "context-remaining", "current-dir"]
                .map(str::to_owned)
                .into(),
            Self::Title => ["activity", "thread-name", "project-name"]
                .map(str::to_owned)
                .into(),
        }
    }

    fn parse(self, args: &str) -> Result<SurfaceChange> {
        let items = match args.trim() {
            "off" => Vec::new(),
            "default" | "on" => self.defaults(),
            "colors on" if self == Self::StatusLine => return Ok(SurfaceChange::Colors(true)),
            "colors off" if self == Self::StatusLine => return Ok(SurfaceChange::Colors(false)),
            args => {
                let mut items = Vec::new();
                for id in args
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|id| !id.is_empty())
                {
                    let id = match self {
                        Self::StatusLine => {
                            id.parse::<StatusLineItem>().map(|item| item.to_string())
                        }
                        Self::Title => id.parse::<TerminalTitleItem>().map(|item| item.to_string()),
                    }
                    .map_err(|_| eyre!("Unknown display item: {id}"))?;
                    if !items.contains(&id) {
                        items.push(id);
                    }
                }
                items
            }
        };
        Ok(SurfaceChange::Items(self, items))
    }
}

impl ShellState {
    pub(super) fn run_status_surface_command(
        &mut self,
        surface: Surface,
        args: &str,
    ) -> Result<()> {
        if args.is_empty() {
            let prefs = &self.status_surfaces.preferences;
            let selected: Vec<String> = match surface {
                Surface::StatusLine => prefs.status.iter().map(ToString::to_string).collect(),
                Surface::Title => prefs.title.iter().map(ToString::to_string).collect(),
            };
            let choices: Vec<(String, &'static str)> = match surface {
                Surface::StatusLine => StatusLineItem::iter()
                    .map(|item| (item.to_string(), item.description()))
                    .collect(),
                Surface::Title => TerminalTitleItem::iter()
                    .map(|item| (item.to_string(), item.description()))
                    .collect(),
            };
            let mut options = vec![
                SelectorOption::new(
                    SelectorValue::StatusSurface(SurfaceChange::Items(surface, surface.defaults())),
                    "Use default items",
                    "Restore the native item order",
                ),
                SelectorOption::new(
                    SelectorValue::StatusSurface(SurfaceChange::Items(surface, Vec::new())),
                    "Hide",
                    "Disable this display",
                )
                .current(selected.is_empty()),
            ];
            if surface == Surface::StatusLine {
                options.push(SelectorOption::new(
                    SelectorValue::StatusSurface(SurfaceChange::Colors(!prefs.colors)),
                    if prefs.colors {
                        "Use plain colors"
                    } else {
                        "Use theme colors"
                    },
                    "Change status line colors",
                ));
            }
            for (id, description) in choices {
                let active = selected.contains(&id);
                let mut next = selected.clone();
                if active {
                    next.retain(|item| item != &id);
                } else {
                    next.push(id.clone());
                }
                options.push(SelectorOption::new(
                    SelectorValue::StatusSurface(SurfaceChange::Items(surface, next)),
                    format!("{} {id}", if active { "[x]" } else { "[ ]" }),
                    description,
                ));
            }
            self.open_selector(SelectorState::new(surface.label(), options));
            return Ok(());
        }
        self.save_status_surface(surface.parse(args)?)
    }

    pub(super) fn save_status_surface(&mut self, change: SurfaceChange) -> Result<()> {
        if self.pets.has_work()
            || self.pending_worktree.is_some()
            || self.has_pending_backend_action(ActionGroup::Workspace)
            || self.has_pending_backend_action(ActionGroup::SessionSwitch)
            || self.has_pending_backend_action(ActionGroup::ConversationBranch)
            || self.has_pending_backend_action(ActionGroup::Settings)
        {
            bail!("Wait for the pending settings or session action before saving display settings");
        }
        let layers = self
            .status_surfaces
            .layers
            .clone()
            .ok_or_else(|| eyre!("Local display settings are unavailable"))?;
        let path = self.client_config_path.clone();
        let (key, value) = match &change {
            SurfaceChange::Items(surface, items) => {
                let key = match surface {
                    Surface::StatusLine => "status_line",
                    Surface::Title => "terminal_title",
                };
                let mut value = toml_edit::Array::new();
                for item in items {
                    value.push(item);
                }
                (key, toml_edit::value(value))
            }
            SurfaceChange::Colors(enabled) => {
                ("status_line_use_colors", toml_edit::value(*enabled))
            }
        };
        let selector = self.selector.clone();
        self.backend_actions
            .start(Some(ActionGroup::Settings), async move {
                let result = async {
                    ConfigEditsBuilder::for_config_path(&path)
                        .with_edits([ConfigEdit::SetPath {
                            segments: vec!["tui".into(), key.into()],
                            value,
                        }])
                        .apply()
                        .await
                        .map_err(|error| eyre!(error.to_string()))?;
                    let layers = crate::presentation_config::reload(&layers, &path).await?;
                    Preferences::from_layers(&layers)
                }
                .await;
                BackendActionResult::StatusSurface {
                    change,
                    selector,
                    result,
                }
            });
        Ok(())
    }

    pub(super) fn complete_status_surface(
        &mut self,
        change: SurfaceChange,
        selector: Option<SelectorState<SelectorValue>>,
        result: Result<Preferences>,
    ) {
        match result {
            Ok(preferences) => {
                let applied = match change {
                    SurfaceChange::Items(Surface::StatusLine, items) => {
                        preferences
                            .status
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            == items
                    }
                    SurfaceChange::Items(Surface::Title, items) => {
                        preferences
                            .title
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            == items
                    }
                    SurfaceChange::Colors(colors) => preferences.colors == colors,
                };
                self.status_surfaces.preferences = preferences;
                self.status_surfaces.metadata.invalidate();
                if self.selector == selector {
                    self.selector = None;
                }
                self.terminal_clear_requested.set(true);
                self.push_status(if applied {
                    "display settings saved"
                } else {
                    "saved locally; a higher-priority setting controls the active display"
                });
            }
            Err(error) => self.report_action_error("failed to save display settings", error),
        }
    }

    pub(super) fn render_status_surface(&self, area: Rect, buf: &mut Buffer) {
        let mut spans = vec![Span::raw(" ")];
        for item in &self.status_surfaces.preferences.status {
            if let Some(value) = self.status_surface_value(*item) {
                if spans.len() > 1 {
                    spans.push(" | ".dim());
                }
                let value: String = value
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(512)
                    .collect();
                spans.push(if self.status_surfaces.preferences.colors {
                    value.fg(super::design::palette::focus())
                } else {
                    value.into()
                });
            }
        }
        Paragraph::new(Line::from(spans)).render(area, buf);
    }

    pub(super) fn refresh_terminal_title(&self) {
        let title = self
            .status_surfaces
            .preferences
            .title
            .iter()
            .filter_map(|item| self.terminal_title_value(*item))
            .map(|value| value.chars().take(240).collect::<String>())
            .collect::<Vec<_>>()
            .join(" | ");
        let mut managed = self
            .status_surfaces
            .title
            .lock()
            .expect("terminal title state");
        let revision = crate::terminal_title::managed_title_revision();
        if managed.last.as_ref() == Some(&title) && managed.revision == revision {
            return;
        }
        let result = if title.is_empty() {
            crate::terminal_title::clear_managed_terminal_title();
            Ok(())
        } else {
            crate::terminal_title::set_managed_terminal_title(&title)
        };
        match result {
            Ok(()) => {
                managed.last = Some(title);
                managed.revision = crate::terminal_title::managed_title_revision();
            }
            Err(error) => tracing::debug!(%error, "could not update terminal title"),
        }
    }
}

#[cfg(test)]
#[path = "status_surfaces_tests.rs"]
mod tests;
