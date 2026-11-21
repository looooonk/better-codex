use super::*;
use codex_protocol::ThreadId;
use codex_protocol::protocol::TokenUsage;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn archived_usage_respects_turn_boundaries_and_preserves_the_archive() -> io::Result<()> {
    let dir = tempfile::tempdir()?;
    let plain = dir.path().join("session.jsonl");
    let archive = plain.with_extension("jsonl.zst");
    let thread = ThreadId::new();
    let mut history = vec![b' '; TAIL_BYTES as usize];
    history.push(b'\n');
    let mut ends = Vec::new();
    for (turn, count) in [("first", 20), ("first", 42), ("second", 15), ("first", 90)] {
        let usage = TokenUsage {
            output_tokens: count,
            ..Default::default()
        };
        let record = json!({
            "timestamp": "2026-11-21T00:00:00Z",
            "type": "token_usage_record",
            "payload": {
                "thread_id": thread, "session_id": thread, "turn_id": turn,
                "root_turn_id": turn, "response_id": "response", "usage": usage,
                "turn_token_usage": usage, "thread_token_usage": usage,
            }
        });
        serde_json::to_writer(&mut history, &record)?;
        history.push(b'\n');
        if count == 42 || count == 15 {
            ends.push((turn.to_owned(), history.len() as u64));
        }
    }
    // A later boundary includes a newer record for "first", beyond that turn's requested end.
    ends.push(("unreported".into(), history.len() as u64));
    let encoded = zstd::encode_all(history.as_slice(), /*level*/ 1)?;
    std::fs::write(&archive, &encoded)?;
    let expected = HashMap::from([("first".into(), 42), ("second".into(), 15)]);
    assert_eq!(read(&plain, ends).await, expected);
    assert_eq!(std::fs::read(archive)?, encoded);
    assert!(!plain.exists());
    Ok(())
}
