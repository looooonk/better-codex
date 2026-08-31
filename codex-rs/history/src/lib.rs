//! Model-history and persisted-rollout domain types.

mod initial;
mod persisted;

pub use initial::InitialHistory;
pub use initial::ResumedHistory;
pub use persisted::CompactedItem;
pub use persisted::RolloutItem;
pub use persisted::RolloutLine;

#[cfg(test)]
#[path = "initial_tests.rs"]
mod initial_tests;
#[cfg(test)]
#[path = "persisted_tests.rs"]
mod persisted_tests;
