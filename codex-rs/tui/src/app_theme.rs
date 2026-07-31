use codex_config::types::TuiAppTheme;
use ratatui::style::Color;
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

// Palette reads are render-scoped instead of process-global so separate shell instances and
// parallel snapshot tests can use different themes without contaminating one another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThemePalette {
    pub(crate) base: Color,
    pub(crate) dark: Color,
    pub(crate) surface: Color,
    pub(crate) elevated: Color,
    pub(crate) diff_added_background: Color,
    pub(crate) diff_removed_background: Color,
    pub(crate) border: Color,
    pub(crate) text: Color,
    pub(crate) muted: Color,
    pub(crate) focus: Color,
    pub(crate) cyan: Color,
    pub(crate) purple: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) error: Color,
}

const TOKYO_NIGHT: ThemePalette = ThemePalette {
    base: Color::Rgb(26, 27, 38),
    dark: Color::Rgb(22, 22, 30),
    surface: Color::Rgb(36, 40, 59),
    elevated: Color::Rgb(41, 46, 66),
    diff_added_background: Color::Rgb(33, 41, 34),
    diff_removed_background: Color::Rgb(60, 23, 15),
    border: Color::Rgb(65, 72, 104),
    text: Color::Rgb(192, 202, 245),
    muted: Color::Rgb(86, 95, 137),
    focus: Color::Rgb(122, 162, 247),
    cyan: Color::Rgb(125, 207, 255),
    purple: Color::Rgb(187, 154, 247),
    success: Color::Rgb(158, 206, 106),
    warning: Color::Rgb(224, 175, 104),
    error: Color::Rgb(247, 118, 142),
};

const GRUVBOX_DARK: ThemePalette = ThemePalette {
    base: Color::Rgb(40, 40, 40),
    dark: Color::Rgb(29, 32, 33),
    surface: Color::Rgb(60, 56, 54),
    elevated: Color::Rgb(80, 73, 69),
    diff_added_background: Color::Rgb(48, 56, 31),
    diff_removed_background: Color::Rgb(68, 43, 36),
    border: Color::Rgb(102, 92, 84),
    text: Color::Rgb(235, 219, 178),
    muted: Color::Rgb(168, 153, 132),
    focus: Color::Rgb(131, 165, 152),
    cyan: Color::Rgb(142, 192, 124),
    purple: Color::Rgb(211, 134, 155),
    success: Color::Rgb(184, 187, 38),
    warning: Color::Rgb(250, 189, 47),
    error: Color::Rgb(251, 73, 52),
};

const CATPPUCCIN_MOCHA: ThemePalette = ThemePalette {
    base: Color::Rgb(30, 30, 46),
    dark: Color::Rgb(24, 24, 37),
    surface: Color::Rgb(49, 50, 68),
    elevated: Color::Rgb(69, 71, 90),
    diff_added_background: Color::Rgb(38, 53, 47),
    diff_removed_background: Color::Rgb(61, 37, 47),
    border: Color::Rgb(88, 91, 112),
    text: Color::Rgb(205, 214, 244),
    muted: Color::Rgb(127, 132, 156),
    focus: Color::Rgb(137, 180, 250),
    cyan: Color::Rgb(137, 220, 235),
    purple: Color::Rgb(203, 166, 247),
    success: Color::Rgb(166, 227, 161),
    warning: Color::Rgb(249, 226, 175),
    error: Color::Rgb(243, 139, 168),
};

thread_local! {
    static ACTIVE_PALETTE: Cell<ThemePalette> = const { Cell::new(TOKYO_NIGHT) };
}

#[must_use = "the guard must stay alive for the themed render"]
pub(crate) struct ActiveThemeGuard {
    previous: ThemePalette,
    _not_send: PhantomData<Rc<()>>,
}

impl Drop for ActiveThemeGuard {
    fn drop(&mut self) {
        ACTIVE_PALETTE.set(self.previous);
    }
}

pub(crate) fn activate(theme: TuiAppTheme) -> ActiveThemeGuard {
    let palette = match theme {
        TuiAppTheme::TokyoNight => TOKYO_NIGHT,
        TuiAppTheme::GruvboxDark => GRUVBOX_DARK,
        TuiAppTheme::CatppuccinMocha => CATPPUCCIN_MOCHA,
    };
    ActiveThemeGuard {
        previous: ACTIVE_PALETTE.replace(palette),
        _not_send: PhantomData,
    }
}

pub(crate) fn palette() -> ThemePalette {
    ACTIVE_PALETTE.get()
}

#[cfg(test)]
#[path = "app_theme_tests.rs"]
mod tests;
