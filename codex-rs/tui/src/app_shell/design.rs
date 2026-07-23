use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Span;

#[allow(
    dead_code,
    reason = "semantic roles are adopted by app-shell views incrementally"
)]
pub(super) mod palette {
    use ratatui::style::Color;

    pub const BASE: Color = Color::Rgb(26, 27, 38);
    pub const DARK: Color = Color::Rgb(22, 22, 30);
    pub const SURFACE: Color = Color::Rgb(36, 40, 59);
    pub const ELEVATED: Color = Color::Rgb(41, 46, 66);
    pub const DIFF_ADDED_BACKGROUND: Color = Color::Rgb(33, 41, 34);
    pub const DIFF_REMOVED_BACKGROUND: Color = Color::Rgb(60, 23, 15);
    pub const BORDER: Color = Color::Rgb(65, 72, 104);
    pub const TEXT: Color = Color::Rgb(192, 202, 245);
    pub const MUTED: Color = Color::Rgb(86, 95, 137);
    pub const FOCUS: Color = Color::Rgb(122, 162, 247);
    pub const CYAN: Color = Color::Rgb(125, 207, 255);
    pub const PURPLE: Color = Color::Rgb(187, 154, 247);
    pub const SUCCESS: Color = Color::Rgb(158, 206, 106);
    pub const WARNING: Color = Color::Rgb(224, 175, 104);
    pub const ERROR: Color = Color::Rgb(247, 118, 142);
}

// Compatibility aliases for app-shell views that have not moved to semantic roles yet.
pub(super) const MOCHA_BASE: Color = palette::BASE;
pub(super) const MOCHA_MANTLE: Color = palette::DARK;
pub(super) const MOCHA_SURFACE0: Color = palette::SURFACE;

const PANE_PADDING: u16 = 1;
const MODAL_MAX_WIDTH: u16 = 72;

#[derive(Debug, Clone, Copy)]
pub(super) enum Tone {
    Dim,
    Focus,
    Success,
    Danger,
    Codex,
}

pub(super) fn pane_style(color: Color) -> Style {
    Style::new().fg(palette::TEXT).bg(color)
}

pub(super) fn selection_style() -> Style {
    pane_style(palette::ELEVATED)
}

pub(super) fn text_selection_style() -> Style {
    Style::new().fg(palette::DARK).bg(palette::FOCUS)
}

pub(super) fn fill_rect(buf: &mut Buffer, area: Rect, color: Color) {
    let style = pane_style(color);
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

pub(super) fn pane_content_rect(area: Rect) -> Rect {
    let horizontal_padding = inset_for(area.width, PANE_PADDING);
    let vertical_padding = inset_for(area.height, PANE_PADDING);
    Rect::new(
        area.x.saturating_add(horizontal_padding),
        area.y.saturating_add(vertical_padding),
        area.width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        area.height
            .saturating_sub(vertical_padding.saturating_mul(2)),
    )
}

pub(super) fn title_rect(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width, area.height.min(1))
}

pub(super) fn body_rect_after_title(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    )
}

pub(super) fn centered_band_rect(area: Rect, height: u16) -> Rect {
    let available_height = area.height.saturating_sub(4);
    let height = height.min(available_height).max(available_height.min(5));
    let available_width = area.width.saturating_sub(4);
    let width = available_width.min(MODAL_MAX_WIDTH);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

pub(super) fn tab_span(label: String, active: bool) -> Span<'static> {
    if active {
        tone_span(label, Tone::Focus).bold()
    } else {
        tone_span(label, Tone::Dim)
    }
}

pub(super) fn badge_span(label: impl Into<String>, tone: Tone) -> Span<'static> {
    tone_span(label.into(), tone).bold()
}

pub(super) fn tone_span(text: String, tone: Tone) -> Span<'static> {
    let color = match tone {
        Tone::Dim => palette::MUTED,
        Tone::Focus => palette::FOCUS,
        Tone::Success => palette::SUCCESS,
        Tone::Danger => palette::ERROR,
        Tone::Codex => palette::PURPLE,
    };
    Span::styled(text, Style::new().fg(color))
}

fn inset_for(size: u16, padding: u16) -> u16 {
    padding.min(size.saturating_sub(1) / 2)
}
