use super::super::AppServerSession;
use super::super::ForkGoalContinuation;
use super::super::ThreadParamsMode;
use super::ResumePermissions;
use crate::legacy_core::config::ConfigBuilder;
use crate::legacy_core::config::ConfigOverrides;
use codex_app_server_client::AppServerClient;
use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::JSONRPCMessage;
use codex_protocol::ThreadId;
use codex_protocol::config_types::SandboxMode;
use codex_utils_absolute_path::AbsolutePathBuf;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn only_direct_launch_choices_override_saved_permissions() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let profile = home.path().join("work.config.toml");
    std::fs::write(
        &profile,
        "default_permissions = \":read-only\"\napproval_policy = \"never\"\napprovals_reviewer = \"auto_review\"\n",
    )?;
    let builder = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .loader_overrides(codex_config::LoaderOverrides {
            user_config_path: Some(AbsolutePathBuf::try_from(profile)?),
            user_config_profile: Some("work".parse()?),
            ..codex_config::LoaderOverrides::without_managed_config_for_tests()
        });
    let config = builder.clone().build().await?;
    assert_eq!(
        ResumePermissions::from_overrides(&config, &ConfigOverrides::default()),
        ResumePermissions::default(),
    );
    let direct_rules = builder
        .clone()
        .cli_overrides(vec![(
            "permissions.safe.extends".into(),
            ":read-only".into(),
        )])
        .build()
        .await?;
    assert_eq!(
        ResumePermissions::from_overrides(&direct_rules, &ConfigOverrides::default()),
        ResumePermissions {
            profile: true,
            ..Default::default()
        },
    );
    let config = builder
        .cli_overrides(vec![("approval_policy".into(), "on-request".into())])
        .build()
        .await?;
    assert_eq!(
        ResumePermissions::from_overrides(
            &config,
            &ConfigOverrides {
                default_permissions: Some(":read-only".into()),
                cwd: Some(home.path().to_path_buf()),
                ..Default::default()
            },
        ),
        ResumePermissions {
            approval_policy: true,
            profile: true,
            workspace_roots: true,
            ..Default::default()
        },
    );
    Ok(())
}

#[tokio::test]
async fn remote_history_rejects_permission_overrides_before_dispatch() -> color_eyre::Result<()> {
    let home = tempfile::tempdir()?;
    let builder = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(codex_config::LoaderOverrides::without_managed_config_for_tests());
    let config = builder.clone().build().await?;
    let cli_config = builder
        .cli_overrides(vec![("approval_policy".into(), "on-request".into())])
        .build()
        .await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let mut requests = Vec::new();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else {
                continue;
            };
            let response = if request.method == "initialize" {
                json!({"id": request.id, "result": {"userAgent": "permission-test"}})
            } else {
                requests.push(request.method);
                json!({"id": request.id, "error": {"code": -32600, "message": "remote request observed"}})
            };
            socket
                .send(Message::Text(response.to_string().into()))
                .await
                .unwrap();
        }
        requests
    });
    let client = RemoteAppServerClient::connect(RemoteAppServerConnectArgs {
        endpoint: RemoteAppServerEndpoint::WebSocket {
            websocket_url: url,
            auth_token: None,
        },
        client_name: "permission-test".to_string(),
        client_version: "0.0.0".to_string(),
        experimental_api: true,
        required_human_approval: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: 8,
    })
    .await?;
    let mut session =
        AppServerSession::new(AppServerClient::Remote(client), ThreadParamsMode::Remote);
    let thread_id = ThreadId::new();
    for (config, overrides) in [
        (
            &config,
            ConfigOverrides {
                sandbox_mode: Some(SandboxMode::ReadOnly),
                ..Default::default()
            },
        ),
        (
            &config,
            ConfigOverrides {
                workspace_roots: Some(vec![AbsolutePathBuf::from_absolute_path(home.path())?]),
                ..Default::default()
            },
        ),
        (&cli_config, ConfigOverrides::default()),
    ] {
        session = session.with_launch_overrides(&overrides);
        assert_eq!(
            session
                .resume_thread(config.clone(), thread_id)
                .await
                .unwrap_err()
                .to_string(),
            "Permission overrides are not supported when resuming a remote task."
        );
        assert_eq!(
            session
                .resume_thread_in_background(config.clone(), thread_id)
                .await
                .unwrap_err()
                .to_string(),
            "Permission overrides are not supported when resuming a remote task."
        );
        assert_eq!(
            session
                .fork_thread(config.clone(), thread_id)
                .await
                .unwrap_err()
                .to_string(),
            "Permission overrides are not supported when forking a remote task."
        );
        assert_eq!(
            session
                .fork_thread_before_turn_in_background(
                    config.clone(),
                    thread_id,
                    "turn-1".to_string(),
                    ForkGoalContinuation::DeferUntilNextTurn
                )
                .await
                .unwrap_err()
                .to_string(),
            "Permission overrides are not supported when forking a remote task."
        );
    }
    session = session.with_launch_overrides(&ConfigOverrides {
        cwd: Some(home.path().to_path_buf()),
        ..Default::default()
    });
    assert!(
        session
            .resume_thread(config.clone(), thread_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("remote request observed")
    );
    assert!(
        session
            .fork_thread(config, thread_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("remote request observed")
    );
    session.shutdown().await?;
    assert_eq!(server.await?, vec!["thread/resume", "thread/fork"]);
    Ok(())
}
