use std::collections::HashMap;

use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::ToolRequestUserInputAnswer;
use codex_app_server_protocol::ToolRequestUserInputOption;
use codex_app_server_protocol::ToolRequestUserInputQuestion;
use codex_app_server_protocol::ToolRequestUserInputResponse;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingUserInput {
    request_id: RequestId,
    title: String,
    questions: Vec<ToolRequestUserInputQuestion>,
    current_index: usize,
    answers: HashMap<String, ToolRequestUserInputAnswer>,
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
        })
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
        let result = serde_json::to_value(ToolRequestUserInputResponse {
            answers: self.answers.clone(),
        })
        .map_err(|err| format!("failed to serialize tool input response: {err}"))?;
        Ok(UserInputAdvance::Complete {
            request_id: self.request_id.clone(),
            result,
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
