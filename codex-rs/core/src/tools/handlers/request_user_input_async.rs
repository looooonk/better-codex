use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::items::AgentMessageContent;
use codex_protocol::items::AgentMessageItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use std::collections::BTreeMap;

const TOOL_NAME: &str = "request_user_input_async";
const MAX_ARGUMENT_BYTES: usize = 3_000;

pub struct RequestUserInputAsyncHandler;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Questions {
    questions: Vec<Question>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    title: String,
    options: Option<Vec<String>>,
}

impl ToolExecutor<ToolInvocation> for RequestUserInputAsyncHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        let mut title = JsonSchema::string(Some(
            "A self-contained question, up to 500 characters.".to_string(),
        ));
        title.max_length = Some(500);
        let mut option = JsonSchema::string(Some(
            "A suggested answer, up to 150 characters.".to_string(),
        ));
        option.max_length = Some(150);
        let mut options = JsonSchema::array(option, Some("Optional suggested answers, recommended answer first. Users can always reply with their own text; no answer is submitted automatically.".to_string()));
        options.min_items = Some(1);
        options.max_items = Some(8);
        let mut questions = JsonSchema::array(
            JsonSchema::object(
                BTreeMap::from([
                    ("title".to_string(), title),
                    ("options".to_string(), options),
                ]),
                Some(vec!["title".to_string()]),
                Some(false.into()),
            ),
            None,
        );
        questions.min_items = Some(1);
        questions.max_items = Some(3);
        ToolSpec::Function(ResponsesApiTool {
            name: TOOL_NAME.to_string(),
            description: "Ask up to three concise questions to request missing information, preferences, clarification, or approval. Questions and suggested answers appear in the conversation. This tool returns immediately; the user replies through the composer as a new user message while you continue independent work. An unanswered question is not approval. Keep the entire JSON argument under 3000 bytes.".to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(BTreeMap::from([("questions".to_string(), questions)]), Some(vec!["questions".to_string()]), Some(false.into())),
            output_schema: None,
        })
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            let ToolInvocation {
                session,
                turn,
                call_id,
                payload,
                ..
            } = invocation;
            if turn.session_source.is_non_root_agent() {
                return Err(FunctionCallError::RespondToModel(
                    "Only the root thread can ask the user questions".to_string(),
                ));
            }
            let ToolPayload::Function { arguments } = payload else {
                return Err(FunctionCallError::RespondToModel(
                    "Expected function arguments".to_string(),
                ));
            };
            if arguments.len() > MAX_ARGUMENT_BYTES {
                return Err(FunctionCallError::RespondToModel(
                    "Questions must fit within 3000 bytes of JSON".to_string(),
                ));
            }
            let args: Questions = parse_arguments(&arguments)?;
            if !(1..=3).contains(&args.questions.len())
                || args.questions.iter().any(|question| {
                    question.title.trim().is_empty()
                        || question.title.chars().count() > 500
                        || question.options.as_ref().is_some_and(|options| {
                            !(1..=8).contains(&options.len())
                                || options.iter().any(|option| {
                                    option.trim().is_empty() || option.chars().count() > 150
                                })
                        })
                })
            {
                return Err(FunctionCallError::RespondToModel("Provide 1-3 non-empty questions (up to 500 characters each), with 1-8 non-empty options (up to 150 characters each) when options are included".to_string()));
            }
            let text = args
                .questions
                .into_iter()
                .map(|question| {
                    let mut lines = vec![question.title];
                    lines.extend(
                        question
                            .options
                            .unwrap_or_default()
                            .into_iter()
                            .map(|option| format!("- {option}")),
                    );
                    lines.join("\n")
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            let item = TurnItem::AgentMessage(AgentMessageItem {
                id: call_id,
                content: vec![AgentMessageContent::Text { text }],
                phase: Some(MessagePhase::Commentary),
                memory_citation: None,
            });
            turn.turn_metadata_state
                .mark_user_input_requested_during_turn();
            session.emit_turn_item_started(&turn, &item).await;
            session.emit_turn_item_completed(&turn, item).await;
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                r#"{"accepted":true}"#.to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for RequestUserInputAsyncHandler {}
