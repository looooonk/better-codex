use std::sync::Arc;

use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionRequestId;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::QueuedSubmission;
use codex_app_server_protocol::ThreadQueueAddParams;
use codex_app_server_protocol::ThreadQueueAddResponse;
use codex_app_server_protocol::ThreadQueueDeleteParams;
use codex_app_server_protocol::ThreadQueueDeleteResponse;
use codex_app_server_protocol::ThreadQueueListParams;
use codex_app_server_protocol::ThreadQueueListResponse;
use codex_app_server_protocol::ThreadQueueReorderParams;
use codex_app_server_protocol::ThreadQueueReorderResponse;
use codex_app_server_protocol::ThreadQueueStartParams;
use codex_app_server_protocol::ThreadQueueStartResponse;
use codex_app_server_protocol::ThreadQueueUpdateParams;
use codex_app_server_protocol::ThreadQueueUpdateResponse;
use codex_state::QueuedSubmissionState;

use super::thread_queue_service::ThreadQueueService;
use super::thread_queue_support::QUEUE_LIST_DEFAULT_LIMIT;
use super::thread_queue_support::QUEUE_LIST_MAX_LIMIT;
use super::thread_queue_support::api_queued_submission;
use super::thread_queue_support::ensure_direct_input_allowed;
use super::thread_queue_support::parse_cursor;
use super::thread_queue_support::prepare_payload;
use super::thread_queue_support::queue_error;
use super::thread_queue_support::queue_turn;

#[derive(Clone)]
pub(crate) struct ThreadQueueRequestProcessor {
    service: Arc<ThreadQueueService>,
}

impl ThreadQueueRequestProcessor {
    pub(crate) fn new(service: Arc<ThreadQueueService>) -> Self {
        Self { service }
    }

    pub(crate) async fn add(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadQueueAddParams,
    ) -> Result<(), JSONRPCErrorError> {
        let requested_input = params.input.clone();
        let (thread_id, loaded_thread, source) =
            self.service.require_thread(&params.thread_id).await?;
        ensure_direct_input_allowed(loaded_thread.as_deref(), &source)?;
        self.service
            .recover_before_queue_access(thread_id, loaded_thread.as_deref())
            .await?;
        let payload = prepare_payload(&params.input)?;
        let record = self
            .service
            .state_db()?
            .enqueue_queued_submission(thread_id, &payload, &params.client_user_message_id)
            .await
            .map_err(queue_error)?;
        let should_wake = record.state == QueuedSubmissionState::Pending;
        self.service
            .send_changed_response(
                request_id,
                ThreadQueueAddResponse {
                    queued_submission: QueuedSubmission {
                        id: record.id,
                        input: requested_input,
                        client_user_message_id: record.client_user_message_id,
                    },
                }
                .into(),
                thread_id,
            )
            .await;
        if should_wake {
            self.service.wake_if_loaded(thread_id).await;
        }
        Ok(())
    }

    pub(crate) async fn list(
        &self,
        params: ThreadQueueListParams,
    ) -> Result<ThreadQueueListResponse, JSONRPCErrorError> {
        let (thread_id, loaded_thread, _) = self.service.require_thread(&params.thread_id).await?;
        self.service
            .recover_before_queue_access(thread_id, loaded_thread.as_deref())
            .await?;
        let offset = parse_cursor(params.cursor.as_deref())?;
        let limit = params
            .limit
            .map(|limit| limit as usize)
            .unwrap_or(QUEUE_LIST_DEFAULT_LIMIT)
            .clamp(1, QUEUE_LIST_MAX_LIMIT);
        let mut records = self
            .service
            .state_db()?
            .list_queued_submissions(thread_id, offset, limit.saturating_add(1))
            .await
            .map_err(queue_error)?;
        let next_cursor = if records.len() > limit {
            records.truncate(limit);
            Some(offset.saturating_add(limit).to_string())
        } else {
            None
        };
        Ok(ThreadQueueListResponse {
            data: records
                .into_iter()
                .map(api_queued_submission)
                .collect::<Result<Vec<_>, _>>()?,
            next_cursor,
        })
    }

    pub(crate) async fn update(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadQueueUpdateParams,
    ) -> Result<(), JSONRPCErrorError> {
        let (thread_id, loaded_thread, source) =
            self.service.require_thread(&params.thread_id).await?;
        ensure_direct_input_allowed(loaded_thread.as_deref(), &source)?;
        self.service
            .recover_before_queue_access(thread_id, loaded_thread.as_deref())
            .await?;
        let payload = prepare_payload(&params.input)?;
        let record = self
            .service
            .state_db()?
            .update_queued_submission(thread_id, &params.queued_submission_id, &payload)
            .await
            .map_err(queue_error)?
            .ok_or_else(|| {
                invalid_request(format!(
                    "queued submission not found: {}",
                    params.queued_submission_id
                ))
            })?;
        self.service
            .send_changed_response(
                request_id,
                ThreadQueueUpdateResponse {
                    queued_submission: api_queued_submission(record)?,
                }
                .into(),
                thread_id,
            )
            .await;
        Ok(())
    }

    pub(crate) async fn delete(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadQueueDeleteParams,
    ) -> Result<(), JSONRPCErrorError> {
        let (thread_id, loaded_thread, _) = self.service.require_thread(&params.thread_id).await?;
        self.service
            .recover_before_queue_access(thread_id, loaded_thread.as_deref())
            .await?;
        let deleted = self
            .service
            .state_db()?
            .delete_queued_submission(thread_id, &params.queued_submission_id)
            .await
            .map_err(queue_error)?;
        let response = ThreadQueueDeleteResponse { deleted };
        if deleted {
            self.service
                .send_changed_response(request_id, response.into(), thread_id)
                .await;
            self.service.wake_if_loaded(thread_id).await;
        } else {
            self.service
                .outgoing()
                .send_response_as(request_id, response.into())
                .await;
        }
        Ok(())
    }

    pub(crate) async fn reorder(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadQueueReorderParams,
    ) -> Result<(), JSONRPCErrorError> {
        let (thread_id, loaded_thread, _) = self.service.require_thread(&params.thread_id).await?;
        self.service
            .recover_before_queue_access(thread_id, loaded_thread.as_deref())
            .await?;
        self.service
            .state_db()?
            .reorder_queued_submissions(thread_id, &params.queued_submission_ids)
            .await
            .map_err(queue_error)?;
        self.service
            .send_changed_response(request_id, ThreadQueueReorderResponse {}.into(), thread_id)
            .await;
        Ok(())
    }

    pub(crate) async fn start(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadQueueStartParams,
    ) -> Result<(), JSONRPCErrorError> {
        let (thread_id, loaded_thread, source) =
            self.service.require_thread(&params.thread_id).await?;
        ensure_direct_input_allowed(loaded_thread.as_deref(), &source)?;
        self.service
            .recover_before_queue_access(thread_id, loaded_thread.as_deref())
            .await?;
        let thread = loaded_thread.ok_or_else(|| {
            invalid_request("resume/subscribe the thread before starting a queued message")
        })?;
        let trace = self
            .service
            .outgoing()
            .request_trace_context(&request_id)
            .await;
        match self
            .service
            .start_explicit(
                thread_id,
                thread.as_ref(),
                params.queued_submission_id.as_deref(),
                trace,
            )
            .await
        {
            Ok(started) => {
                self.service
                    .outgoing()
                    .record_request_turn_id(&request_id, &started.turn_id)
                    .await;
                let response = ThreadQueueStartResponse {
                    turn: queue_turn(started.turn_id, started.status),
                };
                if started.queue_changed {
                    self.service
                        .send_changed_response(request_id, response.into(), thread_id)
                        .await;
                } else {
                    self.service
                        .outgoing()
                        .send_response_as(request_id, response.into())
                        .await;
                }
                self.service.wake_if_loaded(thread_id).await;
            }
            Err(failure) => {
                self.service
                    .outgoing()
                    .send_error(request_id, failure.error)
                    .await;
                if failure.queue_changed {
                    self.service.notify_changed(thread_id).await;
                }
            }
        }
        Ok(())
    }
}
