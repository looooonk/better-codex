use super::ComposerDraft;
use super::ComposerState;
use super::QueueEdit;
use super::QueuedMessage;
use super::QueuedMessageIdentity;
use codex_app_server_protocol::QueuedSubmission;
use codex_app_server_protocol::UserInput;
use std::collections::HashSet;
use std::collections::VecDeque;

impl ComposerState {
    pub(in crate::app_shell) fn restore_failed_queued_submission(&mut self, submission: &str) {
        let Some(draft) = self
            .queued_index
            .and_then(|_| self.draft_before_queue.as_mut())
        else {
            self.restore_failed_submission(submission);
            return;
        };
        let text = draft.input.text().to_string();
        draft.input.set_text(if text.is_empty() {
            submission.to_string()
        } else {
            format!("{submission}\n\n{text}")
        });
        draft.history_index = None;
        draft.draft_before_history.clear();
    }

    pub(in crate::app_shell) fn clone_without_queue(&self) -> Self {
        let mut composer = self.clone();
        composer.finish_queued_message_edit();
        composer.queued.clear();
        composer.queue_edits.clear();
        composer
    }

    pub(in crate::app_shell) fn queue_current_message(&mut self) -> bool {
        self.queue_current_message_with_client_id(format!("local-{}", uuid::Uuid::new_v4()))
    }

    pub(in crate::app_shell) fn queue_current_message_with_client_id(
        &mut self,
        client_user_message_id: String,
    ) -> bool {
        if self.queued_index.is_some() {
            return self.finish_queued_message_edit();
        }
        let message = self.submission_text();
        if message.trim().is_empty() {
            return false;
        }

        self.queued.push_back(QueuedMessage {
            id: None,
            client_user_message_id,
            text: message,
            editable: true,
        });
        self.clear();
        true
    }

    pub(in crate::app_shell) fn replace_queued_submissions(
        &mut self,
        submissions: Vec<QueuedSubmission>,
    ) {
        self.finish_queued_message_edit();
        let durable_client_ids = submissions
            .iter()
            .map(|submission| submission.client_user_message_id.as_str())
            .collect::<HashSet<_>>();
        let pending_local = self
            .queued
            .iter()
            .filter(|message| {
                message.id.is_none()
                    && !durable_client_ids.contains(message.client_user_message_id.as_str())
            })
            .cloned()
            .collect::<VecDeque<_>>();
        self.queued = submissions
            .into_iter()
            .map(|submission| QueuedMessage {
                id: Some(submission.id),
                client_user_message_id: submission.client_user_message_id,
                text: super::super::format_user_inputs(&submission.input),
                editable: matches!(
                    submission.input.as_slice(),
                    [UserInput::Text { text_elements, .. }] if text_elements.is_empty()
                ),
            })
            .chain(pending_local)
            .collect();
    }

    pub(in crate::app_shell) fn confirm_queued_submission(&mut self, submission: QueuedSubmission) {
        let Some(message) = self.queued.iter_mut().find(|message| {
            message.client_user_message_id == submission.client_user_message_id
        }) else {
            return;
        };
        message.id = Some(submission.id);
    }

    pub(in crate::app_shell) fn remove_queued_submission_for_client(
        &mut self,
        client_id: &str,
    ) -> Option<String> {
        if let Some(index) = self
            .queued
            .iter()
            .position(|message| message.client_user_message_id == client_id)
        {
            let selected = self.queued_index == Some(index);
            let text = if selected {
                self.submission_text()
            } else {
                self.queued[index].text.clone()
            };
            if selected {
                self.queued.remove(index);
                self.restore_queue_draft();
            } else {
                self.remove_queued_submission_at(index);
            }
            return (!text.trim().is_empty()).then_some(text);
        }
        None
    }

    pub(in crate::app_shell) fn drain_queue_edits(
        &mut self,
    ) -> impl Iterator<Item = QueueEdit> + '_ {
        self.queue_edits.drain(..)
    }

    pub(in crate::app_shell) fn has_queued_messages(&self) -> bool {
        !self.queued.is_empty()
    }

    pub(in crate::app_shell) fn queued_count(&self) -> usize {
        self.queued.len()
    }

    pub(in crate::app_shell) fn queued_messages(
        &self,
    ) -> impl DoubleEndedIterator<Item = (usize, &str)> + ExactSizeIterator {
        self.queued
            .iter()
            .enumerate()
            .map(|(index, message)| (index, message.text.as_str()))
    }

    pub(in crate::app_shell) fn queued_edit_position(&self) -> Option<(usize, usize)> {
        self.queued_index
            .map(|index| (index.saturating_add(1), self.queued.len()))
    }

    pub(in crate::app_shell) fn edit_queued_message(&mut self, mut index: usize) -> bool {
        if !self.queued.get(index).is_some_and(|message| message.editable) {
            return false;
        }
        match self.queued_index {
            Some(current) if current == index => return true,
            Some(current) => {
                if self.save_queued_message_edit() && current < index {
                    index = index.saturating_sub(1);
                }
            }
            None => {
                self.draft_before_queue = Some(ComposerDraft {
                    input: self.input.clone(),
                    history_index: self.history_index,
                    draft_before_history: self.draft_before_history.clone(),
                });
            }
        }
        self.select_queued_message(index);
        true
    }

    pub(in crate::app_shell) fn edit_previous_queued_message(&mut self) -> bool {
        if self.queued.is_empty() {
            return false;
        }

        let index = if let Some(index) = self.queued_index {
            self.save_queued_message_edit();
            if self.queued.is_empty() {
                self.restore_queue_draft();
                return true;
            }
            let upper = index
                .saturating_sub(1)
                .min(self.queued.len().saturating_sub(1));
            let Some(index) = (0..=upper)
                .rev()
                .find(|index| self.queued[*index].editable)
            else {
                self.restore_queue_draft();
                return true;
            };
            index
        } else {
            let Some(index) = (0..self.queued.len())
                .rev()
                .find(|index| self.queued[*index].editable)
            else {
                return false;
            };
            self.draft_before_queue = Some(ComposerDraft {
                input: self.input.clone(),
                history_index: self.history_index,
                draft_before_history: self.draft_before_history.clone(),
            });
            index
        };
        self.select_queued_message(index);
        true
    }

    pub(in crate::app_shell) fn edit_next_queued_message(&mut self) -> bool {
        let Some(index) = self.queued_index else {
            return false;
        };
        let removed = self.save_queued_message_edit();
        let next = if removed {
            index
        } else {
            index.saturating_add(1)
        };
        if let Some(next) = (next..self.queued.len()).find(|index| self.queued[*index].editable) {
            self.select_queued_message(next);
        } else {
            self.restore_queue_draft();
        }
        true
    }

    pub(in crate::app_shell) fn finish_queued_message_edit(&mut self) -> bool {
        if self.queued_index.is_none() {
            return false;
        }
        self.save_queued_message_edit();
        self.restore_queue_draft();
        true
    }

    pub(in crate::app_shell) fn reorder_queued_message(&mut self, offset: isize) -> bool {
        let Some(index) = self.queued_index else {
            return false;
        };
        if self.save_queued_message_edit() {
            if self.queued.is_empty() {
                self.restore_queue_draft();
            } else if let Some(index) = self.editable_index_near(index) {
                self.select_queued_message(index);
            } else {
                self.restore_queue_draft();
            }
            return true;
        }
        let destination = index.saturating_add_signed(offset);
        if destination >= self.queued.len() || destination == index {
            return true;
        }
        self.queued.swap(index, destination);
        self.queued_index = Some(destination);
        self.queue_edits.push_back(QueueEdit::Reorder {
            order: self
                .queued
                .iter()
                .map(|message| QueuedMessageIdentity {
                    id: message.id.clone(),
                    client_user_message_id: message.client_user_message_id.clone(),
                })
                .collect(),
        });
        true
    }

    pub(in crate::app_shell) fn prepare_next_queued_message(&mut self) -> Option<String> {
        self.finish_queued_message_edit();
        self.queued.front().map(|message| message.text.clone())
    }

    pub(in crate::app_shell) fn confirm_next_queued_message(&mut self, message: &str) {
        if self.queued.front().map(|queued| queued.text.as_str()) == Some(message) {
            self.queued.pop_front();
        }
        self.remember_submission(message);
    }

    fn restore_queue_draft(&mut self) {
        self.queued_index = None;
        let draft = self.draft_before_queue.take().unwrap_or_default();
        self.input = draft.input;
        self.history_index = draft.history_index;
        self.draft_before_history = draft.draft_before_history;
    }

    fn select_queued_message(&mut self, index: usize) {
        self.queued_index = Some(index);
        if let Some(message) = self.queued.get(index) {
            self.set_text(message.text.clone());
        }
    }

    fn save_queued_message_edit(&mut self) -> bool {
        let Some(index) = self.queued_index else {
            return false;
        };
        let message = self.submission_text();
        if message.trim().is_empty() {
            if let Some(removed) = self.queued.remove(index) {
                self.queue_edits.push_back(QueueEdit::Delete {
                    id: removed.id,
                    client_user_message_id: removed.client_user_message_id,
                });
                return true;
            }
            return false;
        }
        if let Some(queued) = self.queued.get_mut(index)
            && queued.text != message
        {
            queued.text = message.clone();
            self.queue_edits.push_back(QueueEdit::Update {
                id: queued.id.clone(),
                client_user_message_id: queued.client_user_message_id.clone(),
                text: message,
            });
        }
        false
    }

    fn remove_queued_submission_at(&mut self, index: usize) {
        if self.queued.remove(index).is_none() {
            return;
        }
        let Some(selected) = self.queued_index else {
            return;
        };
        if selected > index {
            self.queued_index = Some(selected.saturating_sub(1));
        } else if selected == index {
            if self.queued.is_empty() {
                self.restore_queue_draft();
            } else if let Some(index) = self.editable_index_near(index) {
                self.select_queued_message(index);
            } else {
                self.restore_queue_draft();
            }
        }
    }

    fn editable_index_near(&self, index: usize) -> Option<usize> {
        let index = index.min(self.queued.len());
        (index..self.queued.len())
            .find(|index| self.queued[*index].editable)
            .or_else(|| {
                (0..index)
                    .rev()
                    .find(|index| self.queued[*index].editable)
            })
    }
}
