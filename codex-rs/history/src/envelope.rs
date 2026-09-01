use std::borrow::Borrow;
use std::ops::Deref;
use std::ops::DerefMut;

use codex_protocol::models::ResponseItem;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// A model-history item with room for history-only metadata.
///
/// Persistence keeps the response item intact and stores its metadata separately.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseItemEnvelope {
    pub item: ResponseItem,
    pub metadata: Option<CodexHarnessMetadata>,
}

/// Metadata owned by the Codex harness and persisted with a response item.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema)]
pub struct CodexHarnessMetadata {
    /// Whether a developer message was supplied by an app-server client.
    #[serde(default)]
    pub client_authored: bool,
}

impl ResponseItemEnvelope {
    /// Wraps a raw Responses API item for persisted history.
    pub fn new(item: ResponseItem) -> Self {
        Self {
            item,
            metadata: None,
        }
    }

    /// Unwraps the raw Responses API item.
    pub fn into_item(self) -> ResponseItem {
        self.item
    }
}

impl From<ResponseItem> for ResponseItemEnvelope {
    fn from(item: ResponseItem) -> Self {
        Self::new(item)
    }
}

impl Deref for ResponseItemEnvelope {
    type Target = ResponseItem;

    fn deref(&self) -> &Self::Target {
        &self.item
    }
}

impl DerefMut for ResponseItemEnvelope {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.item
    }
}

impl Borrow<ResponseItem> for ResponseItemEnvelope {
    fn borrow(&self) -> &ResponseItem {
        &self.item
    }
}
