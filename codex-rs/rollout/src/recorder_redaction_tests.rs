use super::*;
use codex_protocol::models::FunctionCallOutputContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::LocalShellAction;
use codex_protocol::models::LocalShellExecAction;
use codex_protocol::models::LocalShellStatus;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::TempDir;

const SECRET: &str = "example_synthetic_bearer_token_123456";

fn function_call() -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".to_string(),
            namespace: None,
            arguments: format!(r#"{{"authorization":"Bearer {SECRET}"}}"#),
            call_id: "call-function".to_string(),
            encrypted_function_args: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}

fn curl_user_call() -> RolloutItem {
    RolloutItem::ResponseItem(
        ResponseItem::LocalShellCall {
            id: None,
            call_id: Some("call-curl-user".to_string()),
            status: LocalShellStatus::Completed,
            action: LocalShellAction::Exec(LocalShellExecAction {
                command: ["curl", "-u", "alice:hunter2", "https://example.test"]
                    .map(str::to_string)
                    .to_vec(),
                timeout_ms: None,
                working_directory: None,
                env: None,
                user: None,
            }),
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    )
}

#[tokio::test]
async fn jsonl_writer_redacts_copies_without_mutating_live_items() -> std::io::Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("rollout.jsonl");
    let file = tokio::fs::File::create(&path).await?;
    let mut writer = JsonlWriter { file };
    let items = vec![
        RolloutItem::ResponseItem(
            ResponseItem::LocalShellCall {
                id: None,
                call_id: Some("call-shell".to_string()),
                status: LocalShellStatus::Completed,
                action: LocalShellAction::Exec(LocalShellExecAction {
                    command: vec![
                        "curl".to_string(),
                        "-H".to_string(),
                        format!("Authorization: Bearer {SECRET}"),
                    ],
                    timeout_ms: None,
                    working_directory: None,
                    env: Some(HashMap::from([(
                        "ACCESS_TOKEN".to_string(),
                        SECRET.to_string(),
                    )])),
                    user: None,
                }),
                internal_chat_message_metadata_passthrough: None,
            }
            .into(),
        ),
        function_call(),
        RolloutItem::ResponseItem(
            ResponseItem::CustomToolCall {
                id: None,
                status: Some("completed".to_string()),
                call_id: "call-custom".to_string(),
                name: "exec".to_string(),
                namespace: None,
                input: format!(r#"{{"apiKey":"{SECRET}"}}"#),
                internal_chat_message_metadata_passthrough: None,
            }
            .into(),
        ),
    ];

    for item in &items {
        writer.write_rollout_item(item, /*ordinal*/ None).await?;
    }

    let raw_items = serde_json::to_string(&items)?;
    assert!(raw_items.contains(SECRET));
    let persisted = std::fs::read_to_string(path)?;
    assert!(!persisted.contains(SECRET));
    assert_eq!(persisted.matches("[REDACTED_SECRET]").count(), 4);
    assert!(persisted.contains("call-shell"));
    assert!(persisted.contains("call-function"));
    assert!(persisted.contains("call-custom"));
    Ok(())
}

#[tokio::test]
async fn old_unsanitized_rollout_replays_redacted_and_preserves_encrypted_content()
-> std::io::Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("old-rollout.jsonl");
    let encrypted = "gAAAAABopaque-ciphertext".to_string();
    let output = RolloutItem::ResponseItem(
        ResponseItem::FunctionCallOutput {
            id: None,
            call_id: "call-output".to_string(),
            output: FunctionCallOutputPayload::from_content_items(vec![
                FunctionCallOutputContentItem::EncryptedContent {
                    encrypted_content: encrypted.clone(),
                },
                FunctionCallOutputContentItem::InputText {
                    text: "Authorization: Basic short-secret".to_string(),
                },
            ]),
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    );
    let lines = [function_call(), curl_user_call(), output]
        .into_iter()
        .map(|item| {
            serde_json::to_string(&RolloutLine {
                timestamp: "2026-08-14T00:00:00.000Z".to_string(),
                ordinal: None,
                item,
            })
        })
        .collect::<serde_json::Result<Vec<_>>>()?
        .join("\n");
    std::fs::write(&path, format!("{lines}\n"))?;

    let (items, thread_id, parse_errors) = RolloutRecorder::load_rollout_items(&path).await?;

    assert_eq!(thread_id, None);
    assert_eq!(parse_errors, 0);
    let serialized = serde_json::to_string(&items)?;
    assert!(serialized.contains("call-function"));
    assert!(serialized.contains("call-output"));
    assert!(serialized.contains("call-curl-user"));
    assert!(!serialized.contains("alice:hunter2"));
    assert!(!serialized.contains("short-secret"));
    assert!(!serialized.contains(SECRET));
    assert!(serialized.contains("[REDACTED_SECRET]"));
    let RolloutItem::ResponseItem(codex_history::ResponseItemEnvelope {
        item: ResponseItem::FunctionCallOutput { output, .. },
        ..
    }) = &items[2]
    else {
        panic!("expected function call output");
    };
    assert_eq!(
        output.content_items(),
        Some(
            [
                FunctionCallOutputContentItem::EncryptedContent {
                    encrypted_content: encrypted,
                },
                FunctionCallOutputContentItem::InputText {
                    text: "Authorization: [REDACTED_SECRET]".to_string(),
                },
            ]
            .as_slice()
        )
    );
    Ok(())
}
