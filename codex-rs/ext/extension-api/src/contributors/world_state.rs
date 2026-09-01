use std::sync::Arc;

use codex_context_fragments::ContextualUserFragment;
use codex_exec_server_protocol::ExecutorCapabilityDiscoverySnapshot;
use codex_protocol::ThreadId;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_protocol::protocol::TurnEnvironmentSelection;
use serde_json::Value;

use crate::ExtensionData;
use crate::ExtensionMetrics;

/// Host state available while an extension contributes one sampling step's World State.
pub struct WorldStateContributionInput<'a> {
    pub thread_id: ThreadId,
    pub turn_id: &'a str,
    pub environments: &'a [TurnEnvironmentSelection],
    /// Selected roots whose stable environments are ready in this sampling step.
    pub ready_selected_capability_roots: &'a [SelectedCapabilityRoot],
    /// Executor-materialized capability files shared by all consumers in this exact step.
    pub executor_capability_discovery: Option<&'a ExecutorCapabilityDiscoverySnapshot>,
    /// Metrics bound to the effective model for this turn.
    pub extension_metrics: Option<Arc<dyn ExtensionMetrics>>,
    pub session_store: &'a ExtensionData,
    pub thread_store: &'a ExtensionData,
    pub turn_store: &'a ExtensionData,
}

/// What the harness knows about the previous value of one extension-owned section.
pub enum PreviousWorldStateSection<'a> {
    Absent,
    Unknown,
    Known(&'a Value),
}

/// Typed model-visible data rendered by an extension-owned World State section.
pub struct RenderedWorldStateFragment {
    fragment: Box<dyn ContextualUserFragment + Send>,
}

impl RenderedWorldStateFragment {
    pub fn new(fragment: impl ContextualUserFragment + Send + 'static) -> Self {
        Self {
            fragment: Box::new(fragment),
        }
    }

    pub fn into_context_fragment(self) -> Box<dyn ContextualUserFragment + Send> {
        self.fragment
    }

    pub fn body(&self) -> String {
        self.fragment.body()
    }
}

type RenderDiff = dyn for<'a> Fn(PreviousWorldStateSection<'a>) -> Option<RenderedWorldStateFragment>
    + Send
    + Sync;
type LegacyFragmentMatcher = dyn Fn(&str, &str) -> bool + Send + Sync;

/// One extension-owned World State section captured for a sampling step.
///
/// The extension owns the stable ID, comparison snapshot, and diff rendering. The harness owns
/// persistence and the concrete model-context fragment envelope.
#[derive(Clone)]
pub struct WorldStateSectionContribution {
    id: &'static str,
    snapshot: Value,
    render_diff: Arc<RenderDiff>,
    matches_legacy_fragment: Arc<LegacyFragmentMatcher>,
    matches_retained_fragment: Option<Arc<LegacyFragmentMatcher>>,
}

impl WorldStateSectionContribution {
    pub fn new(
        id: &'static str,
        snapshot: Value,
        render_diff: impl for<'a> Fn(
            PreviousWorldStateSection<'a>,
        ) -> Option<RenderedWorldStateFragment>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            id,
            snapshot,
            render_diff: Arc::new(render_diff),
            matches_legacy_fragment: Arc::new(|_, _| false),
            matches_retained_fragment: None,
        }
    }

    pub fn with_legacy_matcher(
        mut self,
        matcher: impl Fn(&str, &str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.matches_legacy_fragment = Arc::new(matcher);
        self
    }

    /// Requires a matching model-visible fragment whenever a persisted snapshot is reused.
    pub fn with_retained_fragment_matcher(
        mut self,
        matcher: impl Fn(&str, &str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.matches_retained_fragment = Some(Arc::new(matcher));
        self
    }

    pub fn id(&self) -> &'static str {
        self.id
    }

    pub fn snapshot(&self) -> &Value {
        &self.snapshot
    }

    pub fn render_diff(
        &self,
        previous: PreviousWorldStateSection<'_>,
    ) -> Option<RenderedWorldStateFragment> {
        (self.render_diff)(previous)
    }

    pub fn matches_legacy_fragment(&self, role: &str, text: &str) -> bool {
        (self.matches_legacy_fragment)(role, text)
    }

    pub fn has_retained_fragment_matcher(&self) -> bool {
        self.matches_retained_fragment.is_some()
    }

    pub fn matches_retained_fragment(&self, role: &str, text: &str) -> bool {
        self.matches_retained_fragment
            .as_ref()
            .is_some_and(|matcher| matcher(role, text))
    }
}
