use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::TurnContextItem;
use codex_protocol::protocol::WorldStateItem;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema, TS)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum RolloutItem {
    SessionMeta(SessionMetaLine),
    ResponseItem(ResponseItem),
    /// Legacy delivery item reconstructed as a model-visible `agent_message`.
    InterAgentCommunication(InterAgentCommunication),
    /// Local delivery metadata that is not part of the Responses API item.
    InterAgentCommunicationMetadata { trigger_turn: bool },
    Compacted(CompactedItem),
    TurnContext(TurnContextItem),
    WorldState(WorldStateItem),
    SecurityRiskScore(codex_protocol::security_risk::SecurityRiskScore),
    EventMsg(EventMsg),
}

#[derive(Serialize, Clone, Debug, PartialEq, JsonSchema, TS)]
pub struct CompactedItem {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_history: Option<Vec<ResponseItem>>,
    /// Monotonic position of this context window within the thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_number: Option<u64>,
    /// UUIDv7 identity of the first context window in this thread's window chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_window_id: Option<String>,
    /// UUIDv7 identity of the context window immediately before this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_window_id: Option<String>,
    /// UUIDv7 identity of this context window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
}

impl From<CompactedItem> for ResponseItem {
    fn from(value: CompactedItem) -> Self {
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: value.message,
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
    }
}

// Before window_number was introduced, the numeric window number was serialized as
// window_id. Accept that shape so existing rollouts remain resumable.
impl<'de> Deserialize<'de> for CompactedItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = SerializedCompactedItem::deserialize(deserializer)?;
        let mut window_number = serialized.window_number;
        let window_id = match serialized.window_id {
            Some(SerializedWindowId::Id(window_id)) => Some(window_id),
            Some(SerializedWindowId::LegacyWindowNumber(legacy_window_number)) => {
                window_number.get_or_insert(legacy_window_number);
                None
            }
            None => None,
        };
        Ok(Self {
            message: serialized.message,
            replacement_history: serialized.replacement_history,
            window_number,
            first_window_id: serialized.first_window_id,
            previous_window_id: serialized.previous_window_id,
            window_id,
        })
    }
}

#[derive(Deserialize)]
struct SerializedCompactedItem {
    message: String,
    #[serde(default)]
    replacement_history: Option<Vec<ResponseItem>>,
    #[serde(default)]
    window_number: Option<u64>,
    #[serde(default)]
    first_window_id: Option<String>,
    #[serde(default)]
    previous_window_id: Option<String>,
    #[serde(default)]
    window_id: Option<SerializedWindowId>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SerializedWindowId {
    Id(String),
    LegacyWindowNumber(u64),
}

#[derive(Serialize, Deserialize, Clone, JsonSchema)]
pub struct RolloutLine {
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u64>,
    #[serde(flatten)]
    pub item: RolloutItem,
}
