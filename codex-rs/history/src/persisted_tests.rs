use anyhow::Result;
use codex_protocol::models::ResponseItem;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn response_item_rollout_line_preserves_shape() -> Result<()> {
    let legacy_line = json!({
        "timestamp": "2025-01-03T12:00:00.000Z",
        "ordinal": 7,
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": "hello",
            }],
        },
    });

    let line = serde_json::from_value::<RolloutLine>(legacy_line.clone())?;
    let RolloutItem::ResponseItem(item) = &line.item else {
        panic!("expected response item");
    };
    assert!(matches!(&item.item, ResponseItem::Message { .. }));
    assert_eq!(item.metadata, None);
    assert_eq!(serde_json::to_value(line)?, legacy_line);
    Ok(())
}

#[test]
fn response_item_replacement_history_preserves_shape() -> Result<()> {
    let legacy_item = json!({
        "message": "summary",
        "replacement_history": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": "hello",
            }],
        }],
    });

    let item = serde_json::from_value::<CompactedItem>(legacy_item.clone())?;
    let replacement_history = item
        .replacement_history
        .as_ref()
        .expect("replacement history");
    assert!(matches!(
        &replacement_history[0].item,
        ResponseItem::Message { .. }
    ));
    assert_eq!(replacement_history[0].metadata, None);
    assert_eq!(serde_json::to_value(item)?, legacy_item);
    Ok(())
}

#[test]
fn response_item_metadata_is_a_sibling_of_payload() -> Result<()> {
    let item = ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: Vec::new(),
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let line = RolloutLine {
        timestamp: "2025-01-03T12:00:00.000Z".to_string(),
        ordinal: Some(7),
        item: RolloutItem::ResponseItem(ResponseItemEnvelope {
            item: item.clone(),
            metadata: Some(CodexHarnessMetadata {
                client_authored: true,
            }),
        }),
    };

    let serialized = serde_json::to_value(&line)?;
    assert_eq!(serialized["payload"], serde_json::to_value(item)?);
    assert_eq!(serialized["metadata"], json!({ "client_authored": true }));
    assert_eq!(serialized["payload"].get("metadata"), None);

    let restored = serde_json::from_value::<RolloutLine>(serialized)?;
    let RolloutItem::ResponseItem(envelope) = restored.item else {
        panic!("expected response item");
    };
    assert_eq!(
        envelope.metadata,
        Some(CodexHarnessMetadata {
            client_authored: true,
        })
    );
    Ok(())
}

#[test]
fn response_item_metadata_ignores_unknown_fields() -> Result<()> {
    let line = serde_json::from_value::<RolloutLine>(json!({
        "timestamp": "2025-01-03T12:00:00.000Z",
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [],
        },
        "metadata": { "future_field": "value" },
    }))?;

    let RolloutItem::ResponseItem(envelope) = line.item else {
        panic!("expected response item");
    };
    assert_eq!(envelope.metadata, Some(CodexHarnessMetadata::default()));
    Ok(())
}

#[test]
fn compacted_history_stores_aligned_metadata_sidecar() -> Result<()> {
    let response_item = ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: Vec::new(),
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let item = CompactedItem {
        message: "summary".to_string(),
        replacement_history: Some(vec![ResponseItemEnvelope {
            item: response_item.clone(),
            metadata: Some(CodexHarnessMetadata {
                client_authored: true,
            }),
        }]),
        window_number: None,
        first_window_id: None,
        previous_window_id: None,
        window_id: None,
    };

    let serialized = serde_json::to_value(&item)?;
    assert_eq!(serialized["replacement_history"], json!([response_item]));
    assert_eq!(
        serialized["replacement_history_metadata"],
        json!([{ "client_authored": true }])
    );
    assert_eq!(serde_json::from_value::<CompactedItem>(serialized)?, item);
    Ok(())
}

#[test]
fn compacted_history_rejects_misaligned_metadata() {
    for malformed in [
        json!({
            "message": "summary",
            "replacement_history": [],
            "replacement_history_metadata": [{}],
        }),
        json!({
            "message": "summary",
            "replacement_history_metadata": [{}],
        }),
    ] {
        let error = serde_json::from_value::<CompactedItem>(malformed)
            .expect_err("misaligned metadata must be rejected");
        assert!(
            error.to_string().contains("replacement_history_metadata"),
            "error: {error}"
        );
    }
}

#[test]
fn rollout_item_schema_matches_persisted_response_shape() -> Result<()> {
    let schema = serde_json::to_value(schemars::schema_for!(RolloutItem))?;
    let variants = schema["oneOf"].as_array().expect("rollout variants");
    assert_eq!(variants.len(), 9);
    let response_item = variants
        .iter()
        .find(|variant| variant["properties"]["type"]["enum"] == json!(["response_item"]))
        .expect("response item schema");
    assert!(response_item["properties"].get("metadata").is_some());
    assert_eq!(
        response_item["properties"]["payload"]["$ref"],
        json!("#/definitions/ResponseItem")
    );
    Ok(())
}

#[test]
fn compacted_item_serializes_window_number_and_id() -> Result<()> {
    let item = CompactedItem {
        message: "summary".to_string(),
        replacement_history: None,
        window_number: Some(3),
        first_window_id: Some("019b3f6e-0000-7000-8000-000000000001".to_string()),
        previous_window_id: Some("019b3f6e-0000-7000-8000-000000000002".to_string()),
        window_id: Some("019b3f6e-7a10-7cc3-8b6e-1d09e2f7a001".to_string()),
    };

    assert_eq!(
        serde_json::to_value(item)?,
        json!({
            "message": "summary",
            "window_number": 3,
            "first_window_id": "019b3f6e-0000-7000-8000-000000000001",
            "previous_window_id": "019b3f6e-0000-7000-8000-000000000002",
            "window_id": "019b3f6e-7a10-7cc3-8b6e-1d09e2f7a001",
        })
    );
    Ok(())
}

#[test]
fn compacted_item_migrates_legacy_numeric_window_id() -> Result<()> {
    let item = serde_json::from_value::<CompactedItem>(json!({
        "message": "summary",
        "window_id": 3,
    }))?;

    assert_eq!(
        item,
        CompactedItem {
            message: "summary".to_string(),
            replacement_history: None,
            window_number: Some(3),
            first_window_id: None,
            previous_window_id: None,
            window_id: None,
        }
    );
    Ok(())
}
