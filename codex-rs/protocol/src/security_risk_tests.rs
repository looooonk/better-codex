use pretty_assertions::assert_eq;

use super::*;

#[test]
fn score_round_trips_with_only_fixed_metadata_and_host_ids() {
    let score = SecurityRiskScore::new("review-1", "turn-1", "action-1", /*score*/ 0.75)
        .expect("valid score");
    let serialized = serde_json::to_value(&score).expect("serialize score");

    assert_eq!(
        serialized,
        serde_json::json!({
            "review_id": "review-1",
            "turn_id": "turn-1",
            "action_id": "action-1",
            "category": "action_risk",
            "status": "reviewed",
            "score": 0.75,
        })
    );
    assert_eq!(
        serde_json::from_value::<SecurityRiskScore>(serialized).expect("deserialize score"),
        score
    );
}

#[test]
fn invalid_scores_and_sensitive_extra_fields_are_rejected() {
    assert_eq!(
        SecurityRiskScore::new("", "turn", "action", /*score*/ 0.5),
        Err(InvalidSecurityRiskScore::InvalidCorrelationId)
    );
    assert_eq!(
        SecurityRiskScore::new("review", "turn", "action", /*score*/ f64::NAN),
        Err(InvalidSecurityRiskScore::InvalidScore)
    );
    assert_eq!(
        SecurityRiskScore::new("review", "turn", "action", /*score*/ 1.01),
        Err(InvalidSecurityRiskScore::InvalidScore)
    );
    assert!(
        serde_json::from_value::<SecurityRiskScore>(serde_json::json!({
            "review_id": "review",
            "turn_id": "turn",
            "action_id": "action",
            "category": "action_risk",
            "status": "reviewed",
            "score": 0.5,
            "rationale": "must never persist",
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SecurityRiskScore>(serde_json::json!({
            "review_id": "review",
            "turn_id": "turn",
            "action_id": "action",
            "category": "free_form_category",
            "status": "reviewed",
            "score": 0.5,
        }))
        .is_err()
    );
}
