use super::*;
use crate::legacy_core::config::ConfigBuilder;
use codex_app_server_protocol::JSONRPCMessage;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

#[test]
fn feature_overrides_allow_daemon_without_allowing_arbitrary_config() {
    for (raw, eligible) in [
        ("features.app_server_websocket_compression=true", true),
        ("features={worktrees=true,shell_tool=false}", true),
        ("features.worktrees='true'", false),
        ("features.worktrees.enabled=true", false),
        ("features={}", false),
        ("suppress_unstable_features_warning=true", true),
        ("tui.fullscreen_transcript=true", true),
        ("tui={fullscreen_transcript=true}", true),
        ("tui={fullscreen_transcript=true,animations=false}", false),
        ("model='test'", false),
    ] {
        let overrides = codex_utils_cli::CliConfigOverrides {
            raw_overrides: vec![raw.to_string()],
        }
        .parse_overrides()
        .unwrap();
        assert_eq!(
            config_exclusion(
                &overrides,
                &LoaderOverrides::default(),
                /*strict_config*/ false,
                /*bypass_hook_trust*/ false
            )
            .is_none(),
            eligible,
            "{raw}"
        );
    }
}

#[test]
fn daemon_features_follow_cli_table_replacement_and_removed_features() {
    let overrides = codex_utils_cli::CliConfigOverrides {
        raw_overrides: [
            "features.api_key_model_discovery=true",
            "features={code_mode_host=false}",
            "features.code_mode_host=true",
            "features.remote_compaction_v2=true",
        ]
        .map(str::to_string)
        .to_vec(),
    }
    .parse_overrides()
    .unwrap();
    assert_eq!(
        server_features(&overrides),
        BTreeMap::from([("code_mode_host".to_string(), true),])
    );
}

#[test]
fn daemon_features_preserve_canonical_image_generation_precedence() {
    for enabled in [false, true] {
        let overrides = codex_utils_cli::CliConfigOverrides {
            raw_overrides: vec![
                format!("features.image_generation={enabled}"),
                format!("features.imagegenext={}", !enabled),
            ],
        }
        .parse_overrides()
        .unwrap();
        assert_eq!(
            server_features(&overrides),
            BTreeMap::from([("image_generation".to_string(), enabled)])
        );
    }
}

#[tokio::test]
async fn daemon_compatibility_respects_policy_and_recovers_from_broken_servers()
-> anyhow::Result<()> {
    for scenario in [
        "matching",
        "conflicting client",
        "managed",
        "legacy managed",
        "missing feature",
        "config deleted",
        "unsupported RPC",
        "old protocol",
    ] {
        let overrides = codex_utils_cli::CliConfigOverrides {
            raw_overrides: vec![if scenario == "missing feature" {
                "features.future_feature=true".into()
            } else {
                "features.api_key_model_discovery=false".into()
            }],
        }
        .parse_overrides()
        .unwrap();
        let home = tempfile::TempDir::new()?;
        let config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .build()
            .await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = RemoteAppServerEndpoint::WebSocket {
            websocket_url: format!("ws://{}", listener.local_addr()?),
            auth_token: None,
        };
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let JSONRPCMessage::Request(request) = serde_json::from_str(&text).unwrap() else {
                    continue;
                };
                let response = match request.method.as_str() {
                    "initialize" => json!({"id": request.id, "result": {
                        "userAgent": format!("codex_cli_rs/{}", if scenario == "old protocol" { "0.1.0" } else { codex_build_info::CODEX_BACKEND_COMPAT_VERSION })
                    }}),
                    "config/read" => json!({"id": request.id, "result": {"config": {}, "layers": [{
                        "name": {"type": "legacyManagedConfigTomlFromFile"},
                        "config": if scenario == "legacy managed" { json!({"features": {"api_key_model_discovery": true}}) } else { json!({}) }
                    }]}}),
                    "configRequirements/read" => json!({"id": request.id, "result": {
                        "requirements": if scenario == "managed" { json!({"featureRequirements": {"api_key_model_discovery": true}}) } else { json!(null) }
                    }}),
                    "experimentalFeature/list" if scenario == "config deleted" => {
                        json!({"id": request.id, "error": {
                            "code": -32603, "message": "failed to reload config: No such file or directory (os error 2)"
                        }})
                    }
                    "experimentalFeature/list" if scenario == "unsupported RPC" => {
                        json!({"id": request.id, "error": {
                            "code": -32601, "message": "method not found"
                        }})
                    }
                    "experimentalFeature/list" => json!({"id": request.id, "result": {"data": [{
                        "name": "api_key_model_discovery", "stage": "beta", "enabled": scenario != "matching", "defaultEnabled": false
                    }], "nextCursor": null}}),
                    method => panic!("unexpected request: {method}"),
                };
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let result = check(&endpoint, &config, &overrides).await;
        server.await?;
        if matches!(scenario, "matching" | "managed" | "legacy managed") {
            result?;
        } else {
            let issue = result.expect_err(scenario);
            let launch = DaemonLaunch::independent(issue.reason);
            assert!(matches!(launch.target, AppServerTarget::Embedded));
            let warning = launch.warning.expect("fallback diagnostic");
            match scenario {
                "conflicting client" => {
                    insta::assert_snapshot!("daemon_feature_mismatch_fallback", warning)
                }
                "config deleted" | "unsupported RPC" => assert!(
                    warning.contains("Experimental feature request failed"),
                    "{warning}"
                ),
                "old protocol" => assert!(warning.contains("protocol 0.1.0 differs"), "{warning}"),
                "missing feature" => assert!(
                    warning.contains("future_feature to be enabled"),
                    "{warning}"
                ),
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn unreachable_daemon_allows_an_independent_session() -> anyhow::Result<()> {
    let home = tempfile::TempDir::new()?;
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await?;
    let endpoint = RemoteAppServerEndpoint::UnixSocket {
        socket_path: AbsolutePathBuf::from_absolute_path(home.path().join("missing.sock"))?,
    };
    let issue = check(&endpoint, &config, &[])
        .await
        .expect_err("missing server");
    let launch = DaemonLaunch::independent(issue.reason);
    assert!(matches!(launch.target, AppServerTarget::Embedded));
    assert!(launch.warning.unwrap().contains("could not connect"));
    Ok(())
}

#[test]
fn independent_launch_options_are_preserved() {
    use clap::Parser;
    for (args, expected) in [
        (vec!["better-codex"], None),
        (vec!["better-codex", "--no-daemon"], Some("--no-daemon")),
        (vec!["better-codex", "--oss"], Some("--oss")),
        (vec!["better-codex", "--profile", "test"], Some("--profile")),
        (
            vec!["better-codex", "--strict-config"],
            Some("--strict-config"),
        ),
    ] {
        let cli = Cli::parse_from(args);
        assert_eq!(
            exclusion(
                &cli,
                &[],
                &LoaderOverrides::default(),
                /*workload_identity_selected*/ false,
                /*exec_server_url*/ None
            ),
            expected
        );
    }
}
