use super::ShellState;
use super::design::palette;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

const DENSE_HELP_MAX_WIDTH: usize = 54;
const HELP_COLUMN_GAP: &str = "│ ";
const WIDE_HELP_COLUMN_WIDTH: usize = 34;
const DENSE_KEY_WIDTH: usize = 7;
const COMPACT_KEY_WIDTH: usize = 10;
const WIDE_KEY_WIDTH: usize = 18;

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
}

impl HelpRow {
    const fn section(title: &'static str) -> Self {
        Self::Section(title)
    }

    const fn shortcut(keys: &'static str, description: &'static str) -> Self {
        Self::Shortcut(Shortcut::new(keys, description))
    }
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

pub(super) fn key_hint_lines(shell: &ShellState, panel_width: usize) -> Vec<Line<'static>> {
    if uses_dense_layout(panel_width) {
        let (left, right) = dense_help_columns(shell);
        return two_column_lines(&left, &right, panel_width, DENSE_KEY_WIDTH);
    }

    let text_width = panel_width.saturating_sub(1);
    let column_width = text_width.saturating_sub(UnicodeWidthStr::width(HELP_COLUMN_GAP)) / 2;
    let detail = if column_width >= WIDE_HELP_COLUMN_WIDTH {
        LabelDetail::Wide
    } else {
        LabelDetail::Compact
    };
    let (left, right) = standard_help_columns(shell, detail);
    two_column_lines(&left, &right, text_width, detail.key_width())
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
            if let Some(row) = right.get(index) {
                spans.push(HELP_COLUMN_GAP.fg(palette::BORDER));
                spans.extend(
                    truncate_line_with_ellipsis_if_overflow(
                        help_row_line(*row, key_width),
                        right_width,
                    )
                    .spans,
                );
            } else {
                spans.push("│".fg(palette::BORDER));
            }
            Line::from(spans)
        })
        .collect()
}

fn help_row_line(row: HelpRow, key_width: usize) -> Line<'static> {
    match row {
        HelpRow::Section(title) => Line::from(vec![
            "◆ ".fg(palette::PURPLE),
            title.to_string().fg(palette::PURPLE).bold(),
        ]),
        HelpRow::Shortcut(shortcut) => {
            let padding = key_width.saturating_sub(UnicodeWidthStr::width(shortcut.keys));
            let key_label = format!(" {}{} ", shortcut.keys, " ".repeat(padding));
            Line::from(vec![
                key_label.fg(palette::CYAN).bg(palette::SURFACE).bold(),
                " ".into(),
                shortcut.description.into(),
            ])
        }
    }
}

fn dense_help_columns(shell: &ShellState) -> (Vec<HelpRow>, Vec<HelpRow>) {
    let contextual = dense_contextual_shortcuts(shell);
    let left = vec![
        HelpRow::section("COMPOSER"),
        HelpRow::shortcut("⌘←→⌫/⌥^", "Line · word"),
        HelpRow::shortcut("S/A↵/Fn", "Newline/nav"),
        HelpRow::Shortcut(contextual[0]),
        HelpRow::Shortcut(contextual[1]),
        HelpRow::shortcut("Alt+M/E", "Model/effort"),
        HelpRow::section("APP"),
        HelpRow::Shortcut(contextual[2]),
        HelpRow::Shortcut(contextual[3]),
    ];
    let right = vec![
        HelpRow::section("DASHBOARD"),
        HelpRow::shortcut("^1–4/Tab", "Switch/page"),
        HelpRow::section("AGENTS"),
        HelpRow::shortcut("↵/jk", "Focus/log"),
        HelpRow::section("SESSIONS"),
        HelpRow::shortcut("^N/↵", "New/focus"),
        HelpRow::shortcut("r/f", "Resume/fork"),
        HelpRow::shortcut("a/u/v/d", "Arc/view/del"),
        HelpRow::shortcut("n · /", "Name/search"),
    ];
    (left, right)
}

fn dense_contextual_shortcuts(shell: &ShellState) -> [Shortcut; 4] {
    if shell.transcript_selection.is_some() {
        return [
            Shortcut::new("↑↓", "Select"),
            Shortcut::new(
                "↵",
                if shell.selected_transcript_is_output() {
                    "Open output"
                } else {
                    "Copy"
                },
            ),
            Shortcut::new("Esc", "Composer"),
            Shortcut::new("^O/^D", "Copy/hide"),
        ];
    }

    if shell.active_turn_id.is_some() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        return [
            Shortcut::new(
                "↵/Tab",
                if editing_queue {
                    "Save edit"
                } else {
                    "Steer/queue"
                },
            ),
            Shortcut::new(
                "Alt+↑↓/^O",
                if editing_queue {
                    "Move/copy"
                } else {
                    "Edit/copy"
                },
            ),
            Shortcut::new("^C/Esc×2", "Stop/exit"),
            Shortcut::new("^D", "Hide dash"),
        ];
    }

    if shell.has_pending_shell_command() {
        return [
            Shortcut::new("↵", "Send"),
            Shortcut::new("Alt+↑/^O", "Select/copy"),
            Shortcut::new("^C/Esc×2", "Cancel/exit"),
            Shortcut::new("^D", "Hide dash"),
        ];
    }

    if shell.composer.has_queued_messages() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        return [
            Shortcut::new(
                if editing_queue { "↵/Tab" } else { "↵" },
                if editing_queue {
                    "Save/resume"
                } else {
                    "Send/resume"
                },
            ),
            Shortcut::new(
                "Alt+↑↓/^O",
                if editing_queue {
                    "Move/copy"
                } else {
                    "Edit/copy"
                },
            ),
            Shortcut::new("^C/Esc×2", "Exit"),
            Shortcut::new("^D", "Hide dash"),
        ];
    }

    [
        Shortcut::new("↵", "Send"),
        Shortcut::new("Alt+↑/^O", "Select/copy"),
        Shortcut::new("^C/Esc×2", "Exit"),
        Shortcut::new("^D", "Hide dash"),
    ]
}

fn standard_help_columns(shell: &ShellState, detail: LabelDetail) -> (Vec<HelpRow>, Vec<HelpRow>) {
    let contextual = standard_contextual_shortcuts(shell, detail);
    let left = vec![
        HelpRow::section("COMPOSER"),
        HelpRow::shortcut(detail.select("⌘←→⌫", "Cmd ←/→/⌫"), "Line boundary"),
        HelpRow::shortcut(detail.select("⌥/^ + ←→", "Opt/Ctrl + ←/→"), "Move by word"),
        HelpRow::shortcut(
            detail.select("S/A+↵", "Shift/Alt + Enter"),
            detail.select("Newline", "Insert newline"),
        ),
        HelpRow::shortcut(
            detail.select("Fn nav/⌫", "Fn + arrows/⌫"),
            detail.select("Page/delete", "Page nav / delete"),
        ),
        HelpRow::Shortcut(contextual[0]),
        HelpRow::Shortcut(contextual[1]),
        HelpRow::shortcut(
            detail.select("Alt+M/E", "Alt+M / Alt+E"),
            detail.select("Model/effort", "Model / effort"),
        ),
        HelpRow::section("APP"),
        HelpRow::Shortcut(contextual[2]),
        HelpRow::section("AGENTS"),
        HelpRow::shortcut(
            detail.select("↵/j/k", "Enter/j/k"),
            detail.select("Focus/inspect", "Focus log/inspect"),
        ),
    ];
    let right = vec![
        HelpRow::section("DASHBOARD"),
        HelpRow::shortcut(
            detail.select("^1/^2", "Ctrl+1 / Ctrl+2"),
            detail.select("Status/agents", "Status / Agents"),
        ),
        HelpRow::shortcut(
            detail.select("^3/^4", "Ctrl+3 / Ctrl+4"),
            detail.select("Sessions/help", "Sessions / Help"),
        ),
        HelpRow::shortcut(
            detail.select("Tab/j/k/↵", "Tab/j/k/Enter"),
            detail.select("Navigate/apply", "Navigate / open"),
        ),
        HelpRow::section("SESSIONS"),
        HelpRow::shortcut(
            detail.select("^N/↵", "Ctrl+N / Enter"),
            detail.select("New/focus", "New / focus"),
        ),
        HelpRow::shortcut("r / f", "Resume / fork"),
        HelpRow::shortcut("a / u", detail.select("Arc/unarc", "Archive/unarchive")),
        HelpRow::shortcut("v / d", detail.select("View/delete", "Archived/delete")),
        HelpRow::shortcut("n · /", detail.select("Name/search", "Rename / search")),
    ];
    (left, right)
}

fn standard_contextual_shortcuts(shell: &ShellState, detail: LabelDetail) -> [Shortcut; 3] {
    if shell.transcript_selection.is_some() {
        return [
            Shortcut::new(detail.select("↑/↓", "Up / Down"), "Select"),
            Shortcut::new(
                detail.select("↵", "Enter"),
                if shell.selected_transcript_is_output() {
                    "Open output"
                } else {
                    "Copy"
                },
            ),
            Shortcut::new(
                detail.select("Esc/^O/^D", "Esc/Ctrl+O/Ctrl+D"),
                detail.select("Back/copy/hide", "Composer/copy/hide"),
            ),
        ];
    }

    if shell.active_turn_id.is_some() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        return [
            Shortcut::new(
                detail.select("↵/Tab", "Enter/Tab"),
                if editing_queue {
                    "Save queued edit"
                } else {
                    "Steer / queue"
                },
            ),
            Shortcut::new(
                detail.select("Alt+↑↓/^O", "Alt+Up/Down/Ctrl+O"),
                if editing_queue {
                    "Traverse / copy"
                } else {
                    "Edit queue / copy"
                },
            ),
            Shortcut::new(
                detail.select("^C/Esc×2/^D", "Ctrl+C/Esc×2/Ctrl+D"),
                detail.select("Stop/exit/hide", "Stop/exit/hide"),
            ),
        ];
    }

    if shell.has_pending_shell_command() {
        return [
            Shortcut::new(detail.select("↵", "Enter"), "Send"),
            Shortcut::new(detail.select("Alt+↑/^O", "Alt+Up/Ctrl+O"), "Select / copy"),
            Shortcut::new(
                detail.select("^C/Esc×2/^D", "Ctrl+C/Esc×2/Ctrl+D"),
                detail.select("Cancel/exit/hide", "Cancel/exit/hide"),
            ),
        ];
    }

    if shell.composer.has_queued_messages() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        return [
            Shortcut::new(
                if editing_queue {
                    detail.select("↵/Tab", "Enter/Tab")
                } else {
                    detail.select("↵", "Enter")
                },
                if editing_queue {
                    "Save / resume"
                } else {
                    "Send / resume"
                },
            ),
            Shortcut::new(
                detail.select("Alt+↑↓/^O", "Alt+Up/Down/Ctrl+O"),
                if editing_queue {
                    "Traverse / copy"
                } else {
                    "Edit queue / copy"
                },
            ),
            Shortcut::new(
                detail.select("^C/Esc×2/^D", "Ctrl+C/Esc×2/Ctrl+D"),
                detail.select("Exit/hide", "Exit / hide"),
            ),
        ];
    }

    [
        Shortcut::new(detail.select("↵", "Enter"), "Send"),
        Shortcut::new(detail.select("Alt+↑/^O", "Alt+Up/Ctrl+O"), "Select / copy"),
        Shortcut::new(
            detail.select("^C/Esc×2/^D", "Ctrl+C/Esc×2/Ctrl+D"),
            detail.select("Exit/hide", "Exit / hide"),
        ),
    ]
}
