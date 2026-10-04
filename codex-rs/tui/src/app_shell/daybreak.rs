use super::ShellState;
use super::backend::AppShellBackend;
use super::backend_actions::ActionGroup;
use super::backend_actions::BackendActionResult;
use super::workspace_requests::WorkspaceRequest;
use super::workspace_requests::WorkspaceResponse;
use super::workspace_requests::workspace_request_id;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::Account;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::GetAccountParams;
use codex_app_server_protocol::GetAccountResponse;
use codex_protocol::ThreadId;
use codex_protocol::turn_input::CyberAccessProgram;
use color_eyre::Result;
use color_eyre::eyre::bail;

impl ShellState {
    pub(super) fn run_daybreak_command<S: AppShellBackend>(&mut self, args: &str, app_server: &S) {
        let enabled = match args.trim() {
            "" => !self.daybreak_enabled,
            "on" => true,
            "off" => false,
            _ => {
                self.push_error("Usage: /daybreak [on|off]");
                return;
            }
        };
        if self.reject_direct_input() || self.reject_unavailable_session_action() {
            return;
        }
        if self.side_parent.is_some() {
            self.push_error(
                "Daybreak is unavailable in side conversations. Use /side return first.",
            );
            return;
        }
        if enabled
            && (self.model_provider_id != "openai"
                || !crate::daybreak::available(&self.available_models))
        {
            self.push_error("Daybreak availability could not be confirmed for this account.");
            return;
        }
        let thread_id = self.thread_id;
        let request = app_server
            .workspace_request_in_background(thread_id, WorkspaceRequest::Daybreak(enabled));
        self.start_backend_action(
            ActionGroup::Workspace,
            "saving Daybreak preference",
            async move {
                BackendActionResult::Workspace {
                    thread_id,
                    result: request.await,
                }
            },
        );
    }
}

pub(super) async fn authorized_program(
    client: &AppServerRequestHandle,
    program: Option<CyberAccessProgram>,
) -> Result<Option<CyberAccessProgram>> {
    let Some(program) = program else {
        return Ok(None);
    };
    let response: GetAccountResponse = client
        .request_typed(ClientRequest::GetAccount {
            request_id: workspace_request_id("daybreak-account"),
            params: GetAccountParams {
                refresh_token: false,
            },
        })
        .await?;
    match (response.account, program) {
        (Some(Account::Chatgpt { .. }), program) => Ok(Some(program)),
        (Some(Account::ApiKey { .. }), CyberAccessProgram::Standard)
        | (None, CyberAccessProgram::Standard)
        | (Some(Account::AmazonBedrock { .. }), CyberAccessProgram::Standard) => Ok(None),
        (Some(Account::ApiKey { .. }), program) => Ok(Some(program)),
        _ => bail!(
            "Daybreak requires the OpenAI provider with ChatGPT sign-in or a supported API key. Use /daybreak off to continue."
        ),
    }
}

pub(super) async fn persist(
    client: AppServerRequestHandle,
    thread_id: ThreadId,
    enabled: bool,
) -> Result<WorkspaceResponse> {
    use codex_app_server_protocol::ConfigValueWriteParams;
    use codex_app_server_protocol::ConfigWriteResponse;
    use codex_app_server_protocol::MergeStrategy;
    use codex_app_server_protocol::ThreadMetadataUpdateParams;
    use codex_app_server_protocol::ThreadMetadataUpdateResponse;
    use codex_app_server_protocol::ThreadReadParams;
    use codex_app_server_protocol::ThreadReadResponse;
    if enabled {
        authorized_program(&client, Some(CyberAccessProgram::DaybreakBlue)).await?;
    }
    let thread_id = thread_id.to_string();
    let response: ThreadReadResponse = client
        .request_typed(ClientRequest::ThreadRead {
            request_id: workspace_request_id("daybreak-thread"),
            params: ThreadReadParams {
                thread_id: thread_id.clone(),
                include_turns: false,
            },
        })
        .await?;
    if !response.thread.ephemeral {
        let _: ThreadMetadataUpdateResponse = client
            .request_typed(ClientRequest::ThreadMetadataUpdate {
                request_id: workspace_request_id("daybreak-save"),
                params: ThreadMetadataUpdateParams {
                    thread_id,
                    daybreak_enabled: Some(enabled),
                    project_id: None,
                    git_info: None,
                },
            })
            .await?;
    }
    let defaults: Result<ConfigWriteResponse, _> = client
        .request_typed(ClientRequest::ConfigValueWrite {
            request_id: workspace_request_id("daybreak-default"),
            params: ConfigValueWriteParams {
                key_path: "daybreak".to_string(),
                value: enabled.into(),
                merge_strategy: MergeStrategy::Replace,
                file_path: None,
                expected_version: None,
            },
        })
        .await;
    Ok(WorkspaceResponse::Daybreak {
        enabled,
        default_error: defaults.err().map(|error| error.to_string()),
    })
}
