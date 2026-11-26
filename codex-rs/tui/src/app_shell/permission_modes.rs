use super::ShellState;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::AskForApproval;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::ActivePermissionProfile;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::SandboxPolicy;
use codex_utils_approval_presets::ApprovalPreset;
use codex_utils_approval_presets::builtin_approval_presets;
use std::path::Path;
use std::sync::LazyLock;

static PRESETS: LazyLock<Vec<ApprovalPreset>> = LazyLock::new(builtin_approval_presets);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PermissionMode {
    Ask,
    AutoReview,
    FullAccess,
    ReadOnly,
}

impl PermissionMode {
    pub(super) const ALL: [Self; 4] = [
        Self::Ask,
        Self::AutoReview,
        Self::FullAccess,
        Self::ReadOnly,
    ];

    pub(super) fn preset(self) -> &'static ApprovalPreset {
        let id = match self {
            Self::Ask | Self::AutoReview => "auto",
            Self::FullAccess => "full-access",
            Self::ReadOnly => "read-only",
        };
        PRESETS
            .iter()
            .find(|preset| preset.id == id)
            .expect("built-in approval preset")
    }

    pub(super) fn reviewer(self) -> ApprovalsReviewer {
        match self {
            Self::AutoReview => ApprovalsReviewer::AutoReview,
            _ => ApprovalsReviewer::User,
        }
    }

    pub(super) fn caption(self) -> &'static str {
        match self {
            Self::Ask => "ⓘ Ask for approval",
            Self::AutoReview => "⛨ Approve for me",
            Self::FullAccess => "⚠ Full access",
            Self::ReadOnly => "◈ Read only",
        }
    }

    pub(super) fn available(self, config: &Config) -> bool {
        let preset = self.preset();
        (self != Self::AutoReview || config.features.enabled(Feature::GuardianApproval))
            && config
                .permissions
                .approval_policy
                .can_set(&preset.approval)
                .is_ok()
            && config
                .config_layer_stack
                .requirements()
                .approvals_reviewer
                .can_set(&self.reviewer())
                .is_ok()
            && config.is_permission_profile_allowed(
                &preset.active_permission_profile.id,
                &preset.permission_profile,
            )
    }
}

// Keep the configured setup separate from the thread's mutable settings. Named
// profiles must be reapplied by ID to preserve their filesystem and network rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConfiguredPermissions {
    approval: AskForApproval,
    reviewer: ApprovalsReviewer,
    sandbox: ConfiguredSandbox,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfiguredSandbox {
    Named(String),
    Legacy(SandboxPolicy),
}

impl ConfiguredSandbox {
    fn profile_id(&self) -> Option<&str> {
        use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS;
        use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_READ_ONLY;
        use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;

        match self {
            Self::Named(id) => Some(id),
            Self::Legacy(SandboxPolicy::DangerFullAccess) => {
                Some(BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS)
            }
            Self::Legacy(policy) if policy == &SandboxPolicy::new_workspace_write_policy() => {
                Some(BUILT_IN_PERMISSION_PROFILE_WORKSPACE)
            }
            Self::Legacy(policy) if policy == &SandboxPolicy::new_read_only_policy() => {
                Some(BUILT_IN_PERMISSION_PROFILE_READ_ONLY)
            }
            Self::Legacy(_) => None,
        }
    }
}

impl ConfiguredPermissions {
    pub(super) fn from_config(config: &Config) -> Option<Self> {
        Self::from_profile(
            config.permissions.approval_policy.value().into(),
            config.approvals_reviewer,
            config.permissions.active_permission_profile().as_ref(),
            config.permissions.permission_profile(),
            config.cwd.as_path(),
        )
    }

    fn from_profile(
        approval: AskForApproval,
        reviewer: ApprovalsReviewer,
        active: Option<&ActivePermissionProfile>,
        profile: &PermissionProfile,
        cwd: &Path,
    ) -> Option<Self> {
        let sandbox = match active {
            Some(active) => ConfiguredSandbox::Named(active.id.clone()),
            None => ConfiguredSandbox::Legacy(profile.to_legacy_sandbox_policy(cwd).ok()?),
        };
        Some(Self {
            approval,
            reviewer,
            sandbox,
        })
    }

    fn preset(&self) -> Option<PermissionMode> {
        PermissionMode::ALL.into_iter().find(|mode| {
            let preset = mode.preset();
            self.approval == preset.approval.into()
                && self.reviewer == mode.reviewer()
                && self.sandbox.profile_id() == Some(preset.active_permission_profile.id.as_str())
        })
    }

    pub(super) fn update(&self, thread_id: ThreadId) -> ThreadSettingsUpdateParams {
        let mut params = ThreadSettingsUpdateParams {
            thread_id: thread_id.to_string(),
            approval_policy: Some(self.approval),
            approvals_reviewer: Some(self.reviewer.into()),
            ..Default::default()
        };
        match &self.sandbox {
            ConfiguredSandbox::Named(id) => params.permissions = Some(id.clone()),
            ConfiguredSandbox::Legacy(policy) => {
                params.sandbox_policy = Some(policy.clone().into())
            }
        }
        params
    }
}

impl ShellState {
    pub(super) fn active_permissions(&self) -> Option<ConfiguredPermissions> {
        ConfiguredPermissions::from_profile(
            self.approval_policy,
            self.approvals_reviewer,
            self.active_permission_profile.as_ref(),
            &self.permission_profile,
            Path::new(&self.cwd),
        )
    }

    pub(super) fn set_configured_permissions(&mut self, configured: Option<ConfiguredPermissions>) {
        self.settings
            .set_custom_permissions(configured.filter(|setup| setup.preset().is_none()));
    }

    pub(super) fn permission_mode(&self) -> Option<PermissionMode> {
        self.active_permissions()?.preset()
    }

    pub(super) fn custom_permissions_active(&self) -> bool {
        let Some(configured) = &self.settings.custom_permissions else {
            return false;
        };
        self.active_permissions().is_some_and(|active| {
            active.approval == configured.approval
                && active.reviewer == configured.reviewer
                && (active.sandbox == configured.sandbox
                    || active
                        .sandbox
                        .profile_id()
                        .is_some_and(|id| Some(id) == configured.sandbox.profile_id()))
        })
    }

    pub(super) fn permission_caption(&self) -> &str {
        self.permission_mode()
            .map(PermissionMode::caption)
            .unwrap_or("⚙ Custom")
    }
}

#[path = "permission_defaults.rs"]
mod defaults;
pub(super) use defaults::WorkspacePermissionDefaults;
