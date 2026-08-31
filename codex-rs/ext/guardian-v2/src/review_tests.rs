use codex_protocol::protocol::GuardianAssessmentOutcome;
use codex_protocol::protocol::GuardianRiskLevel;
use codex_protocol::protocol::GuardianUserAuthorization;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn strict_outcome_parser_rejects_unknown_fields_and_invalid_scores() {
    let valid = parse_outcome(
        r#"{"score":0.75,"risk_level":"high","user_authorization":"low","outcome":"deny","rationale":"unsafe"}"#,
    )
    .expect("valid outcome");
    assert_eq!(
        valid,
        GuardianReviewOutcome {
            score: 0.75,
            risk_level: GuardianRiskLevel::High,
            user_authorization: GuardianUserAuthorization::Low,
            outcome: GuardianAssessmentOutcome::Deny,
            rationale: "unsafe".to_string(),
        }
    );

    assert!(matches!(
        parse_outcome(
            r#"{"score":0.5,"risk_level":"low","user_authorization":"high","outcome":"allow","rationale":"ok","extra":true}"#,
        ),
        Err(GuardianReviewError::InvalidOutput)
    ));
    assert!(matches!(
        parse_outcome(
            r#"{"score":1.1,"risk_level":"critical","user_authorization":"unknown","outcome":"deny","rationale":"bad"}"#,
        ),
        Err(GuardianReviewError::InvalidOutput)
    ));
}

#[test]
fn rationale_remains_bounded_after_secret_redaction() {
    let rationale = "token=12345678 ".repeat(200);
    let output = json!({
        "score": 0.5,
        "risk_level": "medium",
        "user_authorization": "unknown",
        "outcome": "deny",
        "rationale": rationale,
    })
    .to_string();

    let outcome = parse_outcome(&output).expect("valid bounded output");

    assert!(outcome.rationale.len() <= MAX_RATIONALE_BYTES);
    assert!(!outcome.rationale.contains("12345678"));
}

#[test]
fn all_review_failures_require_manual_review() {
    let errors = [
        GuardianReviewError::ActionTooLarge,
        GuardianReviewError::RequestTooLarge,
        GuardianReviewError::InvalidIdentifier,
        GuardianReviewError::InvalidImage,
        GuardianReviewError::Deadline,
        GuardianReviewError::InvalidOutput,
    ];

    assert!(errors.iter().all(GuardianReviewError::requires_manual_review));
}
