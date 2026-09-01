use pretty_assertions::assert_eq;
use serde_json::json;

use super::MAX_JSON_BYTES;
use super::MAX_JSON_DEPTH;
use super::MAX_JSON_NODES;
use super::encode_bounded_json;
use super::parse_bounded_json;

#[test]
fn bounded_json_accepts_normal_values() {
    assert_eq!(
        parse_bounded_json(br#"{"items":[1,true,"text"]}"#),
        Ok(json!({"items": [1, true, "text"]}))
    );
}

#[test]
fn bounded_json_rejects_bytes_nodes_and_depth_before_value_allocation() {
    assert!(
        parse_bounded_json(&vec![b' '; MAX_JSON_BYTES + 1])
            .unwrap_err()
            .contains("bytes")
    );
    let nodes = format!("[{}0]", "0,".repeat(MAX_JSON_NODES));
    assert!(
        parse_bounded_json(nodes.as_bytes())
            .unwrap_err()
            .contains("node limit")
    );
    let nested = format!(
        "{}0{}",
        "[".repeat(MAX_JSON_DEPTH + 2),
        "]".repeat(MAX_JSON_DEPTH + 2)
    );
    assert!(
        parse_bounded_json(nested.as_bytes())
            .unwrap_err()
            .contains("nesting limit")
    );
}

#[test]
fn bounded_json_encoding_preflights_nodes_and_depth() {
    let many_nodes = serde_json::Value::Array(vec![serde_json::Value::Null; MAX_JSON_NODES]);
    assert!(
        encode_bounded_json(&many_nodes, MAX_JSON_BYTES)
            .unwrap_err()
            .contains("node limit")
    );
    let mut deeply_nested = serde_json::Value::Null;
    for _ in 0..MAX_JSON_DEPTH + 2 {
        deeply_nested = serde_json::Value::Array(vec![deeply_nested]);
    }
    assert!(
        encode_bounded_json(&deeply_nested, MAX_JSON_BYTES)
            .unwrap_err()
            .contains("nesting limit")
    );
}
