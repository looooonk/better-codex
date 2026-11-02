mod contrast;

use crate::color::blend;
use crate::terminal_palette::StdoutColorLevel;
use crate::terminal_palette::best_color;
use crate::terminal_palette::default_bg;
use crate::terminal_palette::default_fg;
use crate::terminal_palette::effective_stdout_color_level;
use crate::terminal_palette::rgb_color;
use crate::terminal_palette::stdout_color_level;
use ratatui::style::Color;
use ratatui::style::Style;

pub(crate) const CHATGPT_BLUE_100: (u8, u8, u8) = (164, 205, 251);
pub(crate) const CHATGPT_BLUE_200: (u8, u8, u8) = (99, 168, 248);

const TABLE_SEPARATOR_FG_ALPHA: f32 = 0.20;

/// Returns a low-contrast rule style for separators within markdown tables.
pub(crate) fn table_separator_style() -> Style {
    let (Some(fg), Some(bg)) = (default_fg(), default_bg()) else {
        return Style::default().dim();
    };
    let separator_rgb = blend(fg, bg, TABLE_SEPARATOR_FG_ALPHA);
    match stdout_color_level() {
        StdoutColorLevel::TrueColor => Style::default().fg(rgb_color(separator_rgb)),
        StdoutColorLevel::Ansi256 => Style::default().fg(best_color(separator_rgb)),
        StdoutColorLevel::Ansi16 | StdoutColorLevel::Unknown => Style::default().dim(),
    }
}

pub(crate) fn accent_color() -> ratatui::style::Color {
    crate::app_theme::palette().focus
}

fn background_rgb(background: Option<Color>) -> Option<(u8, u8, u8)> {
    match background {
        Some(Color::Rgb(r, g, b)) => Some((r, g, b)),
        Some(Color::Indexed(index)) if index >= 16 => {
            Some(crate::terminal_palette::XTERM_COLORS[usize::from(index)])
        }
        None | Some(Color::Reset) => default_bg(),
        _ => None,
    }
}

/// Keep theme-derived text readable on its painted surface, preserving terminal-owned ANSI colors.
pub(crate) fn readable_color_on(preferred: Color, background: Option<Color>) -> Color {
    let preferred = match preferred {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(index) if index >= 16 => {
            Some(crate::terminal_palette::XTERM_COLORS[usize::from(index)])
        }
        Color::Reset => default_fg(),
        _ => return preferred,
    };
    preferred.map_or(Color::Reset, |preferred| {
        contrast::foreground(
            preferred,
            background_rgb(background),
            effective_stdout_color_level(),
        )
    })
}
