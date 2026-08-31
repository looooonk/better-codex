use super::*;
use codex_app_server_protocol::PluginAuthPolicy;
use codex_app_server_protocol::PluginAvailability;
use codex_app_server_protocol::PluginInstallPolicy;
use codex_app_server_protocol::PluginSource;
use pretty_assertions::assert_eq;

fn plugin(name: &str, keywords: &[&str]) -> PluginSummary {
    PluginSummary {
        id: format!("{name}@example"),
        remote_plugin_id: None,
        version: None,
        local_version: None,
        name: name.to_string(),
        share_context: None,
        source: PluginSource::Remote,
        installed: false,
        enabled: true,
        install_policy: PluginInstallPolicy::Available,
        install_policy_source: None,
        auth_policy: PluginAuthPolicy::OnUse,
        availability: PluginAvailability::Available,
        interface: None,
        keywords: keywords
            .iter()
            .map(|keyword| (*keyword).to_string())
            .collect(),
    }
}

#[test]
fn search_text_normalization_and_match_ranking_are_stable() {
    let plugin = plugin("Issue-Tracker", &["project management", "tickets"]);

    assert_eq!(normalize_search_text("  Issue_tracker!!! "), "issue tracker");
    assert_eq!(plugin_search_match_rank(&plugin, "issue tracker"), Some(1));
    assert_eq!(plugin_search_match_rank(&plugin, "issue"), Some(2));
    assert_eq!(plugin_search_match_rank(&plugin, "tracker"), Some(3));
    assert_eq!(plugin_search_match_rank(&plugin, "tickets"), Some(4));
    assert_eq!(plugin_search_match_rank(&plugin, "management"), Some(5));
    assert_eq!(plugin_search_match_rank(&plugin, "calendar"), None);
}

#[test]
fn search_scope_separates_builtin_and_personal_marketplaces() {
    assert_eq!(
        [
            ("openai-curated", Some(PluginSearchScope::Global)),
            ("openai-bundled", Some(PluginSearchScope::Global)),
            ("my-marketplace", Some(PluginSearchScope::Personal)),
            ("my-marketplace", Some(PluginSearchScope::Global)),
            ("my-marketplace", Some(PluginSearchScope::Workspace)),
        ]
        .map(|(marketplace, scope)| marketplace_matches_search_scope(marketplace, scope)),
        [true, true, true, false, false]
    );
}

#[test]
fn search_results_never_claim_effective_activation() {
    let mut expected = plugin("calendar", &[]);
    expected.enabled = false;

    assert_eq!(
        plugin_search_result(
            plugin("calendar", &[]),
            "example".to_string(),
            /*marketplace_path*/ None,
        ),
        PluginSearchResult {
            plugin: expected,
            marketplace_name: "example".to_string(),
            marketplace_path: None,
        }
    );
}
