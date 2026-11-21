use codex_protocol::output_tokens::estimate_tokens;
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
    paused_at: Option<Instant>,
    // None when attaching mid-turn: earlier waits cannot be recovered from live events.
    completed_tool_wait: Option<Duration>,
}

impl OutputRate {
    pub(super) fn observe_turn_start(&mut self) {
        self.completed_tool_wait.get_or_insert(Duration::ZERO);
    }

    pub(super) fn invalidate_turn_wait(&mut self) {
        self.completed_tool_wait = None;
    }

    pub(super) fn total_tool_wait(&self, now: Instant) -> Option<Duration> {
        let closed = self.completed_tool_wait?;
        let open = self
            .paused_at
            .map_or(Duration::ZERO, |at| now.duration_since(at));
        Some(closed + open)
    }

    pub(super) fn pause(&mut self, now: Instant) {
        self.paused_at.get_or_insert(now);
    }

    pub(super) fn resume(&mut self, now: Instant) {
        // Shift the bounded window onto the resumed clock so only model time ages it.
        if let Some(paused) = self.paused_at.take() {
            let wait = now.duration_since(paused);
            if let Some(total) = &mut self.completed_tool_wait {
                *total += wait;
            }
            for (at, _) in &mut self.buckets {
                *at += wait;
            }
            if let Some(started) = &mut self.stream_started_at {
                *started += wait;
            }
            self.previous = '\0';
        }
    }

    pub(super) fn record(&mut self, now: Instant, text: &str) {
        let now = self.paused_at.unwrap_or(now);
        if self
            .buckets
            .back()
            .is_none_or(|(at, _)| now.duration_since(*at) >= RATE_WINDOW)
        {
            self.previous = '\0';
        }
        let tokens = estimate_tokens(text, &mut self.previous);
        self.add_tokens(now, tokens);
    }

    pub(super) fn add_tokens(&mut self, now: Instant, tokens: f64) {
        let now = self.paused_at.unwrap_or(now);
        while self
            .buckets
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= RATE_WINDOW)
        {
            self.buckets.pop_front();
        }
        if self.buckets.is_empty() {
            self.stream_started_at = Some(now);
        }

        if let Some((at, count)) = self.buckets.back_mut()
            && now.duration_since(*at) < BUCKET_WIDTH
        {
            *count += tokens;
        } else {
            self.buckets.push_back((now, tokens));
        }
    }

    pub(super) fn tokens_per_second(&self, now: Instant) -> u64 {
        let now = self.paused_at.unwrap_or(now);
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
