use std::ffi::OsStr;
use std::io;

use pretty_assertions::assert_eq;
use rmcp::model::ProtocolVersion;
use rmcp::service::ClientLifecycleMode;

use super::McpProtocolMode;

#[test]
fn protocol_modes_select_compatible_sdk_lifecycles() {
    assert_eq!(
        McpProtocolMode::Legacy.preferred_protocol_version(),
        ProtocolVersion::V_2025_06_18
    );
    assert_eq!(
        McpProtocolMode::Legacy.client_lifecycle(),
        ClientLifecycleMode::Initialize
    );
    assert_eq!(
        McpProtocolMode::V20260728.preferred_protocol_version(),
        ProtocolVersion::V_2026_07_28
    );
    assert_eq!(
        McpProtocolMode::V20260728.client_lifecycle(),
        ClientLifecycleMode::Auto {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            legacy_version: Some(ProtocolVersion::V_2025_06_18),
        }
    );
}

#[test]
fn stdio_requires_both_the_modern_feature_and_a_server_opt_in() {
    let modern_version = Some(OsStr::new("2026-07-28"));

    assert_eq!(
        McpProtocolMode::Legacy
            .stdio_mode(/*requested_version*/ None)
            .unwrap(),
        McpProtocolMode::Legacy
    );
    assert_eq!(
        McpProtocolMode::Legacy.stdio_mode(modern_version).unwrap(),
        McpProtocolMode::Legacy
    );
    assert_eq!(
        McpProtocolMode::V20260728
            .stdio_mode(/*requested_version*/ None)
            .unwrap(),
        McpProtocolMode::Legacy
    );
    assert_eq!(
        McpProtocolMode::V20260728
            .stdio_mode(modern_version)
            .unwrap(),
        McpProtocolMode::V20260728
    );
}

#[test]
fn stdio_rejects_unknown_protocol_markers() {
    let error = McpProtocolMode::V20260728
        .stdio_mode(Some(OsStr::new("1999-01-01")))
        .expect_err("an unknown protocol marker must fail");

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("1999-01-01"));
}

#[test]
fn legacy_stdio_does_not_interpret_existing_protocol_markers() {
    assert_eq!(
        McpProtocolMode::Legacy
            .stdio_mode(Some(OsStr::new("1999-01-01")))
            .unwrap(),
        McpProtocolMode::Legacy
    );
}
