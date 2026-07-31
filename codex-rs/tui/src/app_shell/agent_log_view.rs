use super::agent_activity_render::status_visual;
use super::agent_log::AgentLogState;
use super::design::palette;
use super::design::pane_style;
use super::scrollback_view::ScrollbackFooterMode;
use super::scrollback_view::render_scrollback_footer;
use super::scrollback_view::render_scrollback_frame;
use super::scrollback_view::scrollback_panel_area;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

pub(super) fn render_agent_log(log: &AgentLogState, screen: Rect, buf: &mut Buffer) {
    let title = format!(" Agent log · {} ", log.target.display_name);
    let geometry = render_scrollback_frame(screen, title, buf);

    let (glyph, status_color) = status_visual(log.target.status);
    let header_width = usize::from(geometry.header.width);
    let mut header = vec![truncate_line_with_ellipsis_if_overflow(
        Line::from(vec![
            glyph.fg(status_color).bold(),
            " ".into(),
            log.target.status.label().fg(status_color),
            "  ".into(),
            log.target.path.clone().fg(palette::muted()),
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
                "Task  ".fg(palette::cyan()).bold(),
                first_wrapped_line(task, header_width.saturating_sub(/*rhs*/ 6)).fg(
                    if log.target.task_summary.is_some() {
                        palette::text()
                    } else {
                        palette::muted()
                    },
                ),
            ]),
            header_width,
        ));
    }
    Paragraph::new(header)
        .style(pane_style(palette::surface()))
        .render(geometry.header, buf);

    let body_width = usize::from(geometry.body.width.max(/*other*/ 1));
    let body_height = usize::from(geometry.body.height);
    let transient_lines = if log.is_loading() {
        Some(vec![
            "Loading the complete agent history…"
                .fg(palette::muted())
                .into(),
        ])
    } else {
        log.error().map(|error| {
            word_wrap_lines(
                vec![Line::from(error.to_string().fg(palette::error()))],
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
        .style(pane_style(palette::surface()))
        .render(geometry.body, buf);

    render_scrollback_footer(
        geometry,
        scroll,
        visual_lines,
        body_height,
        if log.is_loading() {
            ScrollbackFooterMode::AgentLogLoading
        } else {
            ScrollbackFooterMode::AgentLogReady
        },
        buf,
    );
}

pub(super) fn agent_log_panel_area(screen: Rect) -> Rect {
    scrollback_panel_area(screen)
}

fn first_wrapped_line(text: &str, width: usize) -> String {
    textwrap::wrap(text, textwrap::Options::new(width.max(1)))
        .first()
        .map_or_else(String::new, std::string::ToString::to_string)
}
