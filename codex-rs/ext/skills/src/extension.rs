use std::collections::HashMap;
use std::sync::Arc;

use codex_exec_server::ExecutorCapabilityDiscoverySnapshot;
use codex_exec_server::FileSystemSandboxContext;
use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_exec_server::ResolvedSelectedCapabilityRoot;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ContextContributor;
use codex_extension_api::ContextualUserFragment;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptFragment;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::TurnInputContext;
use codex_extension_api::TurnInputContributor;
use codex_extension_api::WorldStateContributionInput;
use codex_extension_api::WorldStateSectionContribution;
use codex_mcp::McpResourceClient;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;

use crate::SkillsExtensionConfig;
use crate::ExplicitSkillPromptBudget;
use crate::HostSkillsCatalogInWorldState;
use crate::HostSkillsSnapshot;
use crate::InjectedHostSkillPrompts;
use crate::default_skill_metadata_budget;
use crate::catalog::SkillCatalog;
use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillReadResult;
use crate::catalog::SkillSourceKind;
use crate::provider::HostSkillProvider;
use crate::provider::SkillListQuery;
use crate::provider::SkillReadRequest;
use crate::render::render_extension_catalog;
use crate::fragments::SkillInstructions;
use crate::fragments::SkillResourceAccess;
use crate::selection::collect_explicit_skill_mentions;
use crate::sources::SkillProviders;
use crate::state::ExecutorSkillsStepState;
use crate::state::SkillsThreadState;
use crate::state::SkillsTurnState;
use crate::tools::skill_tools;
use crate::tools::SkillToolAuthority;
use crate::world_state::executor_skills_world_state_section;
use crate::world_state::host_skills_world_state_section;

struct SkillsExtension<C> {
    providers: SkillProviders,
    event_sink: Arc<dyn ExtensionEventSink>,
    config_from_host: Arc<dyn Fn(&C) -> SkillsExtensionConfig + Send + Sync>,
}

impl<C> ThreadLifecycleContributor<C> for SkillsExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_thread_start<'a>(&'a self, input: ThreadStartInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let orchestrator_skills_available = !input
                .environments
                .iter()
                .any(|environment| environment.environment_id == LOCAL_ENVIRONMENT_ID);
            input.thread_store.insert(SkillsThreadState::new(
                (self.config_from_host)(input.config),
                orchestrator_skills_available,
            ));
        })
    }
}

impl<C> ConfigContributor<C> for SkillsExtension<C>
where
    C: Send + Sync + 'static,
{
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &C,
        new_config: &C,
    ) {
        let next_config = (self.config_from_host)(new_config);
        if let Some(state) = thread_store.get::<SkillsThreadState>() {
            state.set_config(next_config);
        } else {
            let orchestrator_skills_available = true;
            thread_store.insert(SkillsThreadState::new(
                next_config,
                orchestrator_skills_available,
            ));
        }
    }
}

impl<C> ContextContributor for SkillsExtension<C>
where
    C: Send + Sync + 'static,
{
    fn contribute_thread_context<'a>(
        &'a self,
        session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<PromptFragment>> + Send + 'a>> {
        Box::pin(async move {
            let Some(thread_state) = thread_store.get::<SkillsThreadState>() else {
                return Vec::new();
            };
            let config = thread_state.config();
            if !config.include_instructions {
                return Vec::new();
            }
            let catalog = self
                .list_skills(
                    SkillListQuery {
                        turn_id: thread_store.level_id().to_string(),
                        executor_roots: Vec::new(),
                        resolved_executor_roots: Vec::new(),
                        host_snapshot: None,
                        include_host_skills: false,
                        include_bundled_skills: config.bundled_skills_enabled,
                        include_orchestrator_skills: thread_state.orchestrator_skills_enabled(),
                        mcp_resources: session_store.get::<McpResourceClient>(),
                        executor_capability_discovery: None,
                    },
                    &thread_state,
                )
                .await;
            for warning in &catalog.warnings {
                self.emit_warning(thread_store.level_id(), warning.clone());
            }
            let model_info = thread_store.get::<ModelInfo>();
            let include_usage = model_info
                .as_deref()
                .is_some_and(|model_info| model_info.include_skills_usage_instructions);
            let (fragment, report) = render_extension_catalog(
                &catalog,
                include_usage,
                model_info.as_deref().and_then(ModelInfo::resolved_context_window),
            );
            if let Some(warning) = report.warning_message() {
                self.emit_warning(thread_store.level_id(), warning);
            }
            fragment
                .map(PromptFragment::developer_capability)
                .into_iter()
                .collect()
        })
    }

    fn contribute_world_state<'a>(
        &'a self,
        input: WorldStateContributionInput<'a>,
    ) -> ExtensionFuture<'a, Vec<WorldStateSectionContribution>> {
        Box::pin(async move {
            let Some(thread_state) = input.thread_store.get::<SkillsThreadState>() else {
                return Vec::new();
            };
            let config = thread_state.config();
            let catalog = thread_state
                .executor_catalog_snapshot(
                    &self.providers,
                    SkillListQuery {
                        turn_id: input.turn_id.to_string(),
                        executor_roots: input.ready_selected_capability_roots.to_vec(),
                        resolved_executor_roots: input
                            .turn_store
                            .get::<Vec<ResolvedSelectedCapabilityRoot>>()
                            .map(|roots| roots.as_ref().clone())
                            .unwrap_or_default(),
                        host_snapshot: None,
                        include_host_skills: false,
                        include_bundled_skills: config.bundled_skills_enabled,
                        include_orchestrator_skills: false,
                        mcp_resources: input.session_store.get::<McpResourceClient>(),
                        executor_capability_discovery: input.executor_capability_discovery.cloned(),
                    },
                )
                .await;
            input
                .turn_store
                .insert(ExecutorSkillsStepState(catalog.clone()));
            let model_info = input.thread_store.get::<ModelInfo>();
            let include_usage = model_info
                .as_deref()
                .is_some_and(|model_info| model_info.include_skills_usage_instructions);
            let (executor_section, executor_report) = executor_skills_world_state_section(
                &catalog,
                config.include_instructions,
                include_usage,
                model_info.as_deref().and_then(ModelInfo::resolved_context_window),
            );
            if let Some(warning) = executor_report.warning_message() {
                self.emit_warning(input.turn_id, warning);
            }
            let mut sections = vec![executor_section];
            if let Some(host_snapshot) = input.turn_store.get::<HostSkillsSnapshot>()
                && self.providers.has_host_provider()
            {
                input.turn_store.insert(HostSkillsCatalogInWorldState);
                sections.push(host_skills_world_state_section(
                    &host_snapshot,
                    config.include_instructions,
                    include_usage,
                    default_skill_metadata_budget(
                        model_info
                            .as_deref()
                            .and_then(|model_info| model_info.context_window),
                    ),
                ));
            }
            sections
        })
    }
}

impl<C> ToolContributor for SkillsExtension<C>
where
    C: Send + Sync + 'static,
{
    fn tools(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
        self.build_skill_tools(
            session_store,
            thread_store,
            /*executor_query*/ None,
            /*sandbox_contexts*/ None,
        )
    }

    fn tools_for_step(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
        step_store: &ExtensionData,
    ) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
        let resolved_executor_roots = step_store
            .get::<Vec<ResolvedSelectedCapabilityRoot>>()
            .map(|roots| roots.as_slice().to_vec())
            .unwrap_or_default();
        let executor_query = (!resolved_executor_roots.is_empty()).then(|| SkillListQuery {
            turn_id: step_store.level_id().to_string(),
            executor_roots: resolved_executor_roots
                .iter()
                .map(|root| root.selected_root().clone())
                .collect(),
            resolved_executor_roots,
            host_snapshot: None,
            include_host_skills: false,
            include_bundled_skills: false,
            include_orchestrator_skills: false,
            mcp_resources: None,
            executor_capability_discovery: step_store
                .get::<ExecutorCapabilityDiscoverySnapshot>()
                .map(|discovery| discovery.as_ref().clone()),
        });
        self.build_skill_tools(
            session_store,
            thread_store,
            executor_query,
            step_store.get::<HashMap<String, FileSystemSandboxContext>>(),
        )
    }
}

impl<C> SkillsExtension<C> {
    fn build_skill_tools(
        &self,
        session_store: &ExtensionData,
        thread_store: &ExtensionData,
        executor_query: Option<SkillListQuery>,
        sandbox_contexts: Option<Arc<HashMap<String, FileSystemSandboxContext>>>,
    ) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
        let Some(thread_state) = thread_store.get::<SkillsThreadState>() else {
            return Vec::new();
        };
        let orchestrator_available = self.providers.has_orchestrator_provider()
            && thread_state.orchestrator_skills_enabled();
        if !orchestrator_available && executor_query.is_none() {
            return Vec::new();
        }

        skill_tools(
            self.providers.clone(),
            session_store.get::<McpResourceClient>(),
            thread_state,
            orchestrator_available,
            executor_query,
            sandbox_contexts,
        )
    }
}

impl<C> TurnInputContributor for SkillsExtension<C>
where
    C: Send + Sync + 'static,
{
    fn contribute<'a>(
        &'a self,
        input: TurnInputContext,
        session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
        turn_store: &'a ExtensionData,
    ) -> ExtensionFuture<'a, Vec<Box<dyn ContextualUserFragment + Send>>> {
        Box::pin(async move {
            let Some(thread_state) = thread_store.get::<SkillsThreadState>() else {
                return Vec::new();
            };

            let config = thread_state.config();
            let host_snapshot = turn_store.get::<HostSkillsSnapshot>();
            let host_catalog_in_world_state =
                turn_store.get::<HostSkillsCatalogInWorldState>().is_some();
            let query = SkillListQuery {
                turn_id: input.turn_id.clone(),
                executor_roots: Vec::new(),
                resolved_executor_roots: Vec::new(),
                host_snapshot: host_snapshot.clone(),
                include_host_skills: !host_catalog_in_world_state,
                include_bundled_skills: config.bundled_skills_enabled,
                include_orchestrator_skills: thread_state.orchestrator_skills_enabled(),
                mcp_resources: session_store.get::<McpResourceClient>(),
                executor_capability_discovery: None,
            };
            let mut catalog = turn_store
                .get::<ExecutorSkillsStepState>()
                .map(|executor_skills| executor_skills.0.clone())
                .unwrap_or_default();
            catalog.extend(self.list_skills(query, &thread_state).await);
            for warning in &catalog.warnings {
                self.emit_warning(&input.turn_id, warning.clone());
            }

            let mut fragments: Vec<Box<dyn ContextualUserFragment + Send>> = Vec::new();
            if config.include_instructions
                && turn_store.get::<HostSkillsCatalogInWorldState>().is_none()
            {
                let mut turn_catalog = catalog.clone();
                turn_catalog.entries.retain(|entry| {
                    entry.authority.kind != SkillSourceKind::Executor
                        && entry.authority.kind != SkillSourceKind::Orchestrator
                });
                let include_usage = thread_store
                    .get::<ModelInfo>()
                    .is_some_and(|model_info| model_info.include_skills_usage_instructions);
                let model_info = thread_store.get::<ModelInfo>();
                let (fragment, report) = render_extension_catalog(
                    &turn_catalog,
                    include_usage,
                    model_info.as_deref().and_then(ModelInfo::resolved_context_window),
                );
                if let Some(warning) = report.warning_message() {
                    self.emit_warning(&input.turn_id, warning.clone());
                    catalog.warnings.push(warning);
                }
                if let Some(fragment) = fragment {
                    fragments.push(Box::new(fragment));
                }
            }

            let selected_entries = collect_explicit_skill_mentions(&input.user_input, &catalog);
            let mut warnings = catalog.warnings.clone();
            let mut main_prompts_injected = false;
            let mut injected_host_skill_prompts = InjectedHostSkillPrompts::default();
            let mut skill_prompt_budget = ExplicitSkillPromptBudget::default();
            for entry in &selected_entries {
                match self
                    .read_main_prompt(
                        entry,
                        host_snapshot.clone(),
                        session_store,
                        turn_store,
                        &thread_state,
                    )
                    .await
                {
                    Ok(read_result) => {
                        let resource_access = (!entry.prompt_visible)
                            .then(|| {
                                SkillToolAuthority::from_authority(&entry.authority).map(
                                    |authority| SkillResourceAccess {
                                        authority,
                                        package: entry.id.0.clone(),
                                        main_resource: entry.main_prompt.as_str().to_string(),
                                    },
                                )
                            })
                            .flatten();
                        let Some((fragment, truncated)) = SkillInstructions::bounded(
                            &entry.name,
                            entry.rendered_path(),
                            &read_result.contents,
                            resource_access,
                        ) else {
                            let warning = format!(
                                "Skill `{}` was omitted because its instructions could not fit within the main prompt context limit.",
                                entry.name
                            );
                            self.emit_warning(&input.turn_id, warning.clone());
                            warnings.push(warning);
                            continue;
                        };
                        if truncated {
                            let warning = format!(
                                "Skill `{}` exceeded the main prompt context limit and was truncated.",
                                entry.name
                            );
                            self.emit_warning(&input.turn_id, warning.clone());
                            warnings.push(warning);
                        }
                        if !skill_prompt_budget.try_reserve(fragment.rendered_bytes()) {
                            let warning = format!(
                                "Skill `{}` was omitted because explicit skill prompts exceeded the turn context limit.",
                                entry.name
                            );
                            self.emit_warning(&input.turn_id, warning.clone());
                            warnings.push(warning);
                            continue;
                        }
                        fragments.push(Box::new(fragment));
                        main_prompts_injected = true;
                        if entry.authority.kind == SkillSourceKind::Host {
                            injected_host_skill_prompts.insert_path(entry.main_prompt.as_str());
                        }
                    }
                    Err(message) => {
                        let warning = format!("Failed to load skill `{}`: {message}", entry.name);
                        self.emit_warning(&input.turn_id, warning.clone());
                        warnings.push(warning);
                    }
                }
            }

            if let Some(host_snapshot) = &host_snapshot {
                for entry in selected_entries
                    .iter()
                    .filter(|entry| entry.authority.kind != SkillSourceKind::Host)
                {
                    for host_skill in host_snapshot
                        .outcome()
                        .skills
                        .iter()
                        .filter(|host_skill| host_skill.name == entry.name)
                    {
                        injected_host_skill_prompts
                            .insert_path(host_skill.path_to_skills_md.to_string_lossy());
                    }
                }
            }

            turn_store.insert(SkillsTurnState {
                catalog,
                selected_entries,
                warnings,
                main_prompts_injected,
            });
            turn_store.insert(skill_prompt_budget);
            if !injected_host_skill_prompts.is_empty() {
                turn_store.insert(injected_host_skill_prompts);
            }

            fragments
        })
    }
}

impl<C> SkillsExtension<C> {
    #[tracing::instrument(level = "trace", skip_all)]
    async fn list_skills(
        &self,
        mut query: SkillListQuery,
        thread_state: &SkillsThreadState,
    ) -> SkillCatalog {
        let include_orchestrator_skills = query.include_orchestrator_skills;
        let orchestrator_query = query.clone();
        let mcp_resources = orchestrator_query.mcp_resources.clone();
        query.include_orchestrator_skills = false;

        let mut catalog = self.providers.list_for_turn(query).await;
        if include_orchestrator_skills {
            let orchestrator_catalog = thread_state
                .orchestrator_catalog_snapshot(
                    mcp_resources.as_deref(),
                    self.providers
                        .list_orchestrator_for_turn(orchestrator_query),
                )
                .await;
            catalog.extend(orchestrator_catalog);
        }
        catalog
    }

    #[tracing::instrument(level = "trace", skip_all, fields(skill = %entry.name))]
    async fn read_main_prompt(
        &self,
        entry: &SkillCatalogEntry,
        host_snapshot: Option<Arc<HostSkillsSnapshot>>,
        session_store: &ExtensionData,
        turn_store: &ExtensionData,
        thread_state: &SkillsThreadState,
    ) -> Result<SkillReadResult, String> {
        let resolved_executor_roots = turn_store
            .get::<Vec<ResolvedSelectedCapabilityRoot>>()
            .map(|roots| roots.as_ref().clone())
            .unwrap_or_default();
        let sandbox_contexts = turn_store.get::<HashMap<String, FileSystemSandboxContext>>();
        let sandbox = entry
            .main_prompt
            .environment_path()
            .and_then(|(environment_id, _)| {
                sandbox_contexts
                    .as_ref()
                    .and_then(|contexts| contexts.get(environment_id).cloned())
            });
        if sandbox_contexts.is_some()
            && entry.main_prompt.environment_path().is_some()
            && sandbox.is_none()
        {
            return Err("failed to resolve the selected environment sandbox".to_string());
        }
        if entry.authority.kind == SkillSourceKind::Executor
            && entry.main_prompt.environment_path().is_some()
            && entry.main_prompt.environment_contents().is_none()
            && !resolved_executor_roots
                .iter()
                .any(|root| root.selected_root().id == entry.authority.id)
        {
            return Err("selected executor skill is no longer available".to_string());
        }
        thread_state
            .read_skill(
                &self.providers,
                SkillReadRequest {
                    authority: entry.authority.clone(),
                    package: entry.id.clone(),
                    resource: entry.main_prompt.clone(),
                    resolved_executor_roots,
                    sandbox,
                    host_snapshot,
                    mcp_resources: session_store.get::<McpResourceClient>(),
                },
            )
            .await
            .map_err(|err| err.message)
    }

    fn emit_warning(&self, turn_id: &str, message: String) {
        self.event_sink.emit(Event {
            id: turn_id.to_string(),
            msg: EventMsg::Warning(WarningEvent { message }),
        });
    }
}

pub fn install<C>(
    registry: &mut ExtensionRegistryBuilder<C>,
    config_from_host: impl Fn(&C) -> SkillsExtensionConfig + Send + Sync + 'static,
) where
    C: Send + Sync + 'static,
{
    install_with_providers(
        registry,
        SkillProviders::new().with_host_provider(Arc::new(HostSkillProvider::new())),
        config_from_host,
    );
}

pub fn install_with_providers<C>(
    registry: &mut ExtensionRegistryBuilder<C>,
    providers: SkillProviders,
    config_from_host: impl Fn(&C) -> SkillsExtensionConfig + Send + Sync + 'static,
) where
    C: Send + Sync + 'static,
{
    let extension = Arc::new(SkillsExtension {
        providers,
        event_sink: registry.event_sink(),
        config_from_host: Arc::new(config_from_host),
    });
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.prompt_contributor(extension.clone());
    registry.turn_input_contributor(extension.clone());
    registry.tool_contributor(extension);
}
