//! Native rollouts record usage and elapsed time, but not per-request generation time.
//! Keep our measurement separate so opening a saved turn only reads session data.
use codex_protocol::ThreadId;
use codex_utils_path::write_atomically;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

#[derive(Deserialize, Serialize)]
struct Measurement {
    elapsed: Duration,
    output_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_time: Option<Duration>,
    // Older clients cached only the interval spent waiting for tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_wait: Option<Duration>,
}

pub(super) fn model_time(
    codex_home: Option<&Path>,
    thread_id: ThreadId,
    turn_id: &str,
    elapsed: Duration,
    output_tokens: i64,
    observed_model_time: Option<Duration>,
) -> Option<Duration> {
    let Some(codex_home) = codex_home else {
        return observed_model_time.map(|time| time.min(elapsed));
    };
    // Remote servers can use arbitrary turn IDs. Keep them out of filesystem paths.
    let key = Sha256::digest(turn_id.as_bytes());
    let path = codex_home
        .join("better-codex")
        .join("turn-timings")
        .join(thread_id.to_string())
        .join(format!("{key:x}.json"));
    if let Some(model_time) = observed_model_time {
        let model_time = model_time.min(elapsed);
        let measurement = Measurement {
            elapsed,
            output_tokens,
            model_time: Some(model_time),
            tool_wait: None,
        };
        if let Ok(json) = serde_json::to_string(&measurement)
            && let Err(error) = write_atomically(&path, &json)
        {
            tracing::debug!(%error, "could not cache turn timing");
        }
        return Some(model_time);
    }

    // A recovered or replayed turn can retain its ID but have a different terminal result.
    let cached: Measurement = serde_json::from_reader(File::open(path).ok()?.take(512)).ok()?;
    if cached.elapsed != elapsed || cached.output_tokens != output_tokens {
        return None;
    }
    cached
        .model_time
        .or_else(|| elapsed.checked_sub(cached.tool_wait?))
}
