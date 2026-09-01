use codex_code_mode_protocol::grpc::MAX_CONTENT_ITEMS;
use pretty_assertions::assert_eq;
use tonic::Code;

use super::ResponseShape;
use super::ResponseAdmission;
use super::preflight_response;

fn length_delimited(field: u8, value: &[u8]) -> Vec<u8> {
    let mut output = vec![field << 3 | 2];
    let mut length = value.len() as u64;
    loop {
        let mut byte = (length & 0x7f) as u8;
        length >>= 7;
        if length != 0 {
            byte |= 0x80;
        }
        output.push(byte);
        if length == 0 {
            break;
        }
    }
    output.extend_from_slice(value);
    output
}

#[test]
fn repeated_content_items_are_rejected_before_prost_decode() {
    let outcome = [0x12, 0].repeat(MAX_CONTENT_ITEMS + 1);
    for (shape, field) in [(ResponseShape::Execute, 2), (ResponseShape::Wait, 1)] {
        let error = preflight_response(shape, &length_delimited(field, &outcome)).unwrap_err();
        assert_eq!(error.code(), Code::ResourceExhausted);
    }
}

#[test]
fn bounded_content_items_and_unrelated_nested_fields_are_accepted() {
    let mut outcome = [0x12, 0].repeat(MAX_CONTENT_ITEMS);
    outcome.extend_from_slice(&length_delimited(7, &[0x12, 0]));

    assert!(preflight_response(ResponseShape::Execute, &length_delimited(2, &outcome)).is_ok());
}

#[test]
fn normal_request_saturation_preserves_critical_headroom() {
    let admission = ResponseAdmission::new();
    let normal = (0..super::MAX_NORMAL_REQUESTS)
        .map(|_| admission.request_permit(super::WAIT_PATH).unwrap())
        .collect::<Vec<_>>();

    assert!(admission.request_permit(super::WAIT_PATH).is_err());
    assert!(admission.request_permit(super::CLOSE_PATH).is_ok());
    drop(normal);
}
