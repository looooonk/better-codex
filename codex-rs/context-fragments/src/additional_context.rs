use codex_utils_string::truncate_middle_with_token_budget;

use crate::ContextualUserFragment;

pub const MAX_ADDITIONAL_CONTEXT_ITEMS: usize = 32;
pub const MAX_ADDITIONAL_CONTEXT_KEY_BYTES: usize = 128;
pub const MAX_ADDITIONAL_CONTEXT_TOTAL_TOKENS: usize = 8_000;
pub const MAX_ADDITIONAL_CONTEXT_VALUE_TOKENS: usize = 1_000;
const ADDITIONAL_CONTEXT_END_MARKER_SUFFIX: &str = ">";
const ADDITIONAL_CONTEXT_START_MARKER_PREFIX: &str = "<external_";
const INVALID_ADDITIONAL_CONTEXT_KEY: &str = "invalid_source";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdditionalContextUserFragment {
    key: String,
    value: String,
}

impl AdditionalContextUserFragment {
    pub fn new(key: String, value: String) -> Self {
        Self {
            key: safe_additional_context_key(key),
            value,
        }
    }
}

impl ContextualUserFragment for AdditionalContextUserFragment {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (
            ADDITIONAL_CONTEXT_START_MARKER_PREFIX,
            ADDITIONAL_CONTEXT_END_MARKER_SUFFIX,
        )
    }

    fn matches_text(text: &str) -> bool {
        let trimmed = text.trim();
        let Some(rest) = trimmed.strip_prefix(ADDITIONAL_CONTEXT_START_MARKER_PREFIX) else {
            return false;
        };
        let Some((key, value_and_close)) = rest.split_once(ADDITIONAL_CONTEXT_END_MARKER_SUFFIX)
        else {
            return false;
        };

        if !is_valid_additional_context_key(key) {
            return false;
        }

        value_and_close.ends_with(&format!("</external_{key}>"))
    }

    fn body(&self) -> String {
        additional_context_body(&self.key, &self.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdditionalContextDeveloperFragment {
    key: String,
    value: String,
}

impl AdditionalContextDeveloperFragment {
    pub fn new(key: String, value: String) -> Self {
        Self {
            key: safe_additional_context_key(key),
            value,
        }
    }
}

impl ContextualUserFragment for AdditionalContextDeveloperFragment {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }

    fn body(&self) -> String {
        additional_context_developer_body(&self.key, &self.value)
    }
}

fn additional_context_body(key: &str, value: &str) -> String {
    let value = bounded_xml_value(value);
    format!("{key}>{value}</external_{key}")
}

fn additional_context_developer_body(key: &str, value: &str) -> String {
    let value = bounded_xml_value(value);
    format!("<{key}>{value}</{key}>")
}

pub fn is_valid_additional_context_key(key: &str) -> bool {
    if key.is_empty() || key.len() > MAX_ADDITIONAL_CONTEXT_KEY_BYTES {
        return false;
    }

    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn safe_additional_context_key(key: String) -> String {
    if is_valid_additional_context_key(&key) {
        key
    } else {
        INVALID_ADDITIONAL_CONTEXT_KEY.to_string()
    }
}

fn bounded_xml_value(value: &str) -> String {
    let escaped = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    truncate_middle_with_token_budget(&escaped, MAX_ADDITIONAL_CONTEXT_VALUE_TOKENS).0
}
