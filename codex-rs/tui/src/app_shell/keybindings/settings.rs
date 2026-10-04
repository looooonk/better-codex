use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::selector::SelectorOption;
use crate::app_shell::selector::SelectorState;
use crate::app_shell::selector::SelectorValue;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use crate::legacy_core::config::edit::keymap_binding_clear_edit;
use crate::legacy_core::config::edit::keymap_bindings_edit;
use codex_config::types::KeybindingsSpec;
use color_eyre::Result;
use color_eyre::eyre::eyre;

impl ShellState {
    pub(in crate::app_shell) async fn run_keymap_command(
        &mut self,
        args: &str,
        config: &crate::legacy_core::config::Config,
    ) -> Result<()> {
        let args = args.trim();
        if args.is_empty() {
            let options = keymap_action_ids()
                .filter(|id| {
                    actions::target(*id).is_some()
                        && configured_binding_for_action(&self.keybindings.source, *id).is_some()
                })
                .map(|id| {
                    let action = format!("{}.{}", id.context.config_name(), id.action);
                    let defaults = actions::defaults(id)
                        .iter()
                        .map(KeyBinding::display_label)
                        .collect::<Vec<_>>()
                        .join(" / ");
                    let defaults = if defaults.is_empty() {
                        "unbound"
                    } else {
                        &defaults
                    };
                    let hint = self
                        .keybindings
                        .hint(id.context.config_name(), id.action, defaults);
                    SelectorOption::new(
                        SelectorValue::Keybinding(action.clone()),
                        action,
                        format!("{hint} - select to change"),
                    )
                })
                .collect();
            self.open_selector(SelectorState::new("Key bindings", options));
            return Ok(());
        }
        if self.pets.has_work()
            || self.has_pending_backend_action(
                crate::app_shell::backend_actions::ActionGroup::Settings,
            )
        {
            return Err(eyre!(
                "Wait for the pending settings update before changing shortcuts"
            ));
        }
        let (name, value) = args.split_once(char::is_whitespace)
            .ok_or_else(|| eyre!("Use /keymap context.action key, unbind, or default. Two-key chords and JSON arrays are supported."))?;
        let (context, action) = name
            .split_once('.')
            .ok_or_else(|| eyre!("Choose a context.action from /keymap"))?;
        let id = crate::keymap::keymap_action_id(context, action)
            .filter(|id| {
                actions::target(*id).is_some()
                    && configured_binding_for_action(&self.keybindings.source, *id).is_some()
            })
            .ok_or_else(|| {
                eyre!("Unsupported action `{name}`. Use /keymap to see available actions.")
            })?;
        let value = value.trim();
        let binding: Option<KeybindingsSpec> = match value {
            "default" => None,
            "unbind" => Some(KeybindingsSpec::Many(Vec::new())),
            value if value.starts_with('[') => Some(serde_json::from_str(value)?),
            value => Some(serde_json::from_value(serde_json::json!(value))?),
        };
        let mut source = serde_json::to_value(&self.keybindings.source)?;
        source[id.context.config_name()][id.action] = serde_json::to_value(&binding)?;
        let source: TuiKeymap = serde_json::from_value(source)?;
        ShellKeymap::from_config(&source).map_err(|error| eyre!(error))?;
        let edit = match &binding {
            Some(binding) => keymap_bindings_edit(
                context,
                action,
                &binding
                    .specs()
                    .iter()
                    .map(|spec| spec.as_str().to_string())
                    .collect::<Vec<_>>(),
            ),
            None => keymap_binding_clear_edit(context, action),
        };
        ConfigEditsBuilder::for_config_path(self.client_config_path.as_path())
            .with_edits([edit])
            .apply()
            .await
            .map_err(|error| eyre!(error.to_string()))?;
        let stack = crate::presentation_config::reload(
            &config.config_layer_stack,
            &self.client_config_path,
        )
        .await?;
        let effective: codex_config::config_toml::ConfigToml =
            stack.effective_config().try_into()?;
        let effective = effective.tui.unwrap_or_default().keymap;
        let overridden = binding.is_some()
            && configured_binding_for_action(&effective, id)
                != configured_binding_for_action(&source, id);
        self.keybindings = ShellKeymap::from_config(&effective).map_err(|error| eyre!(error))?;
        if let Some(parent) = self.side_parent.as_mut() {
            parent.keybindings = self.keybindings.clone();
        }
        self.push_status(if overridden {
            format!("Saved {name}, but a higher-priority configuration layer overrides it")
        } else {
            format!("Saved {name}: {value}")
        });
        Ok(())
    }

    pub(in crate::app_shell) fn keybinding_help_lines(&self) -> Vec<ratatui::text::Line<'static>> {
        use ratatui::style::Stylize;
        let mut lines = vec![
            "CONFIGURED SHORTCUTS".bold().into(),
            "Use /keymap to change a binding; unbind disables it."
                .dim()
                .into(),
        ];
        for id in keymap_action_ids().filter(|id| {
            self.keybindings
                .configured(id.context.config_name(), id.action)
        }) {
            let label = self
                .keybindings
                .hint(id.context.config_name(), id.action, "unbound");
            lines.push(format!("{label}  {}.{}", id.context.config_name(), id.action).into());
        }
        lines
    }
}
