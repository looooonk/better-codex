use super::ShellState;
use super::design::palette;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

const DENSE_HELP_MAX_WIDTH: usize = 54;
const SPACIOUS_HELP_MIN_HEIGHT: usize = 28;
const HELP_COLUMN_GAP: &str = "│ ";
const WIDE_HELP_COLUMN_WIDTH: usize = 34;
const COMPACT_KEY_WIDTH: usize = 10;
const WIDE_KEY_WIDTH: usize = 15;

#[derive(Clone, Copy)]
struct Shortcut {
    keys: &'static str,
    description: &'static str,
}

impl Shortcut {
    const fn new(keys: &'static str, description: &'static str) -> Self {
        Self { keys, description }
    }
}

#[derive(Clone, Copy)]
enum HelpRow {
    Section(&'static str),
    Shortcut(Shortcut),
    Spacer,
}

#[derive(Clone, Copy)]
enum LabelDetail {
    Compact,
    Wide,
}

impl LabelDetail {
    const fn select(self, compact: &'static str, wide: &'static str) -> &'static str {
        match self {
            Self::Compact => compact,
            Self::Wide => wide,
        }
    }

    const fn key_width(self) -> usize {
        match self {
            Self::Compact => COMPACT_KEY_WIDTH,
            Self::Wide => WIDE_KEY_WIDTH,
        }
    }
}

pub(super) fn uses_dense_layout(panel_width: usize) -> bool {
    panel_width <= DENSE_HELP_MAX_WIDTH
}

pub(super) fn key_hint_lines(
    shell: &ShellState,
    panel_width: usize,
    panel_height: usize,
) -> Vec<Line<'static>> {
    let text_width = panel_width.saturating_sub(usize::from(!uses_dense_layout(panel_width)));
    let column_width = text_width
        .saturating_sub(UnicodeWidthStr::width(HELP_COLUMN_GAP))
        .div_ceil(2);
    let detail = if column_width >= WIDE_HELP_COLUMN_WIDTH {
        LabelDetail::Wide
    } else {
        LabelDetail::Compact
    };
    let spacious = panel_height >= SPACIOUS_HELP_MIN_HEIGHT;
    let (left, right) = help_columns(shell, detail, spacious);
    two_column_lines(&left, &right, text_width, detail.key_width())
}

fn help_columns(
    shell: &ShellState,
    detail: LabelDetail,
    spacious: bool,
) -> (Vec<HelpRow>, Vec<HelpRow>) {
    let mut left = Vec::new();
    append_group(
        &mut left,
        "CONVERSATION",
        &conversation_shortcuts(shell, detail),
        spacious,
    );
    append_group(
        &mut left,
        "DASHBOARD",
        &[
            Shortcut::new(
                detail.select("^1 … ^4", "Ctrl+1 … Ctrl+4"),
                detail.select("Open tab", "Open a tab"),
            ),
            Shortcut::new(
                detail.select("Alt+← / →", "Alt+Left / Right"),
                detail.select("Prev/next", "Previous / next tab"),
            ),
            Shortcut::new(
                detail.select("Pg↑ / Pg↓", "Page Up / Page Down"),
                detail.select("Scroll help", "Scroll this guide"),
            ),
            Shortcut::new(
                detail.select("⇧Alt+←/→", "Shift+Alt+← / →"),
                detail.select("Resize", "Resize dashboard"),
            ),
        ],
        spacious,
    );
    append_group(
        &mut left,
        "SESSIONS",
        &[
            Shortcut::new(
                detail.select("↑↓ / jk", "Up/Down or j/k"),
                detail.select("Select", "Select session"),
            ),
            Shortcut::new(
                detail.select("↵ / r", "Enter or r"),
                detail.select("Resume", "Resume session"),
            ),
            Shortcut::new("f", detail.select("Fork", "Fork session")),
            Shortcut::new("a", detail.select("Archive", "Archive session")),
            Shortcut::new("u", detail.select("Unarchive", "Unarchive session")),
            Shortcut::new("v", detail.select("Switch view", "Active / archived")),
            Shortcut::new("n", detail.select("Rename", "Rename session")),
            Shortcut::new("/", detail.select("Search", "Search sessions")),
            Shortcut::new("d", detail.select("Delete", "Delete session")),
        ],
        spacious,
    );

    let mut right = Vec::new();
    append_group(
        &mut right,
        "COMPOSER",
        &composer_shortcuts(shell, detail),
        spacious,
    );
    append_group(
        &mut right,
        "APP",
        &[
            Shortcut::new(
                detail.select("^P", "Ctrl+P"),
                detail.select("Commands", "Command palette"),
            ),
            Shortcut::new(
                detail.select("^D", "Ctrl+D"),
                detail.select("Show/hide", "Toggle dashboard"),
            ),
            Shortcut::new(
                detail.select("^C", "Ctrl+C"),
                detail.select("Stop/cancel", "Interrupt / cancel"),
            ),
            Shortcut::new("Esc ×2", detail.select("Exit app", "Exit application")),
            Shortcut::new(
                detail.select("^N", "Ctrl+N"),
                detail.select("New chat", "New session"),
            ),
        ],
        spacious,
    );
    append_group(
        &mut right,
        "AGENTS",
        &[
            Shortcut::new(
                detail.select("↵", "Enter"),
                detail.select("Focus/log", "Focus / open log"),
            ),
            Shortcut::new(
                detail.select("↑↓ / jk", "Up/Down or j/k"),
                detail.select("Select", "Select agent"),
            ),
            Shortcut::new("g / G", detail.select("First/last", "First / last agent")),
            Shortcut::new("r", detail.select("Reload log", "Reload open log")),
        ],
        spacious,
    );
    (left, right)
}

fn conversation_shortcuts(shell: &ShellState, detail: LabelDetail) -> Vec<Shortcut> {
    let mut shortcuts = Vec::new();
    if shell.transcript_selection.is_some() {
        shortcuts.push(Shortcut::new(
            detail.select("↑ / ↓", "Up / Down"),
            detail.select("Select msgs", "Select messages"),
        ));
    } else if !shell.composer.has_queued_messages() {
        shortcuts.push(Shortcut::new(
            detail.select("Alt+↑ / ↓", "Alt+Up / Down"),
            detail.select("Select msgs", "Select messages"),
        ));
    }
    if shell.selected_transcript_is_output() {
        shortcuts.push(Shortcut::new(
            detail.select("↵", "Enter"),
            detail.select("Open output", "Open selected output"),
        ));
        shortcuts.push(Shortcut::new(
            "c",
            detail.select("Copy item", "Copy selected item"),
        ));
    } else {
        shortcuts.push(Shortcut::new(
            detail.select("↵ / c", "Enter or c"),
            detail.select("Copy item", "Copy selected item"),
        ));
    }
    shortcuts.extend([
        Shortcut::new("e", detail.select("Branch", "Branch from prompt")),
        Shortcut::new(
            detail.select("^O/⌥1-9", "Ctrl+O / Alt+1-9"),
            detail.select("Copy reply", "Copy response"),
        ),
    ]);
    shortcuts
}

fn composer_shortcuts(shell: &ShellState, detail: LabelDetail) -> Vec<Shortcut> {
    let mut shortcuts = vec![
        Shortcut::new(
            detail.select("⌘← / →", "Cmd+Left / Right"),
            detail.select("Line ends", "Line start / end"),
        ),
        Shortcut::new(
            detail.select("⌘⌫", "Cmd+Backspace"),
            detail.select("Delete start", "Delete to line start"),
        ),
        Shortcut::new(
            detail.select("⌥/^ + ←→", "Opt/Ctrl + Left/Right"),
            detail.select("Move word", "Move by word"),
        ),
        Shortcut::new(
            detail.select("S/A + ↵", "Shift/Alt + Enter"),
            detail.select("Newline", "Insert newline"),
        ),
        Shortcut::new("Alt+M", detail.select("Model", "Select model")),
        Shortcut::new("Alt+E", detail.select("Effort", "Select effort")),
    ];

    let editing_queue = shell.composer.queued_edit_position().is_some();
    if editing_queue {
        shortcuts.push(Shortcut::new(
            detail.select("↵ / Tab", "Enter or Tab"),
            detail.select("Save edit", "Save queued edit"),
        ));
    } else if shell.active_turn_id.is_some() {
        shortcuts.push(Shortcut::new(
            detail.select("↵", "Enter"),
            detail.select("Steer turn", "Steer active turn"),
        ));
        shortcuts.push(Shortcut::new(
            "Tab",
            detail.select("Queue next", "Queue follow-up"),
        ));
    }
    if shell.composer.has_queued_messages() {
        shortcuts.push(Shortcut::new(
            detail.select("Alt+↑ / ↓", "Alt+Up / Down"),
            detail.select("Edit queue", "Edit queued messages"),
        ));
        if editing_queue {
            shortcuts.push(Shortcut::new(
                detail.select("⇧Alt+↑/↓", "Shift+Alt+Up/Down"),
                detail.select("Reorder", "Reorder queued messages"),
            ));
        }
    }
    shortcuts
}

fn append_group(
    column: &mut Vec<HelpRow>,
    title: &'static str,
    shortcuts: &[Shortcut],
    spacious: bool,
) {
    if spacious && !column.is_empty() {
        column.push(HelpRow::Spacer);
    }
    column.push(HelpRow::Section(title));
    column.extend(shortcuts.iter().copied().map(HelpRow::Shortcut));
}

fn two_column_lines(
    left: &[HelpRow],
    right: &[HelpRow],
    width: usize,
    key_width: usize,
) -> Vec<Line<'static>> {
    let gap_width = UnicodeWidthStr::width(HELP_COLUMN_GAP);
    let columns_width = width.saturating_sub(gap_width);
    let left_width = columns_width.div_ceil(2);
    let right_width = columns_width.saturating_sub(left_width);

    (0..left.len().max(right.len()))
        .map(|index| {
            let left = left
                .get(index)
                .map(|row| help_row_line(*row, key_width))
                .unwrap_or_default();
            let left = truncate_line_with_ellipsis_if_overflow(left, left_width);
            let rendered_left_width = left.width();
            let mut spans = left.spans;
            spans.push(
                " ".repeat(left_width.saturating_sub(rendered_left_width))
                    .into(),
            );
            match right.get(index) {
                Some(HelpRow::Spacer) | None => spans.push("│".fg(palette::border())),
                Some(row) => {
                    spans.push(HELP_COLUMN_GAP.fg(palette::border()));
                    spans.extend(
                        truncate_line_with_ellipsis_if_overflow(
                            help_row_line(*row, key_width),
                            right_width,
                        )
                        .spans,
                    );
                }
            }
            Line::from(spans)
        })
        .collect()
}

fn help_row_line(row: HelpRow, key_width: usize) -> Line<'static> {
    match row {
        HelpRow::Section(title) => Line::from(vec![
            "◆ ".fg(palette::purple()),
            title.to_string().fg(palette::purple()).bold(),
        ]),
        HelpRow::Shortcut(shortcut) => {
            let padding = key_width.saturating_sub(UnicodeWidthStr::width(shortcut.keys));
            let key_label = format!(" {}{} ", shortcut.keys, " ".repeat(padding));
            vec![
                key_label.fg(palette::cyan()),
                " ".into(),
                shortcut.description.into(),
            ]
            .into()
        }
        HelpRow::Spacer => Line::default(),
    }
}
