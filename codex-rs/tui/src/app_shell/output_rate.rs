use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

const RATE_WINDOW: Duration = Duration::from_secs(3);
const BUCKET_WIDTH: Duration = Duration::from_millis(100);

#[derive(Default)]
pub(super) struct OutputRate {
    // At most 30 buckets, regardless of how the transport splits text into chunks.
    buckets: VecDeque<(Instant, f64)>,
    stream_started_at: Option<Instant>,
    previous: char,
}

impl OutputRate {
    pub(super) fn record(&mut self, now: Instant, text: &str) -> f64 {
        while self
            .buckets
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= RATE_WINDOW)
        {
            self.buckets.pop_front();
        }
        if self.buckets.is_empty() {
            self.stream_started_at = Some(now);
            self.previous = '\0';
        }

        let tokens = estimate_tokens(text, &mut self.previous);
        if let Some((at, count)) = self.buckets.back_mut()
            && now.duration_since(*at) < BUCKET_WIDTH
        {
            *count += tokens;
        } else {
            self.buckets.push_back((now, tokens));
        }
        tokens
    }

    pub(super) fn tokens_per_second(&self, now: Instant) -> u64 {
        let Some(started) = self.stream_started_at else {
            return 0;
        };
        let tokens: f64 = self
            .buckets
            .iter()
            .filter(|(at, _)| now.duration_since(*at) < RATE_WINDOW)
            .map(|(_, tokens)| tokens)
            .sum();
        // Warm up over the observed stream time, with a one-second floor for buffered first chunks.
        let seconds = now
            .duration_since(started)
            .clamp(Duration::from_secs(1), RATE_WINDOW);
        (tokens / seconds.as_secs_f64()).round() as u64
    }
}

pub(super) fn estimate_tokens(text: &str, previous: &mut char) -> f64 {
    let mut tokens = 0.0;
    for c in text.chars() {
        tokens += token_weight(c, *previous);
        *previous = c;
    }
    tokens
}

// Calibrated against o200k_base counts on source code, Markdown, and translated UI text.
// A word start (also "V" in "HeaderView") usually costs more than a letter within a word.
fn token_weight(c: char, previous: char) -> f64 {
    let weight = match c {
        'a'..='z' | 'A'..='Z' => 0.05,
        '0'..='9' if previous.is_ascii_digit() => 0.34,
        '0'..='9' => 1.1,
        '\n' => 0.85,
        _ if c.is_whitespace() => return if previous.is_whitespace() { 0.04 } else { 0.0 },
        _ if c.is_ascii() => 0.54,
        '\u{0080}'..='\u{024f}' => 0.5, // Latin-1 and Latin Extended A/B.
        '\u{0250}'..='\u{07ff}' => 0.18, // Includes Greek, Cyrillic, Hebrew and Arabic.
        '\u{0900}'..='\u{0dff}' => 0.45, // Indic scripts.
        _ if c.len_utf8() == 3 => 0.72,
        _ => 1.75,
    };
    if c.len_utf8() <= 2
        && c.is_alphabetic()
        && (!previous.is_alphabetic() || previous.is_ascii_lowercase() && c.is_ascii_uppercase())
    {
        weight + 0.85
    } else {
        weight
    }
}
