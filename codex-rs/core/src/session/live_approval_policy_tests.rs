use super::*;
use pretty_assertions::assert_eq;

#[test]
fn never_revision_survives_policy_aba_transition() {
    let policy = LiveApprovalPolicy::new(Constrained::allow_any(AskForApproval::OnRequest));
    let before = policy.decision_snapshot();

    policy.replace(Constrained::allow_any(AskForApproval::Never));
    policy.replace(Constrained::allow_any(AskForApproval::OnRequest));

    assert_eq!(
        (before, policy.decision_snapshot()),
        (
            LiveApprovalPolicySnapshot {
                value: AskForApproval::OnRequest,
                revision: 0,
                never_revision: 0,
            },
            LiveApprovalPolicySnapshot {
                value: AskForApproval::OnRequest,
                revision: 2,
                never_revision: 1,
            },
        )
    );
}
