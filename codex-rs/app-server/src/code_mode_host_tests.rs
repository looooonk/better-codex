use pretty_assertions::assert_eq;
use url::Url;

use super::AppServerCodeModeHostArgs;
use super::CodeModeHostTransport;
use super::parse_host_url;

#[test]
fn grpc_host_accepts_local_plaintext_and_secure_endpoints() {
    for endpoint in [
        "http://127.0.0.1:8765",
        "http://[::1]:8765",
        "https://example.test",
        "unix:/tmp/code-mode.sock",
        "unix:///tmp/code-mode.sock",
    ] {
        assert_eq!(
            parse_host_url(endpoint),
            Ok(Url::parse(endpoint).expect("test endpoint should parse"))
        );
    }
}

#[test]
fn grpc_host_rejects_plaintext_remote_endpoints() {
    for endpoint in [
        "http://localhost:8765",
        "http://192.0.2.1:8765",
        "http://example.test:8765",
    ] {
        assert_eq!(
            parse_host_url(endpoint).unwrap_err(),
            "plaintext code-mode hosts must use a loopback IP address"
        );
    }
}

#[test]
fn grpc_host_rejects_components_without_disclosing_them() {
    for endpoint in [
        "http://alice:super-secret@127.0.0.1:8765",
        "https://example.test/super-secret",
        "https://example.test?token=super-secret",
        "https://example.test#super-secret",
        "unix:/tmp/code-mode.sock?token=super-secret",
        "unix://alice@host/super-secret",
        "not-a-url-super-secret",
    ] {
        let error = parse_host_url(endpoint).expect_err("endpoint should be rejected");
        assert!(!error.contains("alice"));
        assert!(!error.contains("super-secret"));
    }
}

#[test]
fn omitted_host_selects_local_transport() {
    assert_eq!(
        CodeModeHostTransport::from(AppServerCodeModeHostArgs::default()),
        CodeModeHostTransport::Local
    );
}

#[test]
fn explicit_host_selects_grpc_transport() {
    let url = Url::parse("https://example.test").expect("test endpoint should parse");
    assert_eq!(
        CodeModeHostTransport::from(AppServerCodeModeHostArgs {
            code_mode_host: Some(url.clone()),
        }),
        CodeModeHostTransport::Grpc(url)
    );
}

#[test]
fn programmatic_transport_selection_is_revalidated() {
    let transport = CodeModeHostTransport::Grpc(
        Url::parse("http://example.test:8765").expect("test endpoint should parse"),
    );
    assert_eq!(
        transport.validate(),
        Err("plaintext code-mode hosts must use a loopback IP address".to_string())
    );
}
