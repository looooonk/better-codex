use codex_protocol::mcp::Resource;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::MAX_HIDDEN_ORCHESTRATOR_SKILLS;
use super::MAX_ORCHESTRATOR_SKILLS;
use super::MAX_RESOURCE_PAGES;
use super::ORCHESTRATOR_SKILL_MIME_TYPE;
use super::OrchestratorSkillDiscovery;
use super::catalog_entry_from_resource;

#[test]
fn explicit_only_metadata_hides_orchestrator_skill_from_prompts() {
    let explicit_only = catalog_entry_from_resource(&resource(Some(json!({
        "skill_name": "deploy",
        "plugin_name": "cloud",
        "allow_implicit_invocation": false
    }))))
    .expect("valid explicit-only skill");
    let missing_policy = catalog_entry_from_resource(&resource(Some(json!({
        "skill_name": "deploy",
        "plugin_name": "cloud"
    }))))
    .expect("valid default skill");
    let invalid_policy = catalog_entry_from_resource(&resource(Some(json!({
        "skill_name": "deploy",
        "plugin_name": "cloud",
        "allow_implicit_invocation": "false"
    }))))
    .expect("valid skill with ignored policy type");

    assert_eq!(
        [
            explicit_only.prompt_visible,
            missing_policy.prompt_visible,
            invalid_policy.prompt_visible,
        ],
        [false, true, true]
    );
}

#[test]
fn hidden_pages_do_not_consume_the_visible_skill_budget() {
    let mut discovery = OrchestratorSkillDiscovery::default();
    let first_page = (0..=MAX_ORCHESTRATOR_SKILLS)
        .map(|index| skill_resource(format!("hidden-{index}"), false))
        .collect::<Vec<_>>();
    discovery.record_page(&first_page);
    discovery.record_page(&[skill_resource("visible-later".to_string(), true)]);

    let hidden_overflow = (MAX_ORCHESTRATOR_SKILLS + 1..=MAX_HIDDEN_ORCHESTRATOR_SKILLS)
        .map(|index| skill_resource(format!("hidden-{index}"), false))
        .collect::<Vec<_>>();
    discovery.record_page(&hidden_overflow);
    while discovery.has_page_capacity() {
        discovery.record_page(&[]);
    }

    let visible_names = discovery
        .catalog
        .entries
        .iter()
        .filter(|entry| entry.prompt_visible)
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        (
            visible_names,
            discovery.visible_skills_seen,
            discovery.hidden_skills_seen,
            discovery.catalog.entries.len(),
            discovery.completed_pages,
            discovery.truncated,
            discovery.has_page_capacity(),
        ),
        (
            vec!["cloud:visible-later"],
            1,
            MAX_HIDDEN_ORCHESTRATOR_SKILLS,
            MAX_HIDDEN_ORCHESTRATOR_SKILLS + 1,
            MAX_RESOURCE_PAGES,
            true,
            false,
        )
    );
}

fn resource(meta: Option<serde_json::Value>) -> Resource {
    Resource {
        annotations: None,
        description: Some("Deploy the service.".to_string()),
        mime_type: Some(ORCHESTRATOR_SKILL_MIME_TYPE.to_string()),
        name: "deploy".to_string(),
        size: None,
        title: None,
        uri: "skill://cloud/deploy".to_string(),
        icons: None,
        meta,
    }
}

fn skill_resource(skill_name: String, allow_implicit_invocation: bool) -> Resource {
    Resource {
        annotations: None,
        description: Some("Test skill.".to_string()),
        mime_type: Some(ORCHESTRATOR_SKILL_MIME_TYPE.to_string()),
        name: skill_name.clone(),
        size: None,
        title: None,
        uri: format!("skill://cloud/{skill_name}"),
        icons: None,
        meta: Some(json!({
            "skill_name": skill_name,
            "plugin_name": "cloud",
            "allow_implicit_invocation": allow_implicit_invocation
        })),
    }
}
