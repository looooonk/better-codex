use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

pub(super) const MOCHA_BASE: Color = Color::Rgb(30, 30, 46);
pub(super) const MOCHA_MANTLE: Color = Color::Rgb(24, 24, 37);
pub(super) const MOCHA_SURFACE0: Color = Color::Rgb(49, 50, 68);
pub(super) const MOCHA_SURFACE1: Color = Color::Rgb(69, 71, 90);

const PANE_PADDING: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub(super) enum Tone {
    Default,
    Dim,
    Focus,
    Success,
    Danger,
    Codex,
}

pub(super) fn pane_style(color: Color) -> Style {
    Style::new().bg(color)
}

pub(super) fn selection_style() -> Style {
    Style::new().bg(MOCHA_SURFACE1)
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
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(area.x, y, area.width, height)
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

pub(super) fn key_hint_line(text: impl Into<String>) -> Line<'static> {
    Line::from(tone_span(text.into(), Tone::Default))
}

pub(super) fn tone_span(text: String, tone: Tone) -> Span<'static> {
    match tone {
        Tone::Default => text.into(),
        Tone::Dim => text.dim(),
        Tone::Focus => text.cyan(),
        Tone::Success => text.green(),
        Tone::Danger => text.red(),
        Tone::Codex => text.magenta(),
    }
}

fn inset_for(size: u16, padding: u16) -> u16 {
    padding.min(size.saturating_sub(1) / 2)
}
