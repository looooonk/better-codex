//! Estimate the live decode rate from arriving output. Measure completed requests
//! separately: their provider counts include reasoning that may never be streamed.
use codex_protocol::output_tokens::estimate_tokens;
use std::time::Duration;
use std::time::Instant;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
const SMOOTHING_SECONDS: f64 = 1.5;

#[derive(Default)]
pub(super) struct OutputRate {
    live: StreamRate,
    request: Option<ModelRequest>,
    completed_time: Duration,
    witnessed_start: bool,
    partial: bool,
    outstanding_reports: usize,
    closed_output_tokens: i64,
}

struct ModelRequest {
    start: Instant,
    end: Option<Instant>,
    reported: Option<i64>,
}

impl OutputRate {
    pub(super) fn observe_turn_start(&mut self) {
        self.witnessed_start = true;
    }

    pub(super) fn invalidate_timing(&mut self) {
        self.partial = true;
    }

    pub(super) fn begin_request(&mut self, now: Instant) {
        if let Some(request) = self.request.take()
            && let Some(end) = request.end
        {
            self.completed_time += end.saturating_duration_since(request.start);
            if let Some(output) = request.reported {
                self.closed_output_tokens = self.closed_output_tokens.saturating_add(output);
            } else {
                self.outstanding_reports += 1;
            }
        }
        // A superseded request without a completion can be a transport retry.
        self.request = Some(ModelRequest {
            start: now,
            end: None,
            reported: None,
        });
        self.live.begin_stream();
    }

    pub(super) fn end_request(&mut self, now: Instant) {
        if let Some(request) = &mut self.request {
            request.end.get_or_insert(now);
        } else {
            self.invalidate_timing();
        }
    }

    pub(super) fn record_usage(&mut self, now: Instant, output_tokens: i64) {
        // Usage travels through the turn event queue. A fast follow-up can
        // deliver its request-start notification before the previous report.
        if self.outstanding_reports > 0 {
            self.outstanding_reports -= 1;
            self.closed_output_tokens = self.closed_output_tokens.saturating_add(output_tokens);
            return;
        }
        let Some(request) = &mut self.request else {
            self.invalidate_timing();
            return;
        };
        if request.reported.replace(output_tokens).is_some() {
            self.invalidate_timing();
            return;
        }
        let end = *request.end.get_or_insert(now);
        let duration = end.saturating_duration_since(request.start);
        // A provider can return an entire reasoning response without streaming
        // any measurable intervals. Its count (including reasoning) is then
        // the first direct observation of output over this request.
        if !self.live.sampled && output_tokens > 0 && duration >= SAMPLE_INTERVAL {
            self.live.rate = Some(output_tokens as f64 / duration.as_secs_f64());
        }
    }

    pub(super) fn model_time(&self, now: Instant, turn_output: i64) -> Option<Duration> {
        let request = self.request.as_ref()?;
        if !self.witnessed_start || self.partial {
            return None;
        }
        if self.closed_output_tokens == turn_output
            && request.reported.is_none()
            && self.outstanding_reports == 0
        {
            // Cancellation can leave one unbilled request in flight.
            return Some(self.completed_time);
        }
        if let Some(output) = request.reported
            && self.outstanding_reports == 0
            && self.closed_output_tokens.saturating_add(output) != turn_output
        {
            return None;
        }
        Some(
            self.completed_time
                + request
                    .end
                    .unwrap_or(now)
                    .saturating_duration_since(request.start),
        )
    }

    pub(super) fn begin_reasoning(&mut self) {
        self.live.begin_stream();
    }

    pub(super) fn record(&mut self, now: Instant, text: &str) {
        let tokens = estimate_tokens(text, &mut self.live.previous);
        self.add_tokens(now, tokens);
    }

    pub(super) fn add_tokens(&mut self, now: Instant, tokens: f64) {
        if tokens > 0.0 && tokens.is_finite() {
            let at = self
                .request
                .as_ref()
                .and_then(|request| request.end)
                .unwrap_or(now);
            self.live.add(at, tokens);
        }
    }

    pub(super) fn tokens_per_second(&self) -> Option<u64> {
        self.live.rate.map(|rate| rate.round() as u64)
    }
}

#[derive(Default)]
struct StreamRate {
    rate: Option<f64>,
    interval_start: Option<Instant>,
    pending_tokens: f64,
    previous: char,
    sampled: bool,
}

impl StreamRate {
    fn begin_stream(&mut self) {
        // Wait for the first packet before advancing the sample clock. Keep the
        // last measured rate throughout the tool wait and the next request's prefill.
        self.interval_start = None;
        self.pending_tokens = 0.0;
        self.previous = '\0';
        self.sampled = false;
    }

    fn add(&mut self, now: Instant, tokens: f64) {
        let start = *self.interval_start.get_or_insert(now);
        let elapsed = now.saturating_duration_since(start);
        // The first packet has no preceding decode interval. Counting it in a
        // short second packet's interval can produce a large startup spike.
        if !self.sampled && elapsed.is_zero() {
            return;
        }
        self.pending_tokens += tokens;
        if elapsed < SAMPLE_INTERVAL {
            return;
        }
        let seconds = elapsed.as_secs_f64();
        let sample = std::mem::take(&mut self.pending_tokens) / seconds;
        let weight = -(-seconds / SMOOTHING_SECONDS).exp_m1();
        self.rate = Some(
            self.rate
                .map_or(sample, |rate| rate + weight * (sample - rate)),
        );
        self.interval_start = Some(now);
        self.sampled = true;
    }
}
