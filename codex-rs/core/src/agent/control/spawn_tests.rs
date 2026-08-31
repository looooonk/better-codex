use codex_protocol::security_risk::SecurityRiskScore;

use super::*;

#[test]
fn forked_history_excludes_security_risk_scores() {
    let item = RolloutItem::SecurityRiskScore(
        SecurityRiskScore::new("review-1", "turn-1", "action-1", /*score*/ 0.92)
            .expect("valid security risk score"),
    );

    assert!(!keep_forked_rollout_item(
        &item,
        /*preserve_reference_context_item*/ false
    ));
    assert!(!keep_forked_rollout_item(
        &item,
        /*preserve_reference_context_item*/ true
    ));
}
