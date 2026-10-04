//! Opportunistic attachment may fall back; automatic startup requires a shared server.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn audited_overrides_allow_daemon_without_allowing_arbitrary_config() {
    for (raw, eligible) in [
        ("features.transcript_v2=true", true),
        ("features.transcript_v2=false", true),
        ("features={transcript_v2=true}", true),
        ("features.transcript_v2='true'", false),
        ("features.worktrees=true", true),
        ("features.worktrees=false", true),
        ("features={worktrees=true}", true),
        (
            "features={worktrees=true,api_key_model_discovery=false}",
            true,
        ),
        ("features.auth_elicitation=false", true),
        ("features.code_mode_host=false", true),
        ("features.mcp_oauth_refresh_coordination=false", true),
        ("suppress_unstable_features_warning=true", true),
        ("suppress_unstable_features_warning='true'", false),
        ("features={worktrees=true,shell_tool=false}", false),
        ("features.shell_tool=false", false),
        ("features.worktrees.enabled=true", false),
        ("tui.fullscreen_transcript=true", true),
        ("tui.fullscreen_transcript=false", true),
        ("tui.fullscreen_transcript=\"true\"", false),
        ("tui={fullscreen_transcript=true}", true),
        ("tui={fullscreen_transcript='true'}", false),
        ("tui={fullscreen_transcript=true,animations=false}", false),
        ("features={}", false),
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
                /*bypass_hook_trust*/ false,
            )
            .is_none(),
            eligible,
            "{raw}"
        );
    }
}

#[test]
fn monorepo_wrapper_overrides_are_eligible_and_select_only_server_features() {
    let overrides = codex_utils_cli::CliConfigOverrides {
        raw_overrides: [
            "features.realtime_conversation=true",
            "features.worktrees=true",
            "features.remote_models=true",
            "features.api_key_model_discovery=true",
            "features.request_rule=true",
            "features.auth_elicitation=true",
            "features.mcp_oauth_refresh_coordination=true",
            "features.responses_websockets_v2=true",
            "features.workspace_owner_usage_nudge=true",
            "features.tool_search_always_defer_mcp_tools=true",
            "features.remote_compaction_v2=true",
            "features.standalone_web_search=true",
            "features.multi_agent_mode=true",
            "features.code_mode_host=true",
            "suppress_unstable_features_warning=true",
        ]
        .map(str::to_string)
        .to_vec(),
    }
    .parse_overrides()
    .unwrap();
    assert_eq!(
        config_exclusion(
            &overrides,
            &LoaderOverrides::default(),
            /*strict_config*/ false,
            /*bypass_hook_trust*/ false,
        ),
        None
    );
    assert_eq!(
        server_features(&overrides),
        std::collections::BTreeMap::from([
            ("api_key_model_discovery".to_string(), true),
            ("auth_elicitation".to_string(), true),
            ("code_mode_host".to_string(), true),
            ("mcp_oauth_refresh_coordination".to_string(), true),
        ])
    );
}

#[test]
fn daemon_features_follow_cli_table_replacement_and_last_value() {
    let overrides = codex_utils_cli::CliConfigOverrides {
        raw_overrides: [
            "features.api_key_model_discovery=true",
            "features={code_mode_host=false}",
            "features.code_mode_host=true",
        ]
        .map(str::to_string)
        .to_vec(),
    }
    .parse_overrides()
    .unwrap();
    assert_eq!(
        server_features(&overrides),
        std::collections::BTreeMap::from([("code_mode_host".to_string(), true),])
    );
}

#[test]
fn local_overrides_and_executor_selection_keep_independent_sessions() {
    use clap::Parser;

    for (args, workload, executor, expected) in [
        (vec!["better-codex"], false, None, None),
        (
            vec!["better-codex", "--no-daemon"],
            false,
            None,
            Some("--no-daemon"),
        ),
        (vec!["better-codex", "--oss"], false, None, Some("--oss")),
        (vec!["better-codex"], true, None, Some("workload identity")),
        (
            vec!["better-codex"],
            false,
            Some(std::ffi::OsStr::new("unix://exec")),
            Some("executor selection (CODEX_EXEC_SERVER_URL)"),
        ),
    ] {
        let cli = Cli::parse_from(args);
        assert_eq!(
            exclusion(&cli, &[], &LoaderOverrides::default(), workload, executor),
            expected
        );
    }
}
