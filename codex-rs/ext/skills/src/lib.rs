pub mod catalog;
mod config;
mod extension;
mod fragments;
// Host loading is staged before its crate-internal runtime caller in this PR stack.
#[allow(dead_code)]
mod loader;
pub mod provider;
mod render;
mod selection;
mod sources;
mod state;
mod tools;
mod world_state;

pub use config::SkillsExtensionConfig;
pub use extension::install;
pub use extension::install_with_providers;
pub use provider::ExecutorSkillProvider;
pub use provider::HostSkillProvider;
pub use provider::OrchestratorSkillProvider;
pub use provider::SkillProvider;
pub use sources::SkillProviderSource;
pub use sources::SkillProviders;
