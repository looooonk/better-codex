use super::backend::app_shell_request_id;
use anyhow::Result;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::ThreadRealtimeListVoicesParams;
use codex_app_server_protocol::ThreadRealtimeListVoicesResponse;
use codex_app_server_protocol::ThreadRealtimeStartParams;
use codex_app_server_protocol::ThreadRealtimeStartResponse;
use codex_app_server_protocol::ThreadRealtimeStartTransport;
use codex_app_server_protocol::ThreadRealtimeStopParams;
use codex_app_server_protocol::ThreadRealtimeStopResponse;
use codex_protocol::protocol::RealtimeConversationVersion;
use codex_protocol::protocol::RealtimeOutputModality;
use codex_protocol::protocol::RealtimeVoice;
use codex_realtime_webrtc::AudioDeviceSelection;
use codex_realtime_webrtc::RealtimeWebrtcSession;
use codex_realtime_webrtc::RealtimeWebrtcSessionHandle;
use futures::future::AbortHandle;
use futures::future::AbortRegistration;
use futures::future::Abortable;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 30);

pub(super) enum VoiceEvent {
    Transport(RealtimeWebrtcSessionHandle),
    Connected,
    StopDelayed,
}

pub(super) struct VoiceSession {
    pub(super) thread_id: String,
    pub(super) transport: Option<RealtimeWebrtcSessionHandle>,
    pub(super) answers: mpsc::Sender<String>,
    pub(super) events: mpsc::Receiver<VoiceEvent>,
    pub(super) worker: JoinHandle<Result<()>>,
    abort: AbortHandle,
    native_abort: AbortHandle,
    stopped: mpsc::Sender<()>,
}

impl VoiceSession {
    pub(super) fn start(
        client: AppServerRequestHandle,
        thread_id: String,
        cwd: String,
        devices: AudioDeviceSelection,
    ) -> Self {
        let (abort, registration) = AbortHandle::new_pair();
        let (native_abort, native_registration) = AbortHandle::new_pair();
        let (answers, answer_rx) = mpsc::channel(/*buffer*/ 2);
        let (events_tx, events) = mpsc::channel(/*buffer*/ 2);
        let (stopped, mut stop_confirmation) = mpsc::channel(/*buffer*/ 1);
        let owner_thread = thread_id.clone();
        let worker_native_abort = native_abort.clone();
        let worker = tokio::spawn(async move {
            let lifecycle_events = events_tx.clone();
            let mut requested = false;
            let result = Abortable::new(
                run(
                    &client,
                    &owner_thread,
                    cwd,
                    devices,
                    native_registration,
                    answer_rx,
                    events_tx,
                    &mut requested,
                ),
                registration,
            )
            .await;
            worker_native_abort.abort();
            // The RPC only queues shutdown. Keep ownership until its close event arrives.
            if requested && stop_confirmation.try_recv().is_err() {
                let stopped = tokio::time::timeout(
                    REQUEST_TIMEOUT,
                    client.request_typed::<ThreadRealtimeStopResponse>(
                        ClientRequest::ThreadRealtimeStop {
                            request_id: app_shell_request_id("voice-stop"),
                            params: ThreadRealtimeStopParams {
                                thread_id: owner_thread,
                            },
                        },
                    ),
                )
                .await;
                if (matches!(stopped, Ok(Ok(_))) || stopped.is_err())
                    && tokio::time::timeout(REQUEST_TIMEOUT, stop_confirmation.recv())
                        .await
                        .is_err()
                {
                    let _ = lifecycle_events.send(VoiceEvent::StopDelayed).await;
                    let _ = stop_confirmation.recv().await;
                }
                if matches!(result, Ok(Ok(())) | Err(_)) {
                    stopped??;
                }
            }
            result.unwrap_or(Ok(()))
        });
        Self {
            thread_id,
            transport: None,
            answers,
            events,
            worker,
            abort,
            native_abort,
            stopped,
        }
    }

    pub(super) fn stop(&self) {
        self.native_abort.abort();
        self.abort.abort();
        if let Some(transport) = &self.transport {
            transport.close();
        }
    }

    pub(super) fn confirm_stop(&self) {
        let _ = self.stopped.try_send(());
    }
}

impl Drop for VoiceSession {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn run(
    client: &AppServerRequestHandle,
    thread_id: &str,
    cwd: String,
    devices: AudioDeviceSelection,
    native_abort: AbortRegistration,
    mut answers: mpsc::Receiver<String>,
    events: mpsc::Sender<VoiceEvent>,
    requested: &mut bool,
) -> Result<()> {
    let voice = tokio::time::timeout(REQUEST_TIMEOUT, async {
        let voices: ThreadRealtimeListVoicesResponse = client
            .request_typed(ClientRequest::ThreadRealtimeListVoices {
                request_id: app_shell_request_id("voice-list"),
                params: ThreadRealtimeListVoicesParams {},
            })
            .await?;
        let config: ConfigReadResponse = client
            .request_typed(ClientRequest::ConfigRead {
                request_id: app_shell_request_id("voice-config"),
                params: ConfigReadParams {
                    include_layers: false,
                    cwd: Some(cwd),
                },
            })
            .await?;
        let preference = config
            .config
            .additional
            .get("realtime")
            .and_then(|value| value.get("voice"));
        let voice = match preference {
            None | Some(serde_json::Value::Null) => Some(voices.voices.default_v1),
            Some(value) => serde_json::from_value(value.clone()).ok(),
        };
        Ok::<_, anyhow::Error>(voice)
    })
    .await??;
    let native =
        tokio::task::spawn_blocking(move || RealtimeWebrtcSession::start(native_abort, devices))
            .await??;
    events
        .send(VoiceEvent::Transport(native.handle.clone()))
        .await?;
    *requested = true;
    tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.request_typed::<ThreadRealtimeStartResponse>(ClientRequest::ThreadRealtimeStart {
            request_id: app_shell_request_id("voice-start"),
            params: start_params(thread_id.to_string(), native.offer_sdp, voice),
        }),
    )
    .await??;
    let answer = tokio::time::timeout(REQUEST_TIMEOUT, answers.recv())
        .await?
        .ok_or_else(|| anyhow::anyhow!("voice signalling disconnected"))?;
    let transport = native.handle.clone();
    tokio::task::spawn_blocking(move || transport.apply_answer_sdp(answer)).await??;
    events.send(VoiceEvent::Connected).await?;
    let mut health = tokio::time::interval(Duration::from_millis(/*millis*/ 100));
    loop {
        health.tick().await;
        if let Some(error) = native.handle.take_error() {
            anyhow::bail!(error);
        }
    }
}

fn start_params(
    thread_id: String,
    sdp: String,
    voice: Option<RealtimeVoice>,
) -> ThreadRealtimeStartParams {
    ThreadRealtimeStartParams {
        thread_id,
        client_managed_handoffs: Some(false),
        delegation_ack_filler: None,
        flush_transcript_tail_on_session_end: Some(true),
        codex_responses_as_items: None,
        codex_response_item_prefix: None,
        codex_response_handoff_mode: None,
        backend_reasoning_status: false,
        codex_response_handoff_channel_prefixes: None,
        model: None,
        output_modality: RealtimeOutputModality::Audio,
        include_startup_context: Some(true),
        initial_items: None,
        realtime_start_instructions: None,
        realtime_end_instructions: None,
        prompt: None,
        realtime_session_id: None,
        transport: Some(ThreadRealtimeStartTransport::Webrtc { sdp }),
        version: Some(RealtimeConversationVersion::V3),
        voice,
    }
}

#[cfg(test)]
#[path = "voice_session_tests.rs"]
mod tests;
