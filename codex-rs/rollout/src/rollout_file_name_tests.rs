use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;
use time::macros::datetime;

use super::RolloutFileName;

#[test]
fn canonical_rollout_file_names_round_trip() {
    let timestamp = datetime!(2026-08-11 18:42:07 UTC);
    let thread_id =
        ThreadId::from_string("019ff1a2-b3c4-7d5e-8f60-112233445566").expect("valid thread id");
    let rollout_id =
        ThreadId::from_string("019ff1a2-b3c4-7d5e-8f60-667788990011").expect("valid rollout id");

    for (file_name, expected) in [
        (
            RolloutFileName::new(timestamp, thread_id, thread_id),
            "rollout-2026-08-11T18-42-07-019ff1a2-b3c4-7d5e-8f60-112233445566.jsonl",
        ),
        (
            RolloutFileName::new(timestamp, thread_id, rollout_id),
            "rollout-2026-08-11T18-42-07-019ff1a2-b3c4-7d5e-8f60-112233445566_019ff1a2-b3c4-7d5e-8f60-667788990011.jsonl",
        ),
    ] {
        let rendered = file_name.render().expect("timestamp should format");
        assert_eq!(rendered, expected);
        assert_eq!(RolloutFileName::parse(rendered.as_str()), Some(file_name));
    }
}

#[test]
fn legacy_and_compressed_rollout_file_names_parse() {
    let expected = RolloutFileName::new(
        datetime!(2025-01-03 12:00:00 UTC),
        ThreadId::from_string("00000000-0000-0000-0000-000000000123")
            .expect("valid thread id"),
        ThreadId::from_string("00000000-0000-0000-0000-000000000123")
            .expect("valid rollout id"),
    );
    let legacy = "rollout-2025-01-03T12-00-00-00000000-0000-0000-0000-000000000123.jsonl";

    assert_eq!(RolloutFileName::parse(legacy), Some(expected));
    assert_eq!(
        RolloutFileName::parse(format!("{legacy}.zst").as_str()),
        Some(expected)
    );
}
