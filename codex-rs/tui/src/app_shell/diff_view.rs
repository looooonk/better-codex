use super::diff_horizontal_scroll::HorizontalScroll;
use super::diff_model::parse_unified_diff;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use std::cell::Cell;
use std::collections::VecDeque;

pub(super) use super::diff_model::DiffCell;
pub(super) use super::diff_model::DiffFile;
pub(super) use super::diff_model::DiffFileKind;
pub(super) use super::diff_model::DiffLineKind;
pub(super) use super::diff_model::DiffStats;
pub(super) use super::diff_model::DiffStatus;

const DIFF_PAGE_STEP: usize = 12;
const MAX_DIFF_UPDATE_BYTES: usize = 256 * 1024;
const MAX_DIFF_UPDATE_LINE_BREAKS: usize = 2_000;
const MAX_DIFF_UPDATE_FILES: usize = 64;
const MAX_DIFF_STORE_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_DIFF_STORE_ROWS: usize = 8_000;
const MAX_DIFF_STORE_FILES: usize = 256;
const MAX_DIFF_STORE_ITEMS: usize = 128;
const MAX_DIFF_STORE_TURNS: usize = 32;

#[derive(Debug, Default)]
pub(super) struct DiffStore {
    turns: VecDeque<StoredTurn>,
    history_truncated: bool,
}

#[derive(Debug)]
struct StoredTurn {
    turn_id: String,
    items: VecDeque<StoredItem>,
    aggregate: Option<StoredFiles>,
}

#[derive(Debug)]
struct StoredItem {
    item_id: String,
    status: DiffStatus,
    truncated: bool,
    files: Vec<DiffFile>,
}

#[derive(Debug)]
struct StoredFiles {
    truncated: bool,
    files: Vec<DiffFile>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DiffStoreSize {
    text_bytes: usize,
    rows: usize,
    files: usize,
    items: usize,
}

impl DiffStore {
    pub(super) fn upsert_item(
        &mut self,
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        changes: impl AsRef<[FileUpdateChange]>,
        status: PatchApplyStatus,
    ) {
        let changes = changes.as_ref();
        let turn_id = turn_id.into();
        let item_id = item_id.into();
        let status = DiffStatus::from(status);
        let StoredFiles { truncated, files } = bounded_change_files(changes, status);
        let turn = self.turn_mut(turn_id);
        if let Some(item) = turn.items.iter_mut().find(|item| item.item_id == item_id) {
            item.status = status;
            item.truncated = truncated;
            item.files = files;
        } else {
            turn.items.push_back(StoredItem {
                item_id,
                status,
                truncated,
                files,
            });
        }
        self.enforce_limits();
    }

    pub(super) fn upsert_turn_diff(&mut self, turn_id: impl Into<String>, unified_diff: &str) {
        let aggregate = bounded_unified_diff(unified_diff);
        self.turn_mut(turn_id.into()).aggregate = Some(aggregate);
        self.enforce_limits();
    }

    pub(super) fn item_files(&self, item_id: &str) -> Option<&[DiffFile]> {
        self.turns
            .iter()
            .flat_map(|turn| &turn.items)
            .find(|item| item.item_id == item_id)
            .map(|item| item.files.as_slice())
    }

    pub(super) fn item_is_truncated(&self, item_id: &str) -> bool {
        self.turns
            .iter()
            .flat_map(|turn| &turn.items)
            .find(|item| item.item_id == item_id)
            .is_some_and(|item| item.truncated)
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

    pub(super) fn session_is_truncated(&self) -> bool {
        self.history_truncated
            || self.turns.iter().any(|turn| {
                turn.aggregate
                    .as_ref()
                    .is_some_and(|aggregate| aggregate.truncated)
                    || turn
                        .items
                        .iter()
                        .any(|item| item.status.is_session_edit() && item.truncated)
            })
    }

    pub(super) fn remove_turn(&mut self, turn_id: &str) {
        self.turns.retain(|turn| turn.turn_id != turn_id);
    }

    pub(super) fn clear(&mut self) {
        self.turns.clear();
        self.history_truncated = false;
    }

    fn turn_mut(&mut self, turn_id: String) -> &mut StoredTurn {
        if let Some(index) = self.turns.iter().position(|turn| turn.turn_id == turn_id) {
            return &mut self.turns[index];
        }
        let index = self.turns.len();
        self.turns.push_back(StoredTurn {
            turn_id,
            items: VecDeque::new(),
            aggregate: None,
        });
        &mut self.turns[index]
    }

    fn session_file_refs(&self) -> impl Iterator<Item = &DiffFile> {
        self.turns.iter().flat_map(|turn| {
            turn.aggregate
                .iter()
                .flat_map(|stored| &stored.files)
                .chain(
                    turn.items
                        .iter()
                        .filter(|item| item.status.is_session_edit())
                        .flat_map(|item| item.files.iter())
                        .filter(move |file| {
                            !turn.aggregate.as_ref().is_some_and(|aggregate| {
                                aggregate.files.iter().any(|aggregate_file| {
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

    fn enforce_limits(&mut self) {
        while self.turns.len() > MAX_DIFF_STORE_TURNS {
            self.turns.pop_front();
            self.history_truncated = true;
        }
        while self.retained_size().items > MAX_DIFF_STORE_ITEMS {
            if !self.remove_oldest_item() {
                break;
            }
            self.history_truncated = true;
        }
        while self.retained_size().exceeds_limits() {
            if self.turns.len() > 1 || !self.remove_oldest_item() {
                self.turns.pop_front();
            }
            self.history_truncated = true;
        }
    }

    fn remove_oldest_item(&mut self) -> bool {
        self.turns
            .iter_mut()
            .find(|turn| !turn.items.is_empty())
            .and_then(|turn| turn.items.pop_front())
            .is_some()
    }

    fn retained_size(&self) -> DiffStoreSize {
        self.turns
            .iter()
            .fold(DiffStoreSize::default(), |mut size, turn| {
                if let Some(aggregate) = &turn.aggregate {
                    size.add_files(&aggregate.files);
                }
                for item in &turn.items {
                    size.items = size.items.saturating_add(1);
                    size.add_files(&item.files);
                }
                size
            })
    }
}

impl DiffStoreSize {
    fn add_files(&mut self, files: &[DiffFile]) {
        self.files = self.files.saturating_add(files.len());
        for file in files {
            self.rows = self.rows.saturating_add(file.rows().len());
            self.text_bytes = self.text_bytes.saturating_add(file.retained_text_bytes());
        }
    }

    fn exceeds_limits(self) -> bool {
        self.text_bytes > MAX_DIFF_STORE_TEXT_BYTES
            || self.rows > MAX_DIFF_STORE_ROWS
            || self.files > MAX_DIFF_STORE_FILES
    }
}

fn bounded_change_files(changes: &[FileUpdateChange], status: DiffStatus) -> StoredFiles {
    let mut remaining_bytes = MAX_DIFF_UPDATE_BYTES;
    let mut remaining_line_breaks = MAX_DIFF_UPDATE_LINE_BREAKS;
    let mut files = Vec::new();
    let mut truncated = false;
    for change in changes.iter().take(MAX_DIFF_UPDATE_FILES) {
        let (diff, line_breaks, diff_truncated) =
            bounded_diff_prefix(&change.diff, remaining_bytes, remaining_line_breaks);
        files.push(DiffFile::from_change_with_diff(change, diff, status));
        remaining_bytes = remaining_bytes.saturating_sub(diff.len());
        remaining_line_breaks = remaining_line_breaks.saturating_sub(line_breaks);
        if diff_truncated {
            truncated = true;
            break;
        }
    }
    truncated |= files.len() < changes.len();
    StoredFiles { truncated, files }
}

fn bounded_unified_diff(unified_diff: &str) -> StoredFiles {
    let (diff, _, mut truncated) = bounded_diff_prefix(
        unified_diff,
        MAX_DIFF_UPDATE_BYTES,
        MAX_DIFF_UPDATE_LINE_BREAKS,
    );
    let mut files = parse_unified_diff(diff);
    if files.len() > MAX_DIFF_UPDATE_FILES {
        files.truncate(MAX_DIFF_UPDATE_FILES);
        truncated = true;
    }
    StoredFiles { truncated, files }
}

fn bounded_diff_prefix(
    diff: &str,
    max_bytes: usize,
    max_line_breaks: usize,
) -> (&str, usize, bool) {
    if diff.is_empty() {
        return (diff, 0, false);
    }
    if max_bytes == 0 || max_line_breaks == 0 {
        return ("", 0, true);
    }
    let mut end = diff.len().min(max_bytes);
    while !diff.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut line_breaks = 0usize;
    for (index, character) in diff[..end].char_indices() {
        if matches!(character, '\n' | '\r') {
            if line_breaks == max_line_breaks {
                end = index;
                break;
            }
            line_breaks = line_breaks.saturating_add(1);
        }
    }
    (&diff[..end], line_breaks, end < diff.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffRetention {
    Complete,
    Truncated,
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
    retention: DiffRetention,
    selected_file: usize,
    scroll: Cell<usize>,
    scroll_max: Cell<usize>,
    pub(super) horizontal_scroll: HorizontalScroll,
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
            retention: DiffRetention::Complete,
            selected_file: 0,
            scroll: Cell::new(0),
            scroll_max: Cell::new(0),
            horizontal_scroll: HorizontalScroll::default(),
        }
    }

    pub(super) fn with_retention(mut self, retention: DiffRetention) -> Self {
        self.retention = retention;
        self
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

    pub(super) fn retention(&self) -> DiffRetention {
        self.retention
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

    pub(super) fn replace_files(&mut self, files: Vec<DiffFile>, retention: DiffRetention) {
        let selected = self.selected_file().map(|file| {
            (
                file.old_label().map(str::to_owned),
                file.new_label().map(str::to_owned),
            )
        });
        self.files = files;
        self.retention = retention;
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
        if self.horizontal_scroll.handle_key(key) {
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
        self.horizontal_scroll.reset();
    }
}

#[cfg(test)]
#[path = "diff_view_tests.rs"]
mod tests;
