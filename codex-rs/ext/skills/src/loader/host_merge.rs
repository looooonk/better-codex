use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use codex_exec_server::ExecutorFileSystem;
use codex_protocol::protocol::Product;
use codex_protocol::protocol::SkillScope;
use codex_skills::LoadedSkillRoot;
use codex_skills::SkillRootSnapshots;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_plugins::PluginSkillRoot;
use futures::StreamExt;
use tokio::sync::Semaphore;

use crate::SkillLoadOutcome;

use super::HostSkillRoot;
use super::MAX_CONCURRENT_ROOT_SCANS;
use super::host::HostSkillRootSnapshot;
use super::host::load_host_skill_root;

pub(crate) async fn load_and_merge_host_skill_roots(
    roots: Vec<HostSkillRoot>,
    root_scan_slots: &Semaphore,
    restriction_product: Option<Product>,
    plugin_skill_snapshots: Option<&SkillRootSnapshots<PluginSkillRoot>>,
) -> SkillLoadOutcome {
    let mut indexed_snapshots = futures::stream::iter(roots.into_iter().enumerate())
        .map(|(root_index, root)| async move {
            let plugin_skill_root = root.plugin_skill_root();
            let file_system = Arc::clone(&root.file_system);
            let _root_scan_slot = root_scan_slots
                .acquire()
                .await
                .unwrap_or_else(|_| unreachable!());
            let cached = plugin_skill_root
                .as_ref()
                .and_then(|root| plugin_skill_snapshots?.get(root));
            let snapshot = match cached {
                Some(snapshot) => HostSkillRootSnapshot {
                    root: snapshot.root,
                    skills: snapshot.skills,
                    skill_discovery_path_by_path: snapshot.skill_discovery_path_by_path,
                    errors: snapshot.errors,
                    file_system,
                    is_agent_plugin: snapshot.is_agent_plugin,
                },
                None => {
                    let snapshot = load_host_skill_root(root).await;
                    if let Some(snapshots) = plugin_skill_snapshots
                        && let Some(plugin_skill_root) = plugin_skill_root
                    {
                        snapshots.insert(
                            plugin_skill_root,
                            LoadedSkillRoot {
                                root: snapshot.root.clone(),
                                skills: snapshot.skills.clone(),
                                skill_discovery_path_by_path: Arc::clone(
                                    &snapshot.skill_discovery_path_by_path,
                                ),
                                errors: snapshot.errors.clone(),
                                is_agent_plugin: snapshot.is_agent_plugin,
                            },
                        );
                    }
                    snapshot
                }
            };
            (root_index, snapshot)
        })
        .buffer_unordered(MAX_CONCURRENT_ROOT_SCANS)
        .collect::<Vec<_>>()
        .await;
    indexed_snapshots.sort_unstable_by_key(|(root_index, _)| *root_index);

    for (_, snapshot) in &mut indexed_snapshots {
        snapshot
            .skills
            .retain(|skill| skill.matches_product_restriction_for_product(restriction_product));
    }

    merge_host_skill_root_snapshots(
        indexed_snapshots
            .into_iter()
            .map(|(_, snapshot)| snapshot)
            .collect(),
    )
}

fn merge_host_skill_root_snapshots(snapshots: Vec<HostSkillRootSnapshot>) -> SkillLoadOutcome {
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    let mut skill_roots = Vec::new();
    let mut skill_root_by_path = HashMap::new();
    let mut skill_discovery_path_by_path = HashMap::new();
    let mut agent_plugin_skill_paths = HashSet::new();
    let mut file_systems_by_skill_path =
        HashMap::<AbsolutePathBuf, Arc<dyn ExecutorFileSystem>>::new();

    for snapshot in snapshots {
        if !snapshot.skills.is_empty() && !skill_roots.contains(&snapshot.root) {
            skill_roots.push(snapshot.root.clone());
        }
        for skill in &snapshot.skills {
            let path = skill.path_to_skills_md.clone();
            if !skill_root_by_path.contains_key(&path) {
                skill_root_by_path.insert(path.clone(), snapshot.root.clone());
                if let Some(discovery_path) = snapshot.skill_discovery_path_by_path.get(&path) {
                    skill_discovery_path_by_path.insert(path.clone(), discovery_path.clone());
                }
                file_systems_by_skill_path.insert(path.clone(), Arc::clone(&snapshot.file_system));
                if snapshot.is_agent_plugin {
                    agent_plugin_skill_paths.insert(path);
                }
            }
        }
        skills.extend(snapshot.skills);
        errors.extend(snapshot.errors);
    }

    let mut seen_paths = HashSet::new();
    skills.retain(|skill| seen_paths.insert(skill.path_to_skills_md.clone()));
    let retained_paths = skills
        .iter()
        .map(|skill| skill.path_to_skills_md.clone())
        .collect::<HashSet<_>>();
    skill_root_by_path.retain(|path, _| retained_paths.contains(path));
    skill_discovery_path_by_path.retain(|path, _| retained_paths.contains(path));
    agent_plugin_skill_paths.retain(|path| retained_paths.contains(path));
    let retained_roots = skill_root_by_path.values().collect::<HashSet<_>>();
    skill_roots.retain(|root| retained_roots.contains(root));
    file_systems_by_skill_path.retain(|path, _| retained_paths.contains(path));
    skills.sort_by(|left, right| {
        scope_rank(left.scope)
            .cmp(&scope_rank(right.scope))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path_to_skills_md.cmp(&right.path_to_skills_md))
    });

    SkillLoadOutcome::from_parts(
        skills,
        errors,
        skill_roots,
        skill_root_by_path,
        skill_discovery_path_by_path,
        agent_plugin_skill_paths,
        file_systems_by_skill_path,
    )
}

fn scope_rank(scope: SkillScope) -> u8 {
    match scope {
        SkillScope::Repo => 0,
        SkillScope::User => 1,
        SkillScope::System => 2,
        SkillScope::Admin => 3,
    }
}

#[cfg(test)]
#[path = "host_merge_tests.rs"]
mod tests;
