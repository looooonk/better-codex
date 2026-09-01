use super::AppServerArgs;
use clap::Parser;
use pretty_assertions::assert_eq;
use toml::Value as TomlValue;
use url::Url;

#[test]
fn app_server_accepts_cli_config_overrides() {
    let args = AppServerArgs::try_parse_from([
        "codex-app-server",
        "-c",
        "model=\"gpt-5-codex\"",
        "--config",
        "sandbox_mode=\"read-only\"",
        "--listen",
        "off",
    ])
    .expect("parse app-server args");

    let parsed_overrides = args
        .config_overrides
        .parse_overrides()
        .expect("parse config overrides");

    assert_eq!(
        parsed_overrides,
        vec![
            (
                "model".to_string(),
                TomlValue::String("gpt-5-codex".to_string()),
            ),
            (
                "sandbox_mode".to_string(),
                TomlValue::String("read-only".to_string()),
            ),
        ]
    );
}

#[test]
fn app_server_accepts_process_scoped_code_mode_host() {
    let args = AppServerArgs::try_parse_from([
        "codex-app-server",
        "--code-mode-host",
        "http://127.0.0.1:45123",
        "--code-mode-host-token-env",
        "CODE_MODE_TOKEN",
        "--listen",
        "off",
    ])
    .expect("parse app-server args");

    assert_eq!(
        args.code_mode_host.code_mode_host,
        Some(Url::parse("http://127.0.0.1:45123").expect("test endpoint should parse"))
    );
    assert_eq!(
        args.code_mode_host.code_mode_host_token_env.as_deref(),
        Some("CODE_MODE_TOKEN")
    );
}

#[test]
fn app_server_rejects_untrusted_code_mode_hosts_without_disclosing_secrets() {
    for endpoint in [
        "http://example.test:45123",
        "http://alice:super-secret@127.0.0.1:45123",
        "https://example.test/super-secret",
        "https://example.test?token=super-secret",
    ] {
        let error =
            AppServerArgs::try_parse_from(["codex-app-server", "--code-mode-host", endpoint])
                .expect_err("invalid endpoint should fail argument parsing");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        let rendered = error.to_string();
        assert!(!rendered.contains("alice"));
        assert!(!rendered.contains("super-secret"));
    }
}
