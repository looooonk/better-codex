use super::*;
use crate::tools::runtimes::unified_exec::UnifiedExecApprovalKey;
use crate::tools::sandboxing::ApprovalStore;
use codex_protocol::models::SandboxPermissions;
use codex_protocol::protocol::ReviewDecision;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn guardian_cwd_preserves_drive_shaped_local_posix_path() {
    let native_cwd = AbsolutePathBuf::try_from(std::path::PathBuf::from("/C:/workspace"))
        .expect("drive-shaped POSIX path should be absolute");
    let cwd = PathUri::from_abs_path(&native_cwd);

    assert_eq!(
        guardian_cwd(codex_exec_server::LOCAL_ENVIRONMENT_ID, cwd)
            .expect("local cwd should retain the host path convention"),
        native_cwd
    );
}

#[test]
fn guardian_cwd_rejects_foreign_remote_path() {
    let cwd = PathUri::parse("file:///C:/workspace").expect("valid Windows path URI");

    assert!(guardian_cwd(codex_exec_server::REMOTE_ENVIRONMENT_ID, cwd).is_err());
}

#[test]
fn cache_key_serialization_includes_tool_namespace() {
    let key = ApprovalCacheKey::ExecCommand(UnifiedExecApprovalKey {
        environment_id: "local".to_string(),
        command: vec!["echo".to_string()],
        cwd: PathUri::parse("file:///workspace").expect("valid path URI"),
        tty: false,
        sandbox_permissions: SandboxPermissions::UseDefault,
        additional_permissions: None,
    });
    assert_eq!(
        serde_json::to_value(key).expect("cache key should serialize"),
        json!({
            "exec_command": {
                "environment_id": "local",
                "command": ["echo"],
                "cwd": "file:///workspace",
                "tty": false,
                "sandbox_permissions": "use_default",
                "additional_permissions": null,
            }
        })
    );
}

#[test]
fn cache_keys_are_isolated_by_environment() {
    let key = |environment_id: &str| {
        ApprovalCacheKey::ExecCommand(UnifiedExecApprovalKey {
            environment_id: environment_id.to_string(),
            command: vec!["echo".to_string()],
            cwd: PathUri::parse("file:///workspace").expect("valid path URI"),
            tty: false,
            sandbox_permissions: SandboxPermissions::UseDefault,
            additional_permissions: None,
        })
    };
    let mut store = ApprovalStore::default();
    store.put(key("local"), ReviewDecision::ApprovedForSession);

    assert_eq!(
        (store.get(&key("local")), store.get(&key("remote")),),
        (Some(ReviewDecision::ApprovedForSession), None)
    );
}
