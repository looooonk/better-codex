use super::ShellState;
use super::selector::SelectorOption;
use super::selector::SelectorState;
use super::selector::SelectorValue;
use crate::legacy_core::config::Config;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use crate::pets::AmbientPet;
use crate::pets::PetImageRenderError;
use crate::pets::PetImageSupport;
use crate::pets::PetNotificationKind;
use crate::pets::TerminalPetImage;
use crate::tui::FrameRequester;
use codex_config::ConfigLayerStack;
use codex_config::types::TuiPetAnchor;
use codex_http_client::ClientRouteClass;
use codex_http_client::RouteAwareClientPool;
use codex_utils_absolute_path::AbsolutePathBuf;
use ratatui::layout::Rect;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone, Default)]
pub(super) struct PetState(Arc<Mutex<PetRuntime>>);

#[derive(Default)]
struct PetRuntime {
    config: Option<PetConfig>,
    selected: Option<String>,
    pet: Option<AmbientPet>,
    pending: Option<tokio::task::JoinHandle<anyhow::Result<PetUpdate>>>,
    render: TerminalPetImage,
    notification: Option<PetNotificationKind>,
}

#[derive(Clone)]
struct PetConfig {
    home: AbsolutePathBuf,
    path: AbsolutePathBuf,
    requester: FrameRequester,
    animations: bool,
    http: RouteAwareClientPool,
    support: PetImageSupport,
    layers: ConfigLayerStack,
    anchor: TuiPetAnchor,
}

enum PetUpdate {
    Catalog {
        thread_id: codex_protocol::ThreadId,
        choices: Vec<crate::pets::PetChoice>,
    },
    Selected {
        id: String,
        pet: Option<AmbientPet>,
        layers: ConfigLayerStack,
        overridden: bool,
    },
}

impl Drop for PetRuntime {
    fn drop(&mut self) {
        if let Some(task) = self.pending.take() {
            task.abort();
        }
        let _ = self.render.draw(&mut std::io::stdout(), /*request*/ None);
    }
}

impl PetState {
    pub(super) fn configure(&self, config: &Config, requester: FrameRequester) {
        let mut state = self.0.lock().expect("pet state");
        state.config = Some(PetConfig {
            home: config.codex_home.clone(),
            path: super::local_app_theme::selected_config_path(config),
            requester,
            animations: config.animations,
            http: RouteAwareClientPool::new(config.http_client_factory(), ClientRouteClass::Other),
            support: crate::pets::detect_pet_image_support(),
            layers: config.config_layer_stack.clone(),
            anchor: config.tui_pet_anchor,
        });
        state.selected = config.tui_pet.clone();
        if let Some(id) = config
            .tui_pet
            .as_ref()
            .filter(|id| id.as_str() != crate::pets::DISABLED_PET_ID)
            && state
                .config
                .as_ref()
                .is_some_and(|config| config.support.protocol().is_some())
        {
            state.load(id.clone(), /*persist*/ false);
        }
    }

    pub(super) fn has_work(&self) -> bool {
        self.0.lock().expect("pet state").pending.is_some()
    }

    pub(super) fn content_area(&self, area: Rect) -> Rect {
        let state = self.0.lock().expect("pet state");
        let Some(pet) = state.pet.as_ref().filter(|pet| pet.image_enabled()) else {
            return area;
        };
        let width = pet.image_columns().saturating_add(2);
        if area.width >= super::shell_layout::MIN_TERMINAL_WIDTH.saturating_add(width)
            && pet.draw_request(area, area.bottom()).is_some()
        {
            Rect {
                width: area.width - width,
                ..area
            }
        } else {
            area
        }
    }

    pub(super) fn clear(&self) -> std::io::Result<()> {
        let state = self.0.lock().expect("pet state");
        state
            .render
            .draw(&mut std::io::stdout(), /*request*/ None)
            .map_err(std::io::Error::other)
    }
}

impl PetRuntime {
    fn load(&mut self, id: String, persist: bool) {
        let Some(config) = self.config.clone() else {
            return;
        };
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(tokio::spawn(async move {
            let mut pet = load_selected_pet(&id, &config).await?;
            let mut effective_id = id.clone();
            let mut layers = config.layers.clone();
            if persist {
                ConfigEditsBuilder::for_config_path(&config.path)
                    .with_edits([crate::legacy_core::config::edit::tui_pet_edit(&id)])
                    .apply()
                    .await?;
                layers = crate::presentation_config::reload(&layers, &config.path)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                let effective: codex_config::config_toml::ConfigToml =
                    layers.effective_config().try_into()?;
                effective_id = effective
                    .tui
                    .and_then(|tui| tui.pet)
                    .unwrap_or_else(|| crate::pets::DISABLED_PET_ID.into());
                if effective_id != id {
                    pet = load_selected_pet(&effective_id, &config).await?;
                }
            }
            let overridden = effective_id != id;
            Ok(PetUpdate::Selected {
                id: effective_id,
                pet,
                layers,
                overridden,
            })
        }));
    }
}

async fn load_selected_pet(id: &str, config: &PetConfig) -> anyhow::Result<Option<AmbientPet>> {
    if id == crate::pets::DISABLED_PET_ID {
        return Ok(None);
    }
    crate::pets::load_pet_with_assets(
        id.into(),
        config.home.clone(),
        config.requester.clone(),
        config.animations,
        &config.http,
    )
    .await
    .map(Some)
}

impl ShellState {
    pub(super) fn run_pets_command(&mut self, args: &str) {
        if !args.is_empty() {
            self.select_pet(
                match args {
                    "off" | "hide" => crate::pets::DISABLED_PET_ID,
                    "on" => crate::pets::DEFAULT_PET_ID,
                    id => id,
                }
                .into(),
            );
            return;
        }
        let home = self.codex_home.clone();
        let thread_id = self.thread_id;
        let mut state = self.pets.0.lock().expect("pet state");
        if state.pending.is_some() {
            drop(state);
            self.push_status("pet selection is still loading");
            return;
        }
        state.pending = Some(tokio::spawn(async move {
            let choices =
                tokio::task::spawn_blocking(move || crate::pets::available_pet_choices(&home))
                    .await?;
            Ok(PetUpdate::Catalog { thread_id, choices })
        }));
        drop(state);
        self.push_status("loading terminal pets");
    }

    pub(super) fn select_pet(&mut self, id: String) {
        if self.has_pending_backend_action(super::backend_actions::ActionGroup::Settings)
            || self.pets.has_work()
        {
            self.push_status(
                "wait for the current presentation settings change before choosing a pet",
            );
            return;
        }
        let mut state = self.pets.0.lock().expect("pet state");
        let Some(config) = &state.config else {
            drop(state);
            self.push_error("terminal pet settings are unavailable");
            return;
        };
        if id != crate::pets::DISABLED_PET_ID
            && let Some(message) = config.support.unsupported_message()
        {
            drop(state);
            self.push_system(message);
            return;
        }
        state.load(id, /*persist*/ true);
        drop(state);
        self.selector = None;
        self.push_status("loading terminal pet");
    }

    pub(super) async fn poll_pets(&mut self) -> bool {
        let task = {
            let mut state = self.pets.0.lock().expect("pet state");
            if !state
                .pending
                .as_ref()
                .is_some_and(tokio::task::JoinHandle::is_finished)
            {
                return false;
            }
            state.pending.take().expect("finished pet task")
        };
        match task.await {
            Ok(Ok(PetUpdate::Catalog { thread_id, choices })) => {
                if thread_id != self.thread_id || self.active_input_route().is_some() {
                    return true;
                }
                let current = self.pets.0.lock().expect("pet state").selected.clone();
                self.open_selector(SelectorState::new(
                    "TERMINAL PETS",
                    choices
                        .into_iter()
                        .map(|choice| {
                            let selected =
                                current.as_deref().unwrap_or(crate::pets::DISABLED_PET_ID);
                            let is_current = selected == choice.id
                                || choice.legacy_id.as_deref() == Some(selected);
                            SelectorOption::new(
                                SelectorValue::Pet(choice.id),
                                choice.name,
                                choice.description,
                            )
                            .current(is_current)
                        })
                        .collect(),
                ));
            }
            Ok(Ok(PetUpdate::Selected {
                id,
                pet,
                layers,
                overridden,
            })) => {
                let mut state = self.pets.0.lock().expect("pet state");
                state.selected = Some(id.clone());
                state.pet = pet;
                state.notification = None;
                if let Some(config) = &mut state.config {
                    config.layers = layers;
                }
                drop(state);
                if overridden {
                    self.push_status(format!(
                        "Pet preference saved; a higher-priority setting keeps {id} active"
                    ));
                } else {
                    self.push_status(if id == crate::pets::DISABLED_PET_ID {
                        "terminal pet hidden"
                    } else {
                        "terminal pet ready"
                    });
                }
                self.terminal_clear_requested.set(true);
            }
            Ok(Err(error)) => self.push_error(format!("Could not load terminal pet: {error:#}")),
            Err(error) => self.push_error(format!("Terminal pet load failed: {error}")),
        }
        true
    }

    pub(super) fn draw_pet_image(&self, area: Rect) -> std::io::Result<()> {
        let content = self.pets.content_area(area);
        let composer_anchor = self
            .pets
            .0
            .lock()
            .expect("pet state")
            .config
            .as_ref()
            .is_some_and(|config| config.anchor == TuiPetAnchor::Composer);
        let bottom = if composer_anchor {
            super::shell_layout::calculate(self, area)
                .map_or(area.bottom(), |layout| layout.input.bottom())
        } else {
            area.bottom()
        };
        let mut state = self.pets.0.lock().expect("pet state");
        if let Some(pet) = &mut state.pet {
            pet.set_animations_enabled(self.animations);
        }
        let kind = if self.pending_approval.is_some()
            || self.pending_user_input.is_some()
            || self.pending_elicitation.is_some()
        {
            PetNotificationKind::Waiting
        } else if self.status == "disconnected" || self.status == "failed" {
            PetNotificationKind::Failed
        } else if self.active_turn_id.is_some() {
            PetNotificationKind::Running
        } else {
            PetNotificationKind::Review
        };
        if state.notification != Some(kind) {
            if let Some(pet) = &mut state.pet {
                pet.set_notification(kind, /*body*/ None);
            }
            state.notification = Some(kind);
        }
        let overlay = self.active_input_route().is_some();
        let request = if !overlay && content.width < area.width {
            let pet_area = Rect::new(
                content.right(),
                area.y,
                area.right() - content.right(),
                area.height,
            );
            state.pet.as_ref().and_then(|pet| {
                let request = pet.draw_request(pet_area, bottom);
                if request.is_some() {
                    pet.schedule_next_frame();
                }
                request
            })
        } else {
            None
        };
        match state.render.draw(&mut std::io::stdout(), request) {
            Ok(()) => Ok(()),
            Err(PetImageRenderError::Terminal(error)) => Err(error),
            Err(PetImageRenderError::Asset(error)) => {
                tracing::warn!(%error, "terminal pet asset unavailable");
                state.pet = None;
                self.terminal_clear_requested.set(true);
                state
                    .render
                    .draw(&mut std::io::stdout(), /*request*/ None)
                    .map_err(std::io::Error::other)
            }
        }
    }
}

#[cfg(test)]
#[path = "pets_tests.rs"]
mod tests;
