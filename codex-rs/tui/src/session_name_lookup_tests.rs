use super::*;
use pretty_assertions::assert_eq;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Default)]
struct Reader {
    pages: Mutex<VecDeque<ThreadListResponse>>,
    current: Mutex<VecDeque<Thread>>,
    requests: Mutex<Vec<ThreadListParams>>,
}
impl SessionNameReader for Reader {
    async fn list(
        &self,
        params: ThreadListParams,
    ) -> Result<ThreadListResponse, TypedRequestError> {
        self.requests.lock().unwrap().push(params);
        Ok(self
            .pages
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra page"))
    }
    async fn read(&self, thread_id: String) -> Result<Thread, TypedRequestError> {
        let thread = self
            .current
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected read");
        assert_eq!(thread.id, thread_id);
        Ok(thread)
    }
}
fn page(cursor: Option<String>) -> ThreadListResponse {
    ThreadListResponse {
        data: Vec::new(),
        next_cursor: cursor,
        backwards_cursor: None,
    }
}
fn reader(pages: impl IntoIterator<Item = ThreadListResponse>) -> Reader {
    Reader {
        pages: Mutex::new(pages.into_iter().collect()),
        ..Reader::default()
    }
}
fn session() -> Thread {
    serde_json::from_value(serde_json::json!({
        "id":"01987b77-33b8-76e3-9a7f-a367513be004", "sessionId":"session", "name":"requested name",
        "preview":"", "ephemeral":false, "modelProvider":"openai", "createdAt":0, "updatedAt":0,
        "status":{"type":"idle"}, "cwd":"/workspace", "cliVersion":"0.160.0", "source":"exec", "turns":[], "projectId":null
    })).unwrap()
}

#[tokio::test]
async fn cursor_cycle_fails_without_repeating_the_request() {
    let reader = reader([
        page(Some("a".into())),
        page(Some("b".into())),
        page(Some("a".into())),
    ]);
    assert!(
        lookup_with_reader(&reader, "missing", /*archived*/ false)
            .await
            .unwrap_err()
            .to_string()
            .contains("repeated a session cursor")
    );
    assert_eq!(
        reader
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.cursor.clone())
            .collect::<Vec<_>>(),
        vec![None, Some("a".into()), Some("b".into())]
    );
}

#[tokio::test]
async fn unique_cursor_stream_stops_at_the_page_budget() {
    let reader = reader((0..MAX_PAGES).map(|page_index| page(Some(page_index.to_string()))));
    assert!(
        lookup_with_reader(&reader, "missing", /*archived*/ true)
            .await
            .unwrap_err()
            .to_string()
            .contains("session UUID")
    );
    assert_eq!(reader.requests.lock().unwrap().len(), MAX_PAGES);
}

#[tokio::test]
async fn exact_label_includes_exec_sessions_and_is_verified_before_use() {
    let exact = session();
    let mut partial = exact.clone();
    partial.name = Some("requested name suffix".into());
    let reader = reader([ThreadListResponse {
        data: vec![partial, exact.clone()],
        next_cursor: None,
        backwards_cursor: None,
    }]);
    reader.current.lock().unwrap().push_back(exact.clone());
    assert_eq!(
        lookup_with_reader(&reader, "requested name", /*archived*/ false)
            .await
            .unwrap(),
        Some(exact)
    );
    assert_eq!(
        reader
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| (
                request.search_term.clone(),
                request.cursor.clone(),
                request.source_kinds.clone()
            ))
            .collect::<Vec<_>>(),
        vec![(
            None,
            None,
            Some(crate::resume_source_kinds(
                /*include_non_interactive*/ true
            ))
        )]
    );
}

#[tokio::test]
async fn duplicate_names_and_paginated_matches_require_uuid() {
    let first = session();
    let mut second = first.clone();
    second.id = "01987b77-33b8-76e3-9a7f-a367513be005".into();
    let reader = reader([ThreadListResponse {
        data: vec![first.clone(), second.clone()],
        next_cursor: None,
        backwards_cursor: None,
    }]);
    reader
        .current
        .lock()
        .unwrap()
        .extend([first.clone(), second]);
    assert!(
        lookup_with_reader(&reader, "requested name", /*archived*/ false)
            .await
            .unwrap_err()
            .to_string()
            .contains("multiple sessions")
    );
    let reader = self::reader([
        page(Some("older".into())),
        ThreadListResponse {
            data: vec![first.clone()],
            next_cursor: None,
            backwards_cursor: None,
        },
    ]);
    reader.current.lock().unwrap().push_back(first);
    assert!(
        lookup_with_reader(&reader, "requested name", /*archived*/ false)
            .await
            .unwrap_err()
            .to_string()
            .contains("cannot verify a unique")
    );
}

#[tokio::test]
async fn renamed_session_is_not_used_from_stale_list_metadata() {
    let listed = session();
    let mut current = listed.clone();
    current.name = Some("renamed in another client".into());
    let reader = reader([ThreadListResponse {
        data: vec![listed],
        next_cursor: None,
        backwards_cursor: None,
    }]);
    reader.current.lock().unwrap().push_back(current);
    assert_eq!(
        lookup_with_reader(&reader, "requested name", /*archived*/ false)
            .await
            .unwrap(),
        None
    );
}
