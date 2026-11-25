//! The empty conversation mark, sized for terminal cells that are roughly twice as tall as wide.

use super::ShellState;
use super::shell_layout::DashboardPlacement;
use super::shell_layout::ShellLayout;
use crate::terminal_palette::rgb_color;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
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
            && self.active_turn_id.is_none()
    }

    pub(super) fn start_logo_animates(&self) -> bool {
        self.animations && self.shimmer_enabled && self.shows_start_logo()
    }
}

pub(super) fn render(
    shell: &ShellState,
    layout: ShellLayout,
    hover_position: Option<Position>,
    buf: &mut Buffer,
) {
    let mut area = layout.transcript;
    if let Some(DashboardPlacement::Overlay(overlay)) = layout.dashboard {
        area.width = overlay.x.saturating_sub(area.x);
    }
    if shell.has_pending_backend_action(super::backend_actions::ActionGroup::SessionSwitch) {
        render_loading(area, shell.chrome_animation_frame, buf);
        return;
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
    let hover_position = hover_position.filter(|position| area.contains(*position));
    for (y, (row, glyphs)) in mark.rows().zip(rows).enumerate() {
        for (x, (cell, glyph)) in row.columns().zip(glyphs.chars()).enumerate() {
            if glyph == ' ' {
                continue;
            }
            let highlight = if shimmer {
                let sweep_distance = phase.abs_diff(x + y + 5) as f32;
                let distance = hover_position.map_or(sweep_distance, |pointer| {
                    // A terminal row is about two columns tall. Use the nearer light
                    // so the circular hover glow and the passing glint can coexist.
                    let dx = f32::from(cell.x.abs_diff(pointer.x));
                    let dy = 2.0 * f32::from(cell.y.abs_diff(pointer.y));
                    sweep_distance.min(dx.hypot(dy))
                });
                ((5.0 - distance).max(0.0) * 22.0).round() as u8
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

pub(super) fn render_loading(area: Rect, frame: usize, buf: &mut Buffer) {
    use ratatui::widgets::Paragraph;
    use ratatui::widgets::Widget;
    const DOTS: [char; 8] = ['⠋', '⠙', '⠸', '⢰', '⣠', '⣄', '⡆', '⠇'];
    let spinner = DOTS[frame % DOTS.len()];
    Paragraph::new(format!("{spinner} Loading session..."))
        .centered()
        .render(
            Rect::new(
                area.x,
                area.y + area.height.saturating_sub(1) / 2,
                area.width,
                area.height.min(1),
            ),
            buf,
        );
}

#[cfg(test)]
#[path = "start_logo_tests.rs"]
mod tests;
