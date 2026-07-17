use super::diff_model::parse_unified_diff;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::cell::Cell;

pub(super) use super::diff_model::DiffCell;
pub(super) use super::diff_model::DiffFile;
pub(super) use super::diff_model::DiffFileKind;
pub(super) use super::diff_model::DiffLineKind;
pub(super) use super::diff_model::DiffStats;
pub(super) use super::diff_model::DiffStatus;

const DIFF_PAGE_STEP: usize = 12;

#[derive(Debug, Default)]
pub(super) struct DiffStore {
    turns: Vec<StoredTurn>,
}

#[derive(Debug)]
struct StoredTurn {
    turn_id: String,
    items: Vec<StoredItem>,
    aggregate: Option<Vec<DiffFile>>,
}

#[derive(Debug)]
struct StoredItem {
    item_id: String,
    status: DiffStatus,
    files: Vec<DiffFile>,
}

impl DiffStore {
    pub(super) fn upsert_item(
        &mut self,
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        changes: Vec<FileUpdateChange>,
        status: PatchApplyStatus,
    ) {
        let turn_id = turn_id.into();
        let item_id = item_id.into();
        let status = DiffStatus::from(status);
        let files = changes
            .iter()
            .map(|change| DiffFile::from_change(change, status))
            .collect();
        let turn = self.turn_mut(turn_id);
        if let Some(item) = turn.items.iter_mut().find(|item| item.item_id == item_id) {
            item.status = status;
            item.files = files;
        } else {
            turn.items.push(StoredItem {
                item_id,
                status,
                files,
            });
        }
    }

    pub(super) fn upsert_turn_diff(&mut self, turn_id: impl Into<String>, unified_diff: &str) {
        self.turn_mut(turn_id.into()).aggregate = Some(parse_unified_diff(unified_diff));
    }

    pub(super) fn item_files(&self, item_id: &str) -> Option<&[DiffFile]> {
        self.turns
            .iter()
            .flat_map(|turn| &turn.items)
            .find(|item| item.item_id == item_id)
            .map(|item| item.files.as_slice())
    }

    pub(super) fn session_files(&self) -> Vec<DiffFile> {
        self.session_file_refs().cloned().collect()
    }

    pub(super) fn session_stats(&self) -> DiffStats {
        DiffStats::from_files(self.session_file_refs())
    }

    pub(super) fn has_session_edits(&self) -> bool {
        self.session_file_refs().next().is_some()
    }

    pub(super) fn remove_turn(&mut self, turn_id: &str) {
        self.turns.retain(|turn| turn.turn_id != turn_id);
    }

    pub(super) fn clear(&mut self) {
        self.turns.clear();
    }

    fn turn_mut(&mut self, turn_id: String) -> &mut StoredTurn {
        if let Some(index) = self.turns.iter().position(|turn| turn.turn_id == turn_id) {
            return &mut self.turns[index];
        }
        let index = self.turns.len();
        self.turns.push(StoredTurn {
            turn_id,
            items: Vec::new(),
            aggregate: None,
        });
        &mut self.turns[index]
    }

    fn session_file_refs(&self) -> impl Iterator<Item = &DiffFile> {
        self.turns.iter().flat_map(|turn| {
            turn.aggregate.iter().flatten().chain(
                turn.items
                    .iter()
                    .filter(|item| item.status.is_session_edit())
                    .flat_map(|item| item.files.iter())
                    .filter(move |file| {
                        !turn.aggregate.as_ref().is_some_and(|aggregate| {
                            aggregate.iter().any(|aggregate_file| {
                                [file.old_label(), file.new_label()]
                                    .into_iter()
                                    .flatten()
                                    .any(|path| {
                                        aggregate_file.old_label() == Some(path)
                                            || aggregate_file.new_label() == Some(path)
                                    })
                            })
                        })
                    }),
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffViewAction {
    Pending,
    Close,
}

#[derive(Debug)]
pub(super) struct DiffViewState {
    title: String,
    source_item_id: Option<String>,
    files: Vec<DiffFile>,
    selected_file: usize,
    scroll: Cell<usize>,
    scroll_max: Cell<usize>,
}

impl DiffViewState {
    pub(super) fn new(
        title: impl Into<String>,
        source_item_id: Option<String>,
        files: Vec<DiffFile>,
    ) -> Self {
        Self {
            title: title.into(),
            source_item_id,
            files,
            selected_file: 0,
            scroll: Cell::new(0),
            scroll_max: Cell::new(0),
        }
    }

    pub(super) fn title(&self) -> &str {
        &self.title
    }

    pub(super) fn source_item_id(&self) -> Option<&str> {
        self.source_item_id.as_deref()
    }

    pub(super) fn files(&self) -> &[DiffFile] {
        &self.files
    }

    pub(super) fn selected_file_index(&self) -> usize {
        self.selected_file
    }

    pub(super) fn selected_file(&self) -> Option<&DiffFile> {
        self.files.get(self.selected_file)
    }

    pub(super) fn scroll(&self) -> usize {
        self.scroll.get().min(self.scroll_max.get())
    }

    pub(super) fn set_scroll_max(&self, scroll_max: usize) {
        self.scroll_max.set(scroll_max);
        self.scroll.set(self.scroll.get().min(scroll_max));
    }

    pub(super) fn select_file(&mut self, selected: usize) -> bool {
        if selected >= self.files.len() || selected == self.selected_file {
            return false;
        }
        self.selected_file = selected;
        self.reset_scroll();
        true
    }

    pub(super) fn select_previous_file(&mut self) -> bool {
        self.select_file(self.selected_file.saturating_sub(1))
    }

    pub(super) fn select_next_file(&mut self) -> bool {
        self.select_file(
            self.selected_file
                .saturating_add(1)
                .min(self.files.len().saturating_sub(1)),
        )
    }

    pub(super) fn replace_files(&mut self, files: Vec<DiffFile>) {
        let selected = self.selected_file().map(|file| {
            (
                file.old_label().map(str::to_owned),
                file.new_label().map(str::to_owned),
            )
        });
        self.files = files;
        self.selected_file = selected
            .and_then(|selected| {
                self.files.iter().position(|file| {
                    file.identity() == (selected.0.as_deref(), selected.1.as_deref())
                })
            })
            .unwrap_or_default();
        self.reset_scroll();
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) -> DiffViewAction {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            || !matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT)
        {
            return DiffViewAction::Pending;
        }
        match key.code {
            KeyCode::Esc => DiffViewAction::Close,
            KeyCode::Left | KeyCode::Char('[') => {
                self.select_previous_file();
                DiffViewAction::Pending
            }
            KeyCode::Right | KeyCode::Char(']') => {
                self.select_next_file();
                DiffViewAction::Pending
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(/*amount*/ 1);
                DiffViewAction::Pending
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(/*amount*/ 1);
                DiffViewAction::Pending
            }
            KeyCode::PageUp => {
                self.scroll_up(DIFF_PAGE_STEP);
                DiffViewAction::Pending
            }
            KeyCode::PageDown => {
                self.scroll_down(DIFF_PAGE_STEP);
                DiffViewAction::Pending
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll.set(0);
                DiffViewAction::Pending
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll.set(self.scroll_max.get());
                DiffViewAction::Pending
            }
            _ => DiffViewAction::Pending,
        }
    }

    pub(super) fn scroll_up(&self, amount: usize) {
        self.scroll.set(self.scroll().saturating_sub(amount));
    }

    pub(super) fn scroll_down(&self, amount: usize) {
        self.scroll.set(
            self.scroll()
                .saturating_add(amount)
                .min(self.scroll_max.get()),
        );
    }

    fn reset_scroll(&self) {
        self.scroll.set(0);
        self.scroll_max.set(0);
    }
}

#[cfg(test)]
#[path = "diff_view_tests.rs"]
mod tests;
