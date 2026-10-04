use super::ShellState;
use super::backend::AppShellBackend;
use super::voice_session::VoiceEvent;
use super::voice_session::VoiceSession;
use super::voice_settings;
use crate::legacy_core::config::Config;
use codex_app_server_protocol::ServerNotification;
use codex_realtime_webrtc::AudioDeviceSelection;
use codex_realtime_webrtc::RealtimeWebrtcSession;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use tokio::task::JoinHandle;

#[derive(Default)]
pub(super) struct VoiceState {
    session: Option<VoiceSession>,
    settings: Option<JoinHandle<Result<voice_settings::SettingsResult>>>,
    devices: AudioDeviceSelection,
    connected: bool,
    muted: bool,
    stopping: bool,
}

impl VoiceState {
    pub(super) fn for_new_session(&self) -> Self {
        Self {
            devices: self.devices.clone(),
            ..Self::default()
        }
    }

    pub(super) fn configure(&mut self, config: &Config) {
        self.devices = AudioDeviceSelection {
            microphone: config.realtime_audio.microphone.clone(),
            speaker: config.realtime_audio.speaker.clone(),
            channel: config
                .realtime_audio
                .microphone_channel
                .as_ref()
                .map(|channels| channels.as_slice().to_vec()),
        };
    }

    pub(super) fn has_work(&self) -> bool {
        self.session.is_some() || self.settings.is_some()
    }

    pub(super) fn status_label(&self) -> Option<&'static str> {
        self.session.as_ref().map(|_| {
            if self.stopping {
                "voice stopping"
            } else if self.muted {
                "voice muted"
            } else if self.connected {
                "voice live"
            } else {
                "voice connecting"
            }
        })
    }
}

impl ShellState {
    pub(super) fn toggle_voice_mute<S: AppShellBackend>(&mut self, app_server: &S) -> Result<()> {
        self.run_voice_command(if self.voice.muted { "unmute" } else { "mute" }, app_server)
    }

    pub(super) fn run_voice_command<S: AppShellBackend>(
        &mut self,
        args: &str,
        app_server: &S,
    ) -> Result<()> {
        let args = args.trim();
        let command = if args.is_empty() {
            if self.voice.session.is_some() {
                "off"
            } else {
                "on"
            }
        } else {
            args
        };
        match command {
            "off" => {
                self.stop_voice();
                self.push_system("Voice stopping");
            }
            "mute" | "unmute" => {
                let session = self
                    .voice
                    .session
                    .as_ref()
                    .ok_or_else(|| eyre!("Start voice with /voice on first"))?;
                self.voice.muted = command == "mute";
                if let Some(transport) = &session.transport {
                    transport
                        .set_microphone_muted(self.voice.muted)
                        .map_err(|error| eyre!(error.to_string()))?;
                }
                self.push_system(if self.voice.muted {
                    "Voice microphone muted"
                } else {
                    "Voice microphone unmuted"
                });
            }
            "on" => {
                if self.voice.session.is_some() {
                    self.push_system(
                        "A voice conversation is already active. Use /voice off to end it.",
                    );
                    return Ok(());
                }
                if !RealtimeWebrtcSession::is_supported() {
                    return Err(eyre!(
                        "Voice needs a Better Codex installation with its native voice runtime."
                    ));
                }
                let client = app_server
                    .app_server_request_handle()
                    .ok_or_else(|| eyre!("Voice is unavailable for this connection"))?;
                self.voice.session = Some(VoiceSession::start(
                    client,
                    self.thread_id.to_string(),
                    self.cwd.clone(),
                    self.voice.devices.clone(),
                ));
                self.voice.connected = false;
                self.voice.muted = false;
                self.voice.stopping = false;
                self.push_system("Voice connecting. Use /voice mute to mute the microphone or /voice off to end.");
            }
            command if command == "settings" || command.starts_with("settings ") => {
                if self.voice.settings.is_some() {
                    return Err(eyre!("Voice settings are still loading"));
                }
                let client = app_server
                    .app_server_request_handle()
                    .ok_or_else(|| eyre!("Voice settings are unavailable for this connection"))?;
                let settings = command
                    .strip_prefix("settings")
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let path = self.client_config_path.clone();
                let devices = self.voice.devices.clone();
                self.voice.settings = Some(tokio::spawn(voice_settings::execute(
                    client, path, devices, settings,
                )));
            }
            _ => self.push_system("Usage: /voice [on|off|mute|unmute|settings]"),
        }
        Ok(())
    }

    pub(super) fn stop_voice(&mut self) {
        if let Some(session) = &self.voice.session {
            session.stop();
            self.voice.stopping = true;
        }
    }

    pub(super) async fn poll_voice(&mut self) -> bool {
        let mut changed = false;
        let mut connected = false;
        let mut error = None;
        let mut stop_delayed = false;
        if let Some(session) = &mut self.voice.session {
            while let Ok(event) = session.events.try_recv() {
                changed = true;
                match event {
                    VoiceEvent::Transport(transport) => {
                        if self.voice.stopping {
                            transport.close();
                        } else if let Err(err) = transport.set_microphone_muted(self.voice.muted) {
                            error = Some(err.to_string());
                        }
                        session.transport = Some(transport);
                    }
                    VoiceEvent::Connected => connected = !self.voice.stopping,
                    VoiceEvent::StopDelayed => stop_delayed = true,
                }
            }
        }
        if let Some(error) = error {
            self.stop_voice();
            self.push_error(error);
        }
        if connected {
            self.voice.connected = true;
            self.push_system("Voice connected");
        }
        if stop_delayed {
            self.push_error("Voice capture has stopped; waiting for the server to close the conversation before restarting.");
        }
        if self
            .voice
            .session
            .as_ref()
            .is_some_and(|session| session.worker.is_finished())
        {
            let mut session = self.voice.session.take().expect("finished voice session");
            match (&mut session.worker).await {
                Ok(Ok(())) => self.push_system("Voice ended"),
                Ok(Err(error)) => self.push_error(format!("Voice ended: {error}")),
                Err(_) => self.push_error("Voice stopped unexpectedly"),
            }
            changed = true;
        }
        if self
            .voice
            .settings
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            match self
                .voice
                .settings
                .take()
                .expect("finished settings request")
                .await
            {
                Ok(Ok(result)) => {
                    self.voice.devices = result.devices;
                    self.push_system(result.message);
                }
                Ok(Err(error)) => self.push_error(format!("Voice settings: {error}")),
                Err(_) => self.push_error("Voice settings request stopped unexpectedly"),
            }
            changed = true;
        }
        changed
    }

    pub(super) fn handle_voice_notification(&mut self, notification: &ServerNotification) {
        if let ServerNotification::ThreadRealtimeItemCompleted(event) = notification
            && event.thread_id == self.thread_id.to_string()
        {
            self.ingest_realtime_item(event.item.clone());
        }
        let Some(owner) = self
            .voice
            .session
            .as_ref()
            .map(|session| session.thread_id.as_str())
        else {
            return;
        };
        match notification {
            ServerNotification::ThreadRealtimeSdp(event) if event.thread_id == owner => {
                if let Some(session) = &self.voice.session
                    && session.answers.try_send(event.sdp.clone()).is_err()
                {
                    self.stop_voice();
                    self.push_error("Voice signalling failed");
                }
            }
            ServerNotification::ThreadRealtimeError(event) if event.thread_id == owner => {
                let message = event.message.clone();
                self.stop_voice();
                self.push_error(format!("Voice: {message}"));
            }
            ServerNotification::ThreadRealtimeClosed(event) if event.thread_id == owner => {
                if event.reason.as_deref() == Some("requested")
                    && let Some(session) = &self.voice.session
                {
                    session.confirm_stop();
                }
                self.stop_voice();
            }
            _ => {}
        }
    }

    pub(super) fn disconnect_voice(&mut self) {
        self.stop_voice();
        if let Some(session) = &self.voice.session {
            session.confirm_stop();
        }
    }
}

#[cfg(test)]
#[path = "voice_tests.rs"]
mod tests;
