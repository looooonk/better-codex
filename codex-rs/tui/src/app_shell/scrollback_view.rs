use super::design::fill_rect;
use super::design::palette;
use super::design::pane_style;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const MODAL_MARGIN: u16 = 2;
const MAX_MODAL_WIDTH: u16 = 100;
const MAX_MODAL_HEIGHT: u16 = 32;
const MIN_MODAL_HEIGHT: u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ScrollbackGeometry {
    pub(super) modal: Rect,
    pub(super) header: Rect,
    pub(super) body: Rect,
    pub(super) footer: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollbackFooterMode {
    AgentLogLoading,
    AgentLogReady,
    ToolOutputRunning,
    ToolOutputCompleted,
    ToolOutputFailed,
}

pub(super) fn render_scrollback_frame(
    screen: Rect,
    title: impl Into<String>,
    buf: &mut Buffer,
) -> ScrollbackGeometry {
    let geometry = scrollback_geometry(screen);
    buf.set_style(screen, Style::new().add_modifier(Modifier::DIM));
    Clear.render(geometry.modal, buf);
    fill_rect(buf, geometry.modal, palette::SURFACE);

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette::FOCUS))
        .style(pane_style(palette::SURFACE))
        .title(Line::from(title.into()).bold())
        .render(geometry.modal, buf);
    geometry
}

pub(super) fn render_scrollback_footer(
    geometry: ScrollbackGeometry,
    scroll: usize,
    visual_lines: usize,
    visible_lines: usize,
    mode: ScrollbackFooterMode,
    buf: &mut Buffer,
) {
    Paragraph::new(scrollback_footer(
        scroll,
        visual_lines,
        visible_lines,
        geometry.footer.width,
        mode,
    ))
    .style(pane_style(palette::SURFACE))
    .render(geometry.footer, buf);
}

pub(super) fn scrollback_panel_area(screen: Rect) -> Rect {
    scrollback_geometry(screen).modal
}

fn scrollback_geometry(screen: Rect) -> ScrollbackGeometry {
    let available_width = screen.width.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let available_height = screen.height.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let width = available_width.min(MAX_MODAL_WIDTH);
    let height = available_height
        .min(MAX_MODAL_HEIGHT)
        .max(available_height.min(MIN_MODAL_HEIGHT));
    let modal = Rect::new(
        screen
            .x
            .saturating_add(screen.width.saturating_sub(width) / 2),
        screen
            .y
            .saturating_add(screen.height.saturating_sub(height) / 2),
        width,
        height,
    );
    let inner = Rect::new(
        modal.x.saturating_add(u16::from(modal.width > 1)),
        modal.y.saturating_add(u16::from(modal.height > 1)),
        modal.width.saturating_sub(2),
        modal.height.saturating_sub(2),
    );
    let horizontal_padding = u16::from(inner.width > 2);
    let content = Rect::new(
        inner.x.saturating_add(horizontal_padding),
        inner.y,
        inner
            .width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        inner.height,
    );
    let header_height = content.height.min(2);
    let footer_height = u16::from(content.height > header_height);
    let body_height = content
        .height
        .saturating_sub(header_height)
        .saturating_sub(footer_height);
    ScrollbackGeometry {
        modal,
        header: Rect::new(content.x, content.y, content.width, header_height),
        body: Rect::new(
            content.x,
            content.y.saturating_add(header_height),
            content.width,
            body_height,
        ),
        footer: Rect::new(
            content.x,
            content
                .y
                .saturating_add(header_height)
                .saturating_add(body_height),
            content.width,
            footer_height,
        ),
    }
}

fn scrollback_footer(
    scroll: usize,
    visual_lines: usize,
    visible_lines: usize,
    width: u16,
    mode: ScrollbackFooterMode,
) -> Line<'static> {
    let first = usize::from(visual_lines > 0)
        .saturating_add(scroll)
        .min(visual_lines);
    let last = scroll
        .saturating_add(visible_lines)
        .min(visual_lines)
        .max(first);
    let range = format!("{first}-{last}/{visual_lines}");
    let (suffix, suffix_color, hints) = match mode {
        ScrollbackFooterMode::AgentLogLoading => {
            ("loading".to_string(), palette::PURPLE, agent_log_hints())
        }
        ScrollbackFooterMode::AgentLogReady => (range, palette::PURPLE, agent_log_hints()),
        ScrollbackFooterMode::ToolOutputRunning => (
            format!("running · {range}"),
            palette::CYAN,
            tool_output_hints(),
        ),
        ScrollbackFooterMode::ToolOutputCompleted => (
            format!("completed · {range}"),
            palette::SUCCESS,
            tool_output_hints(),
        ),
        ScrollbackFooterMode::ToolOutputFailed => (
            format!("failed · {range}"),
            palette::ERROR,
            tool_output_hints(),
        ),
    };
    let width = usize::from(width);
    let hint = hints
        .into_iter()
        .find(|hint| {
            let spacing = usize::from(!hint.is_empty()) * 3;
            hint.chars().count() + spacing + suffix.chars().count() <= width
        })
        .unwrap_or_default();
    Line::from(vec![
        if hint.is_empty() {
            "".into()
        } else {
            format!(" {hint}  ").fg(palette::MUTED)
        },
        suffix.fg(suffix_color).bold(),
    ])
}

fn agent_log_hints() -> [&'static str; 5] {
    [
        "j/k scroll  PgUp/PgDn page  g/G ends  r reload  Esc close",
        "j/k scroll  PgUp/PgDn  r reload  Esc",
        "↑↓ scroll  PgUp/PgDn  r  Esc",
        "↑↓  Esc",
        "",
    ]
}

fn tool_output_hints() -> [&'static str; 5] {
    [
        "j/k scroll  PgUp/PgDn page  g/G ends  Esc close",
        "j/k scroll  PgUp/PgDn  Esc",
        "↑↓ scroll  PgUp/PgDn  Esc",
        "↑↓  Esc",
        "",
    ]
}
