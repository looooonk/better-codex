use serde::Serialize;

/// One redacted, bounded item available to a Guardian review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GuardianEvidenceEntry {
    pub kind: String,
    pub provenance: Option<String>,
    pub text: String,
}
