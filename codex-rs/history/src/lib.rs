//! Model-history and persisted-rollout domain types.

mod initial;

pub use codex_protocol::protocol::CompactedItem;
pub use codex_protocol::protocol::RolloutItem;
pub use codex_protocol::protocol::RolloutLine;
pub use initial::InitialHistory;
pub use initial::ResumedHistory;

#[cfg(test)]
#[path = "initial_tests.rs"]
mod tests;
