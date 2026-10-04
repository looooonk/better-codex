use super::*;
use crate::app_shell::TranscriptKind;
use crate::app_shell::TranscriptLine;
use crate::app_shell::render::ShellView;
use codex_app_server_protocol::ThreadRealtimeListVoicesResponse;
use codex_protocol::protocol::RealtimeVoice;
use codex_protocol::protocol::RealtimeVoicesList;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[tokio::test]
async fn completed_settings_update_preferences_and_render_controls() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.dashboard_visible = false;
    let devices = AudioDeviceSelection {
        microphone: Some("Desk microphone".to_string()),
        speaker: Some("Headphones".to_string()),
        channel: Some(vec![std::num::NonZeroU16::new(2).unwrap()]),
    };
    let response = ThreadRealtimeListVoicesResponse {
        voices: RealtimeVoicesList {
            v1: vec![RealtimeVoice::Juniper, RealtimeVoice::Sol],
            v2: vec![],
            default_v1: RealtimeVoice::Juniper,
            default_v2: RealtimeVoice::Cedar,
        },
    };
    let message = voice_settings::settings_summary(&response, &devices);
    let result = voice_settings::SettingsResult {
        message: message.clone(),
        devices: devices.clone(),
    };
    shell.voice.settings = Some(tokio::spawn(async { Ok(result) }));
    while !shell.voice.settings.as_ref().unwrap().is_finished() {
        tokio::task::yield_now().await;
    }
    assert!(shell.poll_voice().await);
    assert_eq!(shell.voice.devices, devices);
    assert_eq!(
        shell.transcript,
        std::collections::VecDeque::from([TranscriptLine::new(TranscriptKind::System, message)])
    );
    assert!(!shell.voice.has_work());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 31,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let lines = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    insta::assert_snapshot!(lines.join("\n"));
}

#[tokio::test]
async fn failed_settings_preserve_current_devices() {
    let mut shell = ShellState::snapshot_fixture();
    let devices = AudioDeviceSelection {
        microphone: Some("Current mic".to_string()),
        ..Default::default()
    };
    shell.voice.devices = devices.clone();
    shell.voice.settings = Some(tokio::spawn(async { Err(eyre!("settings unavailable")) }));
    while !shell.voice.settings.as_ref().unwrap().is_finished() {
        tokio::task::yield_now().await;
    }
    assert!(shell.poll_voice().await);
    assert_eq!(shell.voice.devices, devices);
    assert!(!shell.poll_voice().await);
}

#[test]
fn a_new_session_keeps_audio_devices_without_call_status() {
    let voice = VoiceState {
        devices: AudioDeviceSelection {
            microphone: Some("Desk microphone".to_string()),
            ..Default::default()
        },
        connected: true,
        muted: true,
        stopping: true,
        ..Default::default()
    };
    let next = voice.for_new_session();
    assert_eq!(
        (next.devices, next.connected, next.muted, next.stopping),
        (voice.devices, false, false, false)
    );
}
