use std::sync::Arc;

use codex_exec_server::LOCAL_FS;
use codex_protocol::protocol::SkillScope;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tokio::sync::Semaphore;

use super::catalog_from_outcome;
use crate::loader::HostSkillRoot;
use crate::loader::MAX_CONCURRENT_ROOT_SCANS;
use crate::loader::load_and_merge_host_skill_roots;

#[cfg(unix)]
#[tokio::test]
async fn catalog_preserves_symlinked_skill_discovery_path() -> Result<(), Box<dyn std::error::Error>>
{
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
    let outcome = load_and_merge_host_skill_roots(
        vec![HostSkillRoot::host(
            root.clone(),
            SkillScope::User,
            Arc::clone(&LOCAL_FS),
        )],
        &Semaphore::new(MAX_CONCURRENT_ROOT_SCANS),
        /*restriction_product*/ None,
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
