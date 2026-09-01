use std::collections::HashSet;

use pretty_assertions::assert_eq;

use super::*;

fn set<'a>(items: &'a [&'a str]) -> HashSet<&'a str> {
    items.iter().copied().collect()
}

fn assert_mentions(text: &str, expected_names: &[&str], expected_paths: &[&str]) {
    let mentions = extract_tool_mentions(text);
    assert_eq!(mentions.names, set(expected_names));
    assert_eq!(mentions.paths, set(expected_paths));
}

#[test]
fn handles_plain_and_linked_mentions() {
    assert_mentions(
        "use $alpha and [$beta](/tmp/beta)",
        &["alpha", "beta"],
        &["/tmp/beta"],
    );
}

#[test]
fn skips_common_env_vars() {
    assert_mentions("use $PATH and $alpha", &["alpha"], &[]);
    assert_mentions("use [$HOME](/tmp/skill)", &[], &[]);
    assert_mentions("use $XDG_CONFIG_HOME and $beta", &["beta"], &[]);
}

#[test]
fn distinguishes_tool_resource_kinds() {
    assert_eq!(
        [
            tool_kind_for_path("app://calendar"),
            tool_kind_for_path("mcp://server"),
            tool_kind_for_path("plugin://demo"),
            tool_kind_for_path("skill://demo/SKILL.md"),
            tool_kind_for_path("/tmp/demo/SKILL.md"),
        ],
        [
            ToolMentionKind::App,
            ToolMentionKind::Mcp,
            ToolMentionKind::Plugin,
            ToolMentionKind::Skill,
            ToolMentionKind::Skill,
        ]
    );
}

#[test]
fn keeps_namespaces_and_stops_at_non_name_chars() {
    assert_mentions(
        "use $slack:search, $alpha.skill and $beta_extra",
        &["alpha", "beta_extra", "slack:search"],
        &[],
    );
}

#[test]
fn handles_many_sigils_without_looping() {
    let prefix = "$".repeat(256);
    assert_mentions(&format!("{prefix} not-a-mention"), &[], &[]);
}
