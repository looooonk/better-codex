use super::ShellState;
use super::composer_render::wrapped_composer_lines;
use super::design::pane_content_rect;
use super::input_request_view::approval_lines;
use super::input_request_view::elicitation_lines;
use super::input_request_view::request_panel_visual_line_count;
use super::input_request_view::user_input_lines;
use super::navigation::DashboardRoute;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;

const DASHBOARD_SIDE_BY_SIDE_MIN_WIDTH: u16 = 100;
const DASHBOARD_MIN_WIDTH: u16 = 50;
const DASHBOARD_MAX_WIDTH: u16 = 64;
const DASHBOARD_WIDTH_PERCENT: u16 = 34;
const COMPACT_HEADER_HEIGHT: u16 = 2;
const PADDED_HEADER_HEIGHT: u16 = 3;
const PADDED_HEADER_MIN_SCREEN_HEIGHT: u16 = 17;
const INPUT_PANEL_MIN_HEIGHT: u16 = 6;
const INPUT_PANEL_MAX_HEIGHT: u16 = 12;
const INPUT_REQUEST_PANEL_MIN_HEIGHT: u16 = 8;
const COMPACT_INPUT_PANEL_MIN_HEIGHT: u16 = 4;
const HELP_OVERLAY_MIN_HEIGHT: u16 = 10;
const PANE_CHROME_HEIGHT: u16 = 3;
const TRANSCRIPT_MIN_HEIGHT: u16 = 5;

pub(super) const MIN_TERMINAL_WIDTH: u16 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DashboardPlacement {
    Sidebar(Rect),
    Overlay(Rect),
}

impl DashboardPlacement {
    pub(super) fn area(self) -> Rect {
        match self {
            Self::Sidebar(area) | Self::Overlay(area) => area,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShellLayout {
    pub(super) header: Rect,
    pub(super) transcript: Rect,
    pub(super) input: Rect,
    pub(super) dashboard: Option<DashboardPlacement>,
}

pub(super) fn terminal_width_supported(width: u16) -> bool {
    width >= MIN_TERMINAL_WIDTH
}

pub(super) fn calculate(shell: &ShellState, area: Rect) -> Option<ShellLayout> {
    if !terminal_width_supported(area.width) {
        return None;
    }

    let header_height = if area.height >= PADDED_HEADER_MIN_SCREEN_HEIGHT {
        PADDED_HEADER_HEIGHT
    } else {
        COMPACT_HEADER_HEIGHT
    };
    if !shell.dashboard_visible || area.width < DASHBOARD_SIDE_BY_SIDE_MIN_WIDTH {
        let available_height = area.height.saturating_sub(header_height);
        let mut input_height = input_panel_height(shell, available_height, area.width);
        if shell.dashboard_visible && shell.dashboard_route == DashboardRoute::Help {
            let max_input_height = available_height
                .saturating_sub(HELP_OVERLAY_MIN_HEIGHT)
                .max(available_height.min(COMPACT_INPUT_PANEL_MIN_HEIGHT));
            input_height = input_height.min(max_input_height);
        }
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Min(TRANSCRIPT_MIN_HEIGHT),
                Constraint::Length(input_height),
            ])
            .split(area);
        let mut layout = ShellLayout {
            header: main[0],
            transcript: main[1],
            input: main[2],
            dashboard: None,
        };
        if shell.dashboard_visible {
            let width = dashboard_width(area.width).min(layout.transcript.width);
            layout.dashboard = Some(DashboardPlacement::Overlay(Rect::new(
                layout.transcript.right().saturating_sub(width),
                layout.transcript.y,
                width,
                layout.transcript.height,
            )));
        }
        return Some(layout);
    }

    let dashboard_width = dashboard_width(area.width);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(dashboard_width)),
            Constraint::Length(dashboard_width),
        ])
        .split(area);
    let input_height = input_panel_height(
        shell,
        area.height.saturating_sub(header_height),
        horizontal[0].width,
    );
    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(TRANSCRIPT_MIN_HEIGHT),
            Constraint::Length(input_height),
        ])
        .split(horizontal[0]);
    Some(ShellLayout {
        header: main[0],
        transcript: main[1],
        input: main[2],
        dashboard: Some(DashboardPlacement::Sidebar(horizontal[1])),
    })
}

fn dashboard_width(terminal_width: u16) -> u16 {
    u32::from(terminal_width)
        .saturating_mul(u32::from(DASHBOARD_WIDTH_PERCENT))
        .div_ceil(100)
        .try_into()
        .unwrap_or(u16::MAX)
        .clamp(DASHBOARD_MIN_WIDTH, DASHBOARD_MAX_WIDTH)
        .min(terminal_width)
}

fn input_panel_height(shell: &ShellState, available_height: u16, input_width: u16) -> u16 {
    let body_width = pane_content_rect(Rect::new(
        /*x*/ 0,
        /*y*/ 0,
        input_width,
        available_height,
    ))
    .width;
    let request_lines = if let Some(pending) = &shell.pending_approval {
        Some(approval_lines(pending))
    } else if let Some(pending) = &shell.pending_elicitation {
        Some(elicitation_lines(pending))
    } else {
        shell
            .pending_user_input
            .as_ref()
            .map(|pending| user_input_lines(pending, &shell.composer, body_width))
    };
    if let Some(lines) = request_lines {
        let visual_line_count =
            u16::try_from(request_panel_visual_line_count(&lines, body_width)).unwrap_or(u16::MAX);
        let desired_height = visual_line_count
            .saturating_add(PANE_CHROME_HEIGHT)
            .clamp(INPUT_REQUEST_PANEL_MIN_HEIGHT, INPUT_PANEL_MAX_HEIGHT);
        return desired_height.min(available_height);
    }

    let composer_line_count = u16::try_from(
        wrapped_composer_lines(
            shell.composer.text(),
            shell.composer.is_empty(),
            shell.composer.cursor(),
            usize::from(body_width).max(1),
        )
        .len(),
    )
    .unwrap_or(u16::MAX);
    let desired_height = composer_line_count
        .saturating_add(PANE_CHROME_HEIGHT)
        .clamp(INPUT_PANEL_MIN_HEIGHT, INPUT_PANEL_MAX_HEIGHT);
    let max_height = available_height
        .saturating_sub(TRANSCRIPT_MIN_HEIGHT)
        .max(available_height.min(INPUT_PANEL_MIN_HEIGHT));
    desired_height.min(max_height)
}

#[cfg(test)]
#[path = "shell_layout_tests.rs"]
mod tests;
