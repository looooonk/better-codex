use super::*;
use crate::app_shell::render::ShellView;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadRealtimeItemCompletedNotification;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn resumed_speech_keeps_canonical_order_without_duplicate_turn_items() {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.dashboard_visible = false;
    let reply = ThreadItem::AgentMessage {
        id: "reply".to_string(),
        text: "The fix is ready.".to_string(),
        phase: None,
        memory_citation: None,
        delivery: None,
        questions: None,
    };
    let speech = ThreadRealtimeItem {
        id: "speech".to_string(),
        realtime_session_id: "voice".to_string(),
        content: ThreadRealtimeItemContent::TranscriptSegment {
            role: ThreadRealtimeTranscriptRole::User,
            text: "Please fix the bug.".to_string(),
        },
    };
    let turn = Turn {
        id: "turn".to_string(),
        items: vec![reply.clone()],
        items_view: TurnItemsView::Full,
        status: TurnStatus::Completed,
        error: None,
        started_at: None,
        completed_at: None,
        duration_ms: None,
    };
    shell.ingest_thread_history(
        vec![turn],
        Some(ThreadTimeline {
            has_older: false,
            entries: vec![
                ThreadTimelineEntry::Realtime {
                    position: 1,
                    item: speech.clone(),
                },
                ThreadTimelineEntry::TurnStarted {
                    position: 2,
                    turn_id: "turn".to_string(),
                    started_at: None,
                },
                ThreadTimelineEntry::Item {
                    position: 3,
                    turn_id: "turn".to_string(),
                    item: Box::new(reply),
                },
                ThreadTimelineEntry::Realtime {
                    position: 4,
                    item: ThreadRealtimeItem {
                        id: "spoken-reply".to_string(),
                        realtime_session_id: "voice".to_string(),
                        content: ThreadRealtimeItemContent::TranscriptSegment {
                            role: ThreadRealtimeTranscriptRole::Assistant,
                            text: "Ready to review.".to_string(),
                        },
                    },
                },
            ],
        }),
    );
    let expected: std::collections::VecDeque<_> = [
        TranscriptLine::new(TranscriptKind::User, "Voice: Please fix the bug.")
            .item_id("realtime:speech"),
        TranscriptLine::new(TranscriptKind::Assistant, "The fix is ready.").item_id("reply"),
        TranscriptLine::new(TranscriptKind::Assistant, "Voice: Ready to review.")
            .item_id("realtime:spoken-reply"),
    ]
    .into();
    assert_eq!(shell.transcript, expected);
    shell.handle_voice_notification(&ServerNotification::ThreadRealtimeItemCompleted(
        ThreadRealtimeItemCompletedNotification {
            thread_id: shell.thread_id.to_string(),
            item: speech.clone(),
        },
    ));
    shell.handle_voice_notification(&ServerNotification::ThreadRealtimeItemCompleted(
        ThreadRealtimeItemCompletedNotification {
            thread_id: "another-thread".to_string(),
            item: ThreadRealtimeItem {
                id: "unrelated".to_string(),
                ..speech
            },
        },
    ));
    assert_eq!(shell.transcript, expected);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 24,
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
