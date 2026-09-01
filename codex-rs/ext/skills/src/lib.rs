pub mod catalog;
mod config;
mod extension;
mod fragments;
mod host_prompt;
mod host_roots;
mod host_service;
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
pub use host_prompt::ExplicitSkillPromptBudget;
pub use host_prompt::HostSkillPrompts;
pub use host_prompt::InjectedHostSkillPrompts;
pub use host_prompt::MAX_EXPLICIT_SKILL_PROMPT_BYTES;
pub use host_prompt::MAX_EXPLICIT_SKILL_PROMPTS_TOTAL_BYTES;
pub use host_service::HostSkillsLoadInput;
pub use host_service::HostSkillsService;
pub use host_service::bundled_skills_enabled_from_stack;
pub use loader::HostSkillRoot;
pub use provider::ExecutorSkillProvider;
pub use provider::HostSkillProvider;
pub use provider::OrchestratorSkillProvider;
pub use provider::SkillProvider;
pub use sources::SkillProviderSource;
pub use sources::SkillProviders;
pub use state::HostSkillsCatalogInWorldState;

/// Recognizes persisted explicit skill prompts without exposing their fragment implementation.
pub fn is_skill_prompt_fragment(text: &str) -> bool {
    <fragments::SkillInstructions as codex_extension_api::ContextualUserFragment>::matches_text(
        text,
    )
}
