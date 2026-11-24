use super::dashboard::MORE_BUTTON;
use super::dashboard::dashboard_value;
use super::design::palette;
use super::design::selection_style;
use crate::text_input::EditableText;
use crate::text_input::TextInputAction;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListCwdFilter;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadSortKey;
use codex_protocol::ThreadId;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

const SESSION_LIST_LIMIT: u32 = 20;

/// Some servers return short pages even when they have a continuation. Assemble a complete
/// window before publishing it so the visible list changes in a single frame.
pub(super) async fn load_page<F, Fut>(
    mut params: ThreadListParams,
    mut fetch: F,
) -> color_eyre::Result<codex_app_server_protocol::ThreadListResponse>
where
    F: FnMut(ThreadListParams) -> Fut,
    Fut: std::future::Future<
            Output = color_eyre::Result<codex_app_server_protocol::ThreadListResponse>,
        >,
{
    let limit = params.limit.unwrap_or(SESSION_LIST_LIMIT) as usize;
    let mut cursors: HashSet<String> = params.cursor.iter().cloned().collect();
    let mut page = fetch(params.clone()).await?;
    let mut ids: HashSet<_> = page.data.iter().map(|t| t.id.clone()).collect();
    while page.data.len() < limit {
        let Some(cursor) = page.next_cursor.take() else {
            break;
        };
        if !cursors.insert(cursor.clone()) {
            break;
        }
        params.cursor = Some(cursor);
        params.limit = Some((limit - page.data.len()) as u32);
        let next = fetch(params.clone()).await?;
        page.data
            .extend(next.data.into_iter().filter(|t| ids.insert(t.id.clone())));
        page.next_cursor = next.next_cursor;
        if let Some(states) = next.read_states {
            page.read_states
                .get_or_insert_with(Default::default)
                .extend(states);
        }
    }
    Ok(page)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionSearchOutcome {
    LocalFilterOnly,
    RefreshList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionListHit {
    NewSession,
    Thread(ThreadId),
}

#[derive(Debug, Clone, Default)]
pub(super) struct SessionListState {
    all_rows: Vec<SessionRow>,
    rows: Vec<SessionRow>,
    selected: usize,
    scroll_top: Cell<usize>,
    visible_rows: Cell<usize>,
    pub(super) focused: bool,
    search_active: bool,
    search_query: EditableText,
    show_archived: bool,
    rename_draft: Option<EditableText>,
    last_error: Option<String>,
    loaded: bool,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionRow {
    pub(super) thread_id: ThreadId,
    title: String,
    preview: String,
    cwd: PathBuf,
    project_id: Option<String>,
    external_writer: bool,
}

impl SessionListState {
    pub(super) fn first_page_params(&mut self) -> ThreadListParams {
        self.next_cursor = None;
        self.list_params_with_cursor(None)
    }

    pub(super) fn next_page_params(&self) -> Option<ThreadListParams> {
        self.next_cursor
            .clone()
            .map(|cursor| self.list_params_with_cursor(Some(cursor)))
    }

    pub(super) fn list_params_with_cursor(&self, cursor: Option<String>) -> ThreadListParams {
        ThreadListParams {
            current_account_only: true,
            attached_to_thread_id: None,
            excluded_thread_ids: None,
            originators: None,
            project_id: None,
            section_id: None,
            cursor,
            limit: Some(SESSION_LIST_LIMIT),
            sort_key: Some(ThreadSortKey::RecencyAt),
            sort_direction: None,
            model_providers: Some(Vec::new()),
            source_kinds: Some(crate::resume_source_kinds(
                /*include_non_interactive*/ true,
            )),
            archived: Some(self.show_archived),
            cwd: None::<ThreadListCwdFilter>,
            use_state_db_only: false,
            search_term: (!self.search_active && !self.search_query.text().trim().is_empty())
                .then(|| self.search_query.text().trim().to_string()),
            parent_thread_id: None,
            ancestor_thread_id: None,
        }
    }

    pub(super) fn replace_threads(&mut self, threads: Vec<Thread>) {
        self.replace_thread_page(threads, /*next_cursor*/ None);
    }

    pub(super) fn replace_thread_page(
        &mut self,
        threads: Vec<Thread>,
        next_cursor: Option<String>,
    ) {
        self.all_rows = threads
            .into_iter()
            .filter_map(SessionRow::from_thread)
            .collect();
        self.next_cursor = next_cursor;
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
        self.loaded = true;
        self.last_error = None;
    }

    pub(super) fn append_thread_page(&mut self, threads: Vec<Thread>, next_cursor: Option<String>) {
        let mut existing = self
            .all_rows
            .iter()
            .map(|row| row.thread_id)
            .collect::<HashSet<_>>();
        self.all_rows.extend(
            threads
                .into_iter()
                .filter_map(SessionRow::from_thread)
                .filter(|row| existing.insert(row.thread_id)),
        );
        // A repeated cursor cannot produce another page.
        self.next_cursor = next_cursor.filter(|cursor| self.next_cursor.as_ref() != Some(cursor));
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
        self.loaded = true;
        self.last_error = None;
    }

    pub(super) fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
        self.loaded = true;
    }

    pub(super) fn at_top(&self) -> bool {
        self.selected == 0
    }

    pub(super) fn move_selection_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.keep_selection_visible();
    }

    pub(super) fn move_selection_down(&mut self) -> bool {
        let selected = self.selected;
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.rows.len().saturating_sub(1));
        self.keep_selection_visible();
        self.selected != selected
    }

    pub(super) fn has_more(&self) -> bool {
        self.next_cursor.is_some()
    }

    pub(super) fn selected_thread_id(&self) -> Option<ThreadId> {
        self.rows.get(self.selected).map(|row| row.thread_id)
    }

    pub(super) fn project_for_thread(&self, thread: ThreadId) -> Option<&str> {
        self.rows
            .iter()
            .find(|row| row.thread_id == thread)?
            .project_id
            .as_deref()
    }

    pub(super) fn mark_external_writer(&mut self, id: ThreadId) {
        for row in self
            .all_rows
            .iter_mut()
            .chain(&mut self.rows)
            .filter(|row| row.thread_id == id)
        {
            row.external_writer = true;
        }
    }

    pub(super) fn cwd_for_thread(&self, thread_id: ThreadId) -> Option<&PathBuf> {
        self.rows
            .iter()
            .find(|row| row.thread_id == thread_id)
            .map(|row| &row.cwd)
    }

    pub(super) fn selected_line(&self, visible_rows: usize) -> usize {
        let leading = self.leading_line_count();
        if self.rows.is_empty() {
            return leading.saturating_sub(1);
        }
        leading
            + self
                .selected
                .saturating_sub(self.normalized_scroll_top(visible_rows))
    }

    pub(super) fn is_new_session_line(&self, line: usize) -> bool {
        !self.show_archived && line == 1
    }

    pub(super) fn thread_index_at_line(&self, line: usize) -> Option<usize> {
        let offset = line.checked_sub(self.leading_line_count())?;
        let visible = self.visible_rows.get();
        let index = self.normalized_scroll_top(visible) + offset;
        (offset < visible && index < self.rows.len()).then_some(index)
    }

    pub(super) fn hit_at_line(&mut self, line: usize) -> Option<SessionListHit> {
        self.focused = true;
        if self.is_new_session_line(line) {
            return Some(SessionListHit::NewSession);
        }
        self.selected = self.thread_index_at_line(line)?;
        Some(SessionListHit::Thread(self.rows[self.selected].thread_id))
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

    pub(super) fn remove_thread(&mut self, thread_id: ThreadId) {
        self.all_rows.retain(|row| row.thread_id != thread_id);
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
    }

    pub(super) fn rename_thread(&mut self, thread_id: ThreadId, name: String) {
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
        self.search_query.insert_char(ch);
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
    }

    pub(super) fn insert_search_text(&mut self, text: &str) {
        self.search_query.insert_str(text);
        self.apply_search_filter();
        self.normalize_selection_and_scroll();
    }

    pub(super) fn edit_search(&mut self, action: TextInputAction) -> SessionSearchOutcome {
        if !self.search_query.apply(action) {
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
        self.scroll_top.set(0);
        self.last_error = None;
        self.loaded = false;
        self.next_cursor = None;
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
        self.rename_draft = Some(EditableText::new(draft));
    }

    pub(super) fn cancel_rename(&mut self) {
        self.rename_draft = None;
    }

    pub(super) fn push_rename_char(&mut self, ch: char) {
        if let Some(draft) = &mut self.rename_draft {
            draft.insert_char(ch);
        }
    }

    pub(super) fn insert_rename_text(&mut self, text: &str) {
        if let Some(draft) = &mut self.rename_draft {
            draft.insert_str(text);
        }
    }

    pub(super) fn edit_rename(&mut self, action: TextInputAction) {
        if let Some(draft) = &mut self.rename_draft {
            draft.apply(action);
        }
    }

    pub(super) fn take_rename_draft(&mut self) -> Option<String> {
        self.rename_draft
            .take()
            .map(|draft| draft.into_text().trim().to_string())
    }

    pub(super) fn rename_draft(&self) -> Option<String> {
        self.rename_draft
            .as_ref()
            .map(|draft| draft.text().trim().to_string())
    }

    pub(super) fn renaming(&self) -> bool {
        self.rename_draft.is_some()
    }

    pub(super) fn header_lines(&self, width: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if self.show_archived {
            let more = if self.has_more() { "+" } else { "" };
            let count = if self.search_active && !self.search_query.text().trim().is_empty() {
                format!(
                    "{} shown / {}{more} loaded",
                    self.rows.len(),
                    self.all_rows.len()
                )
            } else {
                format!("{}{more} sessions", self.rows.len())
            };
            lines.push(Line::from(vec![
                "Archived".fg(palette::text()).bold(),
                "  ".into(),
                count.fg(palette::muted()),
            ]));
        } else {
            lines.push(Line::from("Recents".fg(palette::text()).bold()));
            lines.push(Line::from(vec![
                "+ New session".fg(palette::text()),
                "  Ctrl+N".fg(palette::muted()),
            ]));
        }
        if self.search_active || !self.search_query.is_empty() {
            let (label, hint) = if self.search_active {
                ("filter*", "  · Enter search all")
            } else {
                ("search", "  · server results")
            };
            let query = if self.search_active {
                self.search_query.text_with_cursor_window(
                    width
                        .saturating_sub(label.len() + 1 + UnicodeWidthStr::width(hint))
                        .max(1),
                )
            } else {
                self.search_query.text().to_string()
            };
            lines.push(Line::from(vec![
                label.fg(palette::cyan()),
                " ".into(),
                dashboard_value(
                    &query,
                    width,
                    label.len() + 1 + UnicodeWidthStr::width(hint),
                )
                .fg(palette::text()),
                hint.fg(palette::muted()),
            ]));
        }
        if let Some(draft) = &self.rename_draft {
            let draft = draft.text_with_cursor_window(width.saturating_sub(8).max(1));
            lines.push(Line::from(vec![
                "rename*".fg(palette::cyan()),
                " ".into(),
                dashboard_value(&draft, width, /*prefix_width*/ 8).fg(palette::text()),
            ]));
        }
        if let Some(error) = &self.last_error {
            lines.push(Line::from(
                dashboard_value(error, width, /*prefix_width*/ 0).fg(palette::error()),
            ));
        } else if !self.loaded {
            lines.push(Line::from("loading sessions".fg(palette::muted())));
        } else if self.rows.is_empty() {
            lines.push(Line::from("no matching sessions".fg(palette::muted())));
        }

        lines
    }

    pub(super) fn max_viewport_rows(&self) -> usize {
        if self.show_archived {
            usize::MAX
        } else {
            SESSION_LIST_LIMIT as usize
        }
    }

    pub(super) fn visible_rows(&self) -> usize {
        self.visible_rows.get()
    }

    pub(super) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn chat_lines(
        &self,
        width: usize,
        visible: usize,
        focused: bool,
        failed_resumes: &HashSet<ThreadId>,
    ) -> Vec<Line<'static>> {
        self.visible_rows.set(visible);
        let top = self.normalized_scroll_top(visible);
        if visible > 0 {
            self.scroll_top.set(top);
        }
        self.rows
            .iter()
            .enumerate()
            .skip(top)
            .take(visible)
            .map(|(index, row)| {
                row_line(
                    row,
                    focused && index == self.selected,
                    !self.show_archived,
                    width,
                    row.external_writer || failed_resumes.contains(&row.thread_id),
                )
            })
            .collect()
    }

    pub(super) fn needs_viewport_fill(&self) -> bool {
        self.has_more()
            && !self.search_active
            && self.last_error.is_none()
            && self.visible_rows.get() > 0
            && self.visible_rows.get() >= self.rows.len().saturating_sub(self.scroll_top.get())
    }

    pub(super) fn scroll_up(&mut self) {
        self.scroll_top.set(self.scroll_top.get().saturating_sub(1));
        self.move_selection_up();
    }

    pub(super) fn scroll_down(&mut self) -> bool {
        let max_top = self.rows.len().saturating_sub(self.visible_rows.get());
        self.scroll_top
            .set(self.scroll_top.get().saturating_add(1).min(max_top));
        !self.move_selection_down() && self.has_more()
    }

    fn normalize_selection_and_scroll(&mut self) {
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.keep_selection_visible();
    }

    fn apply_search_filter(&mut self) {
        let query = self.search_query.text().trim();
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

    fn keep_selection_visible(&self) {
        let visible = self.visible_rows.get();
        if visible > 0 {
            self.scroll_top.set(self.normalized_scroll_top(visible));
        }
    }

    fn normalized_scroll_top(&self, visible_rows: usize) -> usize {
        if self.rows.is_empty() || visible_rows == 0 {
            return 0;
        }

        let max_scroll_top = self.rows.len().saturating_sub(visible_rows);
        let mut scroll_top = self.scroll_top.get().min(max_scroll_top);
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
            .saturating_add(usize::from(!self.show_archived))
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
            project_id: thread.project_id,
            external_writer: thread.extra.is_some_and(|extra| extra.external_writer),
        })
    }

    fn matches_search(&self, query: &str) -> bool {
        self.title.contains(query) || self.preview.contains(query)
    }
}

fn row_line(
    row: &SessionRow,
    selected: bool,
    movable: bool,
    width: usize,
    unavailable: bool,
) -> Line<'static> {
    let marker = if selected { "› " } else { "  " };
    let suffix = if movable { MORE_BUTTON } else { "" };
    let title = dashboard_value(&row.title, width, marker.width() + suffix.width());
    let pad = width.saturating_sub(marker.width() + title.width() + suffix.width());
    let line = Line::from(vec![
        marker.fg(palette::focus()),
        if unavailable {
            title.fg(palette::warning())
        } else {
            title.into()
        },
        " ".repeat(pad).into(),
        suffix.fg(palette::muted()),
    ]);
    if selected {
        line.set_style(selection_style())
    } else {
        line
    }
}
