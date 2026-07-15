use super::dashboard::dashboard_value;
use super::design::palette;
use super::design::selection_style;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListCwdFilter;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadSortKey;
use codex_protocol::ThreadId;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

const SESSION_LIST_LIMIT: u32 = 20;
const SESSION_LIST_LINE_BUDGET: usize = 7;
const SESSION_LIST_DEFAULT_VISIBLE_ROWS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionSearchOutcome {
    LocalFilterOnly,
    RefreshList,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SessionListState {
    all_rows: Vec<SessionRow>,
    rows: Vec<SessionRow>,
    selected: usize,
    scroll_top: usize,
    pub(super) focused: bool,
    search_active: bool,
    search_query: String,
    show_archived: bool,
    rename_draft: Option<String>,
    last_error: Option<String>,
    loaded: bool,
    has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionRow {
    pub(super) thread_id: ThreadId,
    title: String,
    preview: String,
    cwd: PathBuf,
    branch: Option<String>,
}

impl SessionListState {
    pub(super) fn list_params(&self) -> ThreadListParams {
        ThreadListParams {
            cursor: None,
            limit: Some(SESSION_LIST_LIMIT),
            sort_key: Some(ThreadSortKey::RecencyAt),
            sort_direction: None,
            model_providers: None,
            source_kinds: Some(crate::resume_source_kinds(
                /*include_non_interactive*/ true,
            )),
            archived: Some(self.show_archived),
            cwd: None::<ThreadListCwdFilter>,
            use_state_db_only: false,
            search_term: (!self.search_active && !self.search_query.trim().is_empty())
                .then(|| self.search_query.trim().to_string()),
            parent_thread_id: None,
            ancestor_thread_id: None,
        }
    }

    pub(super) fn replace_threads(&mut self, threads: Vec<Thread>) {
        self.replace_thread_page(threads, /*has_more*/ false);
    }

    pub(super) fn replace_thread_page(&mut self, threads: Vec<Thread>, has_more: bool) {
        self.all_rows = threads
            .into_iter()
            .filter_map(SessionRow::from_thread)
            .collect();
        self.has_more = has_more;
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
        self.loaded = true;
        self.last_error = None;
    }

    pub(super) fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
        self.loaded = true;
    }

    pub(super) fn move_selection_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.keep_selection_visible(SESSION_LIST_DEFAULT_VISIBLE_ROWS);
    }

    pub(super) fn move_selection_down(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.rows.len().saturating_sub(1));
        self.keep_selection_visible(SESSION_LIST_DEFAULT_VISIBLE_ROWS);
    }

    pub(super) fn selected_thread_id(&self) -> Option<ThreadId> {
        self.rows.get(self.selected).map(|row| row.thread_id)
    }

    pub(super) fn select_at_line(&mut self, line: usize) -> bool {
        self.focused = true;
        let leading_lines = self.leading_line_count();
        if line < leading_lines {
            return false;
        }
        let visible_rows = SESSION_LIST_LINE_BUDGET.saturating_sub(leading_lines);
        let scroll_top = self.normalized_scroll_top(visible_rows);
        let index = scroll_top.saturating_add(line.saturating_sub(leading_lines));
        if index >= self.rows.len() || index >= scroll_top.saturating_add(visible_rows) {
            return false;
        }
        self.selected = index;
        true
    }

    pub(super) fn selected_title(&self) -> Option<&str> {
        self.rows.get(self.selected).map(|row| row.title.as_str())
    }

    pub(super) fn selected_is_current(&self, thread_id: ThreadId) -> bool {
        self.selected_thread_id() == Some(thread_id)
    }

    pub(super) fn remove_selected(&mut self) -> Option<SessionRow> {
        if self.rows.is_empty() {
            return None;
        }
        let removed = self.rows.remove(self.selected);
        self.all_rows
            .retain(|row| row.thread_id != removed.thread_id);
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
        Some(removed)
    }

    pub(super) fn rename_selected(&mut self, name: String) {
        let Some(thread_id) = self.selected_thread_id() else {
            return;
        };
        if let Some(row) = self
            .all_rows
            .iter_mut()
            .find(|row| row.thread_id == thread_id)
        {
            row.title = name;
        }
        self.apply_search_filter();
        self.selected = self
            .rows
            .iter()
            .position(|row| row.thread_id == thread_id)
            .unwrap_or_default();
        self.normalize_selection_and_scroll();
    }

    pub(super) fn start_search(&mut self) {
        self.search_active = true;
    }

    pub(super) fn push_search_char(&mut self, ch: char) {
        self.search_query.push(ch);
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
    }

    pub(super) fn backspace_search(&mut self) -> SessionSearchOutcome {
        if self.search_query.pop().is_none() {
            return SessionSearchOutcome::LocalFilterOnly;
        }
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
        if self.search_query.is_empty() {
            SessionSearchOutcome::RefreshList
        } else {
            SessionSearchOutcome::LocalFilterOnly
        }
    }

    pub(super) fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_active = false;
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
    }

    pub(super) fn stop_search(&mut self) {
        self.search_active = false;
    }

    pub(super) fn search_active(&self) -> bool {
        self.search_active
    }

    pub(super) fn toggle_archived(&mut self) {
        self.show_archived = !self.show_archived;
        self.all_rows.clear();
        self.rows.clear();
        self.selected = 0;
        self.scroll_top = 0;
        self.has_more = false;
    }

    pub(super) fn show_archived(&self) -> bool {
        self.show_archived
    }

    pub(super) fn start_rename(&mut self) {
        let draft = self
            .selected_title()
            .filter(|title| *title != "untitled thread")
            .unwrap_or_default()
            .to_string();
        self.rename_draft = Some(draft);
    }

    pub(super) fn cancel_rename(&mut self) {
        self.rename_draft = None;
    }

    pub(super) fn push_rename_char(&mut self, ch: char) {
        if let Some(draft) = &mut self.rename_draft {
            draft.push(ch);
        }
    }

    pub(super) fn backspace_rename(&mut self) {
        if let Some(draft) = &mut self.rename_draft {
            draft.pop();
        }
    }

    pub(super) fn take_rename_draft(&mut self) -> Option<String> {
        self.rename_draft
            .take()
            .map(|draft| draft.trim().to_string())
    }

    pub(super) fn renaming(&self) -> bool {
        self.rename_draft.is_some()
    }

    pub(super) fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let focus = if self.focused {
            "● FOCUSED"
        } else {
            "○ CLICK TO FOCUS"
        };
        let mode = if self.show_archived {
            "ARCHIVED"
        } else {
            "ACTIVE"
        };
        let count = if self.search_active && !self.search_query.trim().is_empty() {
            format!(
                "{} shown / {}{} loaded",
                self.rows.len(),
                self.all_rows.len(),
                if self.has_more { "+" } else { "" }
            )
        } else {
            format!(
                "{}{} sessions",
                self.rows.len(),
                if self.has_more { "+" } else { "" }
            )
        };
        lines.push(Line::from(vec![
            focus.fg(if self.focused {
                palette::FOCUS
            } else {
                palette::MUTED
            }),
            "  ".into(),
            mode.fg(palette::TEXT).bold(),
            "  ".into(),
            count.fg(palette::MUTED),
        ]));
        if self.search_active || !self.search_query.is_empty() {
            let (label, hint) = if self.search_active {
                ("filter*", "  · Enter search all")
            } else {
                ("search", "  · server results")
            };
            lines.push(Line::from(vec![
                label.fg(palette::CYAN),
                " ".into(),
                dashboard_value(
                    &self.search_query,
                    width,
                    label.len() + 1 + UnicodeWidthStr::width(hint),
                )
                .fg(palette::TEXT),
                hint.fg(palette::MUTED),
            ]));
        }
        if let Some(draft) = &self.rename_draft {
            lines.push(Line::from(vec![
                "rename*".fg(palette::CYAN),
                " ".into(),
                dashboard_value(draft, width, /*prefix_width*/ 8).fg(palette::TEXT),
            ]));
        }
        if let Some(error) = &self.last_error {
            lines.push(Line::from(
                dashboard_value(error, width, /*prefix_width*/ 0).fg(palette::ERROR),
            ));
        } else if !self.loaded {
            lines.push(Line::from("loading sessions".fg(palette::MUTED)));
        } else if self.rows.is_empty() {
            lines.push(Line::from("no matching sessions".fg(palette::MUTED)));
        }

        let remaining = SESSION_LIST_LINE_BUDGET.saturating_sub(lines.len());
        let scroll_top = self.normalized_scroll_top(remaining);
        for (index, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(scroll_top)
            .take(remaining)
        {
            lines.push(row_line(
                row,
                index == self.selected,
                index,
                self.rows.len(),
                width,
            ));
        }
        lines
    }

    fn normalize_selection_and_scroll(&mut self) {
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.keep_selection_visible(SESSION_LIST_DEFAULT_VISIBLE_ROWS);
    }

    fn apply_search_filter(&mut self) {
        let query = self.search_query.trim();
        if query.is_empty() {
            self.rows.clone_from(&self.all_rows);
            return;
        }

        self.rows = self
            .all_rows
            .iter()
            .filter(|row| row.matches_search(query))
            .cloned()
            .collect();
    }

    fn keep_selection_visible(&mut self, visible_rows: usize) {
        self.scroll_top = self.normalized_scroll_top(visible_rows);
    }

    fn normalized_scroll_top(&self, visible_rows: usize) -> usize {
        if self.rows.is_empty() || visible_rows == 0 {
            return 0;
        }

        let max_scroll_top = self.rows.len().saturating_sub(visible_rows);
        let mut scroll_top = self.scroll_top.min(max_scroll_top);
        if self.selected < scroll_top {
            scroll_top = self.selected;
        }

        let last_visible = scroll_top.saturating_add(visible_rows).saturating_sub(1);
        if self.selected > last_visible {
            scroll_top = self
                .selected
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_scroll_top);
        }

        scroll_top
    }

    fn leading_line_count(&self) -> usize {
        1usize
            .saturating_add(usize::from(
                self.search_active || !self.search_query.is_empty(),
            ))
            .saturating_add(usize::from(self.rename_draft.is_some()))
            .saturating_add(usize::from(
                self.last_error.is_some() || !self.loaded || self.rows.is_empty(),
            ))
    }
}

impl SessionRow {
    fn from_thread(thread: Thread) -> Option<Self> {
        let thread_id = ThreadId::from_string(&thread.id).ok()?;
        let title = thread
            .name
            .or_else(|| {
                let preview = thread.preview.trim();
                (!preview.is_empty()).then(|| preview.to_string())
            })
            .unwrap_or_else(|| "untitled thread".to_string());
        let preview = if thread.preview.trim().is_empty() {
            "(no message yet)".to_string()
        } else {
            thread.preview.trim().to_string()
        };
        Some(Self {
            thread_id,
            title,
            preview,
            cwd: thread.cwd.to_path_buf(),
            branch: thread.git_info.and_then(|git_info| git_info.branch),
        })
    }

    fn matches_search(&self, query: &str) -> bool {
        self.title.contains(query) || self.preview.contains(query)
    }
}

fn row_line(
    row: &SessionRow,
    selected: bool,
    index: usize,
    total: usize,
    width: usize,
) -> Line<'static> {
    let marker = if selected {
        "›".fg(palette::FOCUS).bold()
    } else {
        " ".into()
    };
    let total = total.max(1);
    let position_width = total.to_string().len();
    let position = format!("{:>position_width$}/{total}", index.saturating_add(1));
    let position_width = position.chars().count();
    let position = if selected {
        position.fg(palette::FOCUS)
    } else {
        position.fg(palette::MUTED)
    };
    let mut detail = row
        .branch
        .as_deref()
        .map(|branch| format!(" [{branch}]"))
        .unwrap_or_default();
    if detail.is_empty()
        && let Some(cwd) = row.cwd.file_name().and_then(|name| name.to_str())
    {
        detail = format!(" [{cwd}]");
    }
    let text = format!("{}{detail}", row.title);
    let prefix_width = 3usize.saturating_add(position_width);
    let visible = dashboard_value(&text, width, prefix_width);
    let preview_width =
        width.saturating_sub(prefix_width + UnicodeWidthStr::width(visible.as_str()) + 2);
    let preview = if preview_width > 8 {
        format!(
            "  {}",
            dashboard_value(&row.preview, preview_width, /*prefix_width*/ 0)
        )
        .fg(palette::MUTED)
    } else {
        "".into()
    };
    let line = Line::from(vec![
        marker,
        " ".into(),
        position,
        " ".into(),
        visible.fg(if selected {
            palette::TEXT
        } else {
            palette::MUTED
        }),
        preview,
    ]);
    if selected {
        line.set_style(selection_style())
    } else {
        line
    }
}
