use super::REJECTION_REASON_MAX_BYTES;
use super::ReviewDecision;
use crate::approvals::ExecPolicyAmendment;
use crate::approvals::NetworkPolicyAmendment;
use crate::approvals::NetworkPolicyRuleAction;
use anyhow::Result;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn legacy_denied_string_deserializes_without_a_reason() -> Result<()> {
    assert_eq!(
        serde_json::from_value::<ReviewDecision>(json!("denied"))?,
        ReviewDecision::denied()
    );
    Ok(())
}

#[test]
fn denied_without_a_reason_serializes_as_the_new_shape() -> Result<()> {
    assert_eq!(
        serde_json::to_value(ReviewDecision::denied())?,
        json!({"denied": {"rejection": null}})
    );
    Ok(())
}

#[test]
fn denied_object_without_a_rejection_field_remains_compatible() -> Result<()> {
    assert_eq!(
        serde_json::from_value::<ReviewDecision>(json!({"denied": {}}))?,
        ReviewDecision::denied()
    );
    Ok(())
}

#[test]
fn denial_reason_round_trips_in_the_new_shape() -> Result<()> {
    let decision = ReviewDecision::denied_with_reason("rejected by policy");
    let value = json!({"denied": {"rejection": "rejected by policy"}});

    assert_eq!(serde_json::to_value(&decision)?, value);
    assert_eq!(serde_json::from_value::<ReviewDecision>(value)?, decision);
    Ok(())
}

#[test]
fn denial_reason_is_bounded_at_a_utf8_boundary_on_construction() -> Result<()> {
    let reason = format!("{}é", "a".repeat(REJECTION_REASON_MAX_BYTES - 1));
    let decision = ReviewDecision::denied_with_reason(reason);

    assert_eq!(
        decision.rejection_reason(),
        Some("a".repeat(REJECTION_REASON_MAX_BYTES - 1).as_str())
    );
    assert_eq!(
        serde_json::to_value(decision)?,
        json!({"denied": {"rejection": "a".repeat(REJECTION_REASON_MAX_BYTES - 1)}})
    );
    Ok(())
}

#[test]
fn denial_reason_is_bounded_when_deserialized_from_the_wire() -> Result<()> {
    let reason = format!("{}é", "a".repeat(REJECTION_REASON_MAX_BYTES - 1));
    let decision =
        serde_json::from_value::<ReviewDecision>(json!({"denied": {"rejection": reason}}))?;

    assert_eq!(
        decision.rejection_reason(),
        Some("a".repeat(REJECTION_REASON_MAX_BYTES - 1).as_str())
    );
    Ok(())
}

#[test]
fn interruption_timeout_and_abort_remain_distinct_on_the_wire() -> Result<()> {
    let decisions = [
        (
            ReviewDecision::denied(),
            json!({"denied": {"rejection": null}}),
        ),
        (ReviewDecision::TimedOut, json!("timed_out")),
        (ReviewDecision::Abort, json!("abort")),
    ];

    for (decision, value) in decisions {
        assert_eq!(serde_json::to_value(&decision)?, value);
        assert_eq!(serde_json::from_value::<ReviewDecision>(value)?, decision);
    }
    Ok(())
}

#[test]
fn all_shared_approval_decisions_round_trip() -> Result<()> {
    let decisions = [
        ReviewDecision::Approved,
        ReviewDecision::ApprovedExecpolicyAmendment {
            proposed_execpolicy_amendment: ExecPolicyAmendment::new(vec!["echo".to_string()]),
        },
        ReviewDecision::ApprovedForSession,
        ReviewDecision::ApprovedMcpPolicyAmendment,
        ReviewDecision::NetworkPolicyAmendment {
            network_policy_amendment: NetworkPolicyAmendment {
                host: "example.com".to_string(),
                action: NetworkPolicyRuleAction::Allow,
            },
        },
        ReviewDecision::denied_with_reason("rejected by policy"),
        ReviewDecision::TimedOut,
        ReviewDecision::Abort,
    ];

    for decision in decisions {
        let value = serde_json::to_value(&decision)?;
        assert_eq!(serde_json::from_value::<ReviewDecision>(value)?, decision);
    }
    Ok(())
}
