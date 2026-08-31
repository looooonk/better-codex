use schemars::JsonSchema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use thiserror::Error;
use ts_rs::TS;

const MAX_CORRELATION_ID_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SecurityRiskCategory {
    ActionRisk,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SecurityRiskStatus {
    Reviewed,
}

/// Durable action-correlated classifier output excluded from model and UI history.
#[derive(Clone, Debug, JsonSchema, PartialEq, Serialize, TS)]
pub struct SecurityRiskScore {
    review_id: String,
    turn_id: String,
    action_id: String,
    category: SecurityRiskCategory,
    status: SecurityRiskStatus,
    score: f64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InvalidSecurityRiskScore {
    #[error("security risk correlation identifiers must be non-empty and at most 128 bytes")]
    InvalidCorrelationId,
    #[error("security risk score must be finite and between zero and one")]
    InvalidScore,
}

impl SecurityRiskScore {
    pub fn new(
        review_id: impl Into<String>,
        turn_id: impl Into<String>,
        action_id: impl Into<String>,
        score: f64,
    ) -> Result<Self, InvalidSecurityRiskScore> {
        let record = Self {
            review_id: review_id.into(),
            turn_id: turn_id.into(),
            action_id: action_id.into(),
            category: SecurityRiskCategory::ActionRisk,
            status: SecurityRiskStatus::Reviewed,
            score,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn review_id(&self) -> &str {
        &self.review_id
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn action_id(&self) -> &str {
        &self.action_id
    }

    pub fn category(&self) -> SecurityRiskCategory {
        self.category
    }

    pub fn status(&self) -> SecurityRiskStatus {
        self.status
    }

    pub fn score(&self) -> f64 {
        self.score
    }

    fn validate(&self) -> Result<(), InvalidSecurityRiskScore> {
        if [&self.review_id, &self.turn_id, &self.action_id]
            .into_iter()
            .any(|id| {
                id.trim().is_empty()
                    || id.len() > MAX_CORRELATION_ID_BYTES
                    || id.chars().any(char::is_control)
            })
        {
            return Err(InvalidSecurityRiskScore::InvalidCorrelationId);
        }
        if !self.score.is_finite() || !(0.0..=1.0).contains(&self.score) {
            return Err(InvalidSecurityRiskScore::InvalidScore);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for SecurityRiskScore {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireRecord {
            review_id: String,
            turn_id: String,
            action_id: String,
            category: SecurityRiskCategory,
            status: SecurityRiskStatus,
            score: f64,
        }

        let record = WireRecord::deserialize(deserializer)?;
        let value = Self {
            review_id: record.review_id,
            turn_id: record.turn_id,
            action_id: record.action_id,
            category: record.category,
            status: record.status,
            score: record.score,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "security_risk_tests.rs"]
mod tests;
