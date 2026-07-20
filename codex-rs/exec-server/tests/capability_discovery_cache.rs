use std::sync::Arc;

use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecutorCapabilityDiscoveryCache;
use codex_exec_server::ExecutorCapabilityDiscoverySnapshot;
use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_protocol::capabilities::CapabilityRootLocation;
use codex_protocol::capabilities::SelectedCapabilityRoot;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn snapshot_preserves_order_and_memoizes_results() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let skill_path = root.path().join("SKILL.md");
    std::fs::write(&skill_path, "first instructions")?;
    let selected_roots = vec![
        SelectedCapabilityRoot {
            id: "local-root".to_string(),
            location: CapabilityRootLocation::Environment {
                environment_id: LOCAL_ENVIRONMENT_ID.to_string(),
                path: PathUri::from_host_native_path(root.path())?,
            },
        },
        SelectedCapabilityRoot {
            id: "missing-root".to_string(),
            location: CapabilityRootLocation::Environment {
                environment_id: "missing".to_string(),
                path: PathUri::parse("file:///missing")?,
            },
        },
    ];
    let cache =
        ExecutorCapabilityDiscoveryCache::new(Arc::new(EnvironmentManager::default_for_tests()));

    let first = cache.snapshot(&selected_roots).await;
    assert_eq!(
        snapshot_view(&first),
        vec![
            (
                "local-root".to_string(),
                Ok(Some("first instructions".to_string())),
            ),
            (
                "missing-root".to_string(),
                Err("environment `missing` is unavailable".to_string()),
            ),
        ]
    );
    std::fs::write(skill_path, "changed instructions")?;

    let second = cache.snapshot(&selected_roots).await;
    assert_eq!(snapshot_view(&second), snapshot_view(&first));
    let first_discovery = first.roots()[0].result.as_ref().expect("first discovery");
    let second_discovery = second.roots()[0].result.as_ref().expect("second discovery");
    assert!(Arc::ptr_eq(first_discovery, second_discovery));

    Ok(())
}

fn snapshot_view(
    snapshot: &ExecutorCapabilityDiscoverySnapshot,
) -> Vec<(String, Result<Option<String>, String>)> {
    snapshot
        .roots()
        .iter()
        .map(|entry| {
            (
                entry.selected_root.id.clone(),
                entry
                    .result
                    .as_ref()
                    .map(|discovery| {
                        discovery
                            .skills
                            .first()
                            .map(|skill| skill.instructions.contents.clone())
                    })
                    .map_err(Clone::clone),
            )
        })
        .collect()
}
