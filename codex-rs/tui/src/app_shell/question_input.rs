use super::ShellState;
use super::UserInputAdvance;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::composer::ComposerState;
use super::composer_navigation::ComposerNavigationLayout;
use super::question_popup::QuestionHit;
use super::render::ShellView;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

impl ShellState {
    pub(super) fn focused_editor(&self) -> &ComposerState {
        self.pending_user_input
            .as_ref()
            .map_or(&self.composer, |pending| &pending.draft().editor)
    }

    pub(super) fn focused_editor_mut(&mut self) -> &mut ComposerState {
        self.pending_user_input
            .as_mut()
            .map_or(&mut self.composer, |pending| {
                &mut pending.draft_mut().editor
            })
    }

    pub(super) async fn handle_question_hit<S: AppShellBackend>(
        &mut self,
        hit: QuestionHit,
        backend: &mut S,
    ) -> Result<()> {
        let Some(pending) = &mut self.pending_user_input else {
            return Ok(());
        };
        pending.snooze_auto_resolution();
        match hit {
            QuestionHit::Option(index) => {
                pending.select(index);
                self.resolve_pending_user_input(backend).await?;
            }
            QuestionHit::Editor => pending.select(pending.options().len()),
            QuestionHit::Back => pending.previous(),
            QuestionHit::Submit => self.resolve_pending_user_input(backend).await?,
        }
        Ok(())
    }

    pub(super) async fn handle_question_key<S: AppShellBackend>(
        &mut self,
        key: KeyEvent,
        layout: ComposerNavigationLayout,
        backend: &mut S,
    ) -> Result<()> {
        if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
            && let ComposerNavigationLayout::Area(area) = layout
            && let Some(popup) = (ShellView { shell: self }).question_popup(area)
            && let Some(pending) = &self.pending_user_input
        {
            popup.scroll(pending, if key.code == KeyCode::PageUp { -5 } else { 5 });
            return Ok(());
        }
        let Some(pending) = &mut self.pending_user_input else {
            return Ok(());
        };
        pending.error = None;
        if key.code == KeyCode::F(2) {
            pending.previous();
            return Ok(());
        }
        let count = pending.options().len();
        if !pending.editing() {
            let selected = pending.draft().selected;
            let last = if pending.allows_text() {
                count
            } else {
                count.saturating_sub(1)
            };
            match key.code {
                KeyCode::Down | KeyCode::Tab => pending.select((selected + 1).min(last)),
                KeyCode::Up | KeyCode::BackTab => pending.select(selected.saturating_sub(1)),
                KeyCode::Home => pending.select(0),
                KeyCode::End => pending.select(last),
                KeyCode::Enter | KeyCode::Char(' ') if key.modifiers.is_empty() => {
                    self.resolve_pending_user_input(backend).await?
                }
                KeyCode::Char(ch @ '1'..='9') if (ch as usize - '0' as usize) <= count => {
                    pending.select(ch as usize - '1' as usize);
                }
                KeyCode::Char(ch)
                    if pending.allows_text()
                        && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
                {
                    pending.select(count);
                    let result = pending.draft_mut().editor.insert_char(ch);
                    self.report_composer_insert(result);
                }
                _ => {}
            }
            return Ok(());
        }
        if key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('g')
            && !self
                .keybindings
                .configured("global", "open_external_editor")
        {
            self.request_external_editor();
            return Ok(());
        }
        if key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('o')
            && self.keybindings.configured("global", "copy")
        {
            self.copy_text_selection_with(crate::clipboard_copy::copy_to_clipboard);
            return Ok(());
        }
        let editor = &mut pending.draft_mut().editor;
        if let Some(result) = editor.edit_key(key) {
            self.report_composer_insert(result);
            return Ok(());
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let result = editor.insert_str("    ");
                self.report_composer_insert(result);
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                self.resolve_pending_user_input(backend).await?
            }
            KeyCode::Up => self.move_composer_up(layout),
            KeyCode::Down => self.move_composer_down(layout),
            KeyCode::Esc if count > 0 => pending.select(0),
            _ => {}
        }
        Ok(())
    }

    pub(super) async fn resolve_pending_user_input<S: AppShellBackend>(
        &mut self,
        backend: &mut S,
    ) -> Result<()> {
        if self.has_pending_backend_action(ActionGroup::UserInput) {
            return Ok(());
        }
        let Some(pending) = &mut self.pending_user_input else {
            return Ok(());
        };
        let result = match pending.answer_current() {
            Ok(UserInputAdvance::Next) => {
                self.text_selection = Default::default();
                return Ok(());
            }
            Ok(UserInputAdvance::AsyncReply(items)) => {
                if self.reject_unavailable_session_action() || self.reject_direct_input() {
                    if let Some(pending) = &mut self.pending_user_input {
                        pending.error = self.input_hint.clone();
                    }
                    return Ok(());
                }
                self.send_question_reply(backend, items).await
            }
            Ok(UserInputAdvance::Complete { request_id, result }) => backend
                .resolve_server_request(request_id, result)
                .await
                .map_err(Into::into),
            Err(message) => {
                pending.error = Some(message);
                return Ok(());
            }
        };
        if let Err(error) = result {
            if let Some(pending) = &mut self.pending_user_input {
                pending.error = Some(format!("Could not send answers: {error}"));
            }
        } else {
            self.pending_user_input = None;
            self.text_selection = Default::default();
            self.activate_next_interactive_request();
        }
        Ok(())
    }

    async fn send_question_reply<S: AppShellBackend>(
        &mut self,
        backend: &mut S,
        items: Vec<codex_app_server_protocol::UserInput>,
    ) -> Result<()> {
        let display = super::format_user_inputs(&items);
        if let Some(turn_id) = self.active_turn_id.clone() {
            let client_id = format!("better-codex-steer-{}", uuid::Uuid::new_v4());
            backend
                .turn_steer(super::backend::AppShellTurnSteer {
                    thread_id: self.thread_id,
                    turn_id,
                    client_user_message_id: client_id.clone(),
                    items,
                })
                .await?;
            self.push_user_with_client_id(display, client_id);
        } else {
            let params = self
                .turn_start_params(items)
                .map_err(|message| color_eyre::eyre::eyre!(message))?;
            let response = backend.turn_start(params.clone()).await?;
            self.record_active_turn_started(response.turn.id.clone());
            self.push_user_with_client_id(display, params.client_user_message_id.clone());
            self.record_safety_buffering_turn(response.turn.id, params);
        }
        self.scroll_transcript_to_bottom();
        self.status = "thinking".into();
        Ok(())
    }
}

pub(super) fn visible_answer(text: &str) -> Option<String> {
    use codex_context_fragments::AnsweredQuestion;
    use codex_context_fragments::ContextualUserFragment;
    #[derive(serde::Deserialize)]
    struct Reply {
        question: String,
        answer: String,
    }
    let (start, end) = AnsweredQuestion::type_markers();
    let json = text.strip_prefix(start)?.strip_suffix(end)?;
    let replies: Vec<Reply> = serde_json::from_str(json).ok()?;
    Some(
        replies
            .into_iter()
            .map(|reply| format!("{}\n{}", reply.question, reply.answer))
            .collect::<Vec<_>>()
            .join("\n\n"),
    )
}
