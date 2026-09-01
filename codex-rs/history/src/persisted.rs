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
use serde::Serializer;
use serde::de::Error as _;

use crate::ResponseItemEnvelope;
use crate::persisted_wire::CompactedItemWire;
use crate::persisted_wire::RolloutItemWire;

#[derive(Debug, Clone)]
pub enum RolloutItem {
    SessionMeta(SessionMetaLine),
    ResponseItem(ResponseItemEnvelope),
    /// Legacy delivery item reconstructed as a model-visible `agent_message`.
    InterAgentCommunication(InterAgentCommunication),
    /// Local delivery metadata that is not part of the Responses API item.
    InterAgentCommunicationMetadata {
        trigger_turn: bool,
    },
    Compacted(CompactedItem),
    TurnContext(TurnContextItem),
    WorldState(WorldStateItem),
    SecurityRiskScore(codex_protocol::security_risk::SecurityRiskScore),
    EventMsg(EventMsg),
}

impl Serialize for RolloutItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        RolloutItemWire::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RolloutItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RolloutItemWire::deserialize(deserializer).map(Into::into)
    }
}

impl JsonSchema for RolloutItem {
    fn schema_name() -> String {
        "RolloutItem".to_string()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(concat!(module_path!(), "::RolloutItem"))
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::schema::Schema {
        RolloutItemWire::json_schema(generator)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompactedItem {
    pub message: String,
    pub replacement_history: Option<Vec<ResponseItemEnvelope>>,
    /// Monotonic position of this context window within the thread.
    pub window_number: Option<u64>,
    /// UUIDv7 identity of the first context window in this thread's window chain.
    pub first_window_id: Option<String>,
    /// UUIDv7 identity of the context window immediately before this one.
    pub previous_window_id: Option<String>,
    /// UUIDv7 identity of this context window.
    pub window_id: Option<String>,
}

impl Serialize for CompactedItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        CompactedItemWire::from(self).serialize(serializer)
    }
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
        CompactedItemWire::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl JsonSchema for CompactedItem {
    fn schema_name() -> String {
        "CompactedItem".to_string()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed(concat!(module_path!(), "::CompactedItem"))
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::schema::Schema {
        CompactedItemWire::json_schema(generator)
    }
}

#[derive(Serialize, Deserialize, Clone, JsonSchema)]
pub struct RolloutLine {
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u64>,
    #[serde(flatten)]
    pub item: RolloutItem,
}
