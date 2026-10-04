use super::CompletedItemOrigin;
use super::ShellState;
use super::TranscriptKind;
use super::TranscriptLine;
use super::rewind::RewindAnchor;
use crate::app_server_session::ThreadTimeline;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadRealtimeItem;
use codex_app_server_protocol::ThreadRealtimeItemContent;
use codex_app_server_protocol::ThreadRealtimeSessionOutcome;
use codex_app_server_protocol::ThreadRealtimeTranscriptRole;
use codex_app_server_protocol::ThreadTimelineEntry;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use std::collections::HashMap;
use std::collections::HashSet;

impl ShellState {
    pub(super) fn ingest_thread_history(
        &mut self,
        turns: Vec<Turn>,
        timeline: Option<ThreadTimeline>,
    ) {
        self.automatic_recap.seed(&turns, std::time::Instant::now());
        let Some(timeline) = timeline else {
            self.ingest_turn_history(turns);
            return;
        };
        if timeline.has_older
            || turns
                .iter()
                .any(|turn| turn.items_view != TurnItemsView::Full)
        {
            self.diff_store.mark_history_truncated();
        }
        if timeline.has_older {
            self.push_system("Showing recent conversation history");
        }
        let statuses: HashMap<_, _> = turns
            .iter()
            .map(|turn| (turn.id.as_str(), &turn.status))
            .collect();
        let mut opening_turns = HashSet::new();
        for entry in timeline.entries {
            match entry {
                ThreadTimelineEntry::TurnStarted { turn_id, .. } => {
                    opening_turns.insert(turn_id);
                }
                ThreadTimelineEntry::Item { turn_id, item, .. } => {
                    let rewind_anchor = if opening_turns.remove(&turn_id) {
                        RewindAnchor::for_opening_item(&turn_id, &item)
                    } else {
                        None
                    };
                    let origin = if statuses
                        .get(turn_id.as_str())
                        .is_some_and(|status| **status != TurnStatus::InProgress)
                        && matches!(
                            *item,
                            ThreadItem::FileChange {
                                status: PatchApplyStatus::InProgress,
                                ..
                            }
                        ) {
                        self.diff_store.mark_history_truncated();
                        CompletedItemOrigin::UnconfirmedHistorical
                    } else {
                        CompletedItemOrigin::Historical
                    };
                    self.ingest_completed_item_for_turn(&turn_id, *item, origin, rewind_anchor);
                }
                ThreadTimelineEntry::Realtime { item, .. } => self.ingest_realtime_item(item),
                ThreadTimelineEntry::TurnCompleted { status, error, .. } => {
                    if let Some(error) = error {
                        self.push_error(error.message);
                    }
                    if status == TurnStatus::Interrupted {
                        self.push_status("turn interrupted");
                    }
                    self.push_turn_separator();
                }
            }
        }
    }

    pub(super) fn ingest_realtime_item(&mut self, item: ThreadRealtimeItem) {
        let (kind, text) = match item.content {
            ThreadRealtimeItemContent::RealtimeSessionStarted => (
                TranscriptKind::Status,
                "Voice conversation started".to_string(),
            ),
            ThreadRealtimeItemContent::TranscriptSegment { role, text } => {
                let kind = match role {
                    ThreadRealtimeTranscriptRole::User => TranscriptKind::User,
                    ThreadRealtimeTranscriptRole::Assistant => TranscriptKind::Assistant,
                };
                (kind, format!("Voice: {text}"))
            }
            ThreadRealtimeItemContent::RealtimeSessionClosed { outcome } => match outcome {
                ThreadRealtimeSessionOutcome::Ended => (
                    TranscriptKind::Status,
                    "Voice conversation ended".to_string(),
                ),
                ThreadRealtimeSessionOutcome::Failed => (
                    TranscriptKind::Error,
                    "Voice conversation failed".to_string(),
                ),
            },
            ThreadRealtimeItemContent::BemItemPromoted { .. } => return,
        };
        self.upsert_line(TranscriptLine::new(kind, text).item_id(format!("realtime:{}", item.id)));
    }
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
