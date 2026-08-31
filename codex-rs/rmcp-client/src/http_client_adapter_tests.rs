use super::HttpHeader;
use super::protocol_headers;
use pretty_assertions::assert_eq;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;

#[test]
fn protocol_headers_preserve_utf8_values() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-plugin-name",
        HeaderValue::from_str("café").expect("valid HTTP field value"),
    );

    assert_eq!(
        protocol_headers(&headers),
        vec![HttpHeader {
            name: "x-plugin-name".to_string(),
            value: "café".to_string(),
        }]
    );
}
