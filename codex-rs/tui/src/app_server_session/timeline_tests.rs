use super::*;
use codex_app_server_protocol::ThreadRealtimeItem;
use codex_app_server_protocol::ThreadRealtimeItemContent;
use codex_app_server_protocol::ThreadRealtimeTranscriptRole;
use pretty_assertions::assert_eq;
use std::collections::VecDeque;
use std::sync::Mutex;

struct Reader {
    pages: Mutex<VecDeque<ThreadTimelineListResponse>>,
    requests: Mutex<Vec<ThreadTimelineListParams>>,
}

impl TimelineReader for Reader {
    async fn read_timeline(
        &self,
        params: ThreadTimelineListParams,
    ) -> Result<ThreadTimelineListResponse, TypedRequestError> {
        self.requests.lock().unwrap().push(params);
        Ok(self
            .pages
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected timeline request"))
    }
}

fn page(position: u64, cursor: Option<&str>) -> ThreadTimelineListResponse {
    ThreadTimelineListResponse {
        data: vec![ThreadTimelineEntry::Realtime {
            position,
            item: ThreadRealtimeItem {
                id: position.to_string(),
                realtime_session_id: "voice".to_string(),
                content: ThreadRealtimeItemContent::TranscriptSegment {
                    role: ThreadRealtimeTranscriptRole::User,
                    text: format!("speech {position}"),
                },
            },
        }],
        next_cursor: cursor.map(str::to_string),
        active_realtime_session_at_page_start: Some("voice".to_string()),
    }
}

#[tokio::test]
async fn combines_backwards_pages_in_canonical_order() {
    let newest = page(20, Some("older"));
    let oldest = page(10, None);
    let expected: Vec<_> = oldest.data.iter().chain(&newest.data).cloned().collect();
    let reader = Reader {
        pages: Mutex::new([newest, oldest].into()),
        requests: Mutex::default(),
    };
    let thread_id = ThreadId::new();
    let history = load_thread_timeline(&reader, thread_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((history.entries, history.has_older), (expected, false));
    assert_eq!(
        *reader.requests.lock().unwrap(),
        vec![
            ThreadTimelineListParams {
                thread_id: thread_id.to_string(),
                cursor: None,
                limit: Some(500)
            },
            ThreadTimelineListParams {
                thread_id: thread_id.to_string(),
                cursor: Some("older".to_string()),
                limit: Some(500)
            },
        ]
    );
}

#[tokio::test]
async fn bounds_history_and_detects_cursor_cycles() {
    let reader = Reader {
        pages: Mutex::new(
            (0..10)
                .rev()
                .map(|position| page(position, Some(&position.to_string())))
                .collect(),
        ),
        requests: Mutex::default(),
    };
    let history = load_thread_timeline(&reader, ThreadId::new())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            history.entries.len(),
            history.has_older,
            reader.requests.lock().unwrap().len()
        ),
        (4, true, 4)
    );
    let reader = Reader {
        pages: Mutex::new([page(3, Some("a")), page(2, Some("b")), page(1, Some("a"))].into()),
        requests: Mutex::default(),
    };
    assert!(
        load_thread_timeline(&reader, ThreadId::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("repeated cursor")
    );
}
