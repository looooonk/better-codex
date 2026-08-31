use codex_protocol::mcp::Resource;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::ORCHESTRATOR_SKILL_MIME_TYPE;
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
