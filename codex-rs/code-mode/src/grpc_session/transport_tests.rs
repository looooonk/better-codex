use pretty_assertions::assert_eq;

use super::validate_endpoint;

#[test]
fn endpoint_accepts_loopback_http_and_https() {
    assert_eq!(
        validate_endpoint("http://127.0.0.1:45123")
            .unwrap()
            .as_str(),
        "http://127.0.0.1:45123/"
    );
    assert_eq!(
        validate_endpoint("https://code-mode.example:45123")
            .unwrap()
            .as_str(),
        "https://code-mode.example:45123/"
    );
    assert_eq!(
        validate_endpoint("http://[::1]:45123").unwrap().as_str(),
        "http://[::1]:45123/"
    );
}

#[test]
fn endpoint_rejects_plaintext_remote_hosts() {
    for endpoint in [
        "http://example.com:45123",
        "http://localhost:45123",
        "http://192.0.2.1:45123",
        "http://[2001:db8::1]:45123",
    ] {
        assert_eq!(
            validate_endpoint(endpoint).unwrap_err(),
            "plaintext gRPC code-mode hosts must use a loopback IP address"
        );
    }
}

#[test]
fn endpoint_rejects_components_without_echoing_secrets() {
    for endpoint in [
        "http://alice:super-secret@127.0.0.1:45123",
        "http://127.0.0.1:45123/super-secret",
        "http://127.0.0.1:45123?token=super-secret",
        "http://127.0.0.1:45123#super-secret",
        "not-a-url-super-secret",
    ] {
        let error = validate_endpoint(endpoint).unwrap_err();
        assert!(!error.contains("super-secret"));
    }
}
