use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::command_palette::CommandPaletteAction;
use super::navigation::DashboardRoute;
use super::workspace_requests::WorkspaceRequest;
use super::workspace_requests::WorkspaceResponse;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::ReviewTarget;
use codex_app_server_protocol::ThreadMemoryMode;
use codex_protocol::ThreadId;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use color_eyre::Result;
use color_eyre::eyre::bail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkspaceCommand {
    StatusSurface(super::status_surfaces::Surface),
    Keymap,
    Find,
    Mention,
    Pets,
    Ide,
    Attach,
    Detach,
    Approve,
    Diagnostic(super::diagnostic_commands::DiagnosticCommand),
    New,
    Worktree,
    Resume,
    Fork,
    Model,
    Permissions,
    Status,
    Usage,
    Export,
    Agents,
    Mcp,
    Plugins,
    Compact,
    Review,
    Plan,
    Skills,
    Hooks,
    Experimental,
    Background,
    Clean,
    Memory,
    Pwd,
    Cd,
    Rename,
    Diff,
    Import,
    Theme,
    Voice,
    Daemon,
    Side,
    Daybreak,
    Recap,
}

impl ShellState {
    pub(super) async fn run_workspace_command<S: AppShellBackend>(
        &mut self,
        command: WorkspaceCommand,
        args: &str,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()> {
        let action = match command {
            WorkspaceCommand::Find => {
                self.open_transcript_find(args);
                return Ok(());
            }
            WorkspaceCommand::Keymap => return self.run_keymap_command(args, config).await,
            WorkspaceCommand::StatusSurface(surface) => {
                return self.run_status_surface_command(surface, args);
            }
            WorkspaceCommand::Pets => {
                self.run_pets_command(args);
                return Ok(());
            }
            WorkspaceCommand::Attach => return self.run_attach_command(args).await,
            WorkspaceCommand::Detach => return self.run_detach_command(args),
            WorkspaceCommand::Ide => {
                self.run_ide_command(args);
                return Ok(());
            }
            WorkspaceCommand::Approve if args.is_empty() => {
                self.show_guardian_denials();
                return Ok(());
            }
            WorkspaceCommand::Approve => None,
            WorkspaceCommand::Diagnostic(command) => {
                return self.run_diagnostic_command(command, app_server);
            }
            WorkspaceCommand::Worktree => {
                return self.run_worktree_command(args, config, app_server);
            }
            WorkspaceCommand::Recap => return self.run_recap_command(args, config, app_server),
            WorkspaceCommand::Daybreak => {
                self.run_daybreak_command(args, app_server);
                return Ok(());
            }
            WorkspaceCommand::Side => return self.run_side_command(args, config, app_server).await,
            WorkspaceCommand::Daemon => {
                self.run_daemon_command(args);
                return Ok(());
            }
            WorkspaceCommand::Voice => return self.run_voice_command(args, app_server),
            WorkspaceCommand::New => Some(CommandPaletteAction::NewSession),
            WorkspaceCommand::Resume => Some(CommandPaletteAction::ResumeThread),
            WorkspaceCommand::Fork => Some(CommandPaletteAction::ForkThread),
            WorkspaceCommand::Model => Some(CommandPaletteAction::SwitchModel),
            WorkspaceCommand::Permissions => Some(CommandPaletteAction::ChangePermissions),
            WorkspaceCommand::Compact => Some(CommandPaletteAction::CompactContext),
            WorkspaceCommand::Import => Some(CommandPaletteAction::ImportExternalAgentConfig),
            WorkspaceCommand::Pwd => {
                self.push_system(self.cwd.clone());
                return Ok(());
            }
            WorkspaceCommand::Diff => {
                if !self.open_session_diff_view() {
                    self.push_status("no session changes to review");
                }
                return Ok(());
            }
            WorkspaceCommand::Theme => {
                self.open_app_theme_selector();
                return Ok(());
            }
            WorkspaceCommand::Status => {
                self.dashboard_visible = true;
                self.set_dashboard_route(DashboardRoute::Status);
                return Ok(());
            }
            WorkspaceCommand::Agents => {
                self.dashboard_visible = true;
                self.set_dashboard_route(DashboardRoute::Agents);
                return Ok(());
            }
            WorkspaceCommand::Mcp => {
                self.open_mcp_management();
                return Ok(());
            }
            WorkspaceCommand::Plugins => {
                self.open_plugin_management();
                return Ok(());
            }
            WorkspaceCommand::Mention
            | WorkspaceCommand::Export
            | WorkspaceCommand::Usage
            | WorkspaceCommand::Review
            | WorkspaceCommand::Plan
            | WorkspaceCommand::Skills
            | WorkspaceCommand::Hooks
            | WorkspaceCommand::Experimental
            | WorkspaceCommand::Background
            | WorkspaceCommand::Clean
            | WorkspaceCommand::Memory
            | WorkspaceCommand::Cd
            | WorkspaceCommand::Rename => None,
        };
        if let Some(action) = action {
            return self
                .execute_workspace_action(action, config, app_server)
                .await;
        }
        let request = match self.workspace_request(command, args) {
            Ok(request) => request,
            Err(error) => {
                self.push_error(error.to_string());
                return Ok(());
            }
        };
        if matches!(request, WorkspaceRequest::Review(_))
            && (self.active_turn_id.is_some() || self.has_pending_backend_actions())
        {
            self.push_status("finish active work before starting a review");
            return Ok(());
        }
        let thread_id = self.thread_id;
        let request = app_server.workspace_request_in_background(thread_id, request);
        self.start_backend_action(
            ActionGroup::Workspace,
            "loading workspace action",
            async move {
                BackendActionResult::Workspace {
                    thread_id,
                    result: request.await,
                }
            },
        );
        Ok(())
    }

    fn workspace_request(&self, command: WorkspaceCommand, args: &str) -> Result<WorkspaceRequest> {
        Ok(match command {
            WorkspaceCommand::Mention => WorkspaceRequest::Mention {
                cwd: self.cwd.clone(),
                query: args.to_string(),
            },
            WorkspaceCommand::Approve => self.guardian_approval_request(args)?,
            WorkspaceCommand::Export => {
                let destination = if args.is_empty() {
                    format!("conversation-{}.md", self.thread_id)
                } else {
                    args.to_string()
                };
                let path = std::path::PathBuf::from(destination);
                WorkspaceRequest::Export(if path.is_absolute() {
                    path
                } else {
                    self.resume_cwd_runtime.launch_cwd.join(path)
                })
            }
            WorkspaceCommand::Usage => {
                WorkspaceRequest::Extension(super::extension_commands::ExtensionCommand::Usage {
                    reset: match args {
                        "" | "reset" => false,
                        "reset confirm" => true,
                        _ => bail!("usage: /usage [reset confirm]"),
                    },
                })
            }
            WorkspaceCommand::Skills if !args.is_empty() => {
                WorkspaceRequest::Extension(super::extension_commands::skill_command(args)?)
            }
            WorkspaceCommand::Hooks if !args.is_empty() => WorkspaceRequest::Extension(
                super::extension_commands::hook_command(self.cwd.clone().into(), args)?,
            ),
            WorkspaceCommand::Review => WorkspaceRequest::Review(review_target(args)?),
            WorkspaceCommand::Skills => WorkspaceRequest::Skills(self.cwd.clone().into()),
            WorkspaceCommand::Hooks => WorkspaceRequest::Hooks(self.cwd.clone().into()),
            WorkspaceCommand::Background => WorkspaceRequest::Background,
            WorkspaceCommand::Clean => WorkspaceRequest::Clean,
            WorkspaceCommand::Cd => {
                if args.is_empty() {
                    bail!("usage: /cd <directory>");
                }
                let cwd = std::path::Path::new(args);
                let cwd = if cwd.is_absolute() {
                    cwd.to_path_buf()
                } else {
                    std::path::Path::new(&self.cwd).join(cwd)
                };
                WorkspaceRequest::Cwd(cwd)
            }
            WorkspaceCommand::Rename => {
                if args.is_empty() {
                    bail!("usage: /rename <name>");
                }
                WorkspaceRequest::Rename(args.to_string())
            }
            WorkspaceCommand::Memory => WorkspaceRequest::Memory(match args {
                "" => None,
                "on" => Some(ThreadMemoryMode::Enabled),
                "off" => Some(ThreadMemoryMode::Disabled),
                _ => bail!("usage: /memory [on|off]"),
            }),
            WorkspaceCommand::Experimental => {
                let parts = args.split_whitespace().collect::<Vec<_>>();
                WorkspaceRequest::Experimental(match parts.as_slice() {
                    [] => None,
                    [name, "on"] => Some(((*name).to_string(), true)),
                    [name, "off"] => Some(((*name).to_string(), false)),
                    _ => bail!("usage: /experimental [feature on|off]"),
                })
            }
            WorkspaceCommand::Plan => {
                let current = self.collaboration_mode.as_ref().map(|mode| mode.mode);
                let mode = match args {
                    "" if current == Some(ModeKind::Plan) => ModeKind::Default,
                    "" | "on" => ModeKind::Plan,
                    "off" => ModeKind::Default,
                    _ => bail!("usage: /plan [on|off]"),
                };
                WorkspaceRequest::Plan(CollaborationMode {
                    mode,
                    settings: Settings {
                        model: self.model.clone(),
                        reasoning_effort: self.reasoning_effort.clone(),
                        developer_instructions: None,
                    },
                })
            }
            WorkspaceCommand::StatusSurface(_)
            | WorkspaceCommand::Find
            | WorkspaceCommand::Keymap
            | WorkspaceCommand::Pets
            | WorkspaceCommand::Ide
            | WorkspaceCommand::Diagnostic(_)
            | WorkspaceCommand::Worktree
            | WorkspaceCommand::New
            | WorkspaceCommand::Resume
            | WorkspaceCommand::Fork
            | WorkspaceCommand::Model
            | WorkspaceCommand::Permissions
            | WorkspaceCommand::Status
            | WorkspaceCommand::Agents
            | WorkspaceCommand::Mcp
            | WorkspaceCommand::Plugins
            | WorkspaceCommand::Compact
            | WorkspaceCommand::Pwd
            | WorkspaceCommand::Diff
            | WorkspaceCommand::Import
            | WorkspaceCommand::Theme
            | WorkspaceCommand::Voice
            | WorkspaceCommand::Daemon
            | WorkspaceCommand::Side
            | WorkspaceCommand::Attach
            | WorkspaceCommand::Detach
            | WorkspaceCommand::Recap
            | WorkspaceCommand::Daybreak => bail!("this command uses local workspace controls"),
        })
    }

    pub(super) fn complete_workspace_request(
        &mut self,
        thread_id: ThreadId,
        result: Result<WorkspaceResponse>,
    ) {
        if thread_id != self.thread_id {
            return;
        }
        if matches!(
            self.status.as_str(),
            "loading workspace action" | "loading diagnostics"
        ) {
            self.status = if self.active_turn_id.is_some() {
                "thinking"
            } else {
                "ready"
            }
            .to_string();
        }
        match result {
            Ok(WorkspaceResponse::MentionFiles(files)) => self.open_file_mentions(files),
            Ok(WorkspaceResponse::GuardianApproved(id)) => {
                self.recent_guardian_denials.retain(|event| event.id != id);
                self.push_system(
                    "Approval recorded for one retry. The retry still goes through auto-review."
                        .to_string(),
                );
            }
            Ok(WorkspaceResponse::AutomaticRecap(enabled)) => {
                self.automatic_recap.enabled = enabled;
                if !enabled {
                    self.recap.cancel_automatic();
                }
                let status = if enabled { "on" } else { "off" };
                self.push_system(format!(
                    "Automatic recaps {status}. Manual /recap remains available."
                ));
            }
            Ok(WorkspaceResponse::Daybreak {
                enabled,
                default_error,
            }) => {
                self.daybreak_enabled = enabled;
                let status = if enabled { "on" } else { "off" };
                self.push_system(format!("Daybreak {status}. Applies to new turns."));
                if let Some(error) = default_error {
                    self.push_error(format!("Daybreak was updated for this conversation, but the default could not be saved: {error}"));
                }
            }
            Ok(WorkspaceResponse::Notice(message)) => self.push_system(message),
            Ok(WorkspaceResponse::Mode(mode)) => {
                self.push_status(format!("{} mode enabled", mode.mode.display_name()));
            }
            Err(error) => self.report_action_error("workspace action failed", error),
        }
    }
}

fn review_target(args: &str) -> Result<ReviewTarget> {
    let mut parts = args.split_whitespace();
    match parts.next() {
        None => Ok(ReviewTarget::UncommittedChanges),
        Some(flag @ ("--base" | "--commit")) => {
            let Some(value) = parts.next() else {
                bail!("usage: /review [{flag} <revision>|instructions]");
            };
            if parts.next().is_some() {
                bail!("provide one revision after {flag}");
            }
            Ok(if flag == "--base" {
                ReviewTarget::BaseBranch {
                    branch: value.to_string(),
                }
            } else {
                ReviewTarget::Commit {
                    sha: value.to_string(),
                    title: None,
                }
            })
        }
        Some(_) => Ok(ReviewTarget::Custom {
            instructions: args.to_string(),
        }),
    }
}

#[cfg(test)]
#[path = "workspace_commands_tests.rs"]
mod tests;
