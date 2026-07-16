use super::agent_activity_render::status_visual;
use super::agent_log::AgentLogState;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_style;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
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
struct AgentLogGeometry {
    modal: Rect,
    header: Rect,
    body: Rect,
    footer: Rect,
}

pub(super) fn render_agent_log(log: &AgentLogState, screen: Rect, buf: &mut Buffer) {
    let geometry = agent_log_geometry(screen);
    buf.set_style(screen, Style::new().add_modifier(Modifier::DIM));
    Clear.render(geometry.modal, buf);
    fill_rect(buf, geometry.modal, palette::SURFACE);

    let title = format!(" Agent log · {} ", log.target.display_name);
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette::FOCUS))
        .style(pane_style(palette::SURFACE))
        .title(Line::from(title).bold())
        .render(geometry.modal, buf);

    let (glyph, status_color) = status_visual(log.target.status);
    let header_width = usize::from(geometry.header.width);
    let mut header = vec![truncate_line_with_ellipsis_if_overflow(
        Line::from(vec![
            glyph.fg(status_color).bold(),
            " ".into(),
            log.target.status.label().fg(status_color),
            "  ".into(),
            log.target.path.clone().fg(palette::MUTED),
        ]),
        header_width,
    )];
    if geometry.header.height > 1 {
        let task = log
            .target
            .task_summary
            .as_deref()
            .unwrap_or("No task summary");
        header.push(truncate_line_with_ellipsis_if_overflow(
            Line::from(vec![
                "Task  ".fg(palette::CYAN).bold(),
                first_wrapped_line(task, header_width.saturating_sub(/*rhs*/ 6)).fg(
                    if log.target.task_summary.is_some() {
                        palette::TEXT
                    } else {
                        palette::MUTED
                    },
                ),
            ]),
            header_width,
        ));
    }
    Paragraph::new(header)
        .style(pane_style(palette::SURFACE))
        .render(geometry.header, buf);

    let body_width = usize::from(geometry.body.width.max(/*other*/ 1));
    let body_height = usize::from(geometry.body.height);
    let transient_lines = if log.is_loading() {
        Some(vec![
            "Loading the complete agent history…"
                .fg(palette::MUTED)
                .into(),
        ])
    } else {
        log.error().map(|error| {
            word_wrap_lines(
                vec![Line::from(error.to_string().fg(palette::ERROR))],
                RtOptions::new(body_width),
            )
        })
    };
    let (visible, visual_lines, scroll) = if let Some(wrapped) = transient_lines {
        let visual_lines = wrapped.len();
        log.set_scroll_max(visual_lines.saturating_sub(body_height));
        let scroll = log.scroll();
        let visible = wrapped
            .into_iter()
            .skip(scroll)
            .take(body_height)
            .collect::<Vec<_>>();
        (visible, visual_lines, scroll)
    } else {
        let viewport = log.ready_viewport(body_width, body_height);
        (viewport.lines, viewport.visual_lines, viewport.scroll)
    };
    Paragraph::new(visible)
        .style(pane_style(palette::SURFACE))
        .render(geometry.body, buf);

    Paragraph::new(agent_log_footer(
        scroll,
        visual_lines,
        body_height,
        geometry.footer.width,
        log.is_loading(),
    ))
    .style(pane_style(palette::SURFACE))
    .render(geometry.footer, buf);
}

pub(super) fn agent_log_panel_area(screen: Rect) -> Rect {
    agent_log_geometry(screen).modal
}

fn agent_log_geometry(screen: Rect) -> AgentLogGeometry {
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
    AgentLogGeometry {
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

fn agent_log_footer(
    scroll: usize,
    visual_lines: usize,
    visible_lines: usize,
    width: u16,
    loading: bool,
) -> Line<'static> {
    let position = if loading {
        "loading".to_string()
    } else {
        let first = usize::from(visual_lines > 0)
            .saturating_add(scroll)
            .min(visual_lines);
        let last = scroll
            .saturating_add(visible_lines)
            .min(visual_lines)
            .max(first);
        format!("{first}-{last}/{visual_lines}")
    };
    let width = usize::from(width);
    let hint = [
        "j/k scroll  PgUp/PgDn page  g/G ends  r reload  Esc close",
        "j/k scroll  PgUp/PgDn  r reload  Esc",
        "↑↓ scroll  PgUp/PgDn  r  Esc",
        "↑↓  Esc",
        "",
    ]
    .into_iter()
    .find(|hint| {
        let spacing = usize::from(!hint.is_empty()) * 3;
        hint.chars().count() + spacing + position.chars().count() <= width
    })
    .unwrap_or_default();
    Line::from(vec![
        if hint.is_empty() {
            "".into()
        } else {
            format!(" {hint}  ").fg(palette::MUTED)
        },
        position.fg(palette::PURPLE).bold(),
    ])
}

fn first_wrapped_line(text: &str, width: usize) -> String {
    textwrap::wrap(text, textwrap::Options::new(width.max(1)))
        .first()
        .map_or_else(String::new, std::string::ToString::to_string)
}
