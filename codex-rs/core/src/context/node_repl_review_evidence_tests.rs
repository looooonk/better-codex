use codex_utils_output_truncation::approx_token_count;
use pretty_assertions::assert_eq;

use super::*;

fn text(value: impl Into<String>) -> NodeReplReviewEvidenceItem {
    NodeReplReviewEvidenceItem::Text(value.into())
}

fn image(payload: &str) -> NodeReplReviewEvidenceItem {
    NodeReplReviewEvidenceItem::Image {
        data_url: format!("data:image/png;base64,{payload}"),
    }
}

#[test]
fn record_redacts_and_bounds_text_before_retention() {
    let evidence = NodeReplReviewEvidence::default();
    evidence.record(
        "cell-sk-abcdefghijklmnopqrstuvwxyz012345",
        "call-1",
        vec![text(format!(
            "token=abcdefghijklmnopqrstuvwxyz012345 {}",
            "x".repeat(20_000)
        ))],
    );

    let snapshot = evidence.snapshot();
    let NodeReplReviewEvidenceItem::Text(text) = &snapshot.records[0].items[0] else {
        panic!("expected text evidence");
    };
    assert!(text.contains("[REDACTED_SECRET]"));
    assert!(approx_token_count(text) <= MAX_TEXT_TOKENS);
    assert!(!snapshot.records[0]
        .provenance
        .contains("sk-abcdefghijklmnopqrstuvwxyz012345"));
}

#[test]
fn records_keep_sequence_order_and_evict_after_forty() {
    let evidence = NodeReplReviewEvidence::default();
    for index in 0..45 {
        evidence.record("cell", &format!("call-{index}"), vec![text(index.to_string())]);
    }

    let snapshot = evidence.snapshot();

    assert_eq!(snapshot.sequence, 45);
    assert_eq!(snapshot.omitted_records, 5);
    assert_eq!(snapshot.records.len(), MAX_RECORDS);
    assert_eq!(snapshot.records[0].sequence, 6);
    assert_eq!(snapshot.records[39].sequence, 45);
}

#[test]
fn image_count_and_encoded_size_are_hard_bounded() {
    let evidence = NodeReplReviewEvidence::default();
    evidence.record(
        "cell",
        "call",
        vec![
            image("AAAA"),
            image("BBBB"),
            image("CCCC"),
            image(&"D".repeat(MAX_ENCODED_IMAGE_BYTES + 1)),
        ],
    );

    assert_eq!(
        evidence.snapshot().records[0].items,
        vec![image("AAAA"), image("BBBB")]
    );
}

#[test]
fn empty_success_is_retained_as_a_correlated_record() {
    let evidence = NodeReplReviewEvidence::default();
    evidence.record("cell", "call", Vec::new());

    assert_eq!(
        evidence.snapshot(),
        NodeReplReviewEvidenceSnapshot {
            sequence: 1,
            omitted_records: 0,
            records: vec![NodeReplReviewEvidenceRecord {
                sequence: 1,
                provenance: "tool=node_repl/js cell=cell call=call".to_string(),
                items: Vec::new(),
            }],
        }
    );
}
