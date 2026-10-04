use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

#[test]
fn configured_native_items_render_in_order_without_replacing_the_header() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.context_token_usage.total_tokens = 56_000;
    shell.model_context_window = Some(100_000);
    shell.thread_name = Some("Display controls".into());
    shell.status_surfaces.preferences = Preferences::parse(
        &[
            "thread-name",
            "model-name",
            "context-usage",
            "task-progress",
        ]
        .map(str::to_owned),
        &["app-name", "thread", "context-used"].map(str::to_owned),
        /*colors*/ true,
    )
    .0;
    let title = shell
        .status_surfaces
        .preferences
        .title
        .iter()
        .filter_map(|item| shell.terminal_title_value(*item))
        .collect::<Vec<_>>();
    assert_eq!(
        title,
        ["Better Codex", "Display controls", "Context 50% used"]
    );
    for width in [100, 40] {
        let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 24);
        let mut buffer = Buffer::empty(area);
        crate::app_shell::render::ShellView { shell: &shell }.render(area, &mut buffer);
        let rendered = buffer
            .content
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(format!("status_line_{width}"), rendered);
        let layout = crate::app_shell::shell_layout::calculate(&shell, area).unwrap();
        assert_eq!(layout.status.unwrap().bottom(), layout.transcript.y);
    }
}

#[test]
fn title_picker_and_ordered_arguments_use_native_aliases() {
    let mut shell = ShellState::snapshot_fixture();
    shell.status_surfaces.preferences.title = vec![TerminalTitleItem::ThreadName];
    shell
        .run_status_surface_command(Surface::Title, "")
        .unwrap();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
    );
    let mut buffer = Buffer::empty(area);
    shell
        .selector
        .as_ref()
        .unwrap()
        .render(area, /*pointer*/ None, &mut buffer);
    let rendered = buffer
        .content
        .chunks(100)
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("title_picker", rendered);
    assert_eq!(
        Surface::Title
            .parse("thread, spinner project thread-title")
            .unwrap(),
        SurfaceChange::Items(
            Surface::Title,
            ["thread-title", "activity", "project-name"]
                .map(str::to_owned)
                .into()
        )
    );
    assert!(Surface::StatusLine.parse("unknown-item").is_err());
}

#[tokio::test]
async fn native_profile_persistence_reads_back_higher_priority_overrides() -> Result<()> {
    let home = tempfile::tempdir()?;
    let base = home.path().join("config.toml");
    tokio::fs::write(&base, "[tui]\nstatus_line = ['current-dir']\n").await?;
    let profile = AbsolutePathBuf::from_absolute_path(home.path().join("work.config.toml"))?;
    tokio::fs::write(&profile, "[tui]\nanimations = false\n").await?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(LoaderOverrides {
            user_config_path: Some(profile.clone()),
            user_config_profile: Some("work".parse()?),
            ..LoaderOverrides::without_managed_config_for_tests()
        })
        .cli_overrides(vec![(
            "tui.terminal_title".into(),
            toml::Value::Array(vec![toml::Value::String("thread-name".into())]),
        )])
        .build()
        .await?;
    let app_server = crate::start_embedded_app_server_for_picker(&config).await?;
    let mut shell = ShellState::snapshot_fixture();
    shell.client_config_path = profile.clone();
    assert_eq!(
        shell.status_surfaces.configure(&config),
        Vec::<String>::new()
    );
    shell.run_status_surface_command(Surface::Title, "model project")?;
    tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
        while shell.has_pending_backend_actions() {
            shell.poll_backend_actions(&app_server).await;
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert_eq!(
        shell.status_surfaces.preferences.title,
        vec![TerminalTitleItem::ThreadName]
    );
    let saved: toml::Value = toml::from_str(&tokio::fs::read_to_string(&profile).await?)?;
    assert_eq!(
        saved,
        toml::toml! { [tui] animations = false terminal_title = ["model", "project-name"] }.into()
    );
    assert_eq!(
        tokio::fs::read_to_string(&base).await?,
        "[tui]\nstatus_line = ['current-dir']\n"
    );
    app_server.shutdown().await?;
    Ok(())
}

#[test]
fn unknown_config_items_are_reported_and_missing_optional_data_is_omitted() {
    let (prefs, invalid) = Preferences::parse(
        &["model", "bad-status"].map(str::to_owned),
        &["bad-title".into()],
        /*colors*/ false,
    );
    assert_eq!(
        (prefs, invalid),
        (
            Preferences {
                status: vec![StatusLineItem::ModelName],
                title: Vec::new(),
                colors: false
            },
            ["bad-status", "bad-title"].map(str::to_owned).into()
        )
    );
    let shell = ShellState::snapshot_fixture();
    assert_eq!(
        [
            StatusLineItem::PullRequestNumber,
            StatusLineItem::BranchChanges,
            StatusLineItem::WorkspaceHeadline
        ]
        .map(|item| shell.status_surface_value(item)),
        [None, None, None]
    );
}

#[test]
fn fast_display_follows_native_model_capabilities_and_tier_aliases() {
    let mut shell = ShellState::snapshot_fixture();
    shell.available_models = codex_models_manager::bundled_models_response()
        .unwrap()
        .models
        .into_iter()
        .map(Into::into)
        .collect();
    shell.model = shell
        .available_models
        .iter()
        .find(|model| model.supports_fast_mode())
        .unwrap()
        .model
        .clone();
    for tier in ["fast", "priority"] {
        shell.service_tier = Some(tier.into());
        assert_eq!(
            shell.status_surface_value(StatusLineItem::FastMode),
            Some("Fast on".into())
        );
    }
    shell.service_tier = Some("default".into());
    assert_eq!(
        shell.status_surface_value(StatusLineItem::FastMode),
        Some("Fast off".into())
    );
    shell.available_models.clear();
    assert_eq!(shell.status_surface_value(StatusLineItem::FastMode), None);
}

#[test]
fn completed_display_settings_preserve_a_newer_picker_and_active_run_status() {
    let mut shell = ShellState::snapshot_fixture();
    shell
        .run_status_surface_command(Surface::Title, "")
        .unwrap();
    let original = shell.selector.clone();
    shell
        .run_status_surface_command(Surface::StatusLine, "")
        .unwrap();
    let current = shell.selector.clone();
    let status = shell.status.clone();
    shell.complete_status_surface(
        SurfaceChange::Colors(true),
        original,
        Ok(Preferences {
            colors: true,
            ..Preferences::default()
        }),
    );
    assert_eq!((shell.selector, shell.status), (current, status));
}
