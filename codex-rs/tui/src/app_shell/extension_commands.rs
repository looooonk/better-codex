use super::workspace_requests::workspace_request_id;
use crate::config_update::replace_config_value;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::*;
use color_eyre::Result;
use color_eyre::eyre::bail;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExtensionCommand {
    Skill {
        name: String,
        enabled: bool,
    },
    Hook {
        cwd: PathBuf,
        key: String,
        change: HookChange,
    },
    Usage {
        reset: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum HookChange {
    Enabled(bool),
    Trust(String),
}

pub(super) fn skill_command(args: &str) -> Result<ExtensionCommand> {
    let Some((name, enabled)) = args.rsplit_once(' ') else {
        bail!("usage: /skills [name on|off]");
    };
    let enabled = match enabled {
        "on" => true,
        "off" => false,
        _ => bail!("usage: /skills [name on|off]"),
    };
    if name.is_empty() {
        bail!("provide a skill name");
    }
    Ok(ExtensionCommand::Skill {
        name: name.to_string(),
        enabled,
    })
}

pub(super) fn hook_command(cwd: PathBuf, args: &str) -> Result<ExtensionCommand> {
    let (key, change) = match args.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["on", key] => ((*key).to_string(), HookChange::Enabled(true)),
        ["off", key] => ((*key).to_string(), HookChange::Enabled(false)),
        ["trust", key, hash] => ((*key).to_string(), HookChange::Trust((*hash).to_string())),
        _ => bail!("usage: /hooks [on|off <key>|trust <key> <reviewed hash>]"),
    };
    Ok(ExtensionCommand::Hook { cwd, key, change })
}

pub(super) async fn execute(
    client: AppServerRequestHandle,
    command: ExtensionCommand,
) -> Result<String> {
    match command {
        ExtensionCommand::Skill { name, enabled } => {
            let response: SkillsConfigWriteResponse = client
                .request_typed(ClientRequest::SkillsConfigWrite {
                    request_id: workspace_request_id("skill-config"),
                    params: SkillsConfigWriteParams {
                        name: Some(name.clone()),
                        path: None,
                        enabled,
                    },
                })
                .await?;
            let state = if response.effective_enabled {
                "enabled"
            } else {
                "disabled"
            };
            Ok(format!("{name} {state}"))
        }
        ExtensionCommand::Hook { cwd, key, change } => {
            let response: HooksListResponse = client
                .request_typed(ClientRequest::HooksList {
                    request_id: workspace_request_id("hook-config"),
                    params: HooksListParams { cwds: vec![cwd] },
                })
                .await?;
            let Some(hook) = response
                .data
                .into_iter()
                .flat_map(|entry| entry.hooks)
                .find(|hook| hook.key == key)
            else {
                bail!("hook not found; use /hooks to refresh the list");
            };
            if hook.is_managed {
                bail!("this hook is managed by your administrator");
            }
            let value = match change {
                HookChange::Enabled(enabled) => serde_json::json!({ "enabled": enabled }),
                HookChange::Trust(hash) => {
                    if hash != hook.current_hash {
                        bail!("hook changed; review its current contents and hash using /hooks");
                    }
                    serde_json::json!({ "trusted_hash": hash })
                }
            };
            let mut edit =
                replace_config_value("hooks.state", serde_json::json!({ key.clone(): value }));
            edit.merge_strategy = MergeStrategy::Upsert;
            let _: ConfigWriteResponse = client
                .request_typed(ClientRequest::ConfigBatchWrite {
                    request_id: workspace_request_id("hook-write"),
                    params: ConfigBatchWriteParams {
                        edits: vec![edit],
                        file_path: None,
                        expected_version: None,
                        reload_user_config: true,
                    },
                })
                .await?;
            Ok(format!("Updated hook {key}"))
        }
        ExtensionCommand::Usage { reset: true } => {
            let response: ConsumeAccountRateLimitResetCreditResponse = client
                .request_typed(ClientRequest::ConsumeAccountRateLimitResetCredit {
                    request_id: workspace_request_id("usage-reset"),
                    params: ConsumeAccountRateLimitResetCreditParams {
                        idempotency_key: uuid::Uuid::new_v4().to_string(),
                        credit_id: None,
                    },
                })
                .await?;
            Ok(match response.outcome {
                ConsumeAccountRateLimitResetCreditOutcome::Reset => "Usage limit reset applied.",
                ConsumeAccountRateLimitResetCreditOutcome::AlreadyRedeemed => {
                    "This reset was already applied."
                }
                ConsumeAccountRateLimitResetCreditOutcome::NoCredit => {
                    "No usage resets are available."
                }
                ConsumeAccountRateLimitResetCreditOutcome::NothingToReset => {
                    "No usage limits currently need resetting."
                }
            }
            .to_string())
        }
        ExtensionCommand::Usage { reset: false } => {
            let response: GetAccountRateLimitsResponse = client
                .request_typed(ClientRequest::GetAccountRateLimits {
                    request_id: workspace_request_id("usage"),
                    params: None,
                })
                .await?;
            let mut lines = vec!["Account usage".to_string()];
            let limits = response
                .rate_limits_by_limit_id
                .unwrap_or_else(|| [("codex".to_string(), response.rate_limits)].into());
            let mut limits = limits.into_iter().collect::<Vec<_>>();
            limits.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, limits) in limits {
                for (label, window) in
                    [("primary", limits.primary), ("secondary", limits.secondary)]
                {
                    if let Some(window) = window {
                        lines.push(format!(
                            "{name} {label}: {}% remaining",
                            100 - window.used_percent.clamp(0, 100)
                        ));
                    }
                }
            }
            if let Some(credits) = response.rate_limit_reset_credits {
                lines.push(format!(
                    "{} usage resets available",
                    credits.available_count
                ));
                if credits.available_count > 0 {
                    lines.push("To spend one reset, enter /usage reset confirm.".to_string());
                }
            }
            Ok(lines.join("\n"))
        }
    }
}

#[cfg(test)]
#[path = "extension_commands_tests.rs"]
mod tests;
