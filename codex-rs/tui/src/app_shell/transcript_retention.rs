use super::MAX_TRANSCRIPT_LINES;
use super::ShellState;

// Long runs of small tool calls can share one folded row. Bound their backing storage
// independently so neither folding nor unfolding can retain unlimited detail.
pub(super) const MAX_DETAIL_ITEMS: usize = 4 * MAX_TRANSCRIPT_LINES;
const DETAIL_BYTES: usize = 32 * 1024 * 1024;

impl ShellState {
    pub(super) fn trim_transcript(&mut self) {
        let len = self.transcript.len();
        if len <= MAX_TRANSCRIPT_LINES {
            return;
        }
        let mut start = 0;
        let mut visible = len - self.tool_groups().map(|group| group.rows.len() - 1).sum::<usize>();
        let mut groups = self.tool_groups().peekable();
        while visible > MAX_TRANSCRIPT_LINES {
            start = if groups.peek().is_some_and(|group| group.rows.start == start) {
                groups.next().expect("peeked tool group").rows.end
            } else {
                start + 1
            };
            visible -= 1;
        }
        drop(groups);
        start = start.max(len.saturating_sub(MAX_DETAIL_ITEMS));
        let mut bytes = 0;
        for (index, item) in self.transcript.iter().enumerate().skip(start).rev() {
            bytes += item.text.len() + item.full_text.as_ref().map_or(0, |output| output.len());
            if bytes > DETAIL_BYTES {
                // Preserve at least the normal uncompressed history allowance.
                start = start.max((index + 1).min(len - MAX_TRANSCRIPT_LINES));
                break;
            }
        }
        if start == 0 {
            return;
        }

        // A storage limit can cut through a group. Evict complete calls and carry the
        // disclosure state forward when the remaining calls still belong to that group.
        let boundary = self.tool_groups().find(|group| group.rows.contains(&start))
            .map(|group| (group.rows.end, group.expanded));
        if let Some((end, expanded)) = boundary {
            while start < end && self.transcript[start].tool_action().is_none() {
                start += 1;
            }
            if expanded && start < end {
                self.transcript[start].tool_group_expanded = true;
                self.transcript[start].mark_render_changed();
            }
        }
        self.clear_transcript_text_selection();
        self.transcript.drain(..start);
        self.transcript_selection = self.transcript_selection.and_then(|i| i.checked_sub(start));
        if self.transcript_selection.is_none() {
            self.transcript_selection_needs_reveal.set(false);
        }
    }
}
