mod common;

use codex_config::CONFIG_TOML_FILE;
use codex_config::ConfigLayerSource;
use codex_config::format_config_layer_source;
use codex_config::loader::project_trust_key;
use codex_exec_server::Environment;
use codex_exec_server::EnvironmentCapabilities;
use codex_exec_server::EnvironmentConfigLayer;
use codex_exec_server::EnvironmentConfigLayerStack;
use codex_exec_server::EnvironmentConfigReadParams;
use codex_exec_server::EnvironmentConfigReadResponse;
use codex_exec_server::ExecServerError;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use common::exec_server::exec_server;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_environment_reads_bounded_projected_config() -> anyhow::Result<()> {
    let mut server = exec_server().await?;
    let codex_home =
        AbsolutePathBuf::from_absolute_path(std::fs::canonicalize(server.codex_home())?)?;
    let project = codex_home.join("project");
    let dot_codex = project.join(".codex");
    tokio::fs::create_dir_all(dot_codex.as_path()).await?;
    tokio::fs::write(project.join(".project-root").as_path(), "").await?;
    let project_key = toml::Value::String(project_trust_key(project.as_path())).to_string();
    tokio::fs::write(
        codex_home.join(CONFIG_TOML_FILE).as_path(),
        format!(
            "project_root_markers = [\".project-root\"]\n[projects.{project_key}]\ntrust_level = \"trusted\""
        ),
    )
    .await?;
    tokio::fs::write(
        dot_codex.join(CONFIG_TOML_FILE).as_path(),
        format!(
            "[future_environment]\nrelative_path = \"./executor-relative\"\nunselected = \"do not return\"\noversized = \"{}\"",
            "x".repeat(1024 * 1024)
        ),
    )
    .await?;

    let environment = Environment::create_for_tests(Some(server.websocket_url().to_string()))?;
    assert_eq!(
        environment.info().await?.capabilities,
        EnvironmentCapabilities {
            network_proxy_launch: true,
            environment_config_read: true,
        }
    );
    let params = EnvironmentConfigReadParams {
        cwd: PathUri::from_abs_path(&project),
        config_paths: vec![vec![
            "future_environment".to_string(),
            "relative_path".to_string(),
        ]],
        requirements_paths: Vec::new(),
    };
    let response = environment.read_environment_config(params.clone()).await?;
    let projected_toml = toml::toml! {
        [future_environment]
        relative_path = "./executor-relative"
    };
    assert_eq!(
        response,
        EnvironmentConfigReadResponse {
            user_home_dir: dirs::home_dir()
                .and_then(|home_dir| PathUri::from_host_native_path(home_dir).ok()),
            codex_home_dir: PathUri::from_abs_path(&codex_home),
            hostname: codex_config::host_name(),
            config: EnvironmentConfigLayerStack {
                layers: vec![EnvironmentConfigLayer {
                    source: format_config_layer_source(
                        &ConfigLayerSource::Project {
                            dot_codex_folder: dot_codex.clone(),
                        },
                        CONFIG_TOML_FILE,
                    ),
                    base_dir: PathUri::from_abs_path(&dot_codex),
                    toml: toml::to_string(&projected_toml)?,
                }],
                cloud_insertion_index: 0,
            },
            requirements: EnvironmentConfigLayerStack {
                layers: Vec::new(),
                cloud_insertion_index: 0,
            },
        }
    );

    let oversized = environment
        .read_environment_config(EnvironmentConfigReadParams {
            config_paths: vec![vec![
                "future_environment".to_string(),
                "oversized".to_string(),
            ]],
            ..params.clone()
        })
        .await
        .expect_err("oversized response should fail atomically");
    assert_server_invalid_params(oversized, "environment config response exceeds the ")?;
    assert_eq!(environment.read_environment_config(params).await?, response);

    server.shutdown().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn environment_config_read_rejects_invalid_selectors_and_foreign_cwd() -> anyhow::Result<()> {
    let mut server = exec_server().await?;
    let codex_home =
        AbsolutePathBuf::from_absolute_path(std::fs::canonicalize(server.codex_home())?)?;
    let environment = Environment::create_for_tests(Some(server.websocket_url().to_string()))?;
    let cwd = PathUri::from_abs_path(&codex_home);
    for (config_paths, expected) in [
        (
            Vec::new(),
            "at least one config or requirements path is required",
        ),
        (
            vec![Vec::new()],
            "TOML paths must contain at least one key segment",
        ),
        (
            (0..65).map(|index| vec![index.to_string()]).collect(),
            "at most 64 TOML selectors are allowed",
        ),
        (
            vec![vec!["x".to_string(); 33]],
            "TOML paths may contain at most 32 key segments",
        ),
        (
            vec![vec!["x".repeat(64 * 1024 + 1)]],
            "TOML selector input may contain at most 65536 bytes",
        ),
    ] {
        let error = environment
            .read_environment_config(EnvironmentConfigReadParams {
                cwd: cwd.clone(),
                config_paths,
                requirements_paths: Vec::new(),
            })
            .await
            .expect_err("invalid selectors should fail");
        assert_server_invalid_params(error, expected)?;
    }

    #[cfg(unix)]
    let foreign_cwd = PathUri::parse("file:///C:/workspace")?;
    #[cfg(windows)]
    let foreign_cwd = PathUri::parse("file:///workspace")?;
    let error = environment
        .read_environment_config(EnvironmentConfigReadParams {
            cwd: foreign_cwd,
            config_paths: vec![vec!["model".to_string()]],
            requirements_paths: Vec::new(),
        })
        .await
        .expect_err("foreign cwd should fail closed");
    let ExecServerError::Server { code, .. } = error else {
        anyhow::bail!("expected server error, got {error:?}");
    };
    assert_eq!(code, -32602);

    server.shutdown().await?;
    Ok(())
}

fn assert_server_invalid_params(error: ExecServerError, expected: &str) -> anyhow::Result<()> {
    let ExecServerError::Server { code, message } = error else {
        anyhow::bail!("expected server error, got {error:?}");
    };
    assert_eq!(code, -32602);
    assert!(
        message.starts_with(expected),
        "unexpected error message: {message}"
    );
    Ok(())
}
