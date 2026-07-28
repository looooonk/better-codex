use anyhow::Result;
use predicates::prelude::*;
use tempfile::TempDir;

fn codex_command(codex_home: &std::path::Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[test]
fn root_help_uses_public_better_codex_identity() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Better Codex CLI"))
        .stdout(predicate::str::contains("Usage: better-codex"))
        .stdout(predicate::str::contains("Usage: codex").not());

    Ok(())
}

#[test]
fn root_version_uses_public_better_codex_identity() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("better-codex {}\n", env!("CARGO_PKG_VERSION")));

    Ok(())
}

#[test]
fn zsh_completion_uses_public_better_codex_identity() -> Result<()> {
    let codex_home = TempDir::new()?;

    codex_command(codex_home.path())?
        .args(["completion", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef better-codex"))
        .stdout(predicate::str::contains(
            "compdef _better-codex better-codex",
        ))
        .stdout(predicate::str::contains("#compdef codex").not());

    Ok(())
}
