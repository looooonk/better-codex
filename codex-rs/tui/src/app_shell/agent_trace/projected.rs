use super::Body;
use super::Delivery;
use super::Records;
use super::records::fingerprint;
use crate::app_shell::agent_activity::AgentLifecycleStatus;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::SubAgentActivityKind;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadItemPayload;
use codex_app_server_protocol::TurnStatus;
use std::sync::Arc;

impl Records {
    pub(super) fn projected(thread: &Thread) -> Self {
        let mut records = Self::default();
        for turn in &thread.turns {
            records.status = Some(match turn.status {
                TurnStatus::InProgress => AgentLifecycleStatus::Running,
                TurnStatus::Completed => AgentLifecycleStatus::Completed,
                TurnStatus::Interrupted => AgentLifecycleStatus::Interrupted,
                TurnStatus::Failed => AgentLifecycleStatus::Errored,
            });
            let start = turn
                .started_at
                .unwrap_or(thread.created_at)
                .saturating_mul(1000);
            records
                .spans
                .push((start, turn.completed_at.map(|end| end.saturating_mul(1000))));
            for item in &turn.items {
                let at = item.completed_at_ms.or(item.started_at_ms).unwrap_or(start);
                match &item.payload {
                    ThreadItemPayload::CollabAgentToolCall {
                        id,
                        tool:
                            tool @ (CollabAgentTool::SpawnAgent
                            | CollabAgentTool::SendInput
                            | CollabAgentTool::SendMessage
                            | CollabAgentTool::FollowupTask),
                        status: CollabAgentToolCallStatus::Completed,
                        sender_thread_id,
                        receiver_thread_ids,
                        prompt: text,
                        ..
                    } => {
                        for to in receiver_thread_ids {
                            if sender_thread_id == &thread.id {
                                let task = matches!(
                                    tool,
                                    CollabAgentTool::SpawnAgent | CollabAgentTool::FollowupTask
                                );
                                records.sent.push(Delivery {
                                    at,
                                    id: Some(id.clone()),
                                    from: sender_thread_id.clone(),
                                    to: to.clone(),
                                    body: Arc::new(Body::from_prompt(text.as_deref())),
                                    kind: if task { "Task" } else { "Message" }.into(),
                                    fingerprint: text.as_deref().map(fingerprint),
                                });
                            }
                        }
                    }
                    ThreadItemPayload::SubAgentActivity {
                        id,
                        kind:
                            kind @ (SubAgentActivityKind::Interacted | SubAgentActivityKind::Started),
                        agent_thread_id,
                        ..
                    } => {
                        records.sent.push(Delivery {
                            at,
                            id: Some(id.clone()),
                            from: thread.id.clone(),
                            to: agent_thread_id.clone(),
                            body: Arc::new(Body::Unavailable),
                            kind: if *kind == SubAgentActivityKind::Started {
                                "Task"
                            } else {
                                "Message"
                            }
                            .into(),
                            fingerprint: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        records
    }
}
