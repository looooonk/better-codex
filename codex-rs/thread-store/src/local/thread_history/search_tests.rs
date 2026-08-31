use pretty_assertions::assert_eq;

use super::*;

#[test]
fn occurrence_search_inputs_are_bounded() {
    let mut params = SearchThreadOccurrencesParams {
        thread_id: ThreadId::default(),
        search_term: "x".repeat(MAX_THREAD_HISTORY_INPUT_BYTES + 1),
        cursor: None,
        page_size: 1,
    };
    let error = validate_search_request(&params).expect_err("reject oversized search term");
    assert_eq!(
        error.to_string(),
        "invalid thread-store request: thread/searchOccurrences search_term cannot exceed 65536 bytes"
    );

    params.search_term = "needle".to_string();
    params.page_size = MAX_THREAD_OCCURRENCE_PAGE_SIZE + 1;
    let error = validate_search_request(&params).expect_err("reject oversized page");
    assert_eq!(
        error.to_string(),
        "invalid thread-store request: thread/searchOccurrences page_size cannot exceed 250"
    );

    params.page_size = 1;
    params.cursor = Some("x".repeat(MAX_THREAD_HISTORY_INPUT_BYTES + 1));
    let error = validate_search_request(&params).expect_err("reject oversized cursor");
    assert_eq!(
        error.to_string(),
        "invalid thread-store request: thread/searchOccurrences cursor cannot exceed 65536 bytes"
    );
}
