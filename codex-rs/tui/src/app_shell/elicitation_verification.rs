use super::AppShellBackend;
use super::ElicitationChoice;
use super::ShellState;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::UserVerificationVerifyResponse;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(super) struct VerificationAttempt {
    pub(super) id: RequestId,
    cancel: Arc<CancelVerification>,
}

#[derive(Debug)]
struct CancelVerification(CancellationToken);

impl Drop for CancelVerification {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl PartialEq for VerificationAttempt {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl VerificationAttempt {
    pub(super) fn new() -> Self {
        Self {
            id: RequestId::String(uuid::Uuid::new_v4().to_string()),
            cancel: Arc::new(CancelVerification(CancellationToken::new())),
        }
    }

    pub(super) fn cancellation(&self) -> CancellationToken {
        self.cancel.0.clone()
    }
}

impl ShellState {
    pub(in crate::app_shell) async fn complete_user_verification<S: AppShellBackend>(
        &mut self,
        app_server: &S,
        request_id: RequestId,
        attempt_id: RequestId,
        result: color_eyre::Result<UserVerificationVerifyResponse>,
    ) {
        let Some(pending) = self.pending_elicitation.as_mut().filter(|pending| {
            pending.request_id == request_id
                && pending
                    .verification_attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.id == attempt_id)
        }) else {
            return;
        };
        pending.verification_attempt = None;
        let title = pending.title.clone();
        match result {
            Ok(response) => {
                let result = serde_json::json!({ "action": "accept", "content": response.proof, "_meta": null });
                if let Err(error) = self
                    .finish_pending_elicitation(
                        app_server,
                        ElicitationChoice::Accept,
                        request_id,
                        result,
                        title,
                    )
                    .await
                {
                    self.report_action_error("failed to submit device verification", error);
                }
            }
            Err(error) => self.report_action_error("device verification failed", error),
        }
    }
}
