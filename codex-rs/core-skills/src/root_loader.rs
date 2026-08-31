use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use codex_skills::LoadedSkillRoot;
use codex_skills::SkillRootSnapshots;
use codex_utils_plugins::PluginSkillRoot;

use crate::SkillLoadOutcome;
use crate::loader::SkillRoot;
use crate::loader::SkillRootSnapshot;
use crate::loader::load_skill_root;
use crate::model::SkillFileSystemsByPath;

pub(crate) async fn load_and_merge_skill_roots<I>(
    roots: I,
    plugin_skill_snapshots: Option<&SkillRootSnapshots<PluginSkillRoot>>,
) -> SkillLoadOutcome
where
    I: IntoIterator<Item = SkillRoot>,
{
    let mut root_snapshots = Vec::new();
    for root in roots {
        let file_system = Arc::clone(&root.file_system);
        let cache_key = match (
            root.plugin_id.clone(),
            root.plugin_namespace.clone(),
            root.plugin_root.clone(),
        ) {
            (Some(plugin_id), Some(plugin_namespace), Some(plugin_root)) => Some(PluginSkillRoot {
                path: root.path.clone(),
                plugin_id,
                plugin_namespace,
                plugin_root,
                discovery_mode: root.discovery_mode,
            }),
            _ => None,
        };
        let cached_snapshot = cache_key.as_ref().and_then(|cache_key| {
            plugin_skill_snapshots?.get(cache_key)
        });
        let snapshot = match cached_snapshot {
            Some(snapshot) => SkillRootSnapshot {
                root: snapshot.root,
                skills: snapshot.skills,
                skill_discovery_path_by_path: snapshot.skill_discovery_path_by_path,
                errors: snapshot.errors,
                file_system,
                is_agent_plugin: snapshot.is_agent_plugin,
            },
            None => {
                let snapshot = load_skill_root(root).await;
                if let Some(plugin_skill_snapshots) = plugin_skill_snapshots
                    && let Some(cache_key) = cache_key
                {
                    plugin_skill_snapshots.insert(
                        cache_key,
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
        root_snapshots.push(snapshot);
    }

    merge_skill_root_snapshots(root_snapshots)
}

fn merge_skill_root_snapshots(snapshots: Vec<SkillRootSnapshot>) -> SkillLoadOutcome {
    fn scope_rank(scope: codex_protocol::protocol::SkillScope) -> u8 {
        use codex_protocol::protocol::SkillScope;

        // Higher-priority scopes first (matches root scan order for dedupe).
        match scope {
            SkillScope::Repo => 0,
            SkillScope::User => 1,
            SkillScope::System => 2,
            SkillScope::Admin => 3,
        }
    }

    let mut outcome = SkillLoadOutcome::default();
    let mut skill_roots = Vec::new();
    let mut skill_root_by_path = HashMap::new();
    let mut skill_discovery_path_by_path = HashMap::new();
    let mut agent_plugin_skill_paths = HashSet::new();
    let mut file_systems_by_skill_path = HashMap::new();

    for snapshot in snapshots {
        let SkillRootSnapshot {
            root,
            skills,
            skill_discovery_path_by_path: snapshot_discovery_paths,
            errors,
            file_system,
            is_agent_plugin,
        } = snapshot;
        if !skills.is_empty() && !skill_roots.contains(&root) {
            skill_roots.push(root.clone());
        }
        for skill in &skills {
            let first_owner = !skill_root_by_path.contains_key(&skill.path_to_skills_md);
            skill_root_by_path
                .entry(skill.path_to_skills_md.clone())
                .or_insert_with(|| root.clone());
            file_systems_by_skill_path
                .entry(skill.path_to_skills_md.clone())
                .or_insert_with(|| Arc::clone(&file_system));
            if let Some(discovery_path) = snapshot_discovery_paths.get(&skill.path_to_skills_md) {
                skill_discovery_path_by_path
                    .entry(skill.path_to_skills_md.clone())
                    .or_insert_with(|| discovery_path.clone());
            }
            if first_owner && is_agent_plugin {
                agent_plugin_skill_paths.insert(skill.path_to_skills_md.clone());
            }
        }
        outcome.skills.extend(skills);
        outcome.errors.extend(errors);
    }

    let mut seen = HashSet::new();
    outcome
        .skills
        .retain(|skill| seen.insert(skill.path_to_skills_md.clone()));
    let retained_skill_paths = outcome
        .skills
        .iter()
        .map(|skill| skill.path_to_skills_md.clone())
        .collect::<HashSet<_>>();
    skill_root_by_path.retain(|path, _| retained_skill_paths.contains(path));
    skill_discovery_path_by_path.retain(|path, _| retained_skill_paths.contains(path));
    agent_plugin_skill_paths.retain(|path| retained_skill_paths.contains(path));
    let used_roots = skill_root_by_path.values().cloned().collect::<HashSet<_>>();
    skill_roots.retain(|root| used_roots.contains(root));
    file_systems_by_skill_path.retain(|path, _| retained_skill_paths.contains(path));
    outcome.skill_roots = skill_roots;
    outcome.skill_root_by_path = Arc::new(skill_root_by_path);
    outcome.skill_discovery_path_by_path = Arc::new(skill_discovery_path_by_path);
    outcome.agent_plugin_skill_paths = agent_plugin_skill_paths;
    outcome.file_systems_by_skill_path = SkillFileSystemsByPath::new(file_systems_by_skill_path);

    outcome.skills.sort_by(|a, b| {
        scope_rank(a.scope)
            .cmp(&scope_rank(b.scope))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path_to_skills_md.cmp(&b.path_to_skills_md))
    });

    outcome
}
