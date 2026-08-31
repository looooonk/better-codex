use super::ManagedAuthPolicy;
use codex_protocol::config_types::ForcedLoginMethod;
use pretty_assertions::assert_eq;

#[test]
fn default_policy_preserves_unrestricted_auth_behavior() {
    let policy = ManagedAuthPolicy::default();

    assert_eq!(
        policy.allowed_login_methods(),
        vec![ForcedLoginMethod::Api, ForcedLoginMethod::Chatgpt]
    );
    assert_eq!(policy.allowed_chatgpt_workspaces(), None);
    assert!(policy.is_chatgpt_workspace_allowed("any-workspace"));
}

#[test]
fn repeated_restrictions_only_narrow_the_policy() {
    let policy = ManagedAuthPolicy::default()
        .restrict_login_methods_to([ForcedLoginMethod::Api, ForcedLoginMethod::Chatgpt])
        .restrict_login_methods_to([ForcedLoginMethod::Chatgpt])
        .restrict_chatgpt_workspaces_to([
            " workspace-a ".to_string(),
            "workspace-b".to_string(),
            "workspace-a".to_string(),
        ])
        .restrict_chatgpt_workspaces_to(["workspace-a".to_string(), "workspace-c".to_string()]);

    assert_eq!(
        policy.allowed_login_methods(),
        vec![ForcedLoginMethod::Chatgpt]
    );
    assert_eq!(
        policy.allowed_chatgpt_workspaces(),
        Some(["workspace-a".to_string()].as_slice())
    );
    assert!(policy.is_chatgpt_workspace_allowed("workspace-a"));
    assert!(!policy.is_chatgpt_workspace_allowed("workspace-b"));
}

#[test]
fn empty_workspace_intersection_disables_chatgpt_login() {
    let local = ManagedAuthPolicy::default()
        .restrict_login_methods_to([ForcedLoginMethod::Chatgpt])
        .restrict_chatgpt_workspaces_to(["workspace-a".to_string()]);
    let other =
        ManagedAuthPolicy::default().restrict_chatgpt_workspaces_to(["workspace-b".to_string()]);

    let policy = local.intersect(&other);

    assert_eq!(policy.allowed_login_methods(), Vec::new());
    assert_eq!(policy.allowed_chatgpt_workspaces(), Some([].as_slice()));
}
