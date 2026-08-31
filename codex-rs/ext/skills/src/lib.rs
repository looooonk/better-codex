pub mod catalog;
mod config;
mod extension;
mod fragments;
mod host_roots;
mod host_service;
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
pub use host_service::HostSkillsLoadInput;
pub use host_service::HostSkillsService;
pub use host_service::bundled_skills_enabled_from_stack;
pub use provider::ExecutorSkillProvider;
pub use provider::HostSkillProvider;
pub use provider::OrchestratorSkillProvider;
pub use provider::SkillProvider;
pub use sources::SkillProviderSource;
pub use sources::SkillProviders;
