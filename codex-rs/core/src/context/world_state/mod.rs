mod agents_md;
mod apps_instructions;
mod collaboration_mode;
mod environment;
mod environments_instructions;
mod permissions;
mod plugins_instructions;

use crate::context::ContextualUserFragment;
use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use indexmap::IndexMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Map;
use serde_json::Value;
use sha1::Digest;
use sha1::Sha1;
use std::collections::BTreeMap;
use std::fmt;

pub(crate) use agents_md::AgentsMdState;
pub(crate) use apps_instructions::AppsInstructionsState;
pub(crate) use collaboration_mode::CollaborationModeState;
pub(crate) use environment::EnvironmentsState;
pub(crate) use environments_instructions::EnvironmentsInstructionsState;
pub(crate) use permissions::PermissionsState;
pub(crate) use plugins_instructions::PluginsInstructionsState;

trait ErasedWorldStateSection: Send + Sync {
    fn snapshot(&self) -> Option<Value>;

    fn matches_legacy_fragment(&self, role: &str, text: &str) -> bool;

    fn matches_section_fragment(&self, role: &str, text: &str) -> bool;

    fn has_retained_fragment_matcher(&self) -> bool;

    fn matches_retained_fragment(&self, role: &str, text: &str) -> bool;

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Value>,
    ) -> Option<Box<dyn ContextualUserFragment>>;
}

impl<S: WorldStateSection> ErasedWorldStateSection for S {
    fn snapshot(&self) -> Option<Value> {
        let mut snapshot = match serde_json::to_value(WorldStateSection::snapshot(self)) {
            Ok(snapshot) => snapshot,
            Err(err) => {
                tracing::error!(
                    section_id = S::ID,
                    %err,
                    "failed to serialize world-state section snapshot"
                );
                return None;
            }
        };
        remove_null_object_fields(&mut snapshot);
        if snapshot.is_null() {
            tracing::error!(
                section_id = S::ID,
                "world-state section snapshot cannot be null"
            );
            return None;
        }
        Some(snapshot)
    }

    fn matches_legacy_fragment(&self, role: &str, text: &str) -> bool {
        S::matches_legacy_fragment(role, text)
    }

    fn matches_section_fragment(&self, role: &str, text: &str) -> bool {
        S::matches_section_fragment(role, text)
    }

    fn has_retained_fragment_matcher(&self) -> bool {
        S::has_retained_fragment_matcher()
    }

    fn matches_retained_fragment(&self, role: &str, text: &str) -> bool {
        WorldStateSection::matches_retained_fragment(self, role, text)
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Value>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        let typed_snapshot;
        let previous = match previous {
            PreviousSectionState::Known(previous) => {
                match serde_json::from_value::<S::Snapshot>(previous.clone()) {
                    Ok(previous) => {
                        typed_snapshot = previous;
                        PreviousSectionState::Known(&typed_snapshot)
                    }
                    Err(err) => {
                        tracing::warn!(
                            section_id = S::ID,
                            %err,
                            "failed to restore world-state section snapshot"
                        );
                        PreviousSectionState::Unknown
                    }
                }
            }
            PreviousSectionState::Absent => PreviousSectionState::Absent,
            PreviousSectionState::Unknown => PreviousSectionState::Unknown,
            PreviousSectionState::Stale => PreviousSectionState::Stale,
        };
        WorldStateSection::render_diff(self, previous)
    }
}

struct ExtensionWorldStateSection(WorldStateSectionContribution);

impl ErasedWorldStateSection for ExtensionWorldStateSection {
    fn snapshot(&self) -> Option<Value> {
        let mut snapshot = self.0.snapshot().clone();
        remove_null_object_fields(&mut snapshot);
        (!snapshot.is_null()).then_some(snapshot)
    }

    fn matches_legacy_fragment(&self, role: &str, text: &str) -> bool {
        self.0.matches_legacy_fragment(role, text)
    }

    fn matches_section_fragment(&self, role: &str, text: &str) -> bool {
        self.0.matches_section_fragment(role, text)
    }

    fn has_retained_fragment_matcher(&self) -> bool {
        self.0.has_retained_fragment_matcher()
    }

    fn matches_retained_fragment(&self, role: &str, text: &str) -> bool {
        self.0.matches_retained_fragment(role, text)
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Value>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        let previous = match previous {
            PreviousSectionState::Absent => PreviousWorldStateSection::Absent,
            PreviousSectionState::Unknown => PreviousWorldStateSection::Unknown,
            PreviousSectionState::Stale => PreviousWorldStateSection::Unknown,
            PreviousSectionState::Known(previous) => PreviousWorldStateSection::Known(previous),
        };
        let fragment = self
            .0
            .render_diff(previous)
            .map(RenderedWorldStateFragment::into_context_fragment)?;
        Some(fragment)
    }
}

/// What is known about a section's previously model-visible state.
pub(crate) enum PreviousSectionState<'a, T> {
    /// No persisted snapshot or matching fragment exists in retained history.
    Absent,
    /// Retained history contains the section, but no trustworthy typed snapshot is available.
    Unknown,
    /// The latest retained fragment belongs to this section but is not its current value.
    Stale,
    /// The exact persisted snapshot is available.
    Known(&'a T),
}

/// A typed portion of the state visible to the model.
///
/// Implementations own how their current state is rendered relative to an
/// earlier snapshot of the same section. `ID` is persisted in rollouts and
/// must remain stable. `Snapshot` should contain only the comparison data
/// needed to decide what the model must be told next, and must not serialize
/// to null because merge-patch nulls represent deletion. Sections migrated
/// from older context can recognize their previous fragments through
/// `matches_legacy_fragment`.
pub(crate) trait WorldStateSection: Send + Sync + 'static {
    const ID: &'static str;
    type Snapshot: DeserializeOwned + Serialize;

    fn snapshot(&self) -> Self::Snapshot;

    fn matches_legacy_fragment(_role: &str, _text: &str) -> bool {
        false
    }

    /// Recognizes any model-visible fragment emitted by this section.
    fn matches_section_fragment(role: &str, text: &str) -> bool {
        Self::matches_legacy_fragment(role, text)
    }

    /// Whether retained history must still contain this section's rendered fragment.
    fn has_retained_fragment_matcher() -> bool {
        false
    }

    /// Recognizes this section's rendered fragment in retained model history.
    fn matches_retained_fragment(&self, role: &str, text: &str) -> bool {
        Self::matches_legacy_fragment(role, text)
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>>;
}

/// Stable fingerprint of a fully rendered model-visible World State fragment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct WorldStateHash(String);

impl WorldStateHash {
    pub(crate) fn from_fragment(fragment: &(impl ContextualUserFragment + ?Sized)) -> Self {
        let mut hasher = Sha1::new();
        hasher.update(b"codex-world-state-fragment-v1\0");
        hash_component(&mut hasher, fragment.role());
        hash_component(&mut hasher, &fragment.render());
        Self(format!("{:x}", hasher.finalize()))
    }
}

fn hash_component(hasher: &mut Sha1, value: &str) {
    let value = value.replace("\r\n", "\n");
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

/// Live model-visible state, keyed by the same stable section IDs used in rollouts.
#[derive(Default)]
pub(crate) struct WorldState {
    sections: IndexMap<&'static str, Box<dyn ErasedWorldStateSection>>,
}

/// Compact comparison state for each model-visible world-state section.
#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct WorldStateSnapshot {
    sections: BTreeMap<String, Value>,
}

impl From<&Map<String, Value>> for WorldStateSnapshot {
    fn from(state: &Map<String, Value>) -> Self {
        Self {
            sections: state
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        }
    }
}

impl WorldStateSnapshot {
    pub(crate) fn into_object(self) -> Map<String, Value> {
        self.sections.into_iter().collect()
    }

    /// Returns the RFC 7386 merge patch that advances `previous` to `self`.
    pub(crate) fn merge_patch_from(&self, previous: &Self) -> Option<Map<String, Value>> {
        let mut patch = Map::new();
        for key in previous.sections.keys() {
            if !self.sections.contains_key(key) {
                patch.insert(key.clone(), Value::Null);
            }
        }
        for (key, current) in &self.sections {
            let Some(previous) = previous.sections.get(key) else {
                patch.insert(key.clone(), current.clone());
                continue;
            };
            if let Some(value_patch) = create_merge_patch(previous, current) {
                patch.insert(key.clone(), value_patch);
            }
        }
        (!patch.is_empty()).then_some(patch)
    }

    pub(crate) fn apply_merge_patch(&mut self, patch: &Map<String, Value>) {
        for (key, value) in patch {
            if value.is_null() {
                self.sections.remove(key);
            } else {
                apply_merge_patch_value(
                    self.sections.entry(key.clone()).or_insert(Value::Null),
                    value,
                );
            }
        }
    }
}

impl fmt::Debug for WorldState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorldState")
            .field("section_count", &self.sections.len())
            .finish()
    }
}

impl WorldState {
    pub(crate) fn add_section<S: WorldStateSection>(&mut self, section: S) {
        let id = S::ID;
        assert!(
            !self.sections.contains_key(id),
            "duplicate world-state section ID: {id}"
        );
        self.sections.insert(id, Box::new(section));
    }

    pub(crate) fn add_extension_section(&mut self, section: WorldStateSectionContribution) {
        let id = section.id();
        assert!(
            !self.sections.contains_key(id),
            "duplicate world-state section ID: {id}"
        );
        self.sections
            .insert(id, Box::new(ExtensionWorldStateSection(section)));
    }

    pub(crate) fn snapshot(&self) -> WorldStateSnapshot {
        WorldStateSnapshot {
            sections: self
                .sections
                .iter()
                .filter_map(|(id, section)| {
                    section
                        .snapshot()
                        .map(|snapshot| ((*id).to_string(), snapshot))
                })
                .collect(),
        }
    }

    /// Renders every section as new, without any known previous state.
    pub(crate) fn render_full(&self) -> Vec<Box<dyn ContextualUserFragment>> {
        self.render_with(|_, _| PreviousSectionState::Absent)
    }

    /// Renders each section against the exact persisted snapshot when available.
    pub(crate) fn render_diff(
        &self,
        previous: &WorldStateSnapshot,
    ) -> Vec<Box<dyn ContextualUserFragment>> {
        self.render_with(|id, _| match previous.sections.get(id) {
            Some(previous) => PreviousSectionState::Known(previous),
            None => PreviousSectionState::Absent,
        })
    }

    /// Uses an exact retained fragment as authoritative while requiring sections with retained
    /// matchers to restore themselves whenever that fragment has fallen out of history.
    pub(crate) fn render_history_diff<'a>(
        &self,
        previous: Option<&WorldStateSnapshot>,
        items: impl IntoIterator<Item = &'a ResponseItem> + Clone,
    ) -> Vec<Box<dyn ContextualUserFragment>> {
        self.sections
            .iter()
            .filter_map(|(id, section)| {
                let retention = if section.has_retained_fragment_matcher() {
                    latest_section_fragment_retention(items.clone(), section.as_ref())
                } else {
                    LatestSectionFragmentRetention::None
                };
                if retention == LatestSectionFragmentRetention::Retained {
                    return None;
                }

                let previous = if retention == LatestSectionFragmentRetention::Stale {
                    PreviousSectionState::Stale
                } else if section.has_retained_fragment_matcher()
                    && previous.is_some_and(|previous| previous.sections.contains_key(*id))
                    && !has_legacy_fragment(items.clone(), section.as_ref())
                {
                    PreviousSectionState::Absent
                } else if let Some(previous) =
                    previous.and_then(|previous| previous.sections.get(*id))
                {
                    PreviousSectionState::Known(previous)
                } else if has_legacy_fragment(items.clone(), section.as_ref()) {
                    PreviousSectionState::Unknown
                } else {
                    PreviousSectionState::Absent
                };
                section.render_diff(previous)
            })
            .collect()
    }

    fn render_with<'a>(
        &self,
        mut previous: impl FnMut(&str, &dyn ErasedWorldStateSection) -> PreviousSectionState<'a, Value>,
    ) -> Vec<Box<dyn ContextualUserFragment>> {
        self.sections
            .iter()
            .filter_map(|(id, section)| section.render_diff(previous(id, section.as_ref())))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LatestSectionFragmentRetention {
    None,
    Retained,
    Stale,
}

fn latest_section_fragment_retention<'a>(
    items: impl IntoIterator<Item = &'a ResponseItem>,
    section: &dyn ErasedWorldStateSection,
) -> LatestSectionFragmentRetention {
    let mut latest = LatestSectionFragmentRetention::None;
    for item in items {
        let ResponseItem::Message { role, content, .. } = item else {
            continue;
        };
        for content in content {
            let ContentItem::InputText { text } = content else {
                continue;
            };
            if section.matches_retained_fragment(role, text) {
                latest = LatestSectionFragmentRetention::Retained;
            } else if section.matches_section_fragment(role, text) {
                latest = LatestSectionFragmentRetention::Stale;
            }
        }
    }
    latest
}

fn has_legacy_fragment<'a>(
    items: impl IntoIterator<Item = &'a ResponseItem>,
    section: &dyn ErasedWorldStateSection,
) -> bool {
    items.into_iter().any(|item| {
        matches!(
            item,
            ResponseItem::Message { role, content, .. }
                if content.iter().any(|content| {
                    matches!(
                        content,
                        ContentItem::InputText { text }
                            if section.matches_legacy_fragment(role, text)
                    )
                })
        )
    })
}

fn remove_null_object_fields(value: &mut Value) {
    // RFC 7386 reserves object-valued nulls for deletion, but arrays are replaced whole.
    match value {
        Value::Object(values) => {
            values.retain(|_, value| !value.is_null());
            values.values_mut().for_each(remove_null_object_fields);
        }
        Value::Array(_) => {}
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn create_merge_patch(previous: &Value, current: &Value) -> Option<Value> {
    if previous == current {
        return None;
    }

    let Value::Object(current) = current else {
        return Some(current.clone());
    };
    let previous = previous.as_object();
    let mut patch = Map::new();

    if let Some(previous) = previous {
        for key in previous.keys() {
            if !current.contains_key(key) {
                patch.insert(key.clone(), Value::Null);
            }
        }
    }

    for (key, current_value) in current {
        let Some(previous_value) = previous.and_then(|previous| previous.get(key)) else {
            patch.insert(key.clone(), current_value.clone());
            continue;
        };
        if let Some(value_patch) = create_merge_patch(previous_value, current_value) {
            patch.insert(key.clone(), value_patch);
        }
    }

    Some(Value::Object(patch))
}

fn apply_merge_patch_value(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        target.clone_from(patch);
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    if let Value::Object(target) = target {
        for (key, value) in patch {
            if value.is_null() {
                target.remove(key);
            } else {
                apply_merge_patch_value(target.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
    }
}

#[cfg(test)]
#[path = "world_state_tests.rs"]
mod tests;
