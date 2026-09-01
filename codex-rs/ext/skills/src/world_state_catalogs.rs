use std::sync::Arc;

use codex_exec_server::ResolvedSelectedCapabilityRoot;
use codex_extension_api::ContextualUserFragment;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::WorldStateContributionInput;
use codex_extension_api::WorldStateSectionContribution;
use codex_mcp::McpResourceClient;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;

use crate::HostSkillsSnapshot;
use crate::SkillsExtensionConfig;
use crate::catalog::SkillCatalog;
use crate::provider::SkillListQuery;
use crate::render::AvailableSkillsRender;
use crate::render::RenderedSkillCatalogs;
use crate::render::SkillMetadataBudget;
use crate::render::SkillRenderReport;
use crate::render::render_combined_available_skills;
use crate::render::skill_metadata_budget;
use crate::render_observability::CatalogSurface;
use crate::render_observability::record_catalog_render;
use crate::sources::SkillProviders;
use crate::state::EmittedCatalogBudgetWarnings;
use crate::state::ExecutorSkillsStepState;
use crate::state::HostSkillsCatalogInWorldState;
use crate::state::HostSkillsStepState;
use crate::state::SkillsThreadState;
use crate::world_state::CatalogRenderCallback;
use crate::world_state::executor_skills_world_state_section;
use crate::world_state::host_skills_world_state_section;
use crate::world_state::orchestrator_skills_world_state_section;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum CatalogStatus {
    Unavailable,
    Disabled,
    Enabled,
}

struct CatalogContribution {
    catalog: SkillCatalog,
    status: CatalogStatus,
}

impl CatalogContribution {
    fn unavailable() -> Self {
        Self {
            catalog: SkillCatalog::default(),
            status: CatalogStatus::Unavailable,
        }
    }
}

pub(crate) struct CatalogContributions {
    executor: CatalogContribution,
    orchestrator: CatalogContribution,
    host: CatalogContribution,
}

pub(crate) struct RenderedCatalogContribution {
    kind: CatalogKind,
    pub(crate) status: CatalogStatus,
    rendered: Option<AvailableSkillsRender>,
}

#[derive(Clone, Copy)]
enum CatalogKind {
    Executor,
    Orchestrator,
    Host,
}

impl CatalogKind {
    fn metrics_surface(self) -> CatalogSurface {
        match self {
            Self::Executor => CatalogSurface::ExecutorWorldState,
            Self::Orchestrator => CatalogSurface::OrchestratorWorldState,
            Self::Host => CatalogSurface::HostWorldState,
        }
    }
}

pub(crate) struct CatalogContext<'a> {
    providers: &'a SkillProviders,
    event_sink: Arc<dyn ExtensionEventSink>,
    input: WorldStateContributionInput<'a>,
    thread_state: Arc<SkillsThreadState>,
    config: SkillsExtensionConfig,
    metadata_budget: SkillMetadataBudget,
    include_usage: bool,
    emitted_warnings: Arc<EmittedCatalogBudgetWarnings>,
}

impl<'a> CatalogContext<'a> {
    pub(crate) fn new(
        providers: &'a SkillProviders,
        event_sink: Arc<dyn ExtensionEventSink>,
        input: WorldStateContributionInput<'a>,
    ) -> Option<Self> {
        let thread_state = input.thread_store.get::<SkillsThreadState>()?;
        let config = thread_state.config();
        let model_info = input.thread_store.get::<ModelInfo>();
        let include_usage = model_info
            .as_deref()
            .is_some_and(|model_info| model_info.include_skills_usage_instructions);
        let metadata_budget = skill_metadata_budget(
            model_info
                .as_deref()
                .and_then(ModelInfo::resolved_context_window),
        );
        let emitted_warnings = input
            .turn_store
            .get_or_init(EmittedCatalogBudgetWarnings::default);

        Some(Self {
            providers,
            event_sink,
            input,
            thread_state,
            config,
            metadata_budget,
            include_usage,
            emitted_warnings,
        })
    }

    pub(crate) async fn discover_catalogs(&self) -> CatalogContributions {
        let orchestrator_enabled = self.thread_state.orchestrator_skills_enabled()
            && self.providers.has_orchestrator_provider();
        let query = SkillListQuery {
            turn_id: self.input.turn_id.to_string(),
            executor_roots: self.input.ready_selected_capability_roots.to_vec(),
            resolved_executor_roots: self
                .input
                .turn_store
                .get::<Vec<ResolvedSelectedCapabilityRoot>>()
                .map(|roots| roots.as_ref().clone())
                .unwrap_or_default(),
            host_snapshot: None,
            include_host_skills: false,
            include_bundled_skills: self.config.bundled_skills_enabled,
            include_orchestrator_skills: orchestrator_enabled,
            mcp_resources: self.input.session_store.get::<McpResourceClient>(),
            executor_capability_discovery: self.input.executor_capability_discovery.cloned(),
        };

        let (executor, orchestrator, host) = futures::join!(
            self.discover_executor_catalog(query.clone()),
            self.discover_orchestrator_catalog(query),
            self.discover_host_catalog(),
        );
        for catalog in [&executor.catalog, &orchestrator.catalog, &host.catalog] {
            self.emit_catalog_warnings(catalog);
        }

        CatalogContributions {
            executor,
            orchestrator,
            host,
        }
    }

    async fn discover_executor_catalog(&self, query: SkillListQuery) -> CatalogContribution {
        let catalog = self
            .thread_state
            .executor_catalog_snapshot(self.providers, query)
            .await;
        self.input
            .turn_store
            .insert(ExecutorSkillsStepState(catalog.clone()));
        CatalogContribution {
            catalog,
            status: CatalogStatus::Enabled,
        }
    }

    async fn discover_orchestrator_catalog(&self, query: SkillListQuery) -> CatalogContribution {
        if !self.providers.has_orchestrator_provider() {
            return CatalogContribution::unavailable();
        }
        if !query.include_orchestrator_skills {
            return CatalogContribution {
                catalog: SkillCatalog::default(),
                status: CatalogStatus::Disabled,
            };
        }

        let mcp_resources = query.mcp_resources.clone();
        let catalog = self
            .thread_state
            .orchestrator_catalog_snapshot(
                mcp_resources.as_deref(),
                self.providers.list_orchestrator_for_turn(query),
            )
            .await;
        CatalogContribution {
            catalog,
            status: CatalogStatus::Enabled,
        }
    }

    async fn discover_host_catalog(&self) -> CatalogContribution {
        let Some(host_snapshot) = self
            .input
            .turn_store
            .get::<HostSkillsSnapshot>()
            .filter(|_| self.providers.has_host_provider())
        else {
            return CatalogContribution::unavailable();
        };

        let catalog = self
            .providers
            .list_host_for_turn(SkillListQuery {
                turn_id: self.input.turn_id.to_string(),
                executor_roots: Vec::new(),
                resolved_executor_roots: Vec::new(),
                host_snapshot: Some(host_snapshot),
                include_host_skills: true,
                include_bundled_skills: false,
                include_orchestrator_skills: false,
                mcp_resources: None,
                executor_capability_discovery: None,
            })
            .await;
        self.input
            .turn_store
            .insert(HostSkillsStepState(catalog.clone()));
        CatalogContribution {
            catalog,
            status: CatalogStatus::Enabled,
        }
    }

    pub(crate) fn render_catalogs(
        &self,
        catalogs: CatalogContributions,
    ) -> [RenderedCatalogContribution; 3] {
        let rendered = if self.config.include_instructions {
            render_combined_available_skills(
                &catalogs.executor.catalog,
                &catalogs.orchestrator.catalog,
                &catalogs.host.catalog,
                self.metadata_budget,
                self.include_usage,
            )
        } else {
            RenderedSkillCatalogs::default()
        };

        [
            (CatalogKind::Executor, catalogs.executor, rendered.executor),
            (
                CatalogKind::Orchestrator,
                catalogs.orchestrator,
                rendered.orchestrator,
            ),
            (CatalogKind::Host, catalogs.host, rendered.host),
        ]
        .map(|(kind, catalog, rendered)| RenderedCatalogContribution {
            kind,
            status: catalog.status,
            rendered,
        })
    }

    pub(crate) fn build_world_state_section(
        &self,
        catalog: RenderedCatalogContribution,
    ) -> WorldStateSectionContribution {
        let report = catalog
            .rendered
            .as_ref()
            .map(|rendered| rendered.report.clone())
            .unwrap_or_default();
        let body = catalog
            .rendered
            .and_then(AvailableSkillsRender::into_fragment)
            .map(|fragment| fragment.body());
        let include_instructions = self.config.include_instructions;
        let on_render = self.catalog_render_callback(catalog.kind, catalog.status, report.clone());

        match catalog.kind {
            CatalogKind::Executor => {
                executor_skills_world_state_section(body, include_instructions, on_render)
            }
            CatalogKind::Orchestrator => orchestrator_skills_world_state_section(
                body,
                include_instructions,
                catalog.status == CatalogStatus::Enabled,
                on_render,
            ),
            CatalogKind::Host => {
                self.input.turn_store.insert(HostSkillsCatalogInWorldState);
                host_skills_world_state_section(body, include_instructions, &report, on_render)
            }
        }
    }

    fn catalog_render_callback(
        &self,
        kind: CatalogKind,
        status: CatalogStatus,
        report: SkillRenderReport,
    ) -> CatalogRenderCallback {
        let extension_metrics = self.input.extension_metrics.clone();
        let event_sink = Arc::clone(&self.event_sink);
        let emitted_warnings = Arc::clone(&self.emitted_warnings);
        let turn_id = self.input.turn_id.to_string();
        let budget = self.metadata_budget;
        Box::new(move || {
            if status != CatalogStatus::Enabled {
                return;
            }
            record_catalog_render(
                extension_metrics.as_deref(),
                kind.metrics_surface(),
                budget,
                &report,
            );
            if let Some(message) = report.warning_message()
                && emitted_warnings.insert(&message)
            {
                emit_warning(event_sink.as_ref(), &turn_id, message);
            }
        })
    }

    fn emit_catalog_warnings(&self, catalog: &SkillCatalog) {
        for warning in &catalog.warnings {
            if self.emitted_warnings.insert(warning) {
                emit_warning(self.event_sink.as_ref(), self.input.turn_id, warning.clone());
            }
        }
    }
}

fn emit_warning(event_sink: &dyn ExtensionEventSink, turn_id: &str, message: String) {
    event_sink.emit(Event {
        id: turn_id.to_string(),
        msg: EventMsg::Warning(WarningEvent { message }),
    });
}
