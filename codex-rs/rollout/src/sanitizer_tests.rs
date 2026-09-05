use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

const SECRET: &str = "example_synthetic_bearer_token_123456";

#[test]
fn redacts_nested_credentials_without_changing_ids_or_encrypted_content() {
    let mut value = json!({
        "call_id": "call-1",
        "command": ["curl", "-H", format!("Authorization: Bearer {SECRET}")],
        "parsed_cmd": [{"type": "unknown", "cmd": format!("curl -H 'Bearer {SECRET}'")}],
        "arguments": {"authorization": SECRET, "clientSecret": SECRET},
        "content": [{
            "type": "encrypted_content",
            "encrypted_content": "gAAAAABopaque-ciphertext",
        }],
    });

    redact_persisted_json(&mut value);

    assert_eq!(value["call_id"], "call-1");
    assert_eq!(
        value["content"][0]["encrypted_content"],
        "gAAAAABopaque-ciphertext"
    );
    let serialized = serde_json::to_string(&value).expect("redacted JSON should serialize");
    assert!(serialized.contains(REDACTION));
    assert!(!serialized.contains(SECRET));
}

#[test]
fn redacts_credentials_inside_serialized_tool_arguments() {
    let mut value = json!({
        "arguments": format!(r#"{{"token":"{SECRET}","call_id":"{SECRET}"}}"#),
        "input": format!(r#"{{"Authorization":"Bearer {SECRET}"}}"#),
    });

    redact_persisted_json(&mut value);

    assert_eq!(
        serde_json::from_str::<Value>(value["arguments"].as_str().unwrap()).unwrap(),
        json!({"call_id": SECRET, "token": REDACTION})
    );
    assert_eq!(
        value["input"],
        format!(r#"{{"Authorization":"Bearer {REDACTION}"}}"#)
    );
}

#[test]
fn preserves_encrypted_content_inside_serialized_tool_arguments() {
    let encrypted = "gAAAAABopaque-ciphertext";
    let mut value = json!({
        "arguments": serde_json::to_string(&json!({
            "encrypted_content": encrypted,
        }))
        .expect("nested arguments should serialize"),
    });
    let expected = value.clone();

    redact_persisted_json(&mut value);

    assert_eq!(value, expected);
}

#[test]
fn redacts_contextual_command_credentials_and_preserves_ordinary_arguments() {
    let mut value = json!({
        "call_id": "call-1",
        "command": [
            "curl", "--password", "plain-secret", "--password=inline-secret",
            "-H", "Authorization: Basic short-secret", "--request-id", "request-1",
        ],
        "parsed_cmd": [{
            "type": "unknown",
            "cmd": "curl --user alice:parsed-secret --request-id request-1",
            "query": "--password query-secret",
        }],
        "argv": ["ssh", "-p", "2222", "host.example"],
    });

    redact_persisted_json(&mut value);

    let serialized = serde_json::to_string(&value).expect("redacted JSON should serialize");
    assert!(!serialized.contains("plain-secret"));
    assert!(!serialized.contains("inline-secret"));
    assert!(!serialized.contains("short-secret"));
    assert!(!serialized.contains("parsed-secret"));
    assert!(!serialized.contains("query-secret"));
    assert!(serialized.contains("request-1"));
    assert!(serialized.contains("2222"));
    assert_eq!(value["call_id"], "call-1");
}

#[test]
fn redacts_credentials_inside_shell_wrapped_commands() {
    let mut value = json!({
        "command": [
            "/bin/sh",
            "-lc",
            "curl -u alice:hunter2 https://example.test --request-id request-1",
        ],
    });

    redact_persisted_json(&mut value);

    let inner = value["command"][2]
        .as_str()
        .expect("inner command should remain a string");
    assert!(!inner.contains("alice:hunter2"));
    assert!(inner.contains(REDACTION));
    assert!(inner.contains("https://example.test"));
    assert!(inner.contains("request-1"));
    assert_eq!(value["command"][0], "/bin/sh");
    assert_eq!(value["command"][1], "-lc");
}

#[test]
fn redacts_sensitive_commands_nested_beyond_the_depth_limit() {
    let mut value = json!({
        "command": [
            "/bin/sh",
            "-lc",
            r#"/bin/sh -lc '/bin/sh -lc "curl -u deeply:nested https://example.test"'"#,
        ],
    });

    redact_persisted_json(&mut value);

    let serialized = serde_json::to_string(&value).expect("redacted JSON should serialize");
    assert!(!serialized.contains("deeply:nested"));
    assert!(serialized.contains(REDACTION));
}

#[test]
fn redacts_non_bearer_authorization_headers_in_arbitrary_text() {
    let mut value = json!({
        "output": "request failed\nAuthorization: Basic short-secret\nretained detail",
    });

    redact_persisted_json(&mut value);

    assert_eq!(
        value["output"],
        "request failed\nAuthorization: [REDACTED_SECRET]\nretained detail"
    );
}

#[test]
fn preserves_bearer_scheme_in_structured_authorization_headers() {
    let mut value = json!({
        "command": [
            "curl",
            "-H",
            format!("Authorization: Bearer {SECRET}"),
            "-H",
            "Authorization: Basic short-secret",
        ],
    });

    redact_persisted_json(&mut value);

    assert_eq!(
        value,
        json!({
            "command": [
                "curl",
                "-H",
                "Authorization: Bearer [REDACTED_SECRET]",
                "-H",
                "Authorization: [REDACTED_SECRET]",
            ],
        })
    );
}

#[test]
fn preserves_bearer_scheme_in_plain_text_authorization_headers() {
    let mut value = json!({
        "output": format!(
            "request failed\nAuthorization: Bearer {SECRET}\nAuthorization: Basic short-secret"
        ),
    });

    redact_persisted_json(&mut value);

    assert_eq!(
        value,
        json!({
            "output": concat!(
                "request failed\n",
                "Authorization: Bearer [REDACTED_SECRET]\n",
                "Authorization: [REDACTED_SECRET]"
            ),
        })
    );
}

#[test]
fn preserves_guardian_authorization_fields_while_redacting_nested_secrets() {
    let mut value = json!({
        "user_authorization": "high",
        "review": {
            "userAuthorization": "low",
            "action": {
                "command": ["curl", "-H", format!("Authorization: Bearer {SECRET}")],
            },
        },
    });

    redact_persisted_json(&mut value);

    assert_eq!(
        value,
        json!({
            "user_authorization": "high",
            "review": {
                "userAuthorization": "low",
                "action": {
                    "command": [
                        "curl",
                        "-H",
                        "Authorization: Bearer [REDACTED_SECRET]",
                    ],
                },
            },
        })
    );
}

#[test]
fn redacts_standard_aws_secret_access_key_fields() {
    let mut value = json!({
        "AWS_SECRET_ACCESS_KEY": "synthetic-short-secret",
    });

    redact_persisted_json(&mut value);

    assert_eq!(value["AWS_SECRET_ACCESS_KEY"], REDACTION);
}

#[test]
fn leaves_token_usage_fields_unchanged() {
    let mut value = json!({
        "type": "token_count",
        "info": {
            "total_token_usage": {
                "input_tokens": 21775,
                "cached_input_tokens": 9984,
                "output_tokens": 312,
            },
            "model_context_window": 258400,
        },
        "plan_type": "pro",
    });
    let expected = value.clone();

    redact_persisted_json(&mut value);

    assert_eq!(value, expected);
}
