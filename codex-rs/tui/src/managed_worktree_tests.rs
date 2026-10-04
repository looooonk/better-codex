use super::*;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

async fn fixture() -> Result<(TempDir, WorktreeConfigLoader, Config)> {
    let root = TempDir::new()?;
    let repo = root.path().join("repo");
    let codex_home = root.path().join("home");
    std::fs::create_dir_all(repo.join("nested"))?;
    std::fs::create_dir_all(&codex_home)?;
    std::fs::write(repo.join("nested/example.txt"), "committed contents")?;
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "initial",
        ],
    ] {
        let output = Command::new("git").current_dir(&repo).args(args).output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let repo = repo.canonicalize()?;
    write_trust(&codex_home, &repo, "trusted")?;
    let loader = WorktreeConfigLoader {
        cli_overrides: vec![
            ("features.worktrees".to_string(), toml::Value::Boolean(true)),
            (
                "model".to_string(),
                toml::Value::String("gpt-6.1-sol".to_string()),
            ),
        ],
        overrides: ConfigOverrides {
            cwd: Some(repo.join("nested")),
            ..Default::default()
        },
        loader_overrides: LoaderOverrides::without_managed_config_for_tests(),
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        strict_config: true,
    };
    let source = ConfigBuilder::default()
        .codex_home(codex_home)
        .cli_overrides(loader.cli_overrides.clone())
        .harness_overrides(loader.overrides.clone())
        .loader_overrides(loader.loader_overrides.clone())
        .strict_config(true)
        .build()
        .await?;
    Ok((root, loader, source))
}

fn write_trust(codex_home: &Path, repo: &Path, trust: &str) -> Result<()> {
    let key = toml::Value::String(repo.to_string_lossy().into_owned());
    std::fs::write(
        codex_home.join("config.toml"),
        format!("[projects.{key}]\ntrust_level = \"{trust}\"\n"),
    )?;
    Ok(())
}

#[tokio::test]
async fn native_checkout_inherits_repo_trust_and_binds_destination_thread() -> Result<()> {
    let (_root, loader, source) = fixture().await?;
    let prepared = loader.prepare(&source, source.cwd.to_path_buf()).await?;
    assert!(prepared.config.active_project.is_trusted());
    assert_eq!(
        prepared.config.cwd.to_path_buf(),
        prepared.checkout.root.join("nested")
    );
    assert_eq!(prepared.config.model, source.model);
    assert_eq!(
        std::fs::read_to_string(prepared.config.cwd.join("example.txt"))?,
        "committed contents"
    );
    let thread_id = ThreadId::new();
    prepared.bind(thread_id)?;
    assert_eq!(
        prepared
            .manager
            .owner(&prepared.checkout.root)
            .map_err(|error| eyre!(error.to_string()))?,
        Some(thread_id.to_string())
    );
    Ok(())
}

#[tokio::test]
async fn changed_source_trust_is_checked_before_allocating_checkout() -> Result<()> {
    let (_root, loader, source) = fixture().await?;
    let repo = source.cwd.as_path().parent().unwrap();
    write_trust(source.codex_home.as_path(), repo, "untrusted")?;
    let error = match loader.prepare(&source, source.cwd.to_path_buf()).await {
        Ok(_) => panic!("changed source trust must prevent checkout creation"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("explicitly untrusted source"));
    assert!(!source.codex_home.join("worktrees").exists());
    Ok(())
}
