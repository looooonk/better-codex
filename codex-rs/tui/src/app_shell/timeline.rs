use super::CompletedItemOrigin;
use super::ShellState;
use super::TranscriptKind;
use super::TranscriptLine;
use super::rewind::RewindAnchor;
use crate::app_server_session::ThreadTimeline;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::ThreadItemPayload;
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
        for entry in &timeline.entries {
            if let ThreadTimelineEntry::TurnCompleted {
                turn_id,
                output_tokens: Some(tokens),
                ..
            } = entry
            {
                self.turn_outputs.insert(turn_id.clone(), *tokens);
            }
        }
        // Retain the normal turn history when the optional timeline only supplied usage.
        if !turns.is_empty()
            && !timeline
                .entries
                .iter()
                .any(|entry| matches!(entry, ThreadTimelineEntry::Realtime { .. }))
        {
            self.ingest_turn_history(turns);
            return;
        }
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
        let turns_by_id: HashMap<_, _> =
            turns.iter().map(|turn| (turn.id.as_str(), turn)).collect();
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
                    let origin = if turns_by_id
                        .get(turn_id.as_str())
                        .is_some_and(|turn| turn.status != TurnStatus::InProgress)
                        && matches!(
                            item.payload,
                            ThreadItemPayload::FileChange {
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
                ThreadTimelineEntry::TurnCompleted {
                    turn_id,
                    status,
                    error,
                    started_at,
                    completed_at,
                    duration_ms,
                    ..
                } => {
                    if let Some(error) = error {
                        self.push_error(error.message);
                    }
                    if status == TurnStatus::Interrupted {
                        self.push_status("turn interrupted");
                    }
                    let turn = turns_by_id.get(turn_id.as_str()).copied();
                    let duration = super::turn_summary::turn_duration(
                        duration_ms.or_else(|| turn.and_then(|t| t.duration_ms)),
                        started_at.or_else(|| turn.and_then(|t| t.started_at)),
                        completed_at.or_else(|| turn.and_then(|t| t.completed_at)),
                    );
                    self.push_turn_separator(&turn_id, duration);
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
