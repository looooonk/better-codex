//! Shared thread input snapshot data used when replaying or restoring UI state.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::VecDeque;
use std::ops::Deref;

use crate::bottom_pane::LocalImageAttachment;
use crate::bottom_pane::MentionBinding;
use crate::bottom_pane::QueuedInputAction;
use crate::user_message::UserMessage;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::user_input::TextElement;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum UserMessageHistoryRecord {
    UserMessageText,
    Override(UserMessageHistoryOverride),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UserMessageHistoryOverride {
    pub(crate) text: String,
    pub(crate) text_elements: Vec<TextElement>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QueuedUserMessage {
    pub(crate) user_message: UserMessage,
    pub(crate) action: QueuedInputAction,
    pub(crate) pending_pastes: Vec<(String, String)>,
}

impl QueuedUserMessage {
    pub(crate) fn new(user_message: UserMessage, action: QueuedInputAction) -> Self {
        Self {
            user_message,
            action,
            pending_pastes: Vec::new(),
        }
    }

    pub(crate) fn into_user_message(self) -> UserMessage {
        self.user_message
    }
}

impl From<UserMessage> for QueuedUserMessage {
    fn from(user_message: UserMessage) -> Self {
        Self::new(user_message, QueuedInputAction::Plain)
    }
}

impl Deref for QueuedUserMessage {
    type Target = UserMessage;

    fn deref(&self) -> &Self::Target {
        &self.user_message
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ThreadComposerState {
    pub(crate) text: String,
    pub(crate) local_images: Vec<LocalImageAttachment>,
    pub(crate) remote_image_urls: Vec<String>,
    pub(crate) text_elements: Vec<TextElement>,
    pub(crate) mention_bindings: Vec<MentionBinding>,
    pub(crate) pending_pastes: Vec<(String, String)>,
    pub(crate) cursor: usize,
}

impl ThreadComposerState {
    pub(crate) fn has_content(&self) -> bool {
        !self.text.is_empty()
            || !self.local_images.is_empty()
            || !self.remote_image_urls.is_empty()
            || !self.text_elements.is_empty()
            || !self.mention_bindings.is_empty()
            || !self.pending_pastes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ThreadInputState {
    pub(crate) composer: Option<ThreadComposerState>,
    pub(crate) pending_steers: VecDeque<UserMessage>,
    pub(crate) pending_steer_history_records: VecDeque<UserMessageHistoryRecord>,
    pub(crate) pending_steer_compare_keys: VecDeque<PendingSteerCompareKey>,
    pub(crate) rejected_steers_queue: VecDeque<UserMessage>,
    pub(crate) rejected_steer_history_records: VecDeque<UserMessageHistoryRecord>,
    pub(crate) queued_user_messages: VecDeque<QueuedUserMessage>,
    pub(crate) queued_user_message_history_records: VecDeque<UserMessageHistoryRecord>,
    pub(crate) user_turn_pending_start: bool,
    pub(crate) current_collaboration_mode: CollaborationMode,
    pub(crate) active_collaboration_mask: Option<CollaborationModeMask>,
    pub(crate) task_running: bool,
    pub(crate) agent_turn_running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingSteerCompareKey {
    pub(crate) message: String,
    pub(crate) image_count: usize,
}
