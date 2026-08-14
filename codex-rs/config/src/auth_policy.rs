use codex_protocol::config_types::ForcedLoginMethod;

const LOGIN_METHODS: [ForcedLoginMethod; 2] =
    [ForcedLoginMethod::Api, ForcedLoginMethod::Chatgpt];

/// Authentication restrictions supplied by locally managed requirements.
///
/// The default policy is unrestricted for compatibility with installations
/// that do not define managed authentication requirements. Each `restrict_*`
/// method intersects its input with the current policy, so composing policies
/// can only remove access.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedAuthPolicy {
    login_methods: Allowlist<ForcedLoginMethod>,
    chatgpt_workspaces: Allowlist<String>,
}

impl ManagedAuthPolicy {
    /// Narrows this policy to the supplied login methods.
    #[must_use]
    pub fn restrict_login_methods_to(
        mut self,
        allowed: impl IntoIterator<Item = ForcedLoginMethod>,
    ) -> Self {
        self.login_methods
            .restrict_to(unique_values(allowed.into_iter()));
        self
    }

    /// Narrows this policy to the supplied ChatGPT workspace IDs.
    ///
    /// IDs are trimmed and blank entries are discarded before intersection.
    #[must_use]
    pub fn restrict_chatgpt_workspaces_to(
        mut self,
        allowed: impl IntoIterator<Item = String>,
    ) -> Self {
        let allowed = allowed
            .into_iter()
            .filter_map(|workspace| {
                let workspace = workspace.trim();
                (!workspace.is_empty()).then(|| workspace.to_string())
            });
        self.chatgpt_workspaces
            .restrict_to(unique_values(allowed));
        self
    }

    /// Returns the intersection of this policy and another restriction set.
    #[must_use]
    pub fn intersect(mut self, other: &Self) -> Self {
        self.login_methods.intersect(&other.login_methods);
        self.chatgpt_workspaces
            .intersect(&other.chatgpt_workspaces);
        self
    }

    /// Returns whether the policy permits the requested login method.
    pub fn is_login_method_allowed(&self, method: ForcedLoginMethod) -> bool {
        self.login_methods
            .restricted_values()
            .is_none_or(|allowed| allowed.contains(&method))
            && (method != ForcedLoginMethod::Chatgpt
                || self
                    .chatgpt_workspaces
                    .restricted_values()
                    .is_none_or(|allowed| !allowed.is_empty()))
    }

    /// Returns every login method currently permitted by the policy.
    pub fn allowed_login_methods(&self) -> Vec<ForcedLoginMethod> {
        LOGIN_METHODS
            .into_iter()
            .filter(|method| self.is_login_method_allowed(*method))
            .collect()
    }

    /// Returns the managed ChatGPT workspace allowlist, if one is configured.
    pub fn allowed_chatgpt_workspaces(&self) -> Option<&[String]> {
        self.chatgpt_workspaces.restricted_values()
    }

    /// Returns whether the policy permits the requested ChatGPT workspace.
    pub fn is_chatgpt_workspace_allowed(&self, workspace: &str) -> bool {
        self.chatgpt_workspaces
            .restricted_values()
            .is_none_or(|allowed| allowed.iter().any(|candidate| candidate == workspace))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Allowlist<T> {
    #[default]
    Unrestricted,
    Restricted(Vec<T>),
}

impl<T: Clone + PartialEq> Allowlist<T> {
    fn restrict_to(&mut self, allowed: Vec<T>) {
        match self {
            Self::Unrestricted => *self = Self::Restricted(allowed),
            Self::Restricted(existing) => {
                existing.retain(|value| allowed.contains(value));
            }
        }
    }

    fn intersect(&mut self, other: &Self) {
        if let Self::Restricted(allowed) = other {
            self.restrict_to(allowed.clone());
        }
    }

    fn restricted_values(&self) -> Option<&[T]> {
        match self {
            Self::Unrestricted => None,
            Self::Restricted(allowed) => Some(allowed),
        }
    }

}

fn unique_values<T: PartialEq>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut unique = Vec::new();
    for value in values {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique
}

#[cfg(test)]
#[path = "auth_policy_tests.rs"]
mod tests;
