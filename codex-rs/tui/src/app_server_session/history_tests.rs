use super::*;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

struct Reader {
    pages: Mutex<VecDeque<ThreadTurnsListResponse>>,
    requests: Arc<Mutex<Vec<ThreadTurnsListParams>>>,
}

impl HistoryReader for Reader {
    async fn metadata(&self, _: ThreadId) -> Result<ThreadReadResponse, TypedRequestError> {
        Ok(serde_json::from_value(serde_json::json!({"thread": {
            "id": "01987b77-33b8-76e3-9a7f-a367513be004", "sessionId": "session",
            "preview": "", "ephemeral": false, "modelProvider": "openai", "createdAt": 0,
            "updatedAt": 0, "status": {"type":"idle"}, "cwd": "/workspace", "cliVersion": "0.160.0",
            "source": "cli", "turns": [], "projectId": null
        }}))
        .unwrap())
    }
    async fn turns(
        &self,
        params: ThreadTurnsListParams,
    ) -> Result<ThreadTurnsListResponse, TypedRequestError> {
        self.requests.lock().unwrap().push(params);
        Ok(self
            .pages
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected history request"))
    }
}

fn page(id: &str, cursor: Option<&str>) -> ThreadTurnsListResponse {
    ThreadTurnsListResponse {
        data: vec![Turn {
            id: id.to_string(),
            items: vec![],
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            items_view: Default::default(),
        }],
        next_cursor: cursor.map(str::to_string),
        backwards_cursor: None,
    }
}

#[tokio::test]
async fn full_history_reads_all_pages_in_ascending_order() {
    let first = page("one", Some("older"));
    let last = page("two", None);
    let expected = first
        .data
        .iter()
        .chain(&last.data)
        .cloned()
        .collect::<Vec<_>>();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let reader = Reader {
        pages: Mutex::new([first, last].into()),
        requests: requests.clone(),
    };
    let thread_id = ThreadId::new();
    assert_eq!(
        read_thread_history(reader, thread_id).await.unwrap().turns,
        expected
    );
    assert_eq!(
        *requests.lock().unwrap(),
        vec![
            ThreadTurnsListParams {
                thread_id: thread_id.to_string(),
                cursor: None,
                limit: Some(100),
                sort_direction: Some(SortDirection::Asc),
                items_view: Some(TurnItemsView::Full)
            },
            ThreadTurnsListParams {
                thread_id: thread_id.to_string(),
                cursor: Some("older".to_string()),
                limit: Some(100),
                sort_direction: Some(SortDirection::Asc),
                items_view: Some(TurnItemsView::Full)
            },
        ]
    );
}

#[tokio::test]
async fn repeated_history_cursor_fails_instead_of_looping() {
    let reader = Reader {
        pages: Mutex::new(
            [
                page("one", Some("a")),
                page("two", Some("b")),
                page("three", Some("a")),
            ]
            .into(),
        ),
        requests: Default::default(),
    };
    assert!(
        read_thread_history(reader, ThreadId::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("repeated history cursor")
    );
}
