use std::borrow::Borrow;
use std::ops::Deref;
use std::ops::DerefMut;

use codex_protocol::models::ResponseItem;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// A model-history item with room for history-only metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseItemEnvelope {
    pub item: ResponseItem,
    pub metadata: Option<CodexHarnessMetadata>,
}

/// Metadata owned by the Codex harness and kept separate from provider payloads.
///
/// This intentionally has no fields yet. Keeping it closed prevents history metadata from
/// becoming an untyped extension point.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, JsonSchema)]
pub struct CodexHarnessMetadata {}

impl ResponseItemEnvelope {
    pub fn new(item: ResponseItem) -> Self {
        Self {
            item,
            metadata: None,
        }
    }

    pub fn into_item(self) -> ResponseItem {
        self.item
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
