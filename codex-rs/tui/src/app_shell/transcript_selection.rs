use super::ShellState;
use super::TranscriptKind;

impl ShellState {
    pub(super) fn select_latest_transcript_item(&mut self) {
        self.clear_text_selections();
        self.transcript_selection = self
            .transcript
            .iter()
            .rposition(|line| line.kind == TranscriptKind::User);
        self.transcript_selection_needs_reveal
            .set(self.transcript_selection.is_some());
        self.scroll_transcript_to_bottom();
    }

    pub(super) fn select_first_transcript_item(&mut self) {
        self.clear_text_selections();
        self.transcript_selection = self
            .transcript
            .iter()
            .position(|line| line.kind == TranscriptKind::User);
        self.transcript_selection_needs_reveal
            .set(self.transcript_selection.is_some());
        self.scroll_transcript_to_top();
    }

    pub(super) fn clear_transcript_selection(&mut self) {
        self.transcript_selection = None;
        self.transcript_selection_needs_reveal.set(false);
    }

    pub(super) fn move_transcript_selection_up(&mut self, rows: usize) {
        if rows == 0 {
            return;
        }
        let Some(selected) = self.transcript_selection else {
            self.select_latest_transcript_item();
            return;
        };
        let first_user = self
            .transcript
            .iter()
            .position(|line| line.kind == TranscriptKind::User);
        let next = self
            .transcript
            .iter()
            .enumerate()
            .take(selected)
            .rev()
            .filter(|(_, line)| line.kind == TranscriptKind::User)
            .nth(rows - 1)
            .map(|(index, _)| index)
            .or(first_user);
        let Some(next) = next else {
            self.clear_transcript_selection();
            return;
        };
        self.transcript_selection = Some(next);
        self.scroll_transcript_up(selected.saturating_sub(next));
        self.transcript_selection_needs_reveal.set(true);
    }

    pub(super) fn move_transcript_selection_down(&mut self, rows: usize) {
        if rows == 0 {
            return;
        }
        let Some(selected) = self.transcript_selection else {
            self.select_latest_transcript_item();
            return;
        };
        let last_user = self
            .transcript
            .iter()
            .rposition(|line| line.kind == TranscriptKind::User);
        let next = self
            .transcript
            .iter()
            .enumerate()
            .skip(selected.saturating_add(1))
            .filter(|(_, line)| line.kind == TranscriptKind::User)
            .nth(rows - 1)
            .map(|(index, _)| index)
            .or(last_user);
        let Some(next) = next else {
            self.clear_transcript_selection();
            return;
        };
        self.transcript_selection = Some(next);
        self.scroll_transcript_down(next.saturating_sub(selected));
        self.transcript_selection_needs_reveal.set(true);
    }
}

#[cfg(test)]
#[path = "transcript_selection_tests.rs"]
mod tests;
