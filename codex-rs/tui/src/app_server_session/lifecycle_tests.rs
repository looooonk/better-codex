use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_protocol::models::BaseInstructionsProvenance;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn cloud_lifecycle_leaves_execution_settings_with_the_host() {
    let home = tempfile::tempdir().expect("tempdir");
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await
        .expect("config");
    config.model = Some("local-model".to_string());
    config.service_tier = Some("priority".to_string());
    let thread_id = ThreadId::new();
    assert_eq!(
        thread_start_params_from_config(
            &config,
            ThreadParamsMode::Cloud,
            /*remote_cwd_override*/ None,
            /*session_start_source*/ None
        ),
        ThreadStartParams::default(),
    );
    assert_eq!(
        thread_resume_params_from_config(
            config,
            thread_id,
            ThreadParamsMode::Cloud,
            /*remote_cwd_override*/ None,
            ResumeModelSettings::OverrideFromCurrentConfig,
            ResumePermissions::CURRENT_CONFIG,
        ),
        ThreadResumeParams {
            thread_id: thread_id.to_string(),
            ..ThreadResumeParams::default()
        },
    );
}

#[tokio::test]
async fn model_catalog_base_instructions_are_not_pinned_into_forks() {
    let home = tempfile::tempdir().expect("tempdir");
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await
        .expect("config");
    config.base_instructions = Some("Old model instructions".to_string());
    config.base_instructions_provenance = Some(BaseInstructionsProvenance::Model {
        model: "old-model".to_string(),
    });
    let fork = thread_fork_params_from_config(
        config,
        ThreadId::new(),
        ThreadParamsMode::Embedded,
        /*remote_cwd_override*/ None,
    );
    assert_eq!(fork.base_instructions, None);
}
