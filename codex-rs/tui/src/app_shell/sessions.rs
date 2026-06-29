use super::dashboard::dashboard_value;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadListCwdFilter;
use codex_app_server_protocol::ThreadListParams;
use codex_app_server_protocol::ThreadSortKey;
use codex_protocol::ThreadId;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::PathBuf;

const SESSION_LIST_LIMIT: u32 = 20;

#[derive(Debug, Clone, Default)]
pub(super) struct SessionListState {
    rows: Vec<SessionRow>,
    selected: usize,
    pub(super) focused: bool,
    search_active: bool,
    search_query: String,
    show_archived: bool,
    rename_draft: Option<String>,
    last_error: Option<String>,
    loaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionRow {
    pub(super) thread_id: ThreadId,
    title: String,
    preview: String,
    cwd: PathBuf,
    branch: Option<String>,
    updated_at: i64,
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
            search_term: (!self.search_query.trim().is_empty())
                .then(|| self.search_query.trim().to_string()),
            parent_thread_id: None,
            ancestor_thread_id: None,
        }
    }

    pub(super) fn replace_threads(&mut self, threads: Vec<Thread>) {
        self.rows = threads
            .into_iter()
            .filter_map(SessionRow::from_thread)
            .collect();
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.loaded = true;
        self.last_error = None;
    }

    pub(super) fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
        self.loaded = true;
    }

    pub(super) fn move_selection_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(super) fn move_selection_down(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.rows.len().saturating_sub(1));
    }

    pub(super) fn selected_thread_id(&self) -> Option<ThreadId> {
        self.rows.get(self.selected).map(|row| row.thread_id)
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
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        Some(removed)
    }

    pub(super) fn rename_selected(&mut self, name: String) {
        if let Some(row) = self.rows.get_mut(self.selected) {
            row.title = name;
        }
    }

    pub(super) fn start_search(&mut self) {
        self.search_active = true;
    }

    pub(super) fn push_search_char(&mut self, ch: char) {
        self.search_query.push(ch);
    }

    pub(super) fn backspace_search(&mut self) {
        self.search_query.pop();
    }

    pub(super) fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_active = false;
    }

    pub(super) fn stop_search(&mut self) {
        self.search_active = false;
    }

    pub(super) fn search_active(&self) -> bool {
        self.search_active
    }

    pub(super) fn toggle_archived(&mut self) {
        self.show_archived = !self.show_archived;
        self.selected = 0;
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
            "focused"
        } else {
            "ctrl+1 focus"
        };
        let mode = if self.show_archived {
            "archived"
        } else {
            "active"
        };
        lines.push(Line::from(vec![
            focus.cyan(),
            " ".dim(),
            mode.dim(),
            " ".dim(),
            format!("{} shown", self.rows.len()).dim(),
        ]));
        if self.search_active || !self.search_query.is_empty() {
            let label = if self.search_active {
                "search*"
            } else {
                "search"
            };
            lines.push(Line::from(vec![
                label.cyan(),
                " ".dim(),
                dashboard_value(&self.search_query, width, label.len() + 1).into(),
            ]));
        }
        if let Some(draft) = &self.rename_draft {
            lines.push(Line::from(vec![
                "rename*".cyan(),
                " ".dim(),
                dashboard_value(draft, width, /*prefix_width*/ 8).into(),
            ]));
        }
        if let Some(error) = &self.last_error {
            lines.push(Line::from(
                dashboard_value(error, width, /*prefix_width*/ 0).red(),
            ));
        } else if !self.loaded {
            lines.push(Line::from("loading sessions".dim()));
        } else if self.rows.is_empty() {
            lines.push(Line::from("no matching sessions".dim()));
        }

        let remaining = 7usize.saturating_sub(lines.len());
        for (index, row) in self.rows.iter().take(remaining).enumerate() {
            lines.push(row_line(row, index == self.selected, width));
        }
        let hints = if self.show_archived {
            "r resume  f fork  u unarchive  d delete"
        } else {
            "r resume  f fork  a archive  d delete  n rename"
        };
        lines.push(Line::from(truncate_text(hints, width).dim()));
        lines.push(Line::from("/ search  v archived  esc composer".dim()));
        lines
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
            updated_at: thread.updated_at,
        })
    }
}

fn row_line(row: &SessionRow, selected: bool, width: usize) -> Line<'static> {
    let marker = if selected {
        ">".cyan().bold()
    } else {
        " ".dim()
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
    let age = if row.updated_at > 0 {
        format!(" {}", row.updated_at)
    } else {
        String::new()
    };
    let text = format!("{}{}{}", row.title, detail, age);
    let prefix_width = 2;
    let visible = dashboard_value(&text, width, prefix_width);
    let preview_width = width.saturating_sub(prefix_width + visible.chars().count() + 1);
    let preview = if preview_width > 8 {
        format!(" {}", truncate_text(&row.preview, preview_width)).dim()
    } else {
        "".dim()
    };
    Line::from(vec![marker, " ".dim(), visible.into(), preview])
}
