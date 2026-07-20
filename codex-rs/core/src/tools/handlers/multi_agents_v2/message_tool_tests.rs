use pretty_assertions::assert_eq;

use super::message_content;

#[test]
fn message_content_preserves_large_payloads() {
    let message = "message payload ".repeat(3_000);

    assert_eq!(message_content(message.clone()), Ok(message));
}
