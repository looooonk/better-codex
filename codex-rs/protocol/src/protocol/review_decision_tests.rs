use super::REJECTION_REASON_MAX_BYTES;
use super::ReviewDecision;
use crate::approvals::ExecPolicyAmendment;
use crate::approvals::NetworkPolicyAmendment;
use crate::approvals::NetworkPolicyRuleAction;
use anyhow::Result;
use pretty_assertions::assert_eq;
use schemars::schema_for;
use serde_json::json;
use ts_rs::TS;

#[test]
fn legacy_denied_string_deserializes_without_a_reason() -> Result<()> {
    assert_eq!(
        serde_json::from_value::<ReviewDecision>(json!("denied"))?,
        ReviewDecision::Denied
    );
    Ok(())
}

#[test]
fn denied_without_a_reason_preserves_the_canonical_string() -> Result<()> {
    assert_eq!(ReviewDecision::denied(), ReviewDecision::Denied);
    assert_eq!(
        serde_json::to_value(ReviewDecision::Denied)?,
        json!("denied")
    );
    Ok(())
}

#[test]
fn reasonless_denied_objects_normalize_to_the_canonical_string() -> Result<()> {
    for value in [
        json!({"denied": {}}),
        json!({"denied": {"rejection": null}}),
    ] {
        let decision = serde_json::from_value::<ReviewDecision>(value)?;

        assert_eq!(decision, ReviewDecision::denied());
        assert_eq!(serde_json::to_value(decision)?, json!("denied"));
    }
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
        (ReviewDecision::denied(), json!("denied")),
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
fn schemas_include_canonical_and_reason_bearing_denials() -> Result<()> {
    let schema = serde_json::to_value(schema_for!(ReviewDecision))?;
    let variants = schema
        .get("anyOf")
        .and_then(serde_json::Value::as_array)
        .expect("review decision schema should be a union");
    let canonical = variants
        .iter()
        .find(|variant| {
            variant
                .get("enum")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|values| values.contains(&json!("denied")))
        })
        .expect("review decision schema should include the canonical denial");
    let reason_bearing = variants
        .iter()
        .find(|variant| {
            variant
                .pointer("/properties/denied/properties/rejection")
                .is_some()
        })
        .expect("review decision schema should include a reason-bearing denial");

    assert_eq!(canonical.get("type"), Some(&json!("string")));
    assert_eq!(
        reason_bearing.pointer("/properties/denied/properties/rejection/type"),
        Some(&json!("string"))
    );
    assert_eq!(
        reason_bearing.pointer("/properties/denied/required"),
        Some(&json!(["rejection"]))
    );
    assert_eq!(
        ReviewDecision::inline(),
        r#""approved" | { "approved_execpolicy_amendment": { proposed_execpolicy_amendment: ExecPolicyAmendment, } } | "approved_for_session" | "approved_mcp_policy_amendment" | { "network_policy_amendment": { network_policy_amendment: NetworkPolicyAmendment, } } | "denied" | { "denied": { rejection: string, } } | "timed_out" | "abort""#
    );
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
        ReviewDecision::Denied,
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
