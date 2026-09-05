//! Version metadata shared across Better Codex process boundaries.

#![forbid(unsafe_code)]

/// The upstream Codex release whose backend-facing contract this fork implements.
///
/// This is intentionally independent of the Better Codex package version. Update
/// it when backend request metadata and reserved tool schemas are synchronized
/// with a newer upstream Codex release.
pub const CODEX_BACKEND_COMPAT_VERSION: &str = "0.145.0";

/// Model catalog generation understood by this client, independent of the harness wire contract.
pub const CODEX_MODEL_CATALOG_VERSION: &str = "0.153.0";
