mod aliases;
pub mod catalog;
mod catalog_prompt;
mod config;
mod extension;
mod fragments;
mod host_prompt;
mod host_aliases;
mod host_outcome;
pub mod host_render;
mod host_roots;
mod host_service;
mod host_snapshot;
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
pub use host_outcome::SkillLoadOutcome;
pub use host_outcome::filter_skill_load_outcome_for_product;
pub use host_render::AvailableSkills;
pub use host_render::SkillMetadataBudget;
pub use host_render::SkillRenderReport;
pub use host_render::SkillRenderSideEffects;
pub use host_render::build_available_skills;
pub use host_render::default_skill_metadata_budget;
pub use host_render::render_available_skills_body;
pub use host_render::SKILLS_INTRO_WITH_ABSOLUTE_PATHS;
pub use host_service::HostSkillsLoadInput;
pub use host_service::HostSkillsService;
pub use host_snapshot::HostSkillsSnapshot;
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
