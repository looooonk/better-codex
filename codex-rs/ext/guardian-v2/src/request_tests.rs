use std::collections::HashMap;

use codex_api::ResponsesWsRequest;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::GuardianAssessmentAction;
use codex_utils_output_truncation::approx_token_count;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::*;

fn action(payload: Value) -> GuardianReviewAction {
    GuardianReviewAction {
        review_id: "review-1".to_string(),
        turn_id: "turn-1".to_string(),
        action_id: "action-1".to_string(),
        source: codex_extension_api::ToolCallSource::CodeMode {
            cell_id: "cell-1".to_string(),
            runtime_tool_call_id: "runtime-call-1".to_string(),
        },
        evidence_revision: 7,
        action: GuardianAssessmentAction::McpToolCall {
            server: "node_repl".to_string(),
            tool_name: "js".to_string(),
            connector_id: None,
            connector_name: None,
            tool_title: None,
        },
        request_payload: payload,
    }
}

fn request(payload: Value) -> GuardianReviewRequest {
    GuardianReviewRequest {
        action: action(payload),
        history: Vec::new(),
        evidence: Vec::new(),
        images: Vec::new(),
    }
}

fn attribution() -> SamplingAttribution {
    SamplingAttribution {
        client_metadata: HashMap::from([("turn_id".to_string(), "turn-1".to_string())]),
        service_tier: None,
        thread_id: "thread-1".to_string(),
    }
}

#[test]
fn oversized_action_is_rejected_before_a_sampling_request_exists() {
    let error = build_sampling_request(
        attribution(),
        request(json!({"script": "x".repeat(8_000)})),
    )
    .expect_err("oversized action should be rejected");

    assert!(matches!(error, GuardianReviewError::ActionTooLarge));
}

#[test]
fn request_is_redacted_and_inside_both_serialized_limits() {
    let secret = "sk-abcdefghijklmnopqrstuvwxyz012345";
    let mut request = request(json!({"token": secret, "script": "return 1"}));
    request.evidence = (0..60)
        .map(|index| GuardianEvidenceEntry {
            kind: "node_repl".to_string(),
            provenance: Some(format!("cell-{index}")),
            text: format!("evidence {index} {secret}"),
        })
        .collect();

    let request = build_sampling_request(attribution(), request).expect("bounded request");
    let serialized = serde_json::to_string(&ResponsesWsRequest::ResponseCreate((&request).into()))
        .expect("serialize request");

    assert!(serialized.len() <= MAX_REQUEST_BYTES);
    assert!(approx_token_count(&serialized) < MAX_REQUEST_TOKENS);
    assert!(!serialized.contains(secret));
    assert!(serialized.contains("[REDACTED_SECRET]"));
    assert!(serialized.contains(CODE_MODE_INSTRUCTIONS.trim()));
}

#[test]
fn sampling_body_carries_code_mode_binding_and_evidence_revision() {
    let request = build_sampling_request(
        attribution(),
        request(json!({"script": "return 1"})),
    )
    .expect("bounded request");
    let ResponseItem::Message { content, .. } = &request.input[2] else {
        panic!("expected user message");
    };
    let ContentItem::InputText { text } = &content[0] else {
        panic!("expected text review context");
    };
    let body: Value = serde_json::from_str(text).expect("review body");

    assert_eq!(
        body["binding"],
        json!({
            "source": {
                "type": "codeMode",
                "cellId": "cell-1",
                "runtimeToolCallId": "runtime-call-1",
            },
            "evidenceRevision": 7,
        })
    );
}

#[test]
fn auxiliary_evidence_is_limited_to_its_newest_reserved_entries() {
    let mut request = request(json!({"script": "return 1"}));
    request.evidence = (0..45)
        .map(|index| GuardianEvidenceEntry {
            kind: "tool_output".to_string(),
            provenance: None,
            text: format!("entry-{index}"),
        })
        .collect();

    let request = build_sampling_request(attribution(), request).expect("bounded request");
    let ResponseItem::Message { content, .. } = &request.input[2] else {
        panic!("expected user message");
    };
    let ContentItem::InputText { text } = &content[0] else {
        panic!("expected text review context");
    };
    let body: Value = serde_json::from_str(text).expect("review body");
    let evidence = body["evidence"].as_array().expect("evidence array");

    assert_eq!(evidence.len(), MAX_AUXILIARY_EVIDENCE_ENTRIES);
    assert_eq!(evidence[0]["text"], "entry-25");
    assert_eq!(evidence[19]["text"], "entry-44");
}

#[test]
fn auxiliary_pressure_preserves_the_reserved_transcript() {
    let mut request = request(json!({"script": "return 1"}));
    request.history = (0..MAX_TRANSCRIPT_ENTRIES)
        .map(|index| ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: format!("history-{index} {}", "h".repeat(1_200)),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        })
        .collect();
    request.evidence = (0..MAX_AUXILIARY_EVIDENCE_ENTRIES)
        .map(|index| GuardianEvidenceEntry {
            kind: "node_repl_output".to_string(),
            provenance: Some(format!("auxiliary-{index}")),
            text: format!("auxiliary-{index} {}", "a".repeat(1_200)),
        })
        .collect();

    let request = build_sampling_request(attribution(), request).expect("bounded request");
    let ResponseItem::Message { content, .. } = &request.input[2] else {
        panic!("expected user message");
    };
    let ContentItem::InputText { text } = &content[0] else {
        panic!("expected text review context");
    };
    let body: Value = serde_json::from_str(text).expect("review body");
    let evidence = body["evidence"].as_array().expect("evidence array");

    assert_eq!(
        evidence
            .iter()
            .filter(|entry| entry["kind"] == "message")
            .count(),
        MAX_TRANSCRIPT_ENTRIES
    );
    assert!(
        evidence
            .iter()
            .filter(|entry| entry["kind"] == "node_repl_output")
            .count()
            < MAX_AUXILIARY_EVIDENCE_ENTRIES
    );
}

#[test]
fn image_admission_is_bounded_and_forces_low_detail() {
    let mut request = request(json!({"script": "return 1"}));
    request.images = vec![
        GuardianReviewImage::from_sanitized_data_url("data:image/png;base64,AAAA".to_string())
            .expect("image"),
        GuardianReviewImage::from_sanitized_data_url("data:image/png;base64,BBBB".to_string())
            .expect("image"),
        GuardianReviewImage::from_sanitized_data_url("data:image/png;base64,CCCC".to_string())
            .expect("image"),
    ];

    let request = build_sampling_request(attribution(), request).expect("bounded request");
    let ResponseItem::Message { content, .. } = &request.input[2] else {
        panic!("expected user message");
    };
    let images = content
        .iter()
        .filter_map(|item| match item {
            ContentItem::InputImage { image_url, detail } => Some((image_url, detail)),
            ContentItem::InputText { .. }
            | ContentItem::InputAudio { .. }
            | ContentItem::OutputText { .. } => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(images.len(), MAX_IMAGES);
    assert_eq!(images[0].0, "data:image/png;base64,BBBB");
    assert_eq!(images[1].0, "data:image/png;base64,CCCC");
    assert!(
        images
            .iter()
            .all(|(_, detail)| **detail == Some(ImageDetail::Low))
    );
}

#[test]
fn request_pressure_drops_images_before_text_evidence() {
    let mut request = request(json!({"script": "return 1"}));
    request.evidence = (0..MAX_EVIDENCE_ENTRIES)
        .map(|index| GuardianEvidenceEntry {
            kind: "tool_output".to_string(),
            provenance: Some(format!("entry-{index}")),
            text: "e".repeat(650),
        })
        .collect();
    request.images = vec![
        GuardianReviewImage::from_sanitized_data_url(format!(
            "data:image/png;base64,{}",
            "A".repeat(10_000)
        ))
        .expect("image"),
    ];

    let request = build_sampling_request(attribution(), request).expect("bounded request");
    let ResponseItem::Message { content, .. } = &request.input[2] else {
        panic!("expected user message");
    };
    let ContentItem::InputText { text } = &content[0] else {
        panic!("expected text review context");
    };
    let body: Value = serde_json::from_str(text).expect("review body");

    assert_eq!(
        body["evidence"].as_array().map(Vec::len),
        Some(MAX_AUXILIARY_EVIDENCE_ENTRIES)
    );
    assert!(
        content
            .iter()
            .all(|item| !matches!(item, ContentItem::InputImage { .. }))
    );
}

#[test]
fn invalid_or_oversized_image_is_not_admitted() {
    assert!(matches!(
        GuardianReviewImage::from_sanitized_data_url("https://example.com/image.png".to_string()),
        Err(GuardianReviewError::InvalidImage)
    ));
    assert!(matches!(
        GuardianReviewImage::from_sanitized_data_url(format!(
            "data:image/png;base64,{}",
            "A".repeat(MAX_ENCODED_IMAGE_BYTES + 1)
        )),
        Err(GuardianReviewError::InvalidImage)
    ));
}
