//! Transcript previews and full captures for streaming tools.

use super::ShellState;
use super::ToolBlockStatus;
use super::ToolOutputBuffer;
use super::TranscriptKind;
use super::TranscriptLine;
use super::next_local_output_item_id;

pub(super) const TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS: usize = 8_000;
pub(super) const TRANSCRIPT_OUTPUT_HIGH_WATER_LINES: usize = 160;
const TRANSCRIPT_OUTPUT_LOW_WATER_CHARS: usize = 6_000;
const TRANSCRIPT_OUTPUT_LOW_WATER_LINES: usize = 120;
pub(super) const TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX: &str = "... earlier output omitted ...\n";

impl TranscriptLine {
    #[cfg(test)]
    pub(super) fn output(
        text: impl Into<ToolOutputBuffer>,
        status: ToolBlockStatus,
        item_id: String,
    ) -> Self {
        let full_text = text.into();
        Self::with_output_preview(
            compact_output_for_transcript(&full_text),
            full_text,
            status,
            item_id,
        )
    }

    fn with_output_preview(
        preview: String,
        full_text: ToolOutputBuffer,
        status: ToolBlockStatus,
        item_id: String,
    ) -> Self {
        Self {
            full_text: Some(full_text),
            tool_status: Some(status),
            item_id: Some(item_id),
            ..Self::new(TranscriptKind::Output, preview)
        }
    }
}

impl ShellState {
    #[cfg(test)]
    pub(super) fn push_output(&mut self, text: impl Into<String>) {
        self.push_output_with_status(text, ToolBlockStatus::Running);
    }

    pub(super) fn push_output_with_status(
        &mut self,
        text: impl Into<String>,
        status: ToolBlockStatus,
    ) {
        self.push_output_with_status_for_item(next_local_output_item_id(), text.into(), status);
    }

    pub(super) fn push_output_with_status_for_item(
        &mut self,
        item_id: impl Into<String>,
        text: impl Into<ToolOutputBuffer>,
        status: ToolBlockStatus,
    ) {
        let item_id = item_id.into();
        let mut full_text = text.into();
        let preview = compact_output_for_transcript(&full_text);
        if status != ToolBlockStatus::Running {
            let previous = self.transcript.iter_mut().rev().find(|entry| {
                entry.kind == TranscriptKind::Output
                    && entry.item_id.as_deref() == Some(&item_id)
                    && matches!(
                        entry.tool_status,
                        Some(
                            ToolBlockStatus::Running
                                | ToolBlockStatus::Interrupted
                                | ToolBlockStatus::Unconfirmed
                        )
                    )
            });
            // A completion can carry only a summary. Keep the longer streamed capture
            // for the viewer while the transcript displays the new summary.
            if let Some(previous) = previous {
                if previous
                    .full_text
                    .as_ref()
                    .map_or(previous.text.len(), |output| output.len())
                    > full_text.len()
                {
                    full_text = previous
                        .full_text
                        .take()
                        .unwrap_or_else(|| previous.text.clone().into());
                }
            } else if let Some(open) = self.tool_output.as_ref()
                && open.target.item_id == item_id
                && open.output_buffer().len() > full_text.len()
            {
                full_text.clone_from(open.output_buffer());
            }
        }
        self.replace_open_tool_output(&item_id, &full_text, status);
        self.upsert_line(TranscriptLine::with_output_preview(
            preview, full_text, status, item_id,
        ));
    }

    pub(super) fn push_output_delta_with_status_for_item(
        &mut self,
        item_id: impl Into<String>,
        delta: impl Into<String>,
        status: ToolBlockStatus,
    ) {
        let item_id = item_id.into();
        let delta = delta.into();
        if delta.is_empty() {
            return;
        }

        self.append_open_tool_output(&item_id, &delta, status);

        if let Some(existing) = self.transcript.iter_mut().rev().find(|existing| {
            existing.kind == TranscriptKind::Output && existing.item_id.as_deref() == Some(&item_id)
        }) {
            let full_text = existing
                .full_text
                .get_or_insert_with(|| existing.text.clone().into());
            full_text.append(&delta);
            existing.text = compact_output_for_transcript(full_text);
            existing.tool_status = Some(status);
            existing.mark_render_changed();
            self.trim_transcript();
            return;
        }

        let output = self
            .tool_output
            .as_ref()
            .filter(|output| output.target.item_id == item_id)
            .map_or_else(|| delta.into(), |output| output.output_buffer().clone());
        self.push_output_with_status_for_item(item_id, output, status);
    }

    pub(super) fn update_output_status_for_item(&mut self, item_id: &str, status: ToolBlockStatus) {
        self.update_open_tool_output_status(item_id, status);
        if let Some(existing) = self.transcript.iter_mut().rev().find(|existing| {
            existing.kind == TranscriptKind::Output && existing.item_id.as_deref() == Some(item_id)
        }) {
            existing.tool_status = Some(status);
            existing.mark_render_changed();
        }
    }
}

fn compact_output_for_transcript(text: &str) -> String {
    let source = text
        .strip_prefix(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX)
        .unwrap_or(text);
    let breaks = ['\n', '\r'];
    if source.len() <= TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS * 4
        && source
            .chars()
            .nth(TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS)
            .is_none()
        && source
            .split(breaks)
            .nth(TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
            .is_none()
    {
        return text.to_owned();
    }

    // Work backward from the last line. Both limits then bound the amount
    // of text we normalize and allocate, even for a single very long line.
    let tail = source.strip_suffix(breaks).unwrap_or(source);
    let char_start = tail
        .char_indices()
        .rev()
        .nth(TRANSCRIPT_OUTPUT_LOW_WATER_CHARS - 1)
        .map_or(0, |(start, _)| start);
    let tail = &tail[char_start..];
    let line_start = tail
        .rmatch_indices(breaks)
        .nth(TRANSCRIPT_OUTPUT_LOW_WATER_LINES - 1)
        .map_or(0, |(start, _)| start + 1);
    let tail = tail[line_start..].trim_start_matches(breaks);
    let mut output = String::with_capacity(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX.len() + tail.len());
    output.push_str(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX);
    output.extend(tail.chars().map(|ch| if ch == '\r' { '\n' } else { ch }));
    output
}

#[cfg(test)]
#[path = "transcript_output_tests.rs"]
mod tests;
