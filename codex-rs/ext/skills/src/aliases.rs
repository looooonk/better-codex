use std::collections::HashSet;

use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillSourceKind;
use crate::host_aliases::shared_host_alias_roots;

struct AliasRoot {
    name: String,
    value: String,
}

pub(crate) struct AliasPlan {
    roots: Vec<AliasRoot>,
}

impl AliasPlan {
    pub(crate) fn build(prefix: &str, candidates: &[&str]) -> Option<Self> {
        let mut roots = Vec::new();
        let mut seen = HashSet::new();

        for &root in candidates {
            if !seen.insert(root) {
                continue;
            }

            let index = roots.len();
            roots.push(AliasRoot {
                name: format!("{prefix}{index}"),
                value: root.to_string(),
            });
        }

        (!roots.is_empty()).then_some(Self { roots })
    }

    pub(crate) fn shorten(&self, locator: &str) -> Option<String> {
        self.roots
            .iter()
            .filter_map(|root| {
                let suffix = locator
                    .strip_prefix(root.value.trim_end_matches('/'))?
                    .strip_prefix('/')?;
                Some((root, suffix))
            })
            .max_by_key(|(root, _)| root.value.len())
            .map(|(root, suffix)| format!("{}/{suffix}", root.name))
    }

    pub(crate) fn root_lines(&self) -> Vec<String> {
        self.roots
            .iter()
            .map(|root| format!("- `{}` = `{}`", root.name, root.value))
            .collect()
    }
}

pub(crate) fn build_catalog_alias_plan(entries: &[&SkillCatalogEntry]) -> Option<AliasPlan> {
    let source = &entries.first()?.authority.kind;
    if entries.iter().any(|entry| &entry.authority.kind != source) {
        return None;
    }

    let prefix = match source {
        SkillSourceKind::Host => "h",
        SkillSourceKind::Executor => "e",
        SkillSourceKind::Orchestrator => "o",
        SkillSourceKind::Custom(_) => return None,
    };
    let mut ordered = entries.to_vec();
    ordered.sort_by(|left, right| {
        left.alias_root_order()
            .unwrap_or(usize::MAX)
            .cmp(&right.alias_root_order().unwrap_or(usize::MAX))
            .then_with(|| left.alias_root().cmp(&right.alias_root()))
            .then_with(|| left.id.0.cmp(&right.id.0))
    });
    let roots = match source {
        SkillSourceKind::Host => shared_host_alias_roots(&ordered),
        SkillSourceKind::Executor | SkillSourceKind::Orchestrator => ordered
            .iter()
            .filter_map(|entry| entry.alias_root())
            .map(str::to_string)
            .collect(),
        SkillSourceKind::Custom(_) => return None,
    };
    let roots = roots.iter().map(String::as_str).collect::<Vec<_>>();
    AliasPlan::build(prefix, &roots)
}

#[cfg(test)]
#[path = "aliases_tests.rs"]
mod tests;
