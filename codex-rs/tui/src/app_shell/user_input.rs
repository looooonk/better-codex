use super::composer::ComposerState;
use std::cell::Cell;
use std::time::Duration;

use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use codex_app_server_protocol::AsyncUserInputQuestion;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use codex_app_server_protocol::UserInput;
use codex_context_fragments::AnsweredQuestion;
use codex_context_fragments::ContextualUserFragment;
use codex_protocol::models::ContentItem;
use serde_json::Value;
use tokio::time::Instant;

#[derive(Debug, Clone, PartialEq)]
struct AutoResolution {
    delay_ms: u64,
    deadline: Instant,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct QuestionDraft {
    pub(super) selected: usize,
    pub(super) editor: ComposerState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum QuestionSource {
    Server(RequestId),
    Async(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingUserInput {
    pub(super) source: QuestionSource,
    title: String,
    questions: Vec<ToolRequestUserInputQuestion>,
    current_index: usize,
    drafts: Vec<QuestionDraft>,
    pub(super) error: Option<String>,
    pub(super) scroll: Cell<usize>,
    pub(super) reveal_selection: Cell<bool>,
    auto_resolution: Option<AutoResolution>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum UserInputAdvance {
    Next,
    AsyncReply(Vec<UserInput>),
    Complete {
        request_id: RequestId,
        result: Value,
    },
}

impl PendingUserInput {
    pub(super) fn from_request(request: &ServerRequest) -> Option<Self> {
        let ServerRequest::ToolRequestUserInput { request_id, params } = request else {
            return None;
        };

        Some(Self {
            source: QuestionSource::Server(request_id.clone()),
            title: format!("Tool input: {}", params.item_id),
            questions: params.questions.clone(),
            current_index: 0,
            drafts: vec![QuestionDraft::default(); params.questions.len().max(1)],
            error: None,
            scroll: Cell::default(),
            reveal_selection: Cell::default(),
            auto_resolution: (!params.is_blocking)
                .then_some(120_000)
                .and_then(|delay_ms| {
                    Instant::now()
                        .checked_add(Duration::from_millis(delay_ms))
                        .map(|deadline| AutoResolution { delay_ms, deadline })
                }),
        })
    }

    pub(super) fn from_async(item_id: &str, questions: Vec<AsyncUserInputQuestion>) -> Self {
        let drafts = vec![QuestionDraft::default(); questions.len().max(1)];
        Self {
            source: QuestionSource::Async(item_id.to_owned()),
            title: questions
                .first()
                .map(|q| q.title.clone())
                .unwrap_or_default(),
            questions: questions
                .into_iter()
                .enumerate()
                .map(|(index, q)| ToolRequestUserInputQuestion {
                    id: serde_json::json!(["request_user_input_async", item_id, index]).to_string(),
                    header: String::new(),
                    question: q.title,
                    is_other: true,
                    is_secret: false,
                    options: q.options.map(|options| {
                        options
                            .into_iter()
                            .map(|label| ToolRequestUserInputOption {
                                label,
                                description: String::new(),
                            })
                            .collect()
                    }),
                })
                .collect(),
            current_index: 0,
            drafts,
            error: None,
            scroll: Cell::default(),
            reveal_selection: Cell::default(),
            auto_resolution: None,
        }
    }

    pub(super) fn snooze_auto_resolution(&mut self) {
        self.auto_resolution = None;
    }

    pub(super) fn request_id(&self) -> Option<&RequestId> {
        match &self.source {
            QuestionSource::Server(id) => Some(id),
            QuestionSource::Async(_) => None,
        }
    }

    pub(super) fn title(&self) -> &str {
        &self.title
    }

    pub(super) fn current_question(&self) -> Option<&ToolRequestUserInputQuestion> {
        self.questions.get(self.current_index)
    }

    pub(super) fn question_position(&self) -> (usize, usize) {
        (self.current_index.saturating_add(1), self.questions.len())
    }

    pub(super) fn auto_resolution_ms(&self) -> Option<u64> {
        self.auto_resolution
            .as_ref()
            .map(|resolution| resolution.delay_ms)
    }

    fn auto_resolution_deadline(&self) -> Option<Instant> {
        self.auto_resolution
            .as_ref()
            .map(|resolution| resolution.deadline)
    }

    pub(super) fn draft(&self) -> &QuestionDraft {
        &self.drafts[self.current_index]
    }

    pub(super) fn draft_mut(&mut self) -> &mut QuestionDraft {
        &mut self.drafts[self.current_index]
    }

    pub(super) fn options(&self) -> &[ToolRequestUserInputOption] {
        self.current_question()
            .and_then(|q| q.options.as_deref())
            .unwrap_or_default()
    }

    pub(super) fn allows_text(&self) -> bool {
        self.options().is_empty() || self.current_question().is_some_and(|q| q.is_other)
    }

    pub(super) fn editing(&self) -> bool {
        self.draft().selected == self.options().len() && self.allows_text()
    }

    pub(super) fn select(&mut self, index: usize) {
        self.error = None;
        self.draft_mut().selected = index;
        self.reveal_selection.set(true);
        self.snooze_auto_resolution();
    }

    pub(super) fn previous(&mut self) {
        self.current_index = self.current_index.saturating_sub(1);
        self.scroll.set(0);
        self.reveal_selection.set(true);
    }

    pub(super) fn answer_current(&mut self) -> Result<UserInputAdvance, String> {
        if self.current_question().is_some()
            && self.editing()
            && self.draft().editor.text().trim().is_empty()
        {
            return Err("Enter an answer".to_string());
        }
        if self.current_index + 1 < self.questions.len() {
            self.current_index += 1;
            self.scroll.set(0);
            self.reveal_selection.set(false);
            return Ok(UserInputAdvance::Next);
        }
        if matches!(self.source, QuestionSource::Async(_)) {
            let mut items = Vec::with_capacity(self.questions.len());
            for (question, draft) in self.questions.iter().zip(&self.drafts) {
                let answer = draft
                    .selected_option(question)
                    .unwrap_or_else(|| draft.editor.text());
                let fragment = AnsweredQuestion::new(&question.id, &question.question, answer);
                let ContentItem::InputText { text } = fragment.render() else {
                    return Err("Could not format question reply".into());
                };
                items.push(UserInput::Text {
                    text,
                    text_elements: Vec::new(),
                });
            }
            return Ok(UserInputAdvance::AsyncReply(items));
        }
        let (request_id, result) = self.response(self.questions.len())?;
        Ok(UserInputAdvance::Complete { request_id, result })
    }

    fn response(&self, count: usize) -> Result<(RequestId, Value), String> {
        let answers = self
            .questions
            .iter()
            .zip(&self.drafts)
            .take(count)
            .map(|(question, draft)| {
                let answer = draft.selected_option(question).map_or_else(
                    || format!("user_note: {}", draft.editor.text()),
                    str::to_owned,
                );
                (
                    question.id.clone(),
                    ToolRequestUserInputAnswer {
                        answers: vec![answer],
                    },
                )
            })
            .collect();
        let result = serde_json::to_value(ToolRequestUserInputResponse { answers })
            .map_err(|err| format!("failed to serialize tool input response: {err}"))?;
        Ok((
            self.request_id()
                .ok_or("Expected a server request")?
                .clone(),
            result,
        ))
    }

    fn take_auto_resolution(&mut self) -> Result<Option<(RequestId, Value)>, String> {
        if self.auto_resolution.take().is_none() {
            return Ok(None);
        }
        self.response(self.current_index).map(Some)
    }
}

impl ShellState {
    pub(super) fn pending_user_input_auto_resolution_deadline(&self) -> Option<Instant> {
        self.pending_user_input
            .as_ref()
            .and_then(PendingUserInput::auto_resolution_deadline)
    }

    pub(super) fn start_expired_user_input_resolution<S>(&mut self, app_server: &S) -> bool
    where
        S: AppShellBackend,
    {
        let Some(pending) = self.pending_user_input.as_mut() else {
            return false;
        };
        if pending
            .auto_resolution_deadline()
            .is_none_or(|deadline| deadline > Instant::now())
        {
            return false;
        }
        let title = pending.title().to_string();
        let (request_id, result) = match pending.take_auto_resolution() {
            Ok(Some(resolution)) => resolution,
            Ok(None) => return false,
            Err(message) => {
                self.push_error(message);
                return false;
            }
        };
        let response = app_server.resolve_server_request_in_background(request_id.clone(), result);
        self.backend_actions
            .start(Some(ActionGroup::UserInput), async move {
                BackendActionResult::UserInputAutoResolution {
                    request_id,
                    title,
                    result: response.await,
                }
            })
    }
}

impl QuestionDraft {
    fn selected_option<'a>(&self, question: &'a ToolRequestUserInputQuestion) -> Option<&'a str> {
        question
            .options
            .as_ref()?
            .get(self.selected)
            .map(|option| option.label.as_str())
    }
}
