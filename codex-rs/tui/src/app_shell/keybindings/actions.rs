use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::backend::AppShellBackend;
use crate::key_hint::alt;
use crate::key_hint::ctrl;
use crate::key_hint::plain;
use crate::key_hint::shift;
use crate::keymap::KeymapContext::*;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app_shell) enum ShellShortcut {
    Key(KeyBinding),
    ExternalEditor,
    Transcript,
    Find,
    Activity,
    Side,
    Warnings,
    ClearTerminal,
    Fast,
    Interrupt,
    Voice,
    VoiceMute,
    Reasoning(i8),
    Approval(&'static str),
}

pub(super) fn target(id: KeymapActionId) -> Option<ShellShortcut> {
    use KeyCode::*;
    Some(match (id.context, id.action) {
        (Global, "open_agents") => ShellShortcut::Key(plain(F(2))),
        (Global, "open_transcript") => ShellShortcut::Transcript,
        (Global, "find_transcript") => ShellShortcut::Find,
        (Global, "focus_activity") => ShellShortcut::Activity,
        (Global, "toggle_side_conversation") => ShellShortcut::Side,
        (Global, "open_warnings") => ShellShortcut::Warnings,
        (Global, "open_external_editor") => ShellShortcut::ExternalEditor,
        (Global, "copy") => ShellShortcut::Key(ctrl(Char('o'))),
        (Global, "clear_terminal") => ShellShortcut::ClearTerminal,
        (Global, "toggle_fast_mode") => ShellShortcut::Fast,
        (Chat, "interrupt_turn") => ShellShortcut::Interrupt,
        (Chat, "toggle_voice") => ShellShortcut::Voice,
        (Voice, "toggle_voice_mute") => ShellShortcut::VoiceMute,
        (Chat, "decrease_reasoning_effort") => ShellShortcut::Reasoning(-1),
        (Chat, "increase_reasoning_effort") => ShellShortcut::Reasoning(1),
        (Chat, "edit_queued_message") => ShellShortcut::Key(alt(Up)),
        (Composer, "submit") | (List, "accept") => ShellShortcut::Key(plain(Enter)),
        (Composer, "queue") => ShellShortcut::Key(plain(Tab)),
        (Composer, "toggle_shortcuts") => ShellShortcut::Key(plain(F(4))),
        (Editor, "insert_newline") => ShellShortcut::Key(shift(Enter)),
        (Editor | List, "move_left") => ShellShortcut::Key(plain(Left)),
        (Editor | List, "move_right") => ShellShortcut::Key(plain(Right)),
        (Editor | List, "move_up") | (Pager, "scroll_up") => ShellShortcut::Key(plain(Up)),
        (Editor | List, "move_down") | (Pager, "scroll_down") => ShellShortcut::Key(plain(Down)),
        (Editor, "move_word_left") => ShellShortcut::Key(alt(Left)),
        (Editor, "move_word_right") => ShellShortcut::Key(alt(Right)),
        (Editor, "move_line_start") | (List | Pager, "jump_top") => ShellShortcut::Key(plain(Home)),
        (Editor, "move_line_end") | (List | Pager, "jump_bottom") => ShellShortcut::Key(plain(End)),
        (Editor, "delete_backward") => ShellShortcut::Key(plain(Backspace)),
        (Editor, "delete_forward") => ShellShortcut::Key(plain(Delete)),
        (Editor, "delete_backward_word") => ShellShortcut::Key(alt(Backspace)),
        (Editor, "kill_line_start") => ShellShortcut::Key(ctrl(Char('u'))),
        (List | Pager, "page_up") => ShellShortcut::Key(plain(PageUp)),
        (List | Pager, "page_down") => ShellShortcut::Key(plain(PageDown)),
        (List, "cancel") | (Pager, "close") => ShellShortcut::Key(plain(Esc)),
        (
            Approval,
            "approve"
            | "approve_for_session"
            | "approve_for_prefix"
            | "deny"
            | "decline"
            | "cancel",
        ) => ShellShortcut::Approval(id.action),
        _ => return None,
    })
}

pub(super) fn defaults(id: KeymapActionId) -> Vec<KeyBinding> {
    use KeyCode::*;
    match (id.context, id.action) {
        (Global, "open_agents") => vec![plain(F(2)), ctrl(Char(' '))],
        (Chat, "interrupt_turn") => vec![ctrl(Char('c'))],
        (Editor, "insert_newline") => {
            vec![shift(Enter), alt(Enter), ctrl(Char('j')), ctrl(Char('m'))]
        }
        (Editor, "move_left") => vec![plain(Left), shift(Left)],
        (Editor, "move_right") => vec![plain(Right), shift(Right)],
        (Editor, "move_word_left") => vec![alt(Left), ctrl(Left), alt(Char('b'))],
        (Editor, "move_word_right") => vec![alt(Right), ctrl(Right), alt(Char('f'))],
        (Editor, "move_line_start") => vec![
            plain(Home),
            ctrl(Char('a')),
            KeyBinding::new(Left, KeyModifiers::SUPER),
        ],
        (Editor, "move_line_end") => vec![
            plain(End),
            ctrl(Char('e')),
            KeyBinding::new(Right, KeyModifiers::SUPER),
        ],
        (Editor, "delete_backward") => {
            vec![plain(Backspace), shift(Backspace), plain(Char('\u{007f}'))]
        }
        (Editor, "delete_forward") => vec![plain(Delete), shift(Delete)],
        (Editor, "kill_line_start") => vec![
            ctrl(Char('u')),
            KeyBinding::new(Backspace, KeyModifiers::SUPER),
            KeyBinding::new(Char('\u{007f}'), KeyModifiers::SUPER),
        ],
        (Editor, "delete_backward_word") => vec![
            alt(Backspace),
            ctrl(Backspace),
            alt(Char('\u{007f}')),
            ctrl(Char('\u{007f}')),
        ],
        (List, "move_up") => vec![plain(Up), plain(Char('k'))],
        (List, "move_down") => vec![plain(Down), plain(Char('j'))],
        (List, "jump_top") => vec![plain(Home), plain(Char('g'))],
        (List, "jump_bottom") => vec![plain(End), plain(Char('G'))],
        (Pager, "scroll_up") => vec![plain(Up), plain(Char('k'))],
        (Pager, "scroll_down") => vec![plain(Down), plain(Char('j'))],
        (Pager, "close") => vec![plain(Esc), plain(Char('q'))],
        (Approval, "approve") => vec![
            plain(Char('a')),
            plain(Char('A')),
            plain(Char('y')),
            plain(Char('Y')),
        ],
        (Approval, "decline") => vec![
            plain(Esc),
            plain(Char('n')),
            plain(Char('N')),
            plain(Char('d')),
            plain(Char('D')),
        ],
        _ => match target(id) {
            Some(ShellShortcut::Key(key)) => vec![key],
            _ => Vec::new(),
        },
    }
}

impl ShellState {
    pub(in crate::app_shell) async fn dispatch_keybinding<S: AppShellBackend>(
        &mut self,
        action: ShellShortcut,
        config: &crate::legacy_core::config::Config,
        app_server: &mut S,
    ) -> Result<()> {
        match action {
            ShellShortcut::Key(_) => unreachable!("key mappings use the existing input route"),
            ShellShortcut::ExternalEditor => self.request_external_editor(),
            ShellShortcut::Transcript => self.select_latest_transcript_item(),
            ShellShortcut::Find => self.open_transcript_find(""),
            ShellShortcut::Side => self.run_side_command("", config, app_server).await?,
            ShellShortcut::Activity => {
                self.dashboard_visible = true;
                self.set_dashboard_route(crate::app_shell::navigation::DashboardRoute::Agents);
                self.session_list.focused = false;
                self.settings.focused = false;
                self.agents_focused = true;
            }
            ShellShortcut::Warnings => {
                self.run_diagnostic_command(
                    crate::app_shell::diagnostic_commands::DiagnosticCommand::Warnings,
                    app_server,
                )?;
            }
            ShellShortcut::ClearTerminal => self.terminal_clear_requested.set(true),
            ShellShortcut::Fast => {
                if !config.features.enabled(codex_features::Feature::FastMode)
                    || self.active_turn_id.is_some()
                    || self.has_pending_backend_action(
                        crate::app_shell::backend_actions::ActionGroup::TurnStart,
                    )
                {
                    self.push_status(
                        "Fast mode can be changed when it is enabled and the conversation is idle",
                    );
                    return Ok(());
                }
                if !self
                    .available_models
                    .iter()
                    .find(|model| model.model == self.model)
                    .is_some_and(codex_protocol::openai_models::ModelPreset::supports_fast_mode)
                {
                    self.push_status("Fast mode is unavailable for the current model");
                    return Ok(());
                }
                use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
                use codex_protocol::config_types::ServiceTier;
                let enabled = self
                    .service_tier
                    .as_deref()
                    .and_then(ServiceTier::from_request_value)
                    == Some(ServiceTier::Fast);
                let tier = if enabled {
                    SERVICE_TIER_DEFAULT_REQUEST_VALUE
                } else {
                    ServiceTier::Fast.request_value()
                };
                self.apply_service_tier(Some(tier.to_string()), app_server);
            }
            ShellShortcut::Interrupt => {
                if self.active_turn_id.is_some() {
                    self.interrupt_active_turn(app_server).await?;
                } else if self.has_pending_shell_command() {
                    self.cancel_shell_command();
                }
            }
            ShellShortcut::Voice => self.run_voice_command("", app_server)?,
            ShellShortcut::VoiceMute => self.toggle_voice_mute(app_server)?,
            ShellShortcut::Reasoning(step) => {
                if let Some(model) = self
                    .available_models
                    .iter()
                    .find(|model| model.model == self.model)
                {
                    let choices = &model.supported_reasoning_efforts;
                    let current = self
                        .reasoning_effort
                        .as_ref()
                        .unwrap_or(&model.default_reasoning_effort);
                    if let Some(index) = choices.iter().position(|choice| &choice.effort == current)
                    {
                        let next = index
                            .saturating_add_signed(isize::from(step))
                            .min(choices.len().saturating_sub(1));
                        let effort = choices[next].effort.clone();
                        self.apply_reasoning_effort(Some(effort), app_server);
                    }
                }
            }
            ShellShortcut::Approval(action) => {
                if let Some(action) = self
                    .pending_approval
                    .as_ref()
                    .and_then(|pending| pending.keymap_action(action))
                {
                    self.handle_pending_approval_action(app_server, action)
                        .await?;
                }
            }
        }
        Ok(())
    }
}
