//! Read the last reported usage near each indexed turn boundary without changing its rollout.
use codex_rollout::RolloutItem;
use codex_rollout::RolloutLine;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::path::Path;

// Usage normally directly precedes completion. Avoid replaying long turns to populate a label.
const TAIL_BYTES: u64 = 64 * 1024;

pub(super) async fn read(path: &Path, turns: Vec<(String, u64)>) -> HashMap<String, i64> {
    let Some(path) = codex_rollout::existing_rollout_path(path).await else {
        return HashMap::new();
    };
    tokio::task::spawn_blocking(move || -> io::Result<_> {
        let file = File::open(&path)?;
        if path.extension().is_some_and(|ext| ext == "zst") {
            // Walk the archive once, in order, without extracting it into the sessions directory.
            let decoder = zstd::stream::read::Decoder::new(file)?;
            read_tails(decoder, turns, |reader, distance| {
                io::copy(&mut reader.take(distance), &mut io::sink()).map(|_| ())
            })
        } else {
            read_tails(file, turns, |file, distance| {
                file.seek_relative(i64::try_from(distance).map_err(io::Error::other)?)
            })
        }
    })
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default()
}

fn read_tails<R: Read>(
    mut reader: R,
    mut turns: Vec<(String, u64)>,
    skip: impl Fn(&mut R, u64) -> io::Result<()>,
) -> io::Result<HashMap<String, i64>> {
    turns.sort_unstable_by_key(|(_, end)| *end);
    let ends: HashMap<_, _> = turns.iter().map(|(id, end)| (id.as_str(), *end)).collect();
    let mut counts = HashMap::new();
    let mut buffer = Vec::new();
    let mut offset = 0;
    for (_, end) in &turns {
        let start = end.saturating_sub(TAIL_BYTES).max(offset);
        if start > offset {
            skip(&mut reader, start - offset)?;
        }
        buffer.resize((end - start) as usize, 0);
        reader.read_exact(&mut buffer)?;
        offset = start;
        for line in buffer.split_inclusive(|byte| *byte == b'\n') {
            offset += line.len() as u64;
            // Skip large message and tool records before asking serde to parse anything.
            if let Ok(text) = std::str::from_utf8(line)
                && text.contains("\"token_usage_record\"")
                && let Ok(RolloutLine {
                    item: RolloutItem::TokenUsageRecord(record),
                    ..
                }) = codex_rollout::parse_rollout_line(text)
                && ends
                    .get(record.turn_id.as_str())
                    .is_some_and(|end| offset <= *end)
            {
                counts.insert(record.turn_id, record.turn_token_usage.output_tokens);
            }
        }
    }
    Ok(counts)
}

#[cfg(test)]
#[path = "turn_usage_tests.rs"]
mod tests;
