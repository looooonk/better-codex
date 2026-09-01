use super::ExecPolicyAmendment;
use super::NetworkPolicyAmendment;
use super::NetworkPolicyRuleAction;
use codex_utils_string::take_bytes_at_char_boundary;
use schemars::JsonSchema;
use schemars::r#gen::SchemaGenerator;
use schemars::schema::Schema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use strum_macros::Display;
use ts_rs::TS;

const REJECTION_REASON_MAX_BYTES: usize = 4 * 1024;

/// A bounded explanation attached to a denied approval decision.
///
/// The optional reason is truncated to a UTF-8 boundary before it can cross a
/// session or app-server wire boundary.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ReviewRejection(Option<String>);

impl ReviewRejection {
    fn with_reason(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let reason = if reason.len() <= REJECTION_REASON_MAX_BYTES {
            reason
        } else {
            take_bytes_at_char_boundary(&reason, REJECTION_REASON_MAX_BYTES).to_string()
        };
        Self(Some(reason))
    }

    /// Returns the bounded rejection reason, when the decision included one.
    pub fn reason(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl<'de> Deserialize<'de> for ReviewRejection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer).map(|reason| match reason {
            Some(reason) => Self::with_reason(reason),
            None => Self::default(),
        })
    }
}

/// User's decision in response to an ExecApprovalRequest.
#[derive(Debug, Clone, PartialEq, Eq, Display, TS)]
#[ts(as = "ReviewDecisionWire")]
pub enum ReviewDecision {
    /// User has approved this command and the agent should execute it.
    Approved,

    /// User has approved this command and wants to apply the proposed execpolicy
    /// amendment so future matching commands are permitted.
    ApprovedExecpolicyAmendment {
        proposed_execpolicy_amendment: ExecPolicyAmendment,
    },

    /// User has approved this request and wants future prompts in the same
    /// session-scoped approval cache to be automatically approved for the
    /// remainder of the session.
    ApprovedForSession,

    /// User has approved this MCP tool call and wants matching future calls to
    /// be automatically approved across sessions.
    ApprovedMcpPolicyAmendment,

    /// User chose to persist a network policy rule (allow/deny) for future
    /// requests to the same host.
    NetworkPolicyAmendment {
        network_policy_amendment: NetworkPolicyAmendment,
    },

    /// User has denied this command and the agent should not execute it, but
    /// it should continue the session and try something else.
    Denied,

    /// User has denied this command with a client-visible explanation. The
    /// agent should not execute it, but it should continue the session.
    DeniedWithReason { rejection: ReviewRejection },

    /// Automatic approval review timed out before reaching a decision.
    TimedOut,

    /// User has denied this command and the agent should not do anything until
    /// the user's next command.
    Abort,
}

/// JSON and TypeScript representation of an approval review decision.
#[derive(Serialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
enum ReviewDecisionWire {
    Approved,
    ApprovedExecpolicyAmendment {
        proposed_execpolicy_amendment: ExecPolicyAmendment,
    },
    ApprovedForSession,
    ApprovedMcpPolicyAmendment,
    NetworkPolicyAmendment {
        network_policy_amendment: NetworkPolicyAmendment,
    },
    Denied,
    #[serde(rename = "denied")]
    DeniedWithReason {
        rejection: String,
    },
    TimedOut,
    Abort,
}

impl From<&ReviewDecision> for ReviewDecisionWire {
    fn from(value: &ReviewDecision) -> Self {
        match value {
            ReviewDecision::Approved => Self::Approved,
            ReviewDecision::ApprovedExecpolicyAmendment {
                proposed_execpolicy_amendment,
            } => Self::ApprovedExecpolicyAmendment {
                proposed_execpolicy_amendment: proposed_execpolicy_amendment.clone(),
            },
            ReviewDecision::ApprovedForSession => Self::ApprovedForSession,
            ReviewDecision::ApprovedMcpPolicyAmendment => Self::ApprovedMcpPolicyAmendment,
            ReviewDecision::NetworkPolicyAmendment {
                network_policy_amendment,
            } => Self::NetworkPolicyAmendment {
                network_policy_amendment: network_policy_amendment.clone(),
            },
            ReviewDecision::Denied => Self::Denied,
            ReviewDecision::DeniedWithReason { rejection } => match rejection.reason() {
                Some(rejection) => Self::DeniedWithReason {
                    rejection: rejection.to_string(),
                },
                None => Self::Denied,
            },
            ReviewDecision::TimedOut => Self::TimedOut,
            ReviewDecision::Abort => Self::Abort,
        }
    }
}

impl Serialize for ReviewDecision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ReviewDecisionWire::from(self).serialize(serializer)
    }
}

impl JsonSchema for ReviewDecision {
    fn schema_name() -> String {
        "ReviewDecision".to_string()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ReviewDecisionWire::json_schema(generator)
    }
}

impl Default for ReviewDecision {
    fn default() -> Self {
        Self::denied()
    }
}

impl ReviewDecision {
    /// Creates a denial without a client-visible rejection reason.
    pub fn denied() -> Self {
        Self::Denied
    }

    /// Creates a denial with a UTF-8-safe, bounded rejection reason.
    pub fn denied_with_reason(reason: impl Into<String>) -> Self {
        Self::DeniedWithReason {
            rejection: ReviewRejection::with_reason(reason),
        }
    }

    /// Returns the bounded rejection reason for a denial decision.
    pub fn rejection_reason(&self) -> Option<&str> {
        match self {
            Self::DeniedWithReason { rejection } => rejection.reason(),
            Self::Approved
            | Self::ApprovedExecpolicyAmendment { .. }
            | Self::ApprovedForSession
            | Self::ApprovedMcpPolicyAmendment
            | Self::NetworkPolicyAmendment { .. }
            | Self::Denied
            | Self::TimedOut
            | Self::Abort => None,
        }
    }

    /// Returns an opaque version of the decision without PII. We can't use an ignored flag
    /// on `serde` because the serialization is required by some surfaces.
    pub fn to_opaque_string(&self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ApprovedExecpolicyAmendment { .. } => "approved_with_amendment",
            Self::ApprovedForSession => "approved_for_session",
            Self::ApprovedMcpPolicyAmendment => "approved_mcp_policy_amendment",
            Self::NetworkPolicyAmendment {
                network_policy_amendment,
            } => match network_policy_amendment.action {
                NetworkPolicyRuleAction::Allow => "approved_with_network_policy_allow",
                NetworkPolicyRuleAction::Deny => "denied_with_network_policy_deny",
            },
            Self::Denied | Self::DeniedWithReason { .. } => "denied",
            Self::TimedOut => "timed_out",
            Self::Abort => "abort",
        }
    }
}

impl<'de> Deserialize<'de> for ReviewDecision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        CompatibleReviewDecision::deserialize(deserializer).map(Into::into)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CompatibleReviewDecision {
    Current(CurrentReviewDecision),
    LegacyDenied(LegacyDeniedDecision),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurrentReviewDecision {
    Approved,
    ApprovedExecpolicyAmendment {
        proposed_execpolicy_amendment: ExecPolicyAmendment,
    },
    ApprovedForSession,
    ApprovedMcpPolicyAmendment,
    NetworkPolicyAmendment {
        network_policy_amendment: NetworkPolicyAmendment,
    },
    Denied {
        #[serde(default)]
        rejection: ReviewRejection,
    },
    TimedOut,
    Abort,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyDeniedDecision {
    Denied,
}

impl From<CompatibleReviewDecision> for ReviewDecision {
    fn from(value: CompatibleReviewDecision) -> Self {
        match value {
            CompatibleReviewDecision::Current(decision) => decision.into(),
            CompatibleReviewDecision::LegacyDenied(LegacyDeniedDecision::Denied) => Self::denied(),
        }
    }
}

impl From<CurrentReviewDecision> for ReviewDecision {
    fn from(value: CurrentReviewDecision) -> Self {
        match value {
            CurrentReviewDecision::Approved => Self::Approved,
            CurrentReviewDecision::ApprovedExecpolicyAmendment {
                proposed_execpolicy_amendment,
            } => Self::ApprovedExecpolicyAmendment {
                proposed_execpolicy_amendment,
            },
            CurrentReviewDecision::ApprovedForSession => Self::ApprovedForSession,
            CurrentReviewDecision::ApprovedMcpPolicyAmendment => Self::ApprovedMcpPolicyAmendment,
            CurrentReviewDecision::NetworkPolicyAmendment {
                network_policy_amendment,
            } => Self::NetworkPolicyAmendment {
                network_policy_amendment,
            },
            CurrentReviewDecision::Denied { rejection } => match rejection.reason() {
                Some(_) => Self::DeniedWithReason { rejection },
                None => Self::Denied,
            },
            CurrentReviewDecision::TimedOut => Self::TimedOut,
            CurrentReviewDecision::Abort => Self::Abort,
        }
    }
}

#[cfg(test)]
#[path = "review_decision_tests.rs"]
mod tests;
