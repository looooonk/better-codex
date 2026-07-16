use super::text::MAX_LATEST_MESSAGE_CHARS;
use super::text::bounded_text;
use super::text::concise_summary;
use codex_app_server_protocol::CollabAgentState;
use codex_app_server_protocol::CollabAgentStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::ThreadItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app_shell) enum AgentItemPhase {
    Started,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app_shell) enum AgentChildEvent {
    Message,
    Reasoning,
    Command,
    Output,
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app_shell) enum AgentLifecycleStatus {
    Unknown,
    PendingInit,
    Running,
    Interrupted,
    Completed,
    Errored,
    Shutdown,
    NotFound,
}

impl AgentLifecycleStatus {
    pub(in crate::app_shell) fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::PendingInit => "starting",
            Self::Running => "running",
            Self::Interrupted => "interrupted",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Shutdown => "stopped",
            Self::NotFound => "not found",
        }
    }
}

impl From<&CollabAgentState> for AgentLifecycleStatus {
    fn from(state: &CollabAgentState) -> Self {
        match state.status {
            CollabAgentStatus::PendingInit => Self::PendingInit,
            CollabAgentStatus::Running => Self::Running,
            CollabAgentStatus::Interrupted => Self::Interrupted,
            CollabAgentStatus::Completed => Self::Completed,
            CollabAgentStatus::Errored => Self::Errored,
            CollabAgentStatus::Shutdown => Self::Shutdown,
            CollabAgentStatus::NotFound => Self::NotFound,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::app_shell) enum AgentTimelineEvent {
    Collaboration {
        tool: CollabAgentTool,
        phase: AgentItemPhase,
        status: CollabAgentToolCallStatus,
    },
    Activity(SubAgentActivityKind),
    ChildItem {
        event: AgentChildEvent,
        phase: AgentItemPhase,
    },
    ChildProgress(AgentChildEvent),
    Lifecycle {
        status: AgentLifecycleStatus,
        retrying: bool,
    },
}

impl AgentTimelineEvent {
    pub(in crate::app_shell) fn label(&self) -> &'static str {
        match self {
            Self::Collaboration {
                tool,
                phase: AgentItemPhase::Started,
                ..
            } => match tool {
                CollabAgentTool::SpawnAgent => "spawning agent",
                CollabAgentTool::SendInput => "sending input",
                CollabAgentTool::ResumeAgent => "resuming agent",
                CollabAgentTool::Wait => "waiting for agent",
                CollabAgentTool::CloseAgent => "stopping agent",
            },
            Self::Collaboration {
                tool,
                phase: AgentItemPhase::Completed,
                status,
            } => collaboration_result_label(tool, status),
            Self::Activity(SubAgentActivityKind::Started) => "agent started",
            Self::Activity(SubAgentActivityKind::Interacted) => "agent interacted",
            Self::Activity(SubAgentActivityKind::Interrupted) => "agent interrupted",
            Self::ChildItem { event, phase } => child_item_label(*event, *phase),
            Self::ChildProgress(AgentChildEvent::Message) => "message",
            Self::ChildProgress(AgentChildEvent::Reasoning) => "reasoning",
            Self::ChildProgress(AgentChildEvent::Output) => "command output",
            Self::ChildProgress(AgentChildEvent::Command | AgentChildEvent::Activity) => "activity",
            Self::Lifecycle { retrying: true, .. } => "agent retrying",
            Self::Lifecycle {
                status,
                retrying: false,
            } => match status {
                AgentLifecycleStatus::Unknown => "agent status unknown",
                AgentLifecycleStatus::PendingInit => "agent starting",
                AgentLifecycleStatus::Running => "agent running",
                AgentLifecycleStatus::Interrupted => "agent interrupted",
                AgentLifecycleStatus::Completed => "agent completed",
                AgentLifecycleStatus::Errored => "agent failed",
                AgentLifecycleStatus::Shutdown => "agent stopped",
                AgentLifecycleStatus::NotFound => "agent not found",
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::app_shell) struct AgentTimelineEntry {
    pub(in crate::app_shell) item_id: String,
    pub(in crate::app_shell) event: AgentTimelineEvent,
    pub(super) detail: Option<String>,
}

impl AgentTimelineEntry {
    pub(in crate::app_shell) fn label(&self) -> String {
        let label = self.event.label();
        self.detail
            .as_deref()
            .and_then(concise_summary)
            .map_or_else(|| label.to_string(), |detail| format!("{label}: {detail}"))
    }
}

pub(super) fn child_item_summary(item: &ThreadItem) -> (AgentChildEvent, Option<String>) {
    match item {
        ThreadItem::AgentMessage { text, .. } => (
            AgentChildEvent::Message,
            bounded_text(text, MAX_LATEST_MESSAGE_CHARS),
        ),
        ThreadItem::Reasoning { summary, .. } => (
            AgentChildEvent::Reasoning,
            summary
                .last()
                .and_then(|text| bounded_text(text, MAX_LATEST_MESSAGE_CHARS)),
        ),
        ThreadItem::CommandExecution {
            command,
            aggregated_output,
            ..
        } => (
            AgentChildEvent::Command,
            bounded_text(
                aggregated_output.as_deref().unwrap_or(command),
                MAX_LATEST_MESSAGE_CHARS,
            ),
        ),
        _ => (AgentChildEvent::Activity, None),
    }
}

fn collaboration_result_label(
    tool: &CollabAgentTool,
    status: &CollabAgentToolCallStatus,
) -> &'static str {
    match status {
        CollabAgentToolCallStatus::Failed => "agent operation failed",
        CollabAgentToolCallStatus::InProgress => "agent operation pending",
        CollabAgentToolCallStatus::Completed => match tool {
            CollabAgentTool::SpawnAgent => "agent spawned",
            CollabAgentTool::SendInput => "input delivered",
            CollabAgentTool::ResumeAgent => "agent resumed",
            CollabAgentTool::Wait => "wait complete",
            CollabAgentTool::CloseAgent => "agent stopped",
        },
    }
}

fn child_item_label(event: AgentChildEvent, phase: AgentItemPhase) -> &'static str {
    match (event, phase) {
        (AgentChildEvent::Message, AgentItemPhase::Started) => "writing message",
        (AgentChildEvent::Message, AgentItemPhase::Completed) => "message completed",
        (AgentChildEvent::Reasoning, AgentItemPhase::Started) => "reasoning started",
        (AgentChildEvent::Reasoning, AgentItemPhase::Completed) => "reasoning completed",
        (AgentChildEvent::Command, AgentItemPhase::Started) => "running command",
        (AgentChildEvent::Command, AgentItemPhase::Completed) => "command completed",
        (AgentChildEvent::Output | AgentChildEvent::Activity, AgentItemPhase::Started) => {
            "activity started"
        }
        (AgentChildEvent::Output | AgentChildEvent::Activity, AgentItemPhase::Completed) => {
            "activity completed"
        }
    }
}
