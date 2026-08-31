use super::*;
use codex_config::ManagedAuthPolicy;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn auth_config(managed_auth_policy: ManagedAuthPolicy) -> AuthConfig {
    AuthConfig {
        codex_home: PathBuf::from("codex-home"),
        auth_credentials_store_mode: AuthCredentialsStoreMode::File,
        keyring_backend_kind: AuthKeyringBackendKind::default(),
        forced_login_method: None,
        chatgpt_base_url: None,
        forced_chatgpt_workspace_id: None,
        managed_auth_policy,
        auth_route_config: None,
    }
}

#[test]
fn reflects_effective_managed_login_methods() {
    let unrestricted = auth_config(ManagedAuthPolicy::default());
    let chatgpt_only = auth_config(
        ManagedAuthPolicy::default().restrict_login_methods_to([ForcedLoginMethod::Chatgpt]),
    );
    let api_only = auth_config(
        ManagedAuthPolicy::default().restrict_login_methods_to([ForcedLoginMethod::Api]),
    );
    let none = auth_config(ManagedAuthPolicy::default().restrict_login_methods_to([]));

    assert_eq!(
        [
            LoginMethodAvailability::from_auth_config(&unrestricted),
            LoginMethodAvailability::from_auth_config(&chatgpt_only),
            LoginMethodAvailability::from_auth_config(&api_only),
            LoginMethodAvailability::from_auth_config(&none),
        ],
        [
            LoginMethodAvailability::All,
            LoginMethodAvailability::ChatGptOnly,
            LoginMethodAvailability::ApiOnly,
            LoginMethodAvailability::None,
        ]
    );
}

#[test]
fn connected_workspace_leaves_policy_to_the_remote_app_server() {
    assert_eq!(
        LoginMethodAvailability::connected_workspace(),
        LoginMethodAvailability::All
    );
}
