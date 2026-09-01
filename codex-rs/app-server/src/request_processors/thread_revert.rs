use std::sync::Arc;
use std::time::Duration;

use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadRevertParams;
use codex_app_server_protocol::ThreadRevertResponse;
use codex_app_server_protocol::ThreadRevertedNotification;
use codex_core::CodexThread;
use codex_core::NewThread;
use codex_core::ThreadConfigSnapshot;
use codex_core::config::Config;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_protocol::protocol::ThreadHistoryMode;

use super::EnsureConversationListenerResult;
use super::ThreadShutdownResult;
use super::internal_error;
use super::invalid_request;
use super::set_thread_status_and_interrupt_stale_turns;
use super::thread_processor::ThreadRequestProcessor;
use super::thread_processor::thread_store_mutation_error;
use super::wait_for_thread_shutdown;
use crate::outgoing_message::ConnectionRequestId;

struct ThreadRevertRuntimeSnapshot {
    config: Config,
    settings: ThreadConfigSnapshot,
    supports_openai_form_elicitation: bool,
}

impl ThreadRequestProcessor {
    pub(crate) async fn thread_revert(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadRevertParams,
        app_server_client_name: Option<String>,
        app_server_client_version: Option<String>,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (response, thread_id, thread) = self
            .thread_revert_response(
                &request_id,
                params,
                app_server_client_name,
                app_server_client_version,
            )
            .await?;
        self.outgoing.send_response(request_id, response).await;
        self.outgoing
            .send_server_notification(ServerNotification::ThreadReverted(
                ThreadRevertedNotification {
                    thread_id: thread_id.to_string(),
                },
            ))
            .await;
        thread.emit_thread_idle_lifecycle_if_idle().await;
        Ok(None)
    }

    async fn thread_revert_response(
        &self,
        request_id: &ConnectionRequestId,
        params: ThreadRevertParams,
        app_server_client_name: Option<String>,
        app_server_client_version: Option<String>,
    ) -> Result<(ThreadRevertResponse, ThreadId, Arc<CodexThread>), JSONRPCErrorError> {
        let _thread_list_state_permit = self.acquire_thread_list_state_permit().await?;
        let ThreadRevertParams {
            thread_id,
            before_turn_id,
        } = params;
        let (thread_id, thread) = self.load_thread(thread_id.as_str()).await?;
        let config_snapshot = thread.config_snapshot().await;
        if !matches!(config_snapshot.history_mode, ThreadHistoryMode::Paginated) {
            return Err(invalid_request(
                "thread/revert only supports paginated threads",
            ));
        }
        let runtime_snapshot = ThreadRevertRuntimeSnapshot {
            config: thread.config().await.as_ref().clone(),
            settings: config_snapshot,
            supports_openai_form_elicitation: thread.supports_openai_form_elicitation(),
        };

        if matches!(
            self.ensure_conversation_listener(
                thread_id,
                request_id.connection_id,
                /*raw_events_enabled*/ false,
            )
            .await?,
            EnsureConversationListenerResult::ConnectionClosed
        ) {
            return Err(internal_error(format!(
                "connection closed before thread {thread_id} could be reverted"
            )));
        }
        let thread_state = self.thread_state_manager.thread_state(thread_id).await;
        let shutdown_drain_rx = thread_state.lock().await.register_shutdown_drain_waiter();

        match wait_for_thread_shutdown(&thread).await {
            ThreadShutdownResult::Complete => {}
            ThreadShutdownResult::SubmitFailed => {
                thread_state.lock().await.take_shutdown_drain_waiter();
                return Err(internal_error(format!(
                    "failed to shut down thread {thread_id} before revert"
                )));
            }
            ThreadShutdownResult::TimedOut => {
                thread_state.lock().await.take_shutdown_drain_waiter();
                return Err(internal_error(format!(
                    "timed out shutting down thread {thread_id} before revert"
                )));
            }
        }
        let drain_result = tokio::time::timeout(Duration::from_secs(10), shutdown_drain_rx)
            .await
            .map_err(|_| {
                internal_error(format!(
                    "timed out waiting for thread {thread_id} listener to drain shutdown events"
                ))
            })
            .and_then(|result| {
                result.map_err(|_| {
                    internal_error(format!(
                        "thread {thread_id} listener stopped before draining shutdown events"
                    ))
                })
            });
        if let Err(err) = drain_result {
            thread_state.lock().await.take_shutdown_drain_waiter();
            return Err(err);
        }
        if self.state_db.is_some() {
            let _ = self
                .thread_queue_service
                .recover_after_shutdown(thread_id)
                .await?;
        }
        if self
            .thread_manager
            .remove_thread(&thread_id)
            .await
            .is_none()
        {
            return Err(internal_error(format!(
                "thread {thread_id} disappeared before revert"
            )));
        }
        self.outgoing
            .cancel_requests_for_thread(thread_id, /*error*/ None)
            .await;

        let revert_result = self
            .thread_store
            .revert_thread(codex_thread_store::RevertThreadParams {
                thread_id,
                before_turn_id,
            })
            .await
            .map_err(|err| thread_store_mutation_error("revert", err));
        let (response, thread) = self
            .reload_paginated_thread(
                request_id,
                thread_id,
                runtime_snapshot,
                app_server_client_name,
                app_server_client_version,
            )
            .await?;
        if let Err(error) = revert_result {
            let thread = Arc::clone(&thread);
            self.thread_queue_service
                .enqueue_background(thread_id, async move {
                    thread.emit_thread_idle_lifecycle_if_idle().await;
                })
                .await;
            return Err(error);
        }
        Ok((response, thread_id, thread))
    }

    async fn reload_paginated_thread(
        &self,
        request_id: &ConnectionRequestId,
        thread_id: ThreadId,
        runtime_snapshot: ThreadRevertRuntimeSnapshot,
        app_server_client_name: Option<String>,
        app_server_client_version: Option<String>,
    ) -> Result<(ThreadRevertResponse, Arc<CodexThread>), JSONRPCErrorError> {
        let ThreadRevertRuntimeSnapshot {
            config,
            settings,
            supports_openai_form_elicitation,
        } = runtime_snapshot;
        let thread_id_string = thread_id.to_string();
        let stored_thread = self
            .read_stored_thread_for_resume(
                thread_id_string.as_str(),
                /*path*/ None,
                /*include_history*/ false,
            )
            .await?;
        let (thread_history, resume_source_thread) = self
            .load_resume_initial_history_from_stored_thread(stored_thread)
            .await?;
        let response_history = thread_history.clone();
        let NewThread {
            thread_id: resumed_thread_id,
            thread: codex_thread,
            session_configured,
        } = self
            .thread_manager
            .resume_thread_with_history(
                config,
                thread_history,
                self.auth_manager.clone(),
                self.request_trace_context(request_id).await,
                supports_openai_form_elicitation,
            )
            .await
            .map_err(|err| internal_error(format!("error reloading thread after revert: {err}")))?;
        if resumed_thread_id != thread_id {
            return Err(internal_error(format!(
                "thread {thread_id} reloaded as {resumed_thread_id} after revert"
            )));
        }
        codex_thread
            .restore_thread_settings(settings)
            .await
            .map_err(|err| {
                internal_error(format!(
                    "failed to restore thread settings after revert: {err}"
                ))
            })?;
        Self::set_app_server_client_info(
            codex_thread.as_ref(),
            app_server_client_name,
            app_server_client_version,
        )
        .await?;
        let SessionConfiguredEvent { rollout_path, .. } = session_configured;
        let rollout_path = rollout_path.ok_or_else(|| {
            internal_error(format!(
                "rollout path missing after reloading thread {thread_id}"
            ))
        })?;
        let thread_state = self.thread_state_manager.thread_state(thread_id).await;
        self.ensure_listener_task_running(thread_id, Arc::clone(&codex_thread), thread_state)
            .await?;
        let mut thread = self
            .load_thread_from_resume_source_or_send_internal(
                thread_id,
                codex_thread.as_ref(),
                &response_history,
                rollout_path.as_path(),
                Some(resume_source_thread),
                /*include_turns*/ false,
            )
            .await
            .map_err(internal_error)?;
        self.thread_watch_manager
            .upsert_thread(thread.clone())
            .await;
        let thread_status = self
            .thread_watch_manager
            .loaded_status_for_thread(&thread.id)
            .await;
        set_thread_status_and_interrupt_stale_turns(
            &mut thread,
            thread_status,
            /*has_live_in_progress_turn*/ false,
        );
        let (turns_backwards_cursor, items_backwards_cursor) =
            Self::paginated_resume_backwards_cursors(self.thread_store.as_ref(), thread_id).await?;
        Ok((
            ThreadRevertResponse {
                thread,
                turns_backwards_cursor,
                items_backwards_cursor,
            },
            codex_thread,
        ))
    }
}
