use super::ShellState;
use super::design::key_hint_line;
use super::design::palette;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

const DENSE_HELP_MAX_WIDTH: usize = 54;
const KEY_HINT_COLUMN_GAP: &str = "│ ";
const KEY_HINT_WIDE_COLUMN_WIDTH: usize = 34;

pub(super) fn uses_dense_layout(panel_width: usize) -> bool {
    panel_width <= DENSE_HELP_MAX_WIDTH
}

pub(super) fn key_hint_lines(shell: &ShellState, panel_width: usize) -> Vec<Line<'static>> {
    if uses_dense_layout(panel_width) {
        return two_column_lines(&dense_key_hint_labels(shell), panel_width);
    }

    let text_width = panel_width.saturating_sub(1);
    let column_width = text_width.saturating_sub(UnicodeWidthStr::width(KEY_HINT_COLUMN_GAP)) / 2;
    let labels = if column_width >= KEY_HINT_WIDE_COLUMN_WIDTH {
        wide_key_hint_labels(shell)
    } else {
        compact_key_hint_labels(shell)
    };
    two_column_lines(&labels, text_width)
}

fn two_column_lines(labels: &[&'static str], width: usize) -> Vec<Line<'static>> {
    let gap_width = UnicodeWidthStr::width(KEY_HINT_COLUMN_GAP);
    let columns_width = width.saturating_sub(gap_width);
    let left_width = columns_width.div_ceil(2);
    let rows = labels.len().div_ceil(2);
    let (left, right) = labels.split_at(rows);

    left.iter()
        .zip(right)
        .map(|(left, right)| {
            let mut spans = key_hint_line(*left).spans;
            spans.push(
                " ".repeat(left_width.saturating_sub(UnicodeWidthStr::width(*left)))
                    .into(),
            );
            spans.push(KEY_HINT_COLUMN_GAP.fg(palette::BORDER));
            spans.extend(key_hint_line(*right).spans);
            Line::from(spans)
        })
        .collect()
}

fn wide_key_hint_labels(shell: &ShellState) -> [&'static str; 18] {
    let contextual = if shell.transcript_selection.is_some() {
        [
            "Up/Down select",
            "Enter copy",
            "Esc composer",
            "Ctrl+D hide dashboard",
        ]
    } else if shell.active_turn_id.is_some() {
        [
            "Enter steer",
            "Ctrl+C interrupt, Esc x2 exit",
            "Alt+Up select, Ctrl+O copy",
            "Ctrl+D hide dashboard",
        ]
    } else {
        [
            "Enter send",
            "Ctrl+C/Esc twice to exit",
            "Alt+Up select, Ctrl+O copy",
            "Ctrl+D hide dashboard",
        ]
    };
    [
        "Alt+Left/Right switch views",
        "Shift/Alt+Enter newline",
        contextual[0],
        contextual[1],
        contextual[2],
        contextual[3],
        "Alt+M model, Alt+E effort",
        "Ctrl+1 Sessions  Ctrl+2 Agents",
        "Ctrl+3 Workspace Ctrl+4 Settings",
        "Ctrl+5 Help",
        "Sessions: Enter focus, j/k move",
        "r resume, f fork, a/u archive",
        "v archived, d delete",
        "n rename, / search",
        "Agents: Enter focus, j/k inspect",
        "Settings: Tab page, Enter select",
        "Selectors: j/k choose, Enter apply",
        if shell.transcript_selection.is_some() {
            "Esc return to composer"
        } else {
            "Esc twice to exit"
        },
    ]
}

fn compact_key_hint_labels(shell: &ShellState) -> [&'static str; 18] {
    let contextual = if shell.transcript_selection.is_some() {
        [
            "↑/↓ select",
            "↵ copy",
            "Esc composer",
            "Ctrl+D hide dashboard",
        ]
    } else if shell.active_turn_id.is_some() {
        [
            "↵ steer",
            "^C stop · Esc×2 exit",
            "Alt+↑ select  ^O copy",
            "Ctrl+D hide dashboard",
        ]
    } else {
        [
            "↵ send",
            "^C/Esc×2 exit",
            "Alt+↑ select  ^O copy",
            "Ctrl+D hide dashboard",
        ]
    };
    [
        "Alt+←/→ switch views",
        "Shift/Alt+↵ newline",
        contextual[0],
        contextual[1],
        contextual[2],
        contextual[3],
        "Alt+M/E model/effort",
        "^1 Session · ^2 Agent",
        "^3/4/5 Work/Set/Help",
        "Session ↵ focus · j/k",
        "r resume · f fork",
        "a/u archive · v show",
        "d delete · n rename",
        "/ search",
        "Agent ↵ focus · j/k",
        "Prefs Tab · ↵ select",
        "Pick j/k · ↵ apply",
        if shell.transcript_selection.is_some() {
            "Esc composer"
        } else {
            "Esc×2 exit"
        },
    ]
}

fn dense_key_hint_labels(shell: &ShellState) -> [&'static str; 14] {
    let contextual = if shell.transcript_selection.is_some() {
        [
            "Alt+←/→ switch views",
            "↑/↓ select  ↵ copy",
            "Ctrl+D hide dashboard",
        ]
    } else if shell.active_turn_id.is_some() {
        [
            "Alt+←/→ view  ^D hide",
            "↵ steer  ^C stop/Esc×2",
            "Alt+↑/^O select/copy",
        ]
    } else {
        [
            "Alt+←/→ view  ^D hide",
            "↵ send  ^C/Esc×2 exit",
            "Alt+↑/^O select/copy",
        ]
    };
    [
        contextual[0],
        "Shift/Alt+↵ newline",
        contextual[1],
        contextual[2],
        "Alt+M/E model/effort",
        "^1 Session · ^2 Agent",
        "^3/4/5 Work/Set/Help",
        "Sess ↵/j/k focus/nav",
        "r/f resume/fork",
        "a/u/v/d arc/show/del",
        "n rename  / search",
        "Agent ↵/j/k focus/nav",
        "Tab/j/k nav · ↵ apply",
        if shell.transcript_selection.is_some() {
            "Esc composer"
        } else {
            "Esc×2 exit"
        },
    ]
}
