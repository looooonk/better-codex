use super::Body;
use super::Delivery;
use super::Records;
use crate::app_shell::agent_activity::AgentLifecycleStatus;
use codex_protocol::models::AgentMessageInputContent;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_protocol::protocol::InterAgentCommunication;
use codex_rollout::RolloutItem;
use std::collections::HashMap;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::Path;
use std::sync::Arc;

impl Records {
    fn delivery(
        &mut self,
        at: i64,
        id: Option<String>,
        from: String,
        to: String,
        body: Body,
        header: &str,
    ) {
        let (kind, payload) = envelope(header);
        let body = match (body, payload) {
            (Body::Text(_), Some(text)) => Body::from_prompt(Some(text)),
            (body, _) => body,
        };
        let fingerprint = if let Body::Text(text) = &body {
            Some(fingerprint(text))
        } else {
            None
        };
        self.messages.push(Delivery {
            at,
            id,
            from,
            to,
            kind: kind.into(),
            fingerprint,
            body: Arc::new(body),
        });
    }

    fn legacy(&mut self, at: i64, communication: InterAgentCommunication, owner: &str) {
        if communication.recipient.as_str() != owner {
            return;
        }
        let text = communication.content;
        let body = if communication.encrypted_content.is_some() {
            Body::Encrypted
        } else {
            Body::Text(text.clone())
        };
        let key = communication.encrypted_content.as_deref().map(fingerprint);
        self.delivery(
            at,
            communication.id.map(|id| id.to_string()),
            communication.author.to_string(),
            owner.into(),
            body,
            &text,
        );
        if let Some(key) = key
            && let Some(msg) = self.messages.last_mut()
        {
            msg.fingerprint = Some(key);
        }
    }

    pub(super) async fn read(
        path: &Path,
        owner: &str,
        thread_id: &str,
    ) -> color_eyre::Result<Self> {
        let mut reader = codex_rollout::open_rollout_line_reader(path).await?;
        let mut records = Self::default();
        let mut calls = HashMap::<String, (String, Body, u64)>::new();
        let mut boundary = 0;
        let mut created = i64::MIN;
        while let Some(line) = reader.next_line().await? {
            // Read the tags before decoding the payload. Tool results and reasoning can
            // dominate the size of a rollout and have no place in the communication graph.
            let Ok(tag) = serde_json::from_str::<RecordTag>(&line) else {
                continue;
            };
            if !tag.relevant() {
                continue;
            }
            let Ok(raw) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let payload = &raw["payload"];
            let kind = payload["type"].as_str().unwrap_or_default();
            if raw["type"] == "session_meta" {
                color_eyre::eyre::ensure!(payload["id"] == thread_id, "Mismatched thread history");
                boundary = payload["subagent_history_start_ordinal"]
                    .as_u64()
                    .unwrap_or(0);
                if let Some(timestamp) = payload["timestamp"].as_str() {
                    created = timestamp_ms(timestamp).unwrap_or(i64::MIN);
                }
                continue;
            }
            if raw["ordinal"].as_u64().is_some_and(|n| n < boundary) {
                continue;
            }
            let Some(at) = raw["timestamp"].as_str().and_then(timestamp_ms) else {
                continue;
            };
            if at < created {
                continue;
            }
            match (raw["type"].as_str(), kind) {
                (Some("event_msg"), "task_started" | "turn_started") => {
                    records.status = Some(AgentLifecycleStatus::Running);
                    records.spans.push((at, None));
                }
                (Some("event_msg"), "task_complete" | "turn_complete" | "turn_aborted") => {
                    records.status = Some(if kind == "turn_aborted" {
                        AgentLifecycleStatus::Interrupted
                    } else if !payload["error"].is_null() {
                        AgentLifecycleStatus::Errored
                    } else {
                        AgentLifecycleStatus::Completed
                    });
                    if let Some(answer) = payload["last_agent_message"].as_str() {
                        records
                            .answers
                            .entry(fingerprint(answer))
                            .or_default()
                            .push(at);
                    }
                    if let Some((_, end)) = records.spans.last_mut() {
                        *end = Some(at);
                    }
                }
                (Some("response_item"), "function_call")
                    if matches!(
                        payload["name"].as_str(),
                        Some("spawn_agent" | "send_message" | "followup_task")
                    ) =>
                {
                    if let Some(id) = payload["call_id"].as_str()
                        && let Some(args) = payload["arguments"]
                            .as_str()
                            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                        && let Some(text) = args["message"].as_str()
                    {
                        let kind = if payload["name"] == "send_message" {
                            "Message"
                        } else {
                            "Task"
                        };
                        calls.insert(
                            id.into(),
                            (
                                kind.into(),
                                Body::from_prompt(Some(text)),
                                fingerprint(text),
                            ),
                        );
                    }
                }
                (Some("event_msg"), "item_completed")
                    if payload["thread_id"] == thread_id
                        && payload["item"]["type"] == "SubAgentActivity" =>
                {
                    let item = &payload["item"];
                    if matches!(item["kind"].as_str(), Some("interacted" | "started"))
                        && let (Some(id), Some(to)) =
                            (item["id"].as_str(), item["agent_thread_id"].as_str())
                    {
                        let kind = if item["kind"] == "started" {
                            "Task"
                        } else {
                            "Message"
                        };
                        let (kind, body, fingerprint) = calls.remove(id).map_or(
                            (kind.into(), Body::Unavailable, None),
                            |(kind, body, key)| (kind, body, Some(key)),
                        );
                        records.sent.push(Delivery {
                            at,
                            id: Some(id.into()),
                            fingerprint,
                            from: thread_id.into(),
                            to: to.into(),
                            body: Arc::new(body),
                            kind,
                        });
                    }
                }
                (Some("response_item"), "agent_message" | "message")
                | (Some("inter_agent_communication"), _) => {
                    let Ok(line) = codex_rollout::decode_rollout_line(raw) else {
                        continue;
                    };
                    match line.item {
                        RolloutItem::ResponseItem(item) => match item.item {
                            ResponseItem::AgentMessage {
                                id,
                                author,
                                recipient,
                                content,
                                ..
                            } if recipient == owner => {
                                let header = content
                                    .first()
                                    .and_then(|part| match part {
                                        AgentMessageInputContent::InputText { text } => {
                                            Some(text.as_str())
                                        }
                                        _ => None,
                                    })
                                    .unwrap_or_default();
                                match plaintext_agent_message_content(&content) {
                                    Some(text) => records.delivery(
                                        at,
                                        id.map(|id| id.to_string()),
                                        author,
                                        recipient,
                                        Body::Text(text.clone()),
                                        &text,
                                    ),
                                    None => {
                                        let key = content.iter().find_map(|part| match part {
                                            AgentMessageInputContent::EncryptedContent {
                                                encrypted_content,
                                            } => Some(fingerprint(encrypted_content)),
                                            _ => None,
                                        });
                                        let body = if key.is_some() {
                                            Body::Encrypted
                                        } else {
                                            Body::Unavailable
                                        };
                                        records.delivery(
                                            at,
                                            id.map(|id| id.to_string()),
                                            author,
                                            recipient,
                                            body,
                                            header,
                                        );
                                        if let Some(msg) = records.messages.last_mut() {
                                            msg.fingerprint = key;
                                        }
                                    }
                                }
                            }
                            ResponseItem::Message { role, content, .. } if role == "assistant" => {
                                if let Some(c) =
                                    InterAgentCommunication::from_message_content(&content)
                                {
                                    records.legacy(at, c, owner);
                                }
                            }
                            _ => {}
                        },
                        RolloutItem::InterAgentCommunication(c) => records.legacy(at, c, owner),
                        _ => {}
                    }
                }
                (Some("event_msg"), "collab_agent_spawn_end" | "collab_agent_interaction_end") => {
                    let Some(to) = payload["new_thread_id"]
                        .as_str()
                        .or(payload["receiver_thread_id"].as_str())
                    else {
                        continue;
                    };
                    if payload["sender_thread_id"] != thread_id {
                        continue;
                    }
                    let text = payload["prompt"].as_str();
                    records.sent.push(Delivery {
                        at,
                        id: payload["call_id"].as_str().map(str::to_owned),
                        from: thread_id.into(),
                        to: to.into(),
                        body: Arc::new(Body::from_prompt(text)),
                        kind: if kind == "collab_agent_spawn_end" {
                            "Task"
                        } else {
                            "Message"
                        }
                        .into(),
                        fingerprint: text.map(fingerprint),
                    });
                }
                _ => {}
            }
        }
        Ok(records)
    }
}

fn timestamp_ms(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp_millis())
}

fn envelope(text: &str) -> (&'static str, Option<&str>) {
    let Some((header, body)) = text.split_once("\nPayload:\n") else {
        return ("Message", None);
    };
    if !header.contains("\nTask name: ") || !header.contains("\nSender: ") {
        return ("Message", None);
    }
    let kind = match header.lines().next() {
        Some("Message Type: NEW_TASK") => "Task",
        Some("Message Type: FINAL_ANSWER") => "Final answer",
        Some("Message Type: MESSAGE") => "Message",
        _ => return ("Message", None),
    };
    (kind, Some(body))
}

pub(super) fn fingerprint(text: &str) -> u64 {
    let mut hash = DefaultHasher::new();
    text.hash(&mut hash);
    hash.finish()
}

#[derive(serde::Deserialize)]
pub(super) struct RecordTag<'a> {
    #[serde(rename = "type")]
    pub(super) kind: &'a str,
    #[serde(borrow)]
    payload: PayloadTag<'a>,
}

#[derive(serde::Deserialize)]
struct PayloadTag<'a> {
    #[serde(rename = "type", default)]
    kind: &'a str,
    #[serde(default)]
    name: Option<&'a str>,
    item: Option<ItemTag<'a>>,
}

#[derive(serde::Deserialize)]
struct ItemTag<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

impl RecordTag<'_> {
    fn relevant(&self) -> bool {
        match (self.kind, self.payload.kind) {
            ("session_meta" | "inter_agent_communication", _)
            | ("response_item", "agent_message" | "message")
            | (
                "event_msg",
                "task_started"
                | "turn_started"
                | "task_complete"
                | "turn_complete"
                | "turn_aborted"
                | "collab_agent_spawn_end"
                | "collab_agent_interaction_end",
            ) => true,
            ("response_item", "function_call") => matches!(
                self.payload.name,
                Some("spawn_agent" | "followup_task" | "send_message")
            ),
            ("event_msg", "item_completed") => self
                .payload
                .item
                .as_ref()
                .is_some_and(|item| item.kind == "SubAgentActivity"),
            _ => false,
        }
    }
}
