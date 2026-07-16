use super::AgentActivity;
use super::AgentLifecycleStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_protocol::AgentPath;
use std::cmp::Ordering;

pub(super) fn agent_order(left: &AgentActivity, right: &AgentActivity) -> Ordering {
    match (&left.path, &right.path) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| left.thread_id.cmp(&right.thread_id))
}

pub(super) fn fallback_status(
    tool: &CollabAgentTool,
    status: &CollabAgentToolCallStatus,
) -> Option<AgentLifecycleStatus> {
    match status {
        CollabAgentToolCallStatus::Failed => Some(AgentLifecycleStatus::Errored),
        CollabAgentToolCallStatus::InProgress => match tool {
            CollabAgentTool::SpawnAgent => Some(AgentLifecycleStatus::PendingInit),
            CollabAgentTool::SendInput | CollabAgentTool::ResumeAgent => {
                Some(AgentLifecycleStatus::Running)
            }
            CollabAgentTool::Wait | CollabAgentTool::CloseAgent => None,
        },
        CollabAgentToolCallStatus::Completed => match tool {
            CollabAgentTool::SpawnAgent
            | CollabAgentTool::SendInput
            | CollabAgentTool::ResumeAgent => Some(AgentLifecycleStatus::Running),
            CollabAgentTool::CloseAgent => Some(AgentLifecycleStatus::Shutdown),
            CollabAgentTool::Wait => None,
        },
    }
}

pub(super) fn agent_parent_path(path: &AgentPath) -> Option<AgentPath> {
    let (parent, _) = path.as_str().rsplit_once('/')?;
    AgentPath::try_from(parent).ok()
}

pub(super) fn agent_path_depth(path: &AgentPath) -> usize {
    path.as_str().matches('/').count().saturating_sub(1)
}
