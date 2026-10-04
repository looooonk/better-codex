use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::diagnostic_commands::DiagnosticRequest;
use crate::app_shell::render::ShellView;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[tokio::test]
async fn native_workspace_requests_update_the_selected_thread_and_read_diagnostics() -> Result<()> {
    let home = tempfile::tempdir()?;
    let destination = home.path().join("workspace");
    std::fs::create_dir(&destination)?;
    let destination = std::fs::canonicalize(destination)?;
    std::fs::write(
        destination.join("native_mention_fixture.rs"),
        "fn example() {}\n",
    )?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let mut app_server = crate::start_embedded_app_server_for_picker(&config).await?;
    let started = app_server.start_thread(&config).await?;
    let id = started.session.thread_id;
    let handle = app_server.request_handle();
    execute(
        handle.clone(),
        id,
        WorkspaceRequest::Cwd(destination.clone()),
    )
    .await?;
    execute(
        handle.clone(),
        id,
        WorkspaceRequest::Rename("Native command test".to_string()),
    )
    .await?;
    let mode = CollaborationMode {
        mode: ModeKind::Plan,
        settings: Settings {
            model: started.session.model,
            reasoning_effort: started.session.reasoning_effort,
            developer_instructions: None,
        },
    };
    let response = execute(handle.clone(), id, WorkspaceRequest::Plan(mode.clone())).await?;
    let WorkspaceResponse::Mode(actual_mode) = response else {
        panic!("expected plan response");
    };
    assert_eq!(actual_mode, mode);
    let mut applied_mode = mode;
    applied_mode.settings.developer_instructions =
        codex_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets()
            .into_iter()
            .find(|preset| preset.mode == Some(ModeKind::Plan))
            .expect("native Plan preset")
            .developer_instructions
            .flatten();
    let settings =
        tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
            loop {
                let event = app_server
                    .next_event()
                    .await
                    .expect("app-server event stream");
                if let codex_app_server_client::AppServerEvent::ServerNotification(notification) =
                    event
                    && let codex_app_server_protocol::ServerNotification::ThreadSettingsUpdated(
                        updated,
                    ) = *notification
                    && updated.thread_id == id.to_string()
                    && updated.thread_settings.collaboration_mode.mode == ModeKind::Plan
                {
                    break updated.thread_settings;
                }
            }
        })
        .await?;
    assert_eq!(
        (settings.cwd.as_path(), settings.collaboration_mode),
        (destination.as_path(), applied_mode)
    );
    let thread = app_server.thread_read(id, /*include_turns*/ false).await?;
    assert_eq!(thread.name.as_deref(), Some("Native command test"));
    let rollout = execute(
        handle.clone(),
        id,
        WorkspaceRequest::Diagnostic(DiagnosticRequest::Rollout),
    )
    .await?;
    let WorkspaceResponse::Notice(rollout) = rollout else {
        panic!("expected rollout path");
    };
    assert_eq!(
        rollout,
        thread
            .path
            .expect("durable thread rollout")
            .display()
            .to_string()
    );
    let diagnostics = execute(
        handle.clone(),
        id,
        WorkspaceRequest::Diagnostic(DiagnosticRequest::Config(
            destination.to_string_lossy().into_owned(),
        )),
    )
    .await?;
    assert!(
        matches!(diagnostics, WorkspaceResponse::Notice(message) if message.contains("Configuration layers") && message.contains("Effective setting sources"))
    );
    let files = crate::app_shell::file_mentions::search(
        handle,
        destination.to_string_lossy().into_owned(),
        "native_mention_fixture".to_string(),
    )
    .await?;
    let matches = files
        .into_iter()
        .map(|file| (PathBuf::from(file.root), file.path, file.match_type))
        .collect::<Vec<_>>();
    assert_eq!(
        matches,
        vec![(
            destination,
            "native_mention_fixture.rs".to_string(),
            codex_app_server_protocol::FuzzyFileSearchMatchType::File
        )]
    );
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn native_feature_toggles_validate_names_and_persist_the_configured_value() -> Result<()> {
    let home = tempfile::tempdir()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .build()
        .await?;
    let mut app_server = crate::start_embedded_app_server_for_picker(&config).await?;
    let started = app_server.start_thread(&config).await?;
    let id = started.session.thread_id;
    let handle = app_server.request_handle();
    let unsupported = execute(
        handle.clone(),
        id,
        WorkspaceRequest::Experimental(Some(("not_a_codex_feature".to_string(), true))),
    )
    .await;
    assert!(
        unsupported
            .unwrap_err()
            .to_string()
            .contains("did not advertise")
    );
    let enabled = execute(
        handle.clone(),
        id,
        WorkspaceRequest::Experimental(Some(("worktrees".to_string(), true))),
    )
    .await?;
    let WorkspaceResponse::Notice(enabled) = enabled else {
        panic!("expected configured feature value");
    };
    assert_eq!(enabled, "worktrees enabled in configuration");
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.transcript.clear();
    shell.complete_workspace_request(shell.thread_id, Ok(WorkspaceResponse::Notice(enabled)));
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 22,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let rendered = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("configured_feature_toggle", rendered);
    let persisted: toml::Value =
        toml::from_str(&std::fs::read_to_string(home.path().join("config.toml"))?)?;
    assert_eq!(persisted["features"]["worktrees"].as_bool(), Some(true));
    let disabled = execute(
        handle,
        id,
        WorkspaceRequest::Experimental(Some(("worktrees".to_string(), false))),
    )
    .await?;
    assert!(
        matches!(disabled, WorkspaceResponse::Notice(message) if message == "worktrees disabled in configuration")
    );
    app_server.shutdown().await?;
    Ok(())
}
