use std::collections::HashMap;
use std::time::Duration;

use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use serde_json::Value;
use tokio::time::Instant;

#[derive(Debug, Clone, PartialEq)]
struct AutoResolution {
    delay_ms: u64,
    deadline: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingUserInput {
    request_id: RequestId,
    title: String,
    questions: Vec<ToolRequestUserInputQuestion>,
    current_index: usize,
    answers: HashMap<String, ToolRequestUserInputAnswer>,
    auto_resolution: Option<AutoResolution>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum UserInputAdvance {
    Next,
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
            request_id: request_id.clone(),
            title: format!("Tool input: {}", params.item_id),
            questions: params.questions.clone(),
            current_index: 0,
            answers: HashMap::new(),
            auto_resolution: params.auto_resolution_ms.and_then(|delay_ms| {
                Instant::now()
                    .checked_add(Duration::from_millis(delay_ms))
                    .map(|deadline| AutoResolution { delay_ms, deadline })
            }),
        })
    }

    pub(super) fn request_id(&self) -> &RequestId {
        &self.request_id
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

    pub(super) fn answer_current(&mut self, answer: String) -> Result<UserInputAdvance, String> {
        let Some(question) = self.current_question() else {
            return self.complete();
        };
        let answer = selected_answer(question, answer)?;
        self.answers.insert(
            question.id.clone(),
            ToolRequestUserInputAnswer {
                answers: vec![answer],
            },
        );
        self.current_index += 1;

        if self.current_index >= self.questions.len() {
            self.complete()
        } else {
            Ok(UserInputAdvance::Next)
        }
    }

    fn complete(&self) -> Result<UserInputAdvance, String> {
        let (request_id, result) = self.response()?;
        Ok(UserInputAdvance::Complete { request_id, result })
    }

    fn response(&self) -> Result<(RequestId, Value), String> {
        let result = serde_json::to_value(ToolRequestUserInputResponse {
            answers: self.answers.clone(),
        })
        .map_err(|err| format!("failed to serialize tool input response: {err}"))?;
        Ok((self.request_id.clone(), result))
    }

    fn take_auto_resolution(&mut self) -> Result<Option<(RequestId, Value)>, String> {
        if self.auto_resolution.take().is_none() {
            return Ok(None);
        }
        self.response().map(Some)
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

fn selected_answer(
    question: &ToolRequestUserInputQuestion,
    answer: String,
) -> Result<String, String> {
    let answer = answer.trim();
    if answer.is_empty() {
        return Err("answer cannot be empty".to_string());
    }

    let options = question
        .options
        .as_deref()
        .filter(|options| !options.is_empty());
    if let Some(options) = options {
        if let Ok(index) = answer.parse::<usize>()
            && let Some(option) = index.checked_sub(1).and_then(|index| options.get(index))
        {
            return Ok(option.label.clone());
        }
        if let Some(option) = matching_option(options, answer) {
            return Ok(option.label.clone());
        }
        if question.is_other {
            return Ok(format!("user_note: {answer}"));
        }
        return Err("answer must match one of the listed options".to_string());
    }

    Ok(format!("user_note: {answer}"))
}

fn matching_option<'a>(
    options: &'a [ToolRequestUserInputOption],
    answer: &str,
) -> Option<&'a ToolRequestUserInputOption> {
    options
        .iter()
        .find(|option| option.label.eq_ignore_ascii_case(answer))
}
