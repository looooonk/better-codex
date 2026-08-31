use std::sync::Arc;

use codex_core_skills::loader::SkillRoot;
use codex_core_skills::loader::load_skills_from_roots;
use codex_exec_server::LOCAL_FS;
use codex_protocol::protocol::SkillScope;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_plugins::SkillDiscoveryMode;
use pretty_assertions::assert_eq;

use super::catalog_from_outcome;

#[cfg(unix)]
#[tokio::test]
async fn catalog_preserves_symlinked_skill_discovery_path(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source = tempfile::tempdir()?;
    let source_skill_dir = source.path().join("linked-skill");
    std::fs::create_dir_all(&source_skill_dir)?;
    std::fs::write(
        source_skill_dir.join("SKILL.md"),
        "---\nname: linked-skill\ndescription: Linked skill.\n---\n# Linked skill\n",
    )?;
    std::os::unix::fs::symlink(&source_skill_dir, root.path().join("linked-skill"))?;

    let root = AbsolutePathBuf::try_from(std::fs::canonicalize(root.path())?)?;
    let outcome = load_skills_from_roots(
        [SkillRoot {
            path: root.clone(),
            scope: SkillScope::User,
            file_system: Arc::clone(&LOCAL_FS),
            plugin_id: None,
            plugin_namespace: None,
            plugin_root: None,
            discovery_mode: SkillDiscoveryMode::Recursive,
        }],
        /*plugin_skill_snapshots*/ None,
    )
    .await;
    let catalog = catalog_from_outcome(&outcome);
    let canonical_path = std::fs::canonicalize(source_skill_dir.join("SKILL.md"))?;
    let discovery_path = root.join("linked-skill/SKILL.md");

    assert_eq!(catalog.entries.len(), 1);
    assert_eq!(
        (
            catalog.entries[0].main_prompt.as_str(),
            catalog.entries[0].display_path.as_deref(),
        ),
        (
            canonical_path.to_string_lossy().as_ref(),
            Some(discovery_path.to_string_lossy().as_ref()),
        )
    );

    Ok(())
}
