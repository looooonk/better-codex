use super::ShellState;
use super::design::key_hint_line;
use super::design::palette;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
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
    let right_width = columns_width.saturating_sub(left_width);
    let rows = labels.len().div_ceil(2);
    let (left, right) = labels.split_at(rows);

    left.iter()
        .zip(right)
        .map(|(left, right)| {
            let left = truncate_line_with_ellipsis_if_overflow(key_hint_line(*left), left_width);
            let rendered_left_width = left.width();
            let mut spans = left.spans;
            spans.push(
                " ".repeat(left_width.saturating_sub(rendered_left_width))
                    .into(),
            );
            spans.push(KEY_HINT_COLUMN_GAP.fg(palette::BORDER));
            spans.extend(
                truncate_line_with_ellipsis_if_overflow(key_hint_line(*right), right_width).spans,
            );
            Line::from(spans)
        })
        .collect()
}

fn wide_key_hint_labels(shell: &ShellState) -> [&'static str; 18] {
    let contextual = if shell.transcript_selection.is_some() {
        [
            "Up/Down select",
            if shell.selected_transcript_is_output() {
                "Enter open output"
            } else {
                "Enter copy"
            },
            "Esc composer",
            "Ctrl+D hide dashboard",
        ]
    } else if shell.active_turn_id.is_some() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        [
            if editing_queue {
                "Enter/Tab save queued edit"
            } else {
                "Enter steer, Tab queue"
            },
            if editing_queue {
                "Alt+Up/Down traverse queue"
            } else {
                "Alt+Up/Down edit queued"
            },
            "Ctrl+C interrupt, Esc x2 exit",
            "Ctrl+O copy, Ctrl+D hide dashboard",
        ]
    } else if shell.has_pending_shell_command() {
        [
            "Enter send",
            "Ctrl+C cancel shell, Esc x2 exit",
            "Alt+Up select, Ctrl+O copy",
            "Ctrl+D hide dashboard",
        ]
    } else if shell.composer.has_queued_messages() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        [
            if editing_queue {
                "Enter save and resume queue"
            } else {
                "Enter send or resume queue"
            },
            if editing_queue {
                "Alt+Up/Down traverse queue"
            } else {
                "Alt+Up/Down edit queued"
            },
            "Ctrl+C/Esc twice to exit",
            "Ctrl+O copy, Ctrl+D hide dashboard",
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
        "Cmd arrows/⌫ line; Opt/Ctrl word",
        "Shift/Alt+Enter newline; Fn nav/⌫",
        contextual[0],
        contextual[1],
        contextual[2],
        contextual[3],
        "Alt+M model, Alt+E effort",
        "Ctrl+1 Status  Ctrl+2 Agents",
        "Ctrl+3 Sessions Ctrl+4 Help",
        "Ctrl+N new, mouse click rows",
        "Sessions: Enter focus, j/k move",
        "r resume, f fork, a/u archive",
        "v archived, d delete",
        "n rename, / search",
        "Agents: Enter focus/log, j/k inspect",
        "Status: Tab page, Enter select",
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
            if shell.selected_transcript_is_output() {
                "↵ open output"
            } else {
                "↵ copy"
            },
            "Esc composer",
            "Ctrl+D hide dashboard",
        ]
    } else if shell.active_turn_id.is_some() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        [
            if editing_queue {
                "↵/Tab save queued edit"
            } else {
                "↵ steer · Tab queue"
            },
            if editing_queue {
                "Alt+↑/↓ traverse queue"
            } else {
                "Alt+↑/↓ edit queued"
            },
            "^C stop · Esc×2 exit",
            "^O copy · ^D hide dashboard",
        ]
    } else if shell.has_pending_shell_command() {
        [
            "↵ send",
            "^C cancel shell · Esc×2 exit",
            "Alt+↑ select  ^O copy",
            "Ctrl+D hide dashboard",
        ]
    } else if shell.composer.has_queued_messages() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        [
            if editing_queue {
                "↵ save + resume queue"
            } else {
                "↵ send/resume queue"
            },
            if editing_queue {
                "Alt+↑/↓ traverse queue"
            } else {
                "Alt+↑/↓ edit queued"
            },
            "^C/Esc×2 exit",
            "^O copy · ^D hide dashboard",
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
        "⌘←→⌫ line · ⌥/^ word",
        "S/A+↵ newline · Fn nav/⌫",
        contextual[0],
        contextual[1],
        contextual[2],
        contextual[3],
        "Alt+M/E model/effort",
        "^1 Status · ^2 Agents",
        "^3 Sessions · ^4 Help",
        "^N new · Session ↵ focus",
        "r resume · f fork",
        "a/u archive · v show",
        "d delete · n rename",
        "/ search",
        "Agent ↵ focus/log · j/k",
        "Status Tab · ↵ select",
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
            if shell.selected_transcript_is_output() {
                "↑/↓ select  ↵ open"
            } else {
                "↑/↓ select  ↵ copy"
            },
            "Esc back · ^D hide",
            "^O copy",
        ]
    } else if shell.active_turn_id.is_some() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        [
            "⌘←→⌫ line · ⌥/^ word",
            if editing_queue {
                "↵/Tab save · ^C stop"
            } else {
                "↵ steer · Tab queue"
            },
            if editing_queue {
                "Alt+↑/↓ traverse"
            } else {
                "Alt+↑/↓ edit · ^C stop"
            },
        ]
    } else if shell.has_pending_shell_command() {
        [
            "⌘←→⌫ line · ⌥/^ word",
            "↵ send  ^C cancel/Esc×2",
            "Alt+↑/^O select/copy",
        ]
    } else if shell.composer.has_queued_messages() {
        let editing_queue = shell.composer.queued_edit_position().is_some();
        [
            "⌘←→⌫ line · ⌥/^ word",
            if editing_queue {
                "↵ save/resume  ^C exit"
            } else {
                "↵ send/resume  ^C exit"
            },
            if editing_queue {
                "Alt+↑/↓ traverse"
            } else {
                "Alt+↑/↓ edit queue"
            },
        ]
    } else {
        [
            "⌘←→⌫ line · ⌥/^ word",
            "↵ send  ^C/Esc×2 exit",
            "Alt+↑/^O select/copy",
        ]
    };
    [
        contextual[0],
        "S/A↵ newline · Fn nav/⌫",
        contextual[1],
        contextual[2],
        "Alt+M/E model/effort",
        "^1 Status · ^2 Agents",
        "^3 Sessions · ^4 Help",
        "^N new · Sess ↵ focus",
        "r/f resume/fork",
        "a/u/v/d arc/show/del",
        "n rename  / search",
        "Agent ↵ focus/log · j/k",
        "Tab/j/k nav · ↵ apply",
        if shell.transcript_selection.is_some() {
            "Esc back · ^D hide"
        } else {
            "Esc×2 exit · ^D hide"
        },
    ]
}
