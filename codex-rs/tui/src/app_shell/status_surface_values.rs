use super::ShellState;
use super::status_surface_items::StatusLineItem;
use super::status_surface_items::TerminalTitleItem;
use crate::status::format_directory_display;
use crate::status::format_tokens_compact;
use codex_app_server_protocol::RateLimitSnapshot;
use codex_app_server_protocol::RateLimitWindow;
use codex_app_server_protocol::TurnPlanStepStatus;
use codex_protocol::models::PermissionProfile;
use std::path::Path;

impl ShellState {
    pub(super) fn status_surface_value(&self, item: StatusLineItem) -> Option<String> {
        let value = match item {
            StatusLineItem::ModelName => self.model.clone(),
            StatusLineItem::ModelWithReasoning => format!(
                "{} {}",
                self.model,
                self.status_surface_value(StatusLineItem::Reasoning)?
            ),
            StatusLineItem::Reasoning => self
                .reasoning_effort
                .as_ref()
                .map(super::settings::reasoning_effort_label)
                .unwrap_or_else(|| "default".into()),
            StatusLineItem::CurrentDir => {
                format_directory_display(Path::new(&self.cwd), /*max_width*/ None)
            }
            StatusLineItem::ProjectRoot => self
                .workspace_git_status
                .as_ref()?
                .git_root
                .as_ref()?
                .file_name()?
                .to_string_lossy()
                .into_owned(),
            StatusLineItem::Hostname => codex_config::os_host_name()?,
            StatusLineItem::GitBranch => self
                .status_surfaces
                .metadata
                .for_cwd(&self.cwd)
                .and_then(|metadata| metadata.branch.clone())
                .or_else(|| self.workspace_git_status.as_ref()?.branch.clone())?,
            StatusLineItem::PullRequestNumber => format!(
                "PR #{}",
                self.status_surfaces
                    .metadata
                    .for_cwd(&self.cwd)?
                    .git
                    .pull_request
                    .as_ref()?
                    .number
            ),
            StatusLineItem::BranchChanges => {
                let stats = self
                    .status_surfaces
                    .metadata
                    .for_cwd(&self.cwd)?
                    .git
                    .branch_change_stats
                    .as_ref()?;
                if stats.additions == 0 && stats.deletions == 0 {
                    "No changes".into()
                } else {
                    format!("+{} -{}", stats.additions, stats.deletions)
                }
            }
            StatusLineItem::Status => self.header_status().into_owned(),
            StatusLineItem::Permissions => self
                .active_permission_profile
                .as_ref()
                .map(|profile| profile.id.clone())
                .unwrap_or_else(|| {
                    match &self.permission_profile {
                        PermissionProfile::Disabled => "full access",
                        PermissionProfile::External { .. } => "external sandbox",
                        PermissionProfile::Managed { .. } => "sandboxed",
                    }
                    .into()
                }),
            StatusLineItem::ApprovalMode => {
                super::settings::approval_policy_label(self.approval_policy).into()
            }
            StatusLineItem::ContextRemaining => format!(
                "Context {}% left",
                100 - super::dashboard::context_used_percent(
                    &self.context_token_usage,
                    self.model_context_window
                )?
            ),
            StatusLineItem::ContextUsed => format!(
                "Context {}% used",
                super::dashboard::context_used_percent(
                    &self.context_token_usage,
                    self.model_context_window
                )?
            ),
            StatusLineItem::FiveHourLimit | StatusLineItem::WeeklyLimit => {
                let snapshot = self
                    .rate_limits
                    .iter()
                    .find(|limit| limit.limit_id.as_deref().is_none_or(|id| id == "codex"))?;
                let window = quota_window(snapshot, item)?;
                let label = match window.window_duration_mins {
                    Some(300) => "5h".into(),
                    Some(10080) => "weekly".into(),
                    Some(minutes) if minutes > 0 => format!("{minutes}m"),
                    _ => if item == StatusLineItem::FiveHourLimit {
                        "primary"
                    } else {
                        "secondary"
                    }
                    .into(),
                };
                format!("{label} {}% left", 100 - window.used_percent.clamp(0, 100))
            }
            StatusLineItem::CodexVersion => crate::version::CODEX_CLI_VERSION.into(),
            StatusLineItem::ContextWindowSize => format!(
                "{} window",
                format_tokens_compact(self.model_context_window?)
            ),
            StatusLineItem::UsedTokens => {
                let total = self
                    .token_usage
                    .input_tokens
                    .saturating_sub(self.token_usage.cached_input_tokens.max(0))
                    .max(0)
                    .saturating_add(self.token_usage.output_tokens.max(0));
                if total == 0 {
                    return None;
                }
                format!("{} used", format_tokens_compact(total))
            }
            StatusLineItem::TotalInputTokens => format!(
                "{} in",
                format_tokens_compact(self.token_usage.input_tokens)
            ),
            StatusLineItem::TotalOutputTokens => format!(
                "{} out",
                format_tokens_compact(self.token_usage.output_tokens)
            ),
            StatusLineItem::ThreadCredits => format!(
                "{} credits",
                format_micros(self.thread_usage.as_ref()?.estimated_usage_credits_micros)
            ),
            StatusLineItem::EstimatedThreadCost => format!(
                "~${}",
                format_micros(self.thread_usage.as_ref()?.estimated_usage_usd_micros?)
            ),
            StatusLineItem::SessionId => self.thread_id.to_string(),
            StatusLineItem::FastMode => {
                if !self
                    .available_models
                    .iter()
                    .find(|model| model.model == self.model)
                    .is_some_and(codex_protocol::openai_models::ModelPreset::supports_fast_mode)
                {
                    return None;
                }
                if matches!(self.service_tier.as_deref(), Some("priority" | "fast")) {
                    "Fast on"
                } else {
                    "Fast off"
                }
                .into()
            }
            StatusLineItem::Daybreak => if self.daybreak_enabled && self.side_parent.is_none() {
                "Daybreak on"
            } else {
                "Daybreak off"
            }
            .into(),
            StatusLineItem::RawOutput => return None,
            StatusLineItem::ThreadName => self
                .thread_name
                .as_ref()
                .filter(|name| !name.trim().is_empty())?
                .trim()
                .into(),
            StatusLineItem::ThreadTitle => self
                .thread_name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| self.thread_id.to_string()),
            StatusLineItem::WorkspaceHeadline => self
                .status_surfaces
                .metadata
                .for_cwd(&self.cwd)?
                .headline
                .clone()?,
            StatusLineItem::TaskProgress => {
                if self.plan_steps.is_empty() {
                    return None;
                }
                let done = self
                    .plan_steps
                    .iter()
                    .filter(|step| matches!(step.status, TurnPlanStepStatus::Completed))
                    .count();
                format!("{done}/{} tasks", self.plan_steps.len())
            }
        };
        Some(value)
    }

    pub(super) fn terminal_title_value(&self, item: TerminalTitleItem) -> Option<String> {
        let status_item = match item {
            TerminalTitleItem::AppName => return Some("Better Codex".into()),
            TerminalTitleItem::Project => {
                return self
                    .status_surface_value(StatusLineItem::ProjectRoot)
                    .or_else(|| {
                        Path::new(&self.cwd)
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                    });
            }
            TerminalTitleItem::Spinner => {
                return if self.pending_approval.is_some()
                    || self.pending_user_input.is_some()
                    || self.pending_elicitation.is_some()
                {
                    Some("[ ! ] Action Required".into())
                } else if self.active_turn_id.is_some() {
                    Some(
                        if self.animations {
                            ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
                                [self.status_spinner_frame % 10]
                        } else {
                            "Working"
                        }
                        .into(),
                    )
                } else {
                    None
                };
            }
            TerminalTitleItem::CurrentDir => StatusLineItem::CurrentDir,
            TerminalTitleItem::Status => StatusLineItem::Status,
            TerminalTitleItem::ThreadName => StatusLineItem::ThreadName,
            TerminalTitleItem::Thread => StatusLineItem::ThreadTitle,
            TerminalTitleItem::GitBranch => StatusLineItem::GitBranch,
            TerminalTitleItem::ContextRemaining => StatusLineItem::ContextRemaining,
            TerminalTitleItem::ContextUsed => StatusLineItem::ContextUsed,
            TerminalTitleItem::FiveHourLimit => StatusLineItem::FiveHourLimit,
            TerminalTitleItem::WeeklyLimit => StatusLineItem::WeeklyLimit,
            TerminalTitleItem::CodexVersion => StatusLineItem::CodexVersion,
            TerminalTitleItem::UsedTokens => StatusLineItem::UsedTokens,
            TerminalTitleItem::TotalInputTokens => StatusLineItem::TotalInputTokens,
            TerminalTitleItem::TotalOutputTokens => StatusLineItem::TotalOutputTokens,
            TerminalTitleItem::ThreadCredits => StatusLineItem::ThreadCredits,
            TerminalTitleItem::EstimatedThreadCost => StatusLineItem::EstimatedThreadCost,
            TerminalTitleItem::SessionId => StatusLineItem::SessionId,
            TerminalTitleItem::FastMode => StatusLineItem::FastMode,
            TerminalTitleItem::Daybreak => StatusLineItem::Daybreak,
            TerminalTitleItem::Model => StatusLineItem::ModelName,
            TerminalTitleItem::ModelWithReasoning => StatusLineItem::ModelWithReasoning,
            TerminalTitleItem::Reasoning => StatusLineItem::Reasoning,
            TerminalTitleItem::TaskProgress => StatusLineItem::TaskProgress,
        };
        self.status_surface_value(status_item)
    }
}

fn quota_window(snapshot: &RateLimitSnapshot, item: StatusLineItem) -> Option<&RateLimitWindow> {
    let primary = snapshot.primary.as_ref();
    let secondary = snapshot.secondary.as_ref();
    match item {
        StatusLineItem::FiveHourLimit => primary
            .filter(|window| window.window_duration_mins != Some(10080))
            .or_else(|| secondary.filter(|window| window.window_duration_mins != Some(10080))),
        StatusLineItem::WeeklyLimit => primary
            .filter(|window| window.window_duration_mins == Some(10080))
            .or(secondary),
        _ => None,
    }
}

fn format_micros(value: i64) -> String {
    if value <= 0 {
        "0".into()
    } else {
        super::thread_usage::format_positive_micros(value)
    }
}
