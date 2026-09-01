use codex_extension_api::ApprovalReviewAction;
use codex_extension_api::ApprovalReviewEvidence;
use codex_extension_api::ApprovalReviewFailure;
use codex_extension_api::ApprovalReviewImage;
use codex_extension_api::ApprovalReviewInput;
use codex_extension_api::ApprovalReviewOutcome;
use codex_extension_api::ApprovalReviewResult;
use codex_extension_api::ToolCallSource;
use codex_protocol::models::ResponseItem;
use codex_secrets::redact_secrets;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;

const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_ACTION_BYTES: usize = 16 * 1024;
const MAX_ACTION_TOKENS: usize = 1_000;
const MAX_HISTORY_ITEMS: usize = 40;
const MAX_HISTORY_ITEM_BYTES: usize = 16 * 1024;
const MAX_HISTORY_BYTES: usize = 64 * 1024;
const MAX_HISTORY_STRING_BYTES: usize = 8 * 1024;
const MAX_HISTORY_STRING_TOKENS: usize = 1_000;
const MAX_EVIDENCE_ITEMS: usize = 40;
const MAX_EVIDENCE_BYTES: usize = 64 * 1024;
const MAX_EVIDENCE_TEXT_BYTES: usize = 8 * 1024;
const MAX_EVIDENCE_TEXT_TOKENS: usize = 1_000;
const MAX_IMAGES: usize = 2;
const MAX_ENCODED_IMAGE_BYTES: usize = 12 * 1024;
const MAX_RATIONALE_BYTES: usize = 4 * 1024;
const MAX_RATIONALE_TOKENS: usize = 1_000;

pub(super) fn prepare_approval_review_input(
    input: ApprovalReviewInput,
) -> Result<ApprovalReviewInput, ApprovalReviewFailure> {
    validate_binding(&input)?;
    let action = prepare_approval_review_action(input.action)?;
    let history = bounded_history(input.history);
    let evidence = bounded_evidence(input.evidence);
    validate_images(&input.images)?;
    Ok(ApprovalReviewInput {
        binding: input.binding,
        action,
        history,
        evidence,
        images: input.images,
        deadline: input.deadline,
        cancellation: input.cancellation,
    })
}

pub(super) fn sanitize_approval_review_result(
    result: ApprovalReviewResult,
) -> ApprovalReviewResult {
    match result {
        ApprovalReviewResult::Allow(outcome) => {
            ApprovalReviewResult::Allow(sanitize_outcome(outcome))
        }
        ApprovalReviewResult::Deny(outcome) => {
            ApprovalReviewResult::Deny(sanitize_outcome(outcome))
        }
        ApprovalReviewResult::ManualReview(failure) => ApprovalReviewResult::ManualReview(failure),
        ApprovalReviewResult::Cancelled => ApprovalReviewResult::Cancelled,
    }
}

fn validate_binding(input: &ApprovalReviewInput) -> Result<(), ApprovalReviewFailure> {
    for identifier in [
        input.binding.thread_id.as_str(),
        input.binding.turn_id.as_str(),
        input.binding.action_id.as_str(),
        input.binding.attempt_id.as_str(),
    ] {
        validate_identifier(identifier)?;
    }
    match &input.binding.source {
        ToolCallSource::Direct => {}
        ToolCallSource::CodeMode {
            cell_id,
            runtime_tool_call_id,
        } => {
            validate_identifier(cell_id)?;
            validate_identifier(runtime_tool_call_id)?;
        }
    }
    Ok(())
}

fn validate_identifier(identifier: &str) -> Result<(), ApprovalReviewFailure> {
    if identifier.trim().is_empty()
        || identifier.len() > MAX_IDENTIFIER_BYTES
        || identifier.chars().any(char::is_control)
        || redact_secrets(identifier.to_string()) != identifier
    {
        Err(ApprovalReviewFailure::InvalidInput)
    } else {
        Ok(())
    }
}

pub(super) fn prepare_approval_review_action(
    action: ApprovalReviewAction,
) -> Result<ApprovalReviewAction, ApprovalReviewFailure> {
    let mut payload = serde_json::json!({
        "canonical": action.assessment_action(),
        "requestPayload": action.request_payload(),
    });
    let raw = serde_json::to_vec(&payload).map_err(|_| ApprovalReviewFailure::InvalidInput)?;
    if raw.len() > MAX_ACTION_BYTES
        || approx_token_count(&String::from_utf8_lossy(&raw)) > MAX_ACTION_TOKENS
    {
        return Err(ApprovalReviewFailure::ActionTooLarge);
    }
    redact_json(&mut payload, MAX_ACTION_BYTES, MAX_ACTION_TOKENS);
    let serialized =
        serde_json::to_vec(&payload).map_err(|_| ApprovalReviewFailure::InvalidInput)?;
    if serialized.len() > MAX_ACTION_BYTES
        || approx_token_count(&String::from_utf8_lossy(&serialized)) > MAX_ACTION_TOKENS
    {
        return Err(ApprovalReviewFailure::ActionTooLarge);
    }
    Ok(match action {
        ApprovalReviewAction::Command {
            source,
            command,
            argv,
            cwd,
            sandbox_permissions,
            additional_permissions,
            justification,
            tty,
        } => {
            reject_secret_in_structured(&cwd)?;
            reject_secret_in_structured(&additional_permissions)?;
            ApprovalReviewAction::Command {
                source,
                command: redact_secrets(command),
                argv: argv.into_iter().map(redact_secrets).collect(),
                cwd,
                sandbox_permissions,
                additional_permissions,
                justification: justification.map(redact_secrets),
                tty,
            }
        }
        ApprovalReviewAction::Execve {
            source,
            program,
            argv,
            cwd,
            additional_permissions,
        } => {
            reject_secret_in_structured(&cwd)?;
            reject_secret_in_structured(&additional_permissions)?;
            ApprovalReviewAction::Execve {
                source,
                program: redact_secrets(program),
                argv: argv.into_iter().map(redact_secrets).collect(),
                cwd,
                additional_permissions,
            }
        }
        ApprovalReviewAction::ApplyPatch { cwd, files, patch } => {
            reject_secret_in_structured(&cwd)?;
            reject_secret_in_structured(&files)?;
            ApprovalReviewAction::ApplyPatch {
                cwd,
                files,
                patch: redact_secrets(patch),
            }
        }
        ApprovalReviewAction::RequestPermissions {
            reason,
            permissions,
        } => {
            reject_secret_in_structured(&permissions)?;
            ApprovalReviewAction::RequestPermissions {
                reason: reason.map(redact_secrets),
                permissions,
            }
        }
    })
}

fn reject_secret_in_structured(value: &impl Serialize) -> Result<(), ApprovalReviewFailure> {
    let serialized =
        serde_json::to_string(value).map_err(|_| ApprovalReviewFailure::InvalidInput)?;
    if redact_secrets(serialized.clone()) == serialized {
        Ok(())
    } else {
        Err(ApprovalReviewFailure::InvalidInput)
    }
}

fn bounded_history(history: Vec<ResponseItem>) -> Vec<ResponseItem> {
    let mut bounded = VecDeque::new();
    let mut retained_bytes = 2_usize;
    for item in history.into_iter().rev() {
        let Ok(mut value) = serde_json::to_value(item) else {
            continue;
        };
        redact_json(
            &mut value,
            MAX_HISTORY_STRING_BYTES,
            MAX_HISTORY_STRING_TOKENS,
        );
        let Ok(bytes) = serde_json::to_vec(&value) else {
            continue;
        };
        let serialized_bytes = bytes.len().saturating_add(1);
        if bytes.len() > MAX_HISTORY_ITEM_BYTES
            || retained_bytes.saturating_add(serialized_bytes) > MAX_HISTORY_BYTES
        {
            continue;
        }
        let Ok(item) = serde_json::from_value(value) else {
            continue;
        };
        retained_bytes = retained_bytes.saturating_add(serialized_bytes);
        bounded.push_front(item);
        if bounded.len() == MAX_HISTORY_ITEMS {
            break;
        }
    }
    bounded.into()
}

fn bounded_evidence(evidence: Vec<ApprovalReviewEvidence>) -> Vec<ApprovalReviewEvidence> {
    let mut bounded = VecDeque::new();
    let mut retained_bytes = 0_usize;
    for entry in evidence.into_iter().rev() {
        let entry = ApprovalReviewEvidence {
            kind: bounded_redacted_text(entry.kind, 128, 32),
            provenance: entry
                .provenance
                .map(|value| bounded_redacted_text(value, 512, 128)),
            text: bounded_redacted_text(
                entry.text,
                MAX_EVIDENCE_TEXT_BYTES,
                MAX_EVIDENCE_TEXT_TOKENS,
            ),
        };
        let retained = entry
            .kind
            .len()
            .saturating_add(entry.provenance.as_ref().map_or(0, String::len))
            .saturating_add(entry.text.len());
        if retained_bytes.saturating_add(retained) > MAX_EVIDENCE_BYTES {
            continue;
        }
        retained_bytes = retained_bytes.saturating_add(retained);
        bounded.push_front(entry);
        if bounded.len() == MAX_EVIDENCE_ITEMS {
            break;
        }
    }
    bounded.into()
}

fn validate_images(images: &[ApprovalReviewImage]) -> Result<(), ApprovalReviewFailure> {
    if images.len() > MAX_IMAGES {
        return Err(ApprovalReviewFailure::InvalidInput);
    }
    let mut encoded_bytes = 0_usize;
    for image in images {
        let Some((metadata, payload)) = image.data_url.split_once(',') else {
            return Err(ApprovalReviewFailure::InvalidInput);
        };
        if !metadata
            .get(..11)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:image/"))
            || !metadata
                .split(';')
                .any(|part| part.eq_ignore_ascii_case("base64"))
            || payload.is_empty()
        {
            return Err(ApprovalReviewFailure::InvalidInput);
        }
        encoded_bytes = encoded_bytes.saturating_add(payload.len());
        if encoded_bytes > MAX_ENCODED_IMAGE_BYTES {
            return Err(ApprovalReviewFailure::InvalidInput);
        }
    }
    Ok(())
}

fn sanitize_outcome(outcome: ApprovalReviewOutcome) -> ApprovalReviewOutcome {
    ApprovalReviewOutcome {
        risk_level: outcome.risk_level,
        user_authorization: outcome.user_authorization,
        rationale: bounded_redacted_text(
            outcome.rationale,
            MAX_RATIONALE_BYTES,
            MAX_RATIONALE_TOKENS,
        ),
    }
}

fn redact_json(value: &mut Value, max_bytes: usize, max_tokens: usize) {
    match value {
        Value::String(text) => {
            *text = bounded_redacted_text(std::mem::take(text), max_bytes, max_tokens);
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| redact_json(value, max_bytes, max_tokens)),
        Value::Object(values) => {
            let mut redacted = serde_json::Map::with_capacity(values.len());
            for (key, mut value) in std::mem::take(values) {
                redact_json(&mut value, max_bytes, max_tokens);
                redacted.insert(bounded_redacted_text(key, 256, 64), value);
            }
            *values = redacted;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn bounded_redacted_text(text: String, max_bytes: usize, max_tokens: usize) -> String {
    let text = truncate_bytes(text, max_bytes.saturating_mul(4));
    let text = redact_secrets(text);
    let text = truncate_text(&text, TruncationPolicy::Tokens(max_tokens));
    truncate_bytes(text, max_bytes)
}

fn truncate_bytes(mut text: String, max_bytes: usize) -> String {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.truncate(end);
    text.shrink_to_fit();
    text
}

#[cfg(test)]
#[path = "approval_review_boundary_tests.rs"]
mod tests;
