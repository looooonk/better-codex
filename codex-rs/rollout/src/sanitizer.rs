//! Secret redaction for copies that leave the live execution/model boundary.

use codex_history::RolloutItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_secrets::redact_secrets;
use regex::Captures;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const MAX_NESTED_COMMAND_DEPTH: usize = 2;
const REDACTION: &str = "[REDACTED_SECRET]";
static AUTHORIZATION_HEADER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    match Regex::new(r"(?im)(?P<prefix>\bauthorization[ \t]*:[ \t]*)(?P<value>[^\r\n]+)") {
        Ok(regex) => regex,
        Err(err) => panic!("invalid authorization header regex: {err}"),
    }
});

/// Redacts recognizable credentials in a serialized persistence or diagnostic copy.
///
/// Identifier fields are left unchanged so replay correlation remains stable. Opaque encrypted
/// content is also preserved: MCP encrypted content must continue to take precedence over any
/// accompanying plaintext representation.
pub fn redact_persisted_json(value: &mut Value) {
    redact_value(value, /*field_name*/ None);
}

/// Returns a redacted copy suitable for the experimental raw app-server diagnostic event.
pub fn redacted_response_item_for_diagnostics(
    item: ResponseItem,
) -> serde_json::Result<ResponseItem> {
    let mut value = serde_json::to_value(item)?;
    redact_persisted_json(&mut value);
    serde_json::from_value(value)
}

/// Returns a redacted event copy for non-execution app-server and trace projections.
pub fn redacted_event_msg_for_diagnostics(event: EventMsg) -> serde_json::Result<EventMsg> {
    let mut value = serde_json::to_value(event)?;
    redact_persisted_json(&mut value);
    serde_json::from_value(value)
}

/// Returns a redacted rollout-item copy for replay into a presentation or diagnostic surface.
pub fn redacted_rollout_item_for_diagnostics(
    item: &RolloutItem,
) -> serde_json::Result<RolloutItem> {
    let mut value = serde_json::to_value(item)?;
    redact_persisted_json(&mut value);
    serde_json::from_value(value)
}

pub(crate) fn potentially_contains_sensitive_material(value: &str) -> bool {
    const NEEDLES: &[&[u8]] = &[
        b"authorization",
        b"bearer",
        b"api_key",
        b"api-key",
        b"apikey",
        b"\"-u\"",
        b" -u ",
        b"--user",
        b"token",
        b"secret",
        b"password",
        b"sk-",
        b"akia",
    ];
    NEEDLES
        .iter()
        .any(|needle| contains_ascii_case_insensitive(value.as_bytes(), needle))
}

fn redact_value(value: &mut Value, field_name: Option<&str>) -> bool {
    if field_name.is_some_and(is_protected_field) {
        return false;
    }
    match value {
        Value::String(text) if field_name.is_some_and(is_command_text_field) => {
            redact_command_text(text)
        }
        Value::String(text) => redact_string(text, field_name),
        Value::Array(values) if field_name.is_some_and(is_command_array_field) => {
            redact_command_array(values)
        }
        Value::Array(values) => {
            let mut changed = false;
            for value in values {
                changed |= redact_value(value, None);
            }
            changed
        }
        Value::Object(object) => {
            let mut changed = false;
            for (name, value) in object {
                changed |= redact_value(value, Some(name));
            }
            changed
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn redact_command_array(values: &mut [Value]) -> bool {
    let Some(mut argv) = values
        .iter()
        .map(Value::as_str)
        .map(|value| value.map(str::to_string))
        .collect::<Option<Vec<_>>>()
    else {
        let mut changed = false;
        for value in values {
            changed |= redact_value(value, None);
        }
        return changed;
    };
    let changed = redact_argv(&mut argv, /*depth*/ 0);
    if changed {
        for (value, argument) in values.iter_mut().zip(argv) {
            *value = Value::String(argument);
        }
    }
    changed
}

fn redact_command_text(text: &mut String) -> bool {
    redact_command_text_at_depth(text, /*depth*/ 0)
}

fn redact_command_text_at_depth(text: &mut String, depth: usize) -> bool {
    let Some(mut argv) = shlex::split(text) else {
        return redact_string(text, /*field_name*/ None);
    };
    if !redact_argv(&mut argv, depth) {
        return false;
    }
    *text =
        shlex::try_join(argv.iter().map(String::as_str)).unwrap_or_else(|_| REDACTION.to_string());
    true
}

fn redact_argv(argv: &mut [String], depth: usize) -> bool {
    let is_curl = argv.first().is_some_and(|executable| {
        executable
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| matches!(name.to_ascii_lowercase().as_str(), "curl" | "curl.exe"))
    });
    let mut redact_next = false;
    let mut inspect_header_next = false;
    let mut changed = false;
    for argument in argv {
        if redact_next {
            *argument = REDACTION.to_string();
            redact_next = false;
            changed = true;
            continue;
        }
        if inspect_header_next {
            inspect_header_next = false;
            if let Some(redacted) = redact_authorization_header(argument) {
                *argument = redacted;
                changed = true;
                continue;
            }
        }
        if let Some((flag, value)) = argument.split_once('=') {
            if is_sensitive_flag(flag, is_curl) {
                *argument = format!("{flag}={REDACTION}");
                changed = true;
                continue;
            }
            if is_header_flag(flag)
                && let Some(redacted) = redact_authorization_header(value)
            {
                *argument = format!("{flag}={redacted}");
                changed = true;
                continue;
            }
        }
        if is_sensitive_flag(argument, is_curl) {
            redact_next = true;
            continue;
        }
        if is_header_flag(argument) {
            inspect_header_next = true;
            continue;
        }
        // Shell wrappers commonly carry a complete command in one argument. Only recurse for
        // strings that already look sensitive, and cap the depth so malformed nesting cannot loop.
        if potentially_contains_sensitive_material(argument)
            && argument.bytes().any(|byte| byte.is_ascii_whitespace())
        {
            if depth >= MAX_NESTED_COMMAND_DEPTH {
                *argument = REDACTION.to_string();
                changed = true;
                continue;
            }
            if redact_command_text_at_depth(argument, depth.saturating_add(1)) {
                changed = true;
                continue;
            }
        }
        let original = std::mem::take(argument);
        let redacted = redact_plain_text(original.clone());
        changed |= redacted != original;
        *argument = redacted;
    }
    changed
}

fn redact_authorization_header(header: &str) -> Option<String> {
    let (name, value) = header.split_once(':')?;
    name.trim()
        .eq_ignore_ascii_case("authorization")
        .then(|| format!("{name}: {}", redacted_authorization_value(value)))
}

fn redacted_authorization_value(value: &str) -> &'static str {
    if value
        .split_ascii_whitespace()
        .next()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("bearer"))
    {
        "Bearer [REDACTED_SECRET]"
    } else {
        REDACTION
    }
}

fn redact_authorization_header_match(captures: &Captures<'_>) -> String {
    format!(
        "{}{}",
        &captures["prefix"],
        redacted_authorization_value(&captures["value"])
    )
}

fn is_command_array_field(field_name: &str) -> bool {
    matches!(
        field_name,
        "args" | "arguments" | "argv" | "command" | "parsed_cmd"
    )
}

fn is_command_text_field(field_name: &str) -> bool {
    matches!(field_name, "cmd" | "command" | "query")
}

fn is_sensitive_flag(flag: &str, is_curl: bool) -> bool {
    (is_curl && matches!(flag.to_ascii_lowercase().as_str(), "-u" | "--user"))
        || flag.eq_ignore_ascii_case("--password")
        || flag.strip_prefix("--").is_some_and(is_credential_field)
}

fn is_header_flag(flag: &str) -> bool {
    flag == "-H" || flag.eq_ignore_ascii_case("--header")
}

fn redact_string(text: &mut String, field_name: Option<&str>) -> bool {
    if field_name.is_some_and(is_credential_field) && !text.is_empty() {
        let redacted = redact_secrets(std::mem::take(text));
        *text = if redacted.contains(REDACTION) {
            redacted
        } else {
            REDACTION.to_string()
        };
        return true;
    }

    if let Ok(mut nested) = serde_json::from_str::<Value>(text) {
        if redact_value(&mut nested, None)
            && let Ok(redacted) = serde_json::to_string(&nested)
        {
            *text = redacted;
            return true;
        }
        return false;
    }

    let original = std::mem::take(text);
    let redacted = redact_plain_text(original.clone());
    let changed = redacted != original;
    *text = redacted;
    changed
}

fn redact_plain_text(text: String) -> String {
    let redacted = redact_secrets(text);
    AUTHORIZATION_HEADER_REGEX
        .replace_all(&redacted, redact_authorization_header_match)
        .into_owned()
}

fn is_protected_field(field_name: &str) -> bool {
    field_name == "id"
        || field_name.ends_with("_id")
        || field_name.ends_with("Id")
        || matches!(
            field_name,
            "encrypted_content" | "encryptedContent" | "user_authorization" | "userAuthorization"
        )
}

fn is_credential_field(field_name: &str) -> bool {
    let normalized = field_name.to_ascii_lowercase().replace('-', "_");
    normalized == "authorization"
        || normalized.ends_with("_authorization")
        || normalized == "aws_secret_access_key"
        || normalized == "api_key"
        || normalized.ends_with("_api_key")
        || normalized.ends_with("apikey")
        || normalized == "token"
        || normalized.ends_with("_token")
        || normalized.ends_with("token")
        || normalized == "secret"
        || normalized.ends_with("_secret")
        || normalized.ends_with("secret")
        || normalized == "password"
        || normalized.ends_with("_password")
        || normalized.ends_with("password")
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

#[cfg(test)]
#[path = "sanitizer_tests.rs"]
mod tests;
