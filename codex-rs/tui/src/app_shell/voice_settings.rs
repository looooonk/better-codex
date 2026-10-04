use super::backend::app_shell_request_id;
use crate::config_update::replace_config_value;
use crate::config_update::write_config_batch;
use crate::legacy_core::config::edit::ConfigEdit;
use crate::legacy_core::config::edit::ConfigEditsBuilder;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadRealtimeListVoicesParams;
use codex_app_server_protocol::ThreadRealtimeListVoicesResponse;
use codex_realtime_webrtc::AudioDeviceKind;
use codex_realtime_webrtc::AudioDeviceSelection;
use codex_realtime_webrtc::VoiceHost;
use codex_utils_absolute_path::AbsolutePathBuf;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use std::num::NonZeroU16;
use std::time::Duration;

pub(super) struct SettingsResult {
    pub(super) message: String,
    pub(super) devices: AudioDeviceSelection,
}

pub(super) async fn execute(
    client: AppServerRequestHandle,
    path: AbsolutePathBuf,
    mut devices: AudioDeviceSelection,
    args: String,
) -> Result<SettingsResult> {
    let message = tokio::time::timeout(Duration::from_secs(/*secs*/ 30), async {
        let (key, value) = args.split_once(' ').map_or((args.as_str(), ""), |(key, value)| (key, value.trim()));
        match key {
            "" | "voice" => {
                let response: ThreadRealtimeListVoicesResponse = client.request_typed(ClientRequest::ThreadRealtimeListVoices {
                    request_id: app_shell_request_id("voice-list"),
                    params: ThreadRealtimeListVoicesParams {},
                }).await?;
                if key.is_empty() {
                    return Ok::<String, color_eyre::Report>(settings_summary(&response, &devices));
                }
                let selected = if value == "default" { serde_json::Value::Null }
                    else {
                        let voice = response.voices.v1.iter().find(|voice| voice.wire_name() == value)
                            .ok_or_else(|| eyre!("Unknown voice. Use /voice settings to list available voices."))?;
                        serde_json::json!(voice)
                    };
                let saved = write_config_batch(client, vec![replace_config_value("realtime.voice", selected)]).await?;
                if saved.status == codex_app_server_protocol::WriteStatus::OkOverridden {
                    return Ok("Voice preference saved, but another configuration layer overrides it.".to_string());
                }
                Ok(format!("Voice preference saved: {value}. It applies to your next voice conversation."))
            }
            "devices" => {
                let install = codex_install_context::InstallContext::current();
                let package = install.package_layout.as_ref().ok_or_else(|| eyre!("Voice runtime is unavailable"))?;
                let mut host = VoiceHost::connect(package, codex_build_info::BuildInfo::get().build_commit())
                    .await.map_err(|_| eyre!("Could not list audio devices. Check the voice installation."))?;
                let mut lines = Vec::new();
                for (label, kind) in [("Microphones", AudioDeviceKind::Input), ("Speakers", AudioDeviceKind::Output)] {
                    let available = host.list_devices(kind).await.map_err(|_| eyre!("Could not list {label}"))?;
                    lines.push(label.to_string());
                    for device in available {
                        let default = if device.is_default { " (default)" } else { "" };
                        lines.push(format!("{}{}: {} channels", device.name, default, device.channels));
                    }
                }
                host.close().await.map_err(|_| eyre!("Could not close voice device lookup"))?;
                Ok(lines.join("\n"))
            }
            "microphone" | "speaker" | "channels" if !value.is_empty() => {
                let config_key = if key == "channels" { "microphone_channel" } else { key };
                let segments = vec!["audio".to_string(), config_key.to_string()];
                let preference = if value == "default" { None } else { Some(value) };
                let channels = if key == "channels" { parse_channels(preference)? } else { None };
                let edit = match preference {
                    None => ConfigEdit::ClearPath { segments },
                    Some(value) => ConfigEdit::SetPath {
                        segments,
                        value: if let Some(channels) = &channels {
                            channels.iter().map(|channel| i64::from(channel.get())).collect::<toml_edit::Array>().into()
                        } else { value.into() },
                    },
                };
                let mut edits = vec![edit];
                if key == "microphone" {
                    edits.push(ConfigEdit::ClearPath { segments: vec!["audio".into(), "microphone_channel".into()] });
                }
                // Capture devices belong to this machine, including for a remote app server.
                ConfigEditsBuilder::for_config_path(path.as_path()).with_edits(edits).apply().await.map_err(|error| eyre!(error.to_string()))?;
                match key {
                    "microphone" => { devices.microphone = preference.map(str::to_string); devices.channel = None; }
                    "speaker" => devices.speaker = preference.map(str::to_string),
                    "channels" => devices.channel = channels,
                    _ => unreachable!(),
                }
                Ok(format!("Voice {key} saved: {value}. It applies to your next voice conversation."))
            }
            _ => Ok("Usage: /voice settings [voice <name|default>|devices|microphone <name|default>|speaker <name|default>|channels <1,2|default>]".to_string()),
        }
    }).await??;
    Ok(SettingsResult { message, devices })
}

pub(super) fn settings_summary(
    response: &ThreadRealtimeListVoicesResponse,
    devices: &AudioDeviceSelection,
) -> String {
    let names = response
        .voices
        .v1
        .iter()
        .map(|voice| voice.wire_name())
        .collect::<Vec<_>>()
        .join(", ");
    let microphone = devices.microphone.as_deref().unwrap_or("system default");
    let speaker = devices.speaker.as_deref().unwrap_or("system default");
    let channels = devices.channel.as_ref().map_or_else(
        || "all".to_string(),
        |channels| {
            channels
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    format!(
        "Voice settings\nVoices: {names}\nDefault voice: {}\nMicrophone: {microphone}\nSpeaker: {speaker}\nMicrophone channels: {channels}\n\n/voice settings voice <name|default>\n/voice settings devices\n/voice settings microphone <name|default>\n/voice settings speaker <name|default>\n/voice settings channels <1,2|default>\n/voice [on|off|mute|unmute]",
        response.voices.default_v1.wire_name()
    )
}

fn parse_channels(value: Option<&str>) -> Result<Option<Vec<NonZeroU16>>> {
    value
        .map(|value| {
            let channels: Vec<_> = value
                .split(',')
                .map(|channel| channel.trim().parse::<NonZeroU16>())
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| {
                    eyre!("Microphone channels must be positive integers separated by commas")
                })?;
            if channels.len() > 32 {
                return Err(eyre!("Select at most 32 microphone channels"));
            }
            Ok(channels)
        })
        .transpose()
}

#[cfg(test)]
#[path = "voice_settings_tests.rs"]
mod tests;
