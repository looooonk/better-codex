use super::ShellState;
use super::TranscriptKind;
use super::diff_view::DiffRetention;
use super::diff_view::DiffViewAction;
use super::diff_view::DiffViewState;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use crossterm::event::KeyEvent;

impl ShellState {
    pub(super) fn record_file_changes(
        &mut self,
        turn_id: &str,
        item_id: &str,
        changes: &[FileUpdateChange],
        status: PatchApplyStatus,
    ) {
        self.diff_store
            .upsert_item(turn_id, item_id, changes, status);
        self.refresh_open_diff_view();
    }

    pub(super) fn record_turn_diff(&mut self, turn_id: &str, diff: &str) {
        self.diff_store.upsert_turn_diff(turn_id, diff);
        self.refresh_open_diff_view();
    }

    pub(super) fn open_diff_view_at(&mut self, transcript_index: usize) -> bool {
        let Some(line) = self.transcript.get(transcript_index) else {
            return false;
        };
        if line.kind != TranscriptKind::Diff {
            return false;
        }
        let Some(item_id) = line.item_id.clone() else {
            return false;
        };
        let Some(files) = self.diff_store.item_files(&item_id).map(<[_]>::to_vec) else {
            return false;
        };
        if files.is_empty() {
            return false;
        }
        let retention = retention(self.diff_store.item_is_truncated(&item_id));

        self.close_agent_log();
        self.close_tool_output();
        self.command_palette = None;
        self.selector = None;
        self.clear_transcript_selection();
        self.diff_view = Some(
            DiffViewState::new("File changes", /*source_item_id*/ Some(item_id), files)
                .with_retention(retention),
        );
        true
    }

    pub(super) fn open_selected_diff_view(&mut self) -> bool {
        self.transcript_selection
            .is_some_and(|index| self.open_diff_view_at(index))
    }

    pub(super) fn open_session_diff_view(&mut self) -> bool {
        let files = self.diff_store.session_files();
        if files.is_empty() {
            return false;
        }
        let retention = retention(self.diff_store.session_is_truncated());

        self.close_agent_log();
        self.close_tool_output();
        self.command_palette = None;
        self.selector = None;
        self.clear_transcript_selection();
        self.diff_view = Some(
            DiffViewState::new("Session edits", /*source_item_id*/ None, files)
                .with_retention(retention),
        );
        true
    }

    pub(super) fn close_diff_view(&mut self) {
        self.diff_view = None;
    }

    pub(super) fn handle_diff_view_key(&mut self, key: KeyEvent) -> bool {
        let Some(view) = &mut self.diff_view else {
            return false;
        };
        if view.handle_key(key) == DiffViewAction::Close {
            self.close_diff_view();
        }
        true
    }

    pub(super) fn select_diff_file(&mut self, index: usize) {
        if let Some(view) = &mut self.diff_view {
            view.select_file(index);
        }
    }

    pub(super) fn scroll_diff_view_up(&self) {
        if let Some(view) = &self.diff_view {
            view.scroll_up(/*amount*/ 3);
        }
    }

    pub(super) fn scroll_diff_view_down(&self) {
        if let Some(view) = &self.diff_view {
            view.scroll_down(/*amount*/ 3);
        }
    }

    pub(super) fn refresh_open_diff_view(&mut self) {
        let Some(source_item_id) = self
            .diff_view
            .as_ref()
            .map(|view| view.source_item_id().map(str::to_owned))
        else {
            return;
        };
        let update = match source_item_id {
            Some(item_id) => self
                .diff_store
                .item_files(&item_id)
                .filter(|files| !files.is_empty())
                .map(|files| {
                    (
                        files.to_vec(),
                        retention(self.diff_store.item_is_truncated(&item_id)),
                    )
                }),
            None => {
                let files = self.diff_store.session_files();
                (!files.is_empty())
                    .then(|| (files, retention(self.diff_store.session_is_truncated())))
            }
        };
        let Some((files, retention)) = update else {
            self.close_diff_view();
            return;
        };
        if let Some(view) = &mut self.diff_view {
            view.replace_files(files, retention);
        }
    }
}

fn retention(truncated: bool) -> DiffRetention {
    if truncated {
        DiffRetention::Truncated
    } else {
        DiffRetention::Complete
    }
}

#[cfg(test)]
#[path = "diff_view_controller_tests.rs"]
mod tests;
