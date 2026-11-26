use super::ShellState;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::AskForApproval;
use codex_features::Feature;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::PermissionProfile;
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

    pub(super) fn description(self) -> &'static str {
        match self {
            Self::Ask => "Edit the workspace. Ask you for broader access.",
            Self::AutoReview => "Edit the workspace. Send requests to automatic review.",
            Self::FullAccess => "Allow unrestricted file and network access.",
            Self::ReadOnly => "Read files. Ask you before edits or network access.",
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

    fn matches(self, shell: &ShellState) -> bool {
        let preset = self.preset();
        if shell.approval_policy != AskForApproval::from(preset.approval)
            || shell.approvals_reviewer != self.reviewer()
        {
            return false;
        }
        if let Some(active) = &shell.active_permission_profile {
            return active.id == preset.active_permission_profile.id;
        }
        // Older sessions can have a concrete sandbox without a named profile.
        let profile = &shell.permission_profile;
        if profile == &preset.permission_profile {
            return true;
        }
        let cwd = Path::new(&shell.cwd);
        let fs = profile.file_system_sandbox_policy();
        match self {
            Self::FullAccess => matches!(profile, PermissionProfile::Disabled),
            Self::ReadOnly => {
                matches!(profile, PermissionProfile::Managed { .. })
                    && !fs.has_full_disk_write_access()
                    && fs.get_writable_roots_with_cwd(cwd).is_empty()
                    && profile.network_sandbox_policy()
                        == preset.permission_profile.network_sandbox_policy()
            }
            Self::Ask | Self::AutoReview => {
                matches!(profile, PermissionProfile::Managed { .. })
                    && !fs.has_full_disk_write_access()
                    && fs.can_write_local_path_with_cwd(cwd, cwd)
            }
        }
    }
}

impl ShellState {
    pub(super) fn permission_mode(&self) -> Option<PermissionMode> {
        PermissionMode::ALL
            .into_iter()
            .find(|mode| mode.matches(self))
    }

    pub(super) fn permission_caption(&self) -> &str {
        self.permission_mode()
            .map(PermissionMode::caption)
            .unwrap_or("⚙ Custom")
    }
}
