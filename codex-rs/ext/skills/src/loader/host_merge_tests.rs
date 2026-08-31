use std::collections::HashMap;
use std::sync::Arc;

use codex_exec_server::LOCAL_FS;
use codex_protocol::protocol::SkillScope;
use codex_skills::SkillMetadata;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

use super::HostSkillRootSnapshot;
use super::merge_host_skill_root_snapshots;

fn skill(name: &str, path: &str, scope: SkillScope) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("{name} description"),
        short_description: None,
        interface: None,
        dependencies: None,
        policy: None,
        path_to_skills_md: test_path_buf(path).abs(),
        scope,
        plugin_id: None,
    }
}

#[test]
fn merge_preserves_first_root_authority_and_stable_scope_order() {
    let shared = skill("shared", "/canonical/shared/SKILL.md", SkillScope::User);
    let repo = skill("repo", "/repo/SKILL.md", SkillScope::Repo);
    let first_root = test_path_buf("/first").abs();
    let second_root = test_path_buf("/second").abs();
    let first_discovery = first_root.join("alias/SKILL.md");
    let second_discovery = second_root.join("other/SKILL.md");
    let snapshots = vec![
        HostSkillRootSnapshot {
            root: first_root.clone(),
            skills: vec![shared.clone()],
            skill_discovery_path_by_path: Arc::new(HashMap::from([(
                shared.path_to_skills_md.clone(),
                first_discovery.clone(),
            )])),
            errors: Vec::new(),
            file_system: Arc::clone(&LOCAL_FS),
            is_agent_plugin: true,
        },
        HostSkillRootSnapshot {
            root: second_root,
            skills: vec![shared.clone(), repo.clone()],
            skill_discovery_path_by_path: Arc::new(HashMap::from([(
                shared.path_to_skills_md.clone(),
                second_discovery,
            )])),
            errors: Vec::new(),
            file_system: Arc::clone(&LOCAL_FS),
            is_agent_plugin: false,
        },
    ];

    let outcome = merge_host_skill_root_snapshots(snapshots);

    assert_eq!(outcome.skills, vec![repo, shared.clone()]);
    assert_eq!(
        outcome.skill_root_for_path(&shared.path_to_skills_md),
        Some(&first_root)
    );
    assert_eq!(
        outcome.skill_discovery_path_for_path(&shared.path_to_skills_md),
        Some(&first_discovery)
    );
    assert!(outcome.is_agent_plugin_skill(&shared));
}
