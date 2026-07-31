use super::ToolBlockStatus;
use super::design::palette;
use super::design::pane_style;
use super::scrollback_view::ScrollbackFooterMode;
use super::scrollback_view::render_scrollback_footer;
use super::scrollback_view::render_scrollback_frame;
use super::scrollback_view::scrollback_panel_area;
use super::tool_output::ToolOutputState;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

pub(super) fn render_tool_output(output: &ToolOutputState, screen: Rect, buf: &mut Buffer) {
    let title = format!(" Tool output · {} ", output.target.title);
    let geometry = render_scrollback_frame(screen, title, buf);
    let (glyph, label, detail, color, footer_mode) =
        status_visual(output.target.status, output.is_truncated());
    let header_width = usize::from(geometry.header.width);
    let mut header = vec![truncate_line_with_ellipsis_if_overflow(
        Line::from(vec![
            glyph.fg(color).bold(),
            " ".into(),
            label.fg(color),
            "  ".into(),
            detail.fg(palette::muted()),
        ]),
        header_width,
    )];
    if geometry.header.height > 1 {
        header.push(truncate_line_with_ellipsis_if_overflow(
            Line::from(vec![
                "Item  ".fg(palette::cyan()).bold(),
                output.target.item_id.clone().fg(palette::muted()),
            ]),
            header_width,
        ));
    }
    Paragraph::new(header)
        .style(pane_style(palette::surface()))
        .render(geometry.header, buf);

    let body_width = usize::from(geometry.body.width.max(/*other*/ 1));
    let body_height = usize::from(geometry.body.height);
    let viewport = output.ready_viewport(body_width, body_height);
    Paragraph::new(viewport.lines)
        .style(pane_style(palette::surface()))
        .render(geometry.body, buf);
    render_scrollback_footer(
        geometry,
        viewport.scroll,
        viewport.visual_lines,
        body_height,
        footer_mode,
        buf,
    );
}

pub(super) fn tool_output_panel_area(screen: Rect) -> Rect {
    scrollback_panel_area(screen)
}

fn status_visual(
    status: ToolBlockStatus,
    truncated: bool,
) -> (
    &'static str,
    &'static str,
    &'static str,
    Color,
    ScrollbackFooterMode,
) {
    match (status, truncated) {
        (ToolBlockStatus::Running, false) => (
            "●",
            "Running",
            "Live output updates automatically",
            palette::cyan(),
            ScrollbackFooterMode::ToolOutputRunning,
        ),
        (ToolBlockStatus::Success, false) => (
            "✓",
            "Completed",
            "Full captured output",
            palette::success(),
            ScrollbackFooterMode::ToolOutputCompleted,
        ),
        (ToolBlockStatus::Fail, false) => (
            "✕",
            "Failed",
            "Full captured output",
            palette::error(),
            ScrollbackFooterMode::ToolOutputFailed,
        ),
        (ToolBlockStatus::Running, true) => (
            "●",
            "Running",
            "Live tail; earlier output omitted",
            palette::cyan(),
            ScrollbackFooterMode::ToolOutputRunning,
        ),
        (ToolBlockStatus::Success, true) => (
            "✓",
            "Completed",
            "Retained tail; earlier output omitted",
            palette::success(),
            ScrollbackFooterMode::ToolOutputCompleted,
        ),
        (ToolBlockStatus::Fail, true) => (
            "✕",
            "Failed",
            "Retained tail; earlier output omitted",
            palette::error(),
            ScrollbackFooterMode::ToolOutputFailed,
        ),
    }
}
