use crate::session::turn_context::TurnContext;
use codex_protocol::AgentPath;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::MultiAgentVersion;
use serde_json::Value;

pub(crate) fn bound_spawn_task_name(item: &mut ResponseItem, turn_context: &TurnContext) {
    if turn_context.multi_agent_version != MultiAgentVersion::V2 {
        return;
    }
    let expected_namespace = turn_context
        .provider
        .capabilities()
        .namespace_tools
        .then_some(turn_context.config.multi_agent_v2.tool_namespace.as_deref())
        .flatten();
    let ResponseItem::FunctionCall {
        name,
        namespace,
        arguments,
        ..
    } = item
    else {
        return;
    };
    if name != "spawn_agent" || namespace.as_deref() != expected_namespace {
        return;
    }
    let Ok(mut args) = serde_json::from_str::<Value>(arguments) else {
        return;
    };
    let Some(task_name) = args.get_mut("task_name") else {
        return;
    };
    if task_name
        .as_str()
        .is_none_or(|task_name| task_name.len() <= AgentPath::MAX_NAME_LENGTH)
    {
        return;
    }
    *task_name = Value::String("x".repeat(AgentPath::MAX_NAME_LENGTH + 1));
    if let Ok(bounded_arguments) = serde_json::to_string(&args) {
        *arguments = bounded_arguments;
    }
}
