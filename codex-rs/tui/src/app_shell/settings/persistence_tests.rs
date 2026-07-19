use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn rollback_edits_restore_existing_values_and_clear_new_values() {
    let config = Map::from_iter([("model".to_string(), json!("gpt-existing"))]);
    let edits = vec![
        replace_config_value("model", json!("gpt-new")),
        replace_config_value("service_tier", json!("fast")),
    ];

    assert_eq!(
        rollback_edits(&config, &edits).expect("rollback edits should build"),
        vec![
            replace_config_value("model", json!("gpt-existing")),
            replace_config_value("service_tier", JsonValue::Null),
        ]
    );
}
