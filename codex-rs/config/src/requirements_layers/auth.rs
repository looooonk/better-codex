use codex_protocol::config_types::ForcedLoginMethod;

use crate::ConfigRequirementsWithSources;
use crate::RequirementSource;
use crate::Sourced;

use super::layer::DomainMergedRequirementsFields;
use super::stack::merge_output_source;

#[derive(Default)]
pub(super) struct AuthRequirementsMergeState {
    allowed_login_methods: Option<Sourced<Vec<ForcedLoginMethod>>>,
    allowed_chatgpt_workspaces: Option<Sourced<Vec<String>>>,
}

impl AuthRequirementsMergeState {
    pub(super) fn merge(
        &mut self,
        incoming: &DomainMergedRequirementsFields,
        source: &RequirementSource,
    ) {
        merge_allowlist(
            &mut self.allowed_login_methods,
            incoming
                .allowed_login_methods
                .as_ref()
                .map(|methods| unique_values(methods.iter().copied())),
            source,
        );
        merge_allowlist(
            &mut self.allowed_chatgpt_workspaces,
            incoming.allowed_chatgpt_workspaces.as_ref().map(|workspaces| {
                unique_values(workspaces.iter().filter_map(|workspace| {
                    let workspace = workspace.trim();
                    (!workspace.is_empty()).then(|| workspace.to_string())
                }))
            }),
            source,
        );
    }

    pub(super) fn apply_to(self, output: &mut ConfigRequirementsWithSources) {
        output.allowed_login_methods = self.allowed_login_methods;
        output.allowed_chatgpt_workspaces = self.allowed_chatgpt_workspaces;
    }
}

fn merge_allowlist<T: PartialEq>(
    existing: &mut Option<Sourced<Vec<T>>>,
    incoming: Option<Vec<T>>,
    source: &RequirementSource,
) {
    let Some(incoming) = incoming else {
        return;
    };
    match existing {
        Some(existing) => {
            existing.value.retain(|value| incoming.contains(value));
            merge_output_source(&mut existing.source, source);
        }
        None => *existing = Some(Sourced::new(incoming, source.clone())),
    }
}

fn unique_values<T: PartialEq>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut unique = Vec::new();
    for value in values {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique
}
