use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::HooksListParams;
use codex_app_server_protocol::HooksListResponse;
use codex_app_server_protocol::MemoryStatusParams;
use codex_app_server_protocol::MemoryStatusResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ReviewDelivery;
use codex_app_server_protocol::ReviewStartParams;
use codex_app_server_protocol::ReviewStartResponse;
use codex_app_server_protocol::ReviewTarget;
use codex_app_server_protocol::SkillsListParams;
use codex_app_server_protocol::SkillsListResponse;
use codex_app_server_protocol::ThreadBackgroundTerminalsCleanParams;
use codex_app_server_protocol::ThreadBackgroundTerminalsCleanResponse;
use codex_app_server_protocol::ThreadBackgroundTerminalsListParams;
use codex_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use codex_app_server_protocol::ThreadMemoryMode;
use codex_app_server_protocol::ThreadMemoryModeSetParams;
use codex_app_server_protocol::ThreadMemoryModeSetResponse;
use codex_app_server_protocol::ThreadSetNameParams;
use codex_app_server_protocol::ThreadSetNameResponse;
use codex_app_server_protocol::ThreadSettingsUpdateParams;
use codex_app_server_protocol::ThreadSettingsUpdateResponse;
use codex_protocol::ThreadId;
use codex_protocol::config_types::CollaborationMode;
use color_eyre::Result;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum WorkspaceRequest {
    Mention { cwd: String, query: String },
    GuardianApproval(Box<codex_protocol::approvals::GuardianAssessmentEvent>),
    Diagnostic(super::diagnostic_commands::DiagnosticRequest),
    AutomaticRecap(bool),
    Export(PathBuf),
    Daybreak(bool),
    Extension(super::extension_commands::ExtensionCommand),
    Review(ReviewTarget),
    Skills(PathBuf),
    Hooks(PathBuf),
    Experimental(Option<(String, bool)>),
    Background,
    Clean,
    Memory(Option<ThreadMemoryMode>),
    Plan(CollaborationMode),
    Cwd(PathBuf),
    Rename(String),
}

#[derive(Debug)]
pub(super) enum WorkspaceResponse {
    MentionFiles(Vec<codex_app_server_protocol::FuzzyFileSearchResult>),
    GuardianApproved(String),
    AutomaticRecap(bool),
    Daybreak {
        enabled: bool,
        default_error: Option<String>,
    },
    Notice(String),
    Mode(CollaborationMode),
}

// Reserved IDs keep timed-out requests bounded until the server answers them.
pub(super) fn workspace_request_id(operation: &'static str) -> RequestId {
    RequestId::String(format!("tui-workspace-{operation}"))
}

pub(super) async fn execute(
    client: AppServerRequestHandle,
    thread_id: ThreadId,
    request: WorkspaceRequest,
) -> Result<WorkspaceResponse> {
    let read_only = matches!(
        &request,
        WorkspaceRequest::Mention { .. }
            | WorkspaceRequest::Diagnostic(_)
            | WorkspaceRequest::Skills(_)
            | WorkspaceRequest::Hooks(_)
            | WorkspaceRequest::Experimental(None)
            | WorkspaceRequest::Background
            | WorkspaceRequest::Memory(None)
            | WorkspaceRequest::Extension(super::extension_commands::ExtensionCommand::Usage {
                reset: false
            })
    );
    let mut response = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 60), execute_inner(client, thread_id, request),
    ).await.map_err(|_| eyre!(if read_only {
        "Workspace request timed out"
    } else {
        "Workspace action timed out. It may still complete; check the current state before retrying."
    }))??;
    if let WorkspaceResponse::Notice(message) = &mut response {
        const MAX_NOTICE_BYTES: usize = 256 * 1024;
        const TRUNCATED: &str = "\n[output truncated]";
        if message.len() > MAX_NOTICE_BYTES {
            let mut end = MAX_NOTICE_BYTES - TRUNCATED.len();
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
            message.push_str(TRUNCATED);
        }
    }
    Ok(response)
}

async fn execute_inner(
    client: AppServerRequestHandle,
    thread_id: ThreadId,
    request: WorkspaceRequest,
) -> Result<WorkspaceResponse> {
    if let WorkspaceRequest::Daybreak(enabled) = request {
        return super::daybreak::persist(client, thread_id, enabled).await;
    }
    if let WorkspaceRequest::AutomaticRecap(enabled) = request {
        return super::automatic_recap::persist(client, enabled).await;
    }
    let request_id = workspace_request_id("workspace");
    let parsed_thread_id = thread_id;
    let thread_id = thread_id.to_string();
    let mut lines = Vec::new();
    match request {
        WorkspaceRequest::Mention { cwd, query } => {
            return super::file_mentions::search(client, cwd, query)
                .await
                .map(WorkspaceResponse::MentionFiles);
        }
        WorkspaceRequest::GuardianApproval(event) => {
            let _: codex_app_server_protocol::ThreadApproveGuardianDeniedActionResponse = client
                .request_typed(ClientRequest::ThreadApproveGuardianDeniedAction {
                    request_id,
                    params: codex_app_server_protocol::ThreadApproveGuardianDeniedActionParams {
                        thread_id,
                        event: serde_json::to_value(&event)?,
                    },
                })
                .await?;
            return Ok(WorkspaceResponse::GuardianApproved(event.id));
        }
        WorkspaceRequest::Diagnostic(request) => {
            return super::diagnostic_commands::execute(client, parsed_thread_id, request)
                .await
                .map(WorkspaceResponse::Notice);
        }
        WorkspaceRequest::Export(path) => {
            return super::transcript_export::export(
                super::transcript_export::NativeExportReader(client),
                parsed_thread_id,
                path,
            )
            .await
            .map(WorkspaceResponse::Notice);
        }
        WorkspaceRequest::AutomaticRecap(_) | WorkspaceRequest::Daybreak(_) => {
            unreachable!("handled before thread ID conversion")
        }
        WorkspaceRequest::Extension(command) => {
            return super::extension_commands::execute(client, command)
                .await
                .map(WorkspaceResponse::Notice);
        }
        WorkspaceRequest::Review(target) => {
            let _: ReviewStartResponse = client
                .request_typed(ClientRequest::ReviewStart {
                    request_id,
                    params: ReviewStartParams {
                        thread_id,
                        target,
                        delivery: Some(ReviewDelivery::Inline),
                    },
                })
                .await?;
            lines.push("Review started".to_string());
        }
        WorkspaceRequest::Skills(cwd) => {
            let response: SkillsListResponse = client
                .request_typed(ClientRequest::SkillsList {
                    request_id,
                    params: SkillsListParams {
                        cwds: vec![cwd],
                        force_reload: true,
                    },
                })
                .await?;
            lines.push("Skills (invoke with $name in your message)".to_string());
            for entry in response.data {
                lines.extend(entry.skills.into_iter().map(|skill| {
                    let status = if skill.enabled { "enabled" } else { "disabled" };
                    format!("{} [{status}]: {}", skill.name, skill.description)
                }));
                lines.extend(entry.errors.into_iter().map(|error| error.message));
            }
        }
        WorkspaceRequest::Hooks(cwd) => {
            let response: HooksListResponse = client
                .request_typed(ClientRequest::HooksList {
                    request_id,
                    params: HooksListParams { cwds: vec![cwd] },
                })
                .await?;
            lines
                .push("Hooks: /hooks on|off <key>; /hooks trust <key> <reviewed hash>".to_string());
            for entry in response.data {
                lines.extend(entry.hooks.into_iter().map(|hook| {
                    let status = if hook.enabled { "enabled" } else { "disabled" };
                    format!(
                        "{}: {:?} [{status}, {:?}]\n  {}\n  {:?}\n  hash: {}",
                        hook.key,
                        hook.event_name,
                        hook.trust_status,
                        hook.source_path.display(),
                        hook.handler,
                        hook.current_hash
                    )
                }));
                lines.extend(entry.warnings);
                lines.extend(entry.errors.into_iter().map(|error| error.message));
            }
        }
        WorkspaceRequest::Experimental(Some((name, enabled))) => {
            let response = crate::experimental_features::write(
                client,
                parsed_thread_id,
                vec![(name.clone(), enabled)],
            )
            .await
            .map_err(color_eyre::eyre::Error::msg)?;
            if let Some(feature) = response
                .features
                .iter()
                .find(|feature| feature.name == name)
            {
                let status = if feature.enabled {
                    "enabled"
                } else {
                    "disabled"
                };
                lines.push(format!("{name} {status} in configuration"));
            }
            if let Some(warning) = response.warning {
                lines.push(warning);
            }
        }
        WorkspaceRequest::Experimental(None) => {
            lines.push("Features (change with /experimental <name> on|off)".to_string());
            let (tx, rx) = tokio::sync::oneshot::channel();
            crate::experimental_features::fetch(
                client,
                Some(parsed_thread_id),
                "better-codex-features",
                tx,
            );
            let features = rx.await?.map_err(color_eyre::eyre::Error::msg)?;
            lines.extend(features.into_iter().map(|feature| {
                let status = if feature.enabled { "on" } else { "off" };
                format!(
                    "{} [{status}, {:?}]: {}",
                    feature.name,
                    feature.stage,
                    feature.description.unwrap_or_default()
                )
            }));
        }
        WorkspaceRequest::Background => {
            lines.push("Background terminals".to_string());
            let mut cursor = None;
            let mut cursors = HashSet::new();
            for _ in 0..10 {
                let response: ThreadBackgroundTerminalsListResponse = client
                    .request_typed(ClientRequest::ThreadBackgroundTerminalsList {
                        request_id: workspace_request_id("terminals"),
                        params: ThreadBackgroundTerminalsListParams {
                            thread_id: thread_id.clone(),
                            cursor,
                            limit: Some(100),
                        },
                    })
                    .await?;
                if response.data.len() > 100 {
                    bail!("Background terminal page exceeds the requested limit");
                }
                lines.extend(
                    response
                        .data
                        .into_iter()
                        .map(|terminal| format!("{}: {}", terminal.process_id, terminal.command)),
                );
                cursor = response.next_cursor;
                let Some(next) = cursor.as_ref() else {
                    break;
                };
                if !cursors.insert(next.clone()) {
                    bail!("Background terminal pagination repeated a cursor");
                }
            }
            if cursor.is_some() {
                lines.push("Showing the first 1,000 terminals".to_string());
            }
        }
        WorkspaceRequest::Clean => {
            let _: ThreadBackgroundTerminalsCleanResponse = client
                .request_typed(ClientRequest::ThreadBackgroundTerminalsClean {
                    request_id,
                    params: ThreadBackgroundTerminalsCleanParams { thread_id },
                })
                .await?;
            lines.push("Background terminals stopped".to_string());
        }
        WorkspaceRequest::Memory(Some(mode)) => {
            let _: ThreadMemoryModeSetResponse = client
                .request_typed(ClientRequest::ThreadMemoryModeSet {
                    request_id,
                    params: ThreadMemoryModeSetParams { thread_id, mode },
                })
                .await?;
            lines.push(format!("Session memory: {mode:?}"));
        }
        WorkspaceRequest::Memory(None) => {
            let response: MemoryStatusResponse = client
                .request_typed(ClientRequest::MemoryStatus {
                    request_id,
                    params: MemoryStatusParams {
                        min_consolidated_threads: None,
                    },
                })
                .await?;
            let status = if response.v2_ready {
                "ready"
            } else {
                "building"
            };
            lines.push(format!(
                "Memory {status}: {} consolidated sessions. Use /memory on|off for this session.",
                response.v2_consolidated_threads
            ));
        }
        WorkspaceRequest::Cwd(cwd) => {
            let _: ThreadSettingsUpdateResponse = client
                .request_typed(ClientRequest::ThreadSettingsUpdate {
                    request_id,
                    params: ThreadSettingsUpdateParams {
                        thread_id,
                        cwd: Some(cwd),
                        ..Default::default()
                    },
                })
                .await?;
            lines.push("Working directory updated".to_string());
        }
        WorkspaceRequest::Rename(name) => {
            let _: ThreadSetNameResponse = client
                .request_typed(ClientRequest::ThreadSetName {
                    request_id,
                    params: ThreadSetNameParams { thread_id, name },
                })
                .await?;
            lines.push("Session renamed".to_string());
        }
        WorkspaceRequest::Plan(mode) => {
            let _: ThreadSettingsUpdateResponse = client
                .request_typed(ClientRequest::ThreadSettingsUpdate {
                    request_id,
                    params: ThreadSettingsUpdateParams {
                        thread_id,
                        collaboration_mode: Some(mode.clone()),
                        ..Default::default()
                    },
                })
                .await?;
            return Ok(WorkspaceResponse::Mode(mode));
        }
    }
    Ok(WorkspaceResponse::Notice(lines.join("\n")))
}

#[cfg(test)]
#[path = "workspace_requests_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "workspace_request_limits_tests.rs"]
mod limits_tests;
