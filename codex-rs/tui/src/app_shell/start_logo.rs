//! The empty conversation mark, sized for terminal cells that are roughly twice as tall as wide.

use super::ShellState;
use super::shell_layout::DashboardPlacement;
use super::shell_layout::ShellLayout;
use crate::terminal_palette::rgb_color;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

const LOGO: [&str; 9] = [
    "   ⢀⣠⣴⠶⠿⠛⠛⠿⠶⣦⣄⡀",
    " ⢀⣴⠟⠋        ⠙⠻⣦⡀",
    "⢀⣾⠋ ⣠⡀         ⠙⣷⡀",
    "⣼⡇  ⠈⠛⢶⣄        ⢸⣧",
    "⣿     ⣠⡿⠃        ⣿",
    "⢻⡇  ⢠⠾⠋  ⣤⣤⣤⣤⡄  ⢸⡟",
    "⠈⢿⣄            ⣠⡿⠁",
    " ⠈⠻⣦⣄        ⣠⣴⠟⠁",
    "   ⠈⠙⠻⠶⣶⣤⣤⣶⠶⠟⠋⠁",
];

const COMPACT_LOGO: [&str; 5] = [
    " ⣠⡴⠞⠛⠛⠳⢦⣄",
    "⣰⠏⢠⡀    ⠹⣆",
    "⣿  ⣹⠆    ⣿",
    "⠹⣆⠘⠁ ⠒⠒⠂⣰⠏",
    " ⠙⠳⢦⣤⣤⡴⠞⠋",
];

const SMALL_LOGO: [&str; 3] = ["⢠⢖⠋⠙⠲⡄", "⣏⢈⡷⢀⡀⣹", "⠘⠮⣄⣠⠴⠃"];

impl ShellState {
    pub(super) fn shows_start_logo(&self) -> bool {
        self.transcript.is_empty()
            && self.streaming_assistant.is_empty()
            && self.streaming_plan.is_empty()
            && self.active_turn_id.is_none()
    }

    pub(super) fn start_logo_animates(&self) -> bool {
        self.animations && self.shimmer_enabled && self.shows_start_logo()
    }
}

pub(super) fn render(shell: &ShellState, layout: ShellLayout, buf: &mut Buffer) {
    let mut area = layout.transcript;
    if let Some(DashboardPlacement::Overlay(overlay)) = layout.dashboard {
        area.width = overlay.x.saturating_sub(area.x);
    }
    let (rows, width): (&[&str], u16) = if area.width >= 18 && area.height >= 9 {
        (&LOGO, 18)
    } else if area.width >= 10 && area.height >= 5 {
        (&COMPACT_LOGO, 10)
    } else if area.width >= 6 && area.height >= 3 {
        (&SMALL_LOGO, 6)
    } else if area.width >= 4 && area.height > 0 {
        (&["⟮⠕⠤⟯"], 4)
    } else {
        return;
    };
    let height = rows.len() as u16;
    let mark = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );

    // Sweep a diagonal highlight across the mark, with a quiet interval between passes.
    let phase = shell.chrome_animation_frame % (usize::from(width + height) + 18);
    let shimmer = shell.animations && shell.shimmer_enabled;
    for (y, (row, glyphs)) in mark.rows().zip(rows).enumerate() {
        for (x, (cell, glyph)) in row.columns().zip(glyphs.chars()).enumerate() {
            if glyph == ' ' {
                continue;
            }
            let distance = phase.abs_diff(x + y + 5);
            let highlight = if shimmer {
                5_usize.saturating_sub(distance) as u8 * 22
            } else {
                0
            };
            let gray = 144 + highlight;
            buf[(cell.x, cell.y)]
                .set_char(glyph)
                .set_fg(rgb_color((gray, gray, gray)));
        }
    }
}

#[cfg(test)]
#[path = "start_logo_tests.rs"]
mod tests;
