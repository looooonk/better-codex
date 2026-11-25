use super::*;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn removes_banned_allow_rules_once() {
    const BANNED_PREFIXES: &[&[&str]] = &[
        &["cmd.exe", "/k"],
        &["git"],
        &["pwsh", "-ec"],
        &["pwsh", "-f"],
    ];
    let codex_home = tempdir().expect("create codex home");
    let policy_path = codex_home.path().join("rules/default.rules");
    std::fs::create_dir_all(policy_path.parent().expect("rules directory"))
        .expect("create rules directory");
    std::fs::write(
        &policy_path,
        r#"prefix_rule(pattern=["git"], decision="allow")
prefix_rule(pattern=["git"], decision="prompt")
prefix_rule(pattern=["git"], decision="deny")
prefix_rule(pattern=["git", "status"], decision="allow")
prefix_rule(pattern=["CMD.EXE", "/K"], decision="allow")
prefix_rule(pattern=["PWSH", "-EC"], decision="allow")
prefix_rule(pattern=["PwSh", "-F"], decision="allow")
network_rule(host="api.github.com", protocol="https", decision="allow")
"#,
    )
    .expect("write legacy policy");

    prefix_rule_migration(codex_home.path(), &policy_path, BANNED_PREFIXES)
        .await
        .expect("run sandbox migration");
    assert_eq!(
        std::fs::read_to_string(&policy_path).expect("read migrated policy"),
        r#"prefix_rule(pattern=["git"], decision="prompt")
prefix_rule(pattern=["git"], decision="deny")
prefix_rule(pattern=["git", "status"], decision="allow")
network_rule(host="api.github.com", protocol="https", decision="allow")
"#
    );
    assert_eq!(
        std::fs::read_to_string(codex_home.path().join(MIGRATION_MARKER_FILENAME))
            .expect("read migration marker"),
        "v1\n"
    );

    let post_migration_policy = r#"prefix_rule(pattern=["git"], decision="allow")
"#;
    std::fs::write(&policy_path, post_migration_policy).expect("write post-migration policy");
    prefix_rule_migration(codex_home.path(), &policy_path, BANNED_PREFIXES)
        .await
        .expect("rerun sandbox migration");
    assert_eq!(
        std::fs::read_to_string(&policy_path).expect("read post-migration policy"),
        post_migration_policy
    );
}

#[tokio::test]
async fn inherited_legacy_exclusions_preserve_source_line_numbers() -> io::Result<()> {
    let home = tempdir()?;
    let path = home.path().join("default.rules");
    let contents = "prefix_rule(pattern=[\"git\"], decision=\"allow\")\ninvalid()\n";
    std::fs::write(&path, contents)?;
    assert_eq!(
        read_inherited_default_rules(home.path(), &path, &[&["git"]]).await?,
        "\ninvalid()\n"
    );
    assert_eq!(std::fs::read_to_string(path)?, contents);
    Ok(())
}
