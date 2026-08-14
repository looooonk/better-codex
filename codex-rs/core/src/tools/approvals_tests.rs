use super::*;
use pretty_assertions::assert_eq;

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
fn timeout_rejection_is_specific_to_the_reviewer() {
    let rejection = |source| {
        let ToolError::Rejected(message) =
            normalize_decision(ReviewDecision::TimedOut, source).expect_err("timeout should reject")
        else {
            panic!("timeout should produce a rejection")
        };
        message
    };
    assert_eq!(
        (
            rejection(ApprovalResolutionSource::Guardian),
            rejection(ApprovalResolutionSource::User),
        ),
        (
            guardian_timeout_message(),
            "approval request timed out".to_string(),
        )
    );
}
