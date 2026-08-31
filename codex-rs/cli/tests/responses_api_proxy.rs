use std::path::Path;

use anyhow::Result;
use predicates::str::contains;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[test]
fn responses_api_proxy_rejects_chatgpt_only_policy_before_stdin_or_binding() -> Result<()> {
    let codex_home = TempDir::new()?;
    let server_info = codex_home.path().join("proxy.json");

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "-c",
        "forced_login_method=\"chatgpt\"",
        "responses-api-proxy",
        "--server-info",
        server_info.to_str().expect("UTF-8 server info path"),
    ])
    .assert()
    .failure()
    .stderr(contains(
        "responses-api-proxy requires API key login, which is disabled by authentication policy",
    ));

    assert!(!server_info.exists());
    Ok(())
}
