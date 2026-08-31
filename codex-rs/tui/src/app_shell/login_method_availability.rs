use codex_login::AuthConfig;
use codex_protocol::config_types::ForcedLoginMethod;

/// Effective login methods that an authentication surface may offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LoginMethodAvailability {
    All,
    ChatGptOnly,
    ApiOnly,
    None,
}

impl LoginMethodAvailability {
    pub(super) fn from_auth_config(auth_config: &AuthConfig) -> Self {
        match (
            auth_config.is_login_method_allowed(ForcedLoginMethod::Chatgpt),
            auth_config.is_login_method_allowed(ForcedLoginMethod::Api),
        ) {
            (true, true) => Self::All,
            (true, false) => Self::ChatGptOnly,
            (false, true) => Self::ApiOnly,
            (false, false) => Self::None,
        }
    }

    /// A connected app server owns authentication policy for its remote workspace.
    pub(super) fn connected_workspace() -> Self {
        Self::All
    }

    pub(super) fn allows_chatgpt(self) -> bool {
        matches!(self, Self::All | Self::ChatGptOnly)
    }

    pub(super) fn allows_api(self) -> bool {
        matches!(self, Self::All | Self::ApiOnly)
    }
}

#[cfg(test)]
#[path = "login_method_availability_tests.rs"]
mod tests;
