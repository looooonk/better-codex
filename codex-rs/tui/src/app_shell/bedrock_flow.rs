use super::*;
use codex_app_server_client::AppServerRequestHandle;
use tokio::task::JoinHandle;

pub(crate) struct BedrockFlow {
    state: BedrockState,
    error: Option<String>,
    client: AppServerRequestHandle,
    pending: Option<JoinHandle<Completion>>,
    write_timed_out: bool,
}

pub(crate) enum FlowAction {
    Back,
    Configured,
    Exit,
}

enum Completion {
    Discovery(Result<BedrockDiscoverResponse, String>),
    Setup(Result<(), String>, BedrockState),
    GovCloud(bool),
    SetupTimedOut,
}

impl Drop for BedrockFlow {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.abort();
        }
    }
}

impl BedrockFlow {
    pub(crate) fn new(client: AppServerRequestHandle) -> Self {
        // An unanswered discovery keeps this slot, bounding canceled and timed-out retries.
        let request_id = RequestId::String("better-codex-bedrock-discover".to_string());
        let state = BedrockState::discovering(request_id.clone());
        let request_client = client.clone();
        let pending = tokio::spawn(async move {
            Completion::Discovery(
                tokio::time::timeout(
                    std::time::Duration::from_secs(/*secs*/ 15),
                    request_client.request_typed::<BedrockDiscoverResponse>(
                        ClientRequest::BedrockDiscover {
                            request_id,
                            params: BedrockDiscoverParams {},
                        },
                    ),
                )
                .await
                .map_err(|_| "Credential discovery timed out".to_string())
                .and_then(|result| result.map_err(|error| error.to_string())),
            )
        });
        Self {
            state,
            client,
            pending: Some(pending),
            error: None,
            write_timed_out: false,
        }
    }

    pub(crate) fn handle_key(&mut self, key: &KeyEvent) -> Option<FlowAction> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
            || self.write_timed_out && key.code == KeyCode::Esc
        {
            return Some(FlowAction::Exit);
        }
        if self.write_timed_out {
            return None;
        }
        // A submitted credential write must finish before another login can start.
        if matches!(
            self.state.view,
            BedrockView::Configuring(_) | BedrockView::CheckingGovCloud(_)
        ) {
            return None;
        }
        self.error = None;
        match self.state.handle_key_event(key) {
            Some(BedrockAction::BackToAuth) => Some(FlowAction::Back),
            Some(BedrockAction::ContinueAfterGovCloudWarning) => Some(FlowAction::Configured),
            Some(BedrockAction::Configure(credential, region)) => {
                let mut fallback = self.state.clone();
                fallback.enter_region(credential.clone(), region.clone());
                let request_id = RequestId::String(uuid::Uuid::new_v4().to_string());
                self.state.view = BedrockView::Configuring(request_id.clone());
                let client = self.client.clone();
                self.pending = Some(tokio::spawn(async move {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(/*secs*/ 60),
                        configure(&client, request_id, credential, region),
                    )
                    .await
                    {
                        Ok(result) => Completion::Setup(result, fallback),
                        Err(_) => Completion::SetupTimedOut,
                    }
                }));
                None
            }
            None => None,
        }
    }

    pub(crate) fn paste(&mut self, text: &str) {
        if self.write_timed_out {
            return;
        }
        if let Some(input) = self.state.active_input_mut() {
            input.push_str(text.trim());
            self.error = None;
        }
    }

    pub(crate) async fn complete(&mut self) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return std::future::pending().await;
        };
        let result = pending.await;
        self.pending = None;
        match result {
            Ok(Completion::Discovery(result)) => {
                self.state = BedrockState::discovered(result.unwrap_or_else(|error| {
                    self.error = Some(format!("Unable to check AWS credentials: {error}"));
                    BedrockDiscoverResponse {
                        profiles: Vec::new(),
                        environment_credentials: Vec::new(),
                    }
                }));
            }
            Ok(Completion::Setup(Err(error), fallback)) => {
                self.state = fallback;
                self.error = Some(format!("Unable to set up Amazon Bedrock: {error}"));
            }
            Ok(Completion::Setup(Ok(()), _)) => {
                let request_id = RequestId::String(uuid::Uuid::new_v4().to_string());
                self.state.view = BedrockView::CheckingGovCloud(request_id.clone());
                let client = self.client.clone();
                self.pending = Some(tokio::spawn(async move {
                    let result = tokio::time::timeout(
                        std::time::Duration::from_secs(/*secs*/ 15),
                        client.request_typed::<BedrockCheckGovCloudRequirementsResponse>(
                            ClientRequest::BedrockCheckGovCloudRequirements {
                                request_id,
                                params: BedrockCheckGovCloudRequirementsParams {},
                            },
                        ),
                    )
                    .await;
                    Completion::GovCloud(
                        matches!(result, Ok(Ok(response)) if response.is_gov_cloud),
                    )
                }));
            }
            Ok(Completion::GovCloud(true)) => {
                self.state.view = BedrockView::GovCloudWarning(Arc::default())
            }
            Ok(Completion::GovCloud(false)) => return true,
            Ok(Completion::SetupTimedOut) => self.write_timed_out = true,
            Err(error) => {
                self.state = BedrockState::discovered(BedrockDiscoverResponse {
                    profiles: Vec::new(),
                    environment_credentials: Vec::new(),
                });
                self.error = Some(format!("Amazon Bedrock setup stopped: {error}"));
            }
        }
        false
    }

    pub(crate) fn render(&self, area: Rect, buffer: &mut Buffer) {
        if self.write_timed_out {
            let text = "Amazon Bedrock setup timed out.\n\nThe credential write may still have completed. Exit and restart Better Codex to check the saved configuration.\n\nEsc or Ctrl+C to exit";
            Paragraph::new(textwrap::fill(text, usize::from(area.width.max(1))))
                .render(area, buffer);
        } else {
            self.state.render(area, buffer, self.error.clone());
        }
    }
}

async fn configure(
    client: &AppServerRequestHandle,
    request_id: RequestId,
    credential: BedrockCredential,
    region: String,
) -> Result<(), String> {
    let params = match credential {
        BedrockCredential::ApiKey(api_key) => LoginAccountParams::AmazonBedrock { api_key, region },
        BedrockCredential::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } => LoginAccountParams::AmazonBedrockAccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
            region,
        },
        BedrockCredential::Profile(profile) => {
            return client
                .request_typed::<BedrockSetupResponse>(ClientRequest::BedrockSetup {
                    request_id,
                    params: BedrockSetupParams::Profile { profile, region },
                })
                .await
                .map(|_| ())
                .map_err(|error| error.to_string());
        }
        BedrockCredential::Environment => {
            return client
                .request_typed::<BedrockSetupResponse>(ClientRequest::BedrockSetup {
                    request_id,
                    params: BedrockSetupParams::Environment { region },
                })
                .await
                .map(|_| ())
                .map_err(|error| error.to_string());
        }
    };
    match client
        .request_typed::<LoginAccountResponse>(ClientRequest::LoginAccount { request_id, params })
        .await
        .map_err(|error| error.to_string())?
    {
        LoginAccountResponse::AmazonBedrock {} => Ok(()),
        _ => Err("Unexpected account/login/start response".to_string()),
    }
}

#[cfg(test)]
#[path = "bedrock_flow_tests.rs"]
mod tests;
