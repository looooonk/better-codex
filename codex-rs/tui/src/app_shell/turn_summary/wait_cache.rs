//! Native rollouts record usage and elapsed time, but not the model's tool-wait intervals.
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
    tool_wait: Duration,
}

pub(super) fn tool_wait(
    codex_home: Option<&Path>,
    thread_id: ThreadId,
    turn_id: &str,
    elapsed: Duration,
    output_tokens: i64,
    observed_wait: Option<Duration>,
) -> Option<Duration> {
    let Some(codex_home) = codex_home else {
        return observed_wait;
    };
    // Remote servers can use arbitrary turn IDs. Keep them out of filesystem paths.
    let key = Sha256::digest(turn_id.as_bytes());
    let path = codex_home
        .join("better-codex")
        .join("turn-timings")
        .join(thread_id.to_string())
        .join(format!("{key:x}.json"));
    if let Some(tool_wait) = observed_wait {
        let measurement = Measurement {
            elapsed,
            output_tokens,
            tool_wait,
        };
        if let Ok(json) = serde_json::to_string(&measurement)
            && let Err(error) = write_atomically(&path, &json)
        {
            tracing::debug!(%error, "could not cache turn timing");
        }
        return Some(tool_wait);
    }

    // A recovered or replayed turn can retain its ID but have a different terminal result.
    let cached: Measurement = serde_json::from_reader(File::open(path).ok()?.take(512)).ok()?;
    (cached.elapsed == elapsed && cached.output_tokens == output_tokens).then_some(cached.tool_wait)
}
