//! Model-history and persisted-rollout domain types.

mod envelope;
mod initial;
mod persisted;

pub use envelope::CodexHarnessMetadata;
pub use envelope::ResponseItemEnvelope;
pub use initial::InitialHistory;
pub use initial::ResumedHistory;
pub use persisted::CompactedItem;
pub use persisted::RolloutItem;
pub use persisted::RolloutLine;

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod envelope_tests;
#[cfg(test)]
#[path = "initial_tests.rs"]
mod initial_tests;
#[cfg(test)]
#[path = "persisted_tests.rs"]
mod persisted_tests;
