use std::borrow::Borrow;
use std::ops::Deref;
use std::ops::DerefMut;

use codex_protocol::models::ResponseItem;

/// A history-owned, in-memory wrapper around a raw response item.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseItemEnvelope {
    pub item: ResponseItem,
}

impl ResponseItemEnvelope {
    pub fn new(item: ResponseItem) -> Self {
        Self { item }
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
