use super::ShellState;
use super::agent_activity_render::agent_activity_overview_lines;
use super::agent_activity_render::agent_activity_thread_at_line;
use super::dashboard::DashboardPanel;
use super::dashboard::MORE_BUTTON;
use super::dashboard::dashboard_panels;
use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::navigation::DashboardRoute;
use super::navigation::DashboardTabs;
use super::shell_layout::DashboardPlacement;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

const SIDEBAR_PANEL_GAP: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DashboardPanelPosition {
    pub(super) column: usize,
    pub(super) line: usize,
    pub(super) width: usize,
}

impl DashboardPanelPosition {
    pub(super) fn on_more_button(&self) -> bool {
        self.column >= more_button_start(self.width)
    }
}

fn more_button_start(width: usize) -> usize {
    width.saturating_sub(MORE_BUTTON.width())
}

#[derive(Debug, Clone, Copy)]
struct PanelLayout {
    index: usize,
    top: usize,
    height: usize,
}

#[derive(Debug, Clone, Copy)]
struct PanelSlice {
    index: usize,
    area: Rect,
    source_row: usize,
}

struct DashboardView {
    panels: Vec<DashboardPanel>,
    navigation: Rect,
    body: Rect,
    body_panels: Vec<PanelLayout>,
    scroll: usize,
    max_scroll: usize,
}

impl DashboardView {
    fn new(shell: &ShellState, placement: DashboardPlacement) -> Self {
        let area = placement.area();
        let content = pane_content_rect(area);
        let mut panels = dashboard_panels(shell, usize::from(content.width));
        let navigation_height = panels
            .first()
            .map(DashboardPanel::height)
            .unwrap_or_default()
            .min(content.height);
        let navigation = Rect::new(content.x, content.y, content.width, navigation_height);
        let body = Rect::new(
            content.x,
            navigation.bottom(),
            content.width,
            content.bottom().saturating_sub(navigation.bottom()),
        );
        let panel_gap = match placement {
            DashboardPlacement::Sidebar(_) => SIDEBAR_PANEL_GAP,
            DashboardPlacement::Overlay(_) => 0,
        };
        let mut body_panels = Vec::with_capacity(panels.len().saturating_sub(1));
        let mut top = if panels.len() > 1 {
            usize::from(panel_gap)
        } else {
            0
        };
        let mut recents = None;
        let max_chat_rows = shell.session_list.max_viewport_rows();
        for (index, panel) in panels.iter().enumerate().skip(1) {
            let mut height = usize::from(panel.height());
            if panel.title == "Sessions" {
                // Recents can fill the body once the projects above it scroll out of view.
                recents = Some((index, top + height));
                height += shell
                    .session_list
                    .row_count()
                    .min(usize::from(body.height).saturating_sub(height))
                    .min(max_chat_rows);
            }
            body_panels.push(PanelLayout { index, top, height });
            top = top.saturating_add(height);
            if index + 1 < panels.len() {
                top = top.saturating_add(usize::from(panel_gap));
            }
        }
        let max_scroll = top.saturating_sub(usize::from(body.height));
        let mut scroll = shell.dashboard_scroll.get().min(max_scroll);
        if shell.dashboard_route == DashboardRoute::Settings
            && shell.settings.focused
            && shell.settings.reveal_selection.replace(false)
            && let Some(settings) = body_panels.first()
        {
            let rows = shell.settings.selected_line_range();
            let first = settings.top + rows.start();
            let last = settings.top + rows.end();
            let earliest = (last + 1).saturating_sub(usize::from(body.height));
            scroll = scroll.max(earliest).min(first).min(max_scroll);
        }
        if shell.dashboard_route == DashboardRoute::Sessions && shell.projects.reveal.replace(false)
        {
            let (panel, row) = if let Some(row) = shell.projects.selected {
                ("Projects", row)
            } else {
                let first_chat = recents.map(|(_, first)| first).unwrap_or_default();
                let capacity = (scroll + usize::from(body.height))
                    .saturating_sub(first_chat)
                    .clamp(1, max_chat_rows);
                ("Sessions", shell.session_list.selected_line(capacity))
            };
            if let Some(layout) = body_panels
                .iter()
                .find(|layout| panels[layout.index].title == panel)
            {
                let line = layout.top + row;
                scroll = scroll
                    .max((line + 1).saturating_sub(usize::from(body.height)))
                    .min(line)
                    .min(max_scroll);
            }
        }
        if let Some((index, first_chat)) = recents {
            let capacity = (scroll + usize::from(body.height))
                .saturating_sub(first_chat)
                .min(max_chat_rows);
            panels[index].lines.extend(shell.session_list.chat_lines(
                usize::from(body.width.saturating_sub(1)),
                capacity,
                shell.session_list.focused && shell.projects.selected.is_none(),
                shell.thread_id,
                &shell.projects.failed_resumes,
            ));
        }
        shell.dashboard_scroll.set(scroll);
        Self {
            panels,
            navigation,
            body,
            body_panels,
            scroll,
            max_scroll,
        }
    }

    fn navigation_slice(&self) -> Option<PanelSlice> {
        (!self.panels.is_empty() && self.navigation.height > 0).then_some(PanelSlice {
            index: 0,
            area: self.navigation,
            source_row: 0,
        })
    }

    fn visible_body_slices(&self) -> Vec<PanelSlice> {
        let viewport_end = self.scroll.saturating_add(usize::from(self.body.height));
        self.body_panels
            .iter()
            .filter_map(|panel| {
                let panel_end = panel.top.saturating_add(panel.height);
                let visible_start = panel.top.max(self.scroll);
                let visible_end = panel_end.min(viewport_end);
                if visible_start >= visible_end {
                    return None;
                }
                Some(PanelSlice {
                    index: panel.index,
                    area: Rect::new(
                        self.body.x,
                        self.body.y.saturating_add(
                            u16::try_from(visible_start.saturating_sub(self.scroll))
                                .unwrap_or(u16::MAX),
                        ),
                        self.body.width,
                        u16::try_from(visible_end.saturating_sub(visible_start))
                            .unwrap_or(u16::MAX),
                    ),
                    source_row: visible_start.saturating_sub(panel.top),
                })
            })
            .collect()
    }

    fn panel_position_at(&self, position: Position, title: &str) -> Option<DashboardPanelPosition> {
        if !self.body.contains(position) {
            return None;
        }
        let logical_row = self
            .scroll
            .saturating_add(usize::from(position.y.saturating_sub(self.body.y)));
        let layout = self.body_panels.iter().find(|layout| {
            (layout.top..layout.top.saturating_add(layout.height)).contains(&logical_row)
                && self.panels[layout.index].title == title
        })?;
        let panel = &self.panels[layout.index];
        let title_height = usize::from(panel.show_title);
        let panel_row = logical_row.saturating_sub(layout.top);
        let inset = u16::from(panel.show_title || matches!(title, "Projects" | "Sessions"));
        let text_x = self.body.x.saturating_add(inset);
        if panel_row < title_height || position.x < text_x {
            return None;
        }
        Some(DashboardPanelPosition {
            column: usize::from(position.x - text_x),
            line: panel_row.saturating_sub(title_height),
            width: usize::from(self.body.width.saturating_sub(inset)),
        })
    }
}

pub(super) fn render_dashboard(
    shell: &ShellState,
    placement: DashboardPlacement,
    pointer: Option<Position>,
    buf: &mut Buffer,
) {
    let area = placement.area();
    fill_rect(buf, area, palette::dark());
    let divider_highlighted = shell.dashboard_resize.dragging
        || pointer.is_some_and(|position| {
            position.x == area.x && (area.y..area.bottom()).contains(&position.y)
        });
    for y in area.y..area.bottom() {
        if let Some(cell) = buf.cell_mut((area.x, y)) {
            cell.set_symbol(if divider_highlighted { "┃" } else { "│" })
                .set_style(
                    Style::new().fg(if shell.dashboard_focused() || divider_highlighted {
                        palette::focus()
                    } else {
                        palette::border()
                    }),
                );
        }
    }

    let view = DashboardView::new(shell, placement);
    if let Some(panel) = view.navigation_slice() {
        render_panel(shell, &view.panels[panel.index], panel, pointer, buf);
    }
    for panel in view.visible_body_slices() {
        render_panel(shell, &view.panels[panel.index], panel, pointer, buf);
    }
}

pub(super) fn selected_setting_area(
    shell: &ShellState,
    placement: DashboardPlacement,
) -> Option<Rect> {
    let view = DashboardView::new(shell, placement);
    let row = *shell.settings.selected_line_range().start();
    view.visible_body_slices().into_iter().find_map(|slice| {
        (view.panels[slice.index].title == "Settings").then_some(())?;
        Some(Rect::new(
            slice.area.x,
            visible_row(slice, row)?,
            slice.area.width,
            1,
        ))
    })
}

pub(super) fn route_at(
    shell: &ShellState,
    placement: DashboardPlacement,
    position: Position,
) -> Option<DashboardRoute> {
    let view = DashboardView::new(shell, placement);
    if !view.navigation.contains(position) {
        return None;
    }
    DashboardTabs::new(view.navigation.width).route_at(position.x.saturating_sub(view.navigation.x))
}

pub(super) fn panel_position_at(
    shell: &ShellState,
    placement: DashboardPlacement,
    position: Position,
    title: &str,
) -> Option<DashboardPanelPosition> {
    DashboardView::new(shell, placement).panel_position_at(position, title)
}

pub(super) fn max_scroll(shell: &ShellState, placement: DashboardPlacement) -> usize {
    DashboardView::new(shell, placement).max_scroll
}

fn render_panel(
    shell: &ShellState,
    panel: &DashboardPanel,
    slice: PanelSlice,
    pointer: Option<Position>,
    buf: &mut Buffer,
) {
    if panel.show_title && slice.source_row == 0 && slice.area.height < 2 {
        return;
    }
    let text_area = if panel.show_title {
        for y in slice.area.y..slice.area.bottom() {
            if let Some(cell) = buf.cell_mut((slice.area.x, y)) {
                cell.set_symbol("▎")
                    .set_style(Style::new().fg(palette::border()));
            }
        }
        Rect::new(
            slice.area.x.saturating_add(1),
            slice.area.y,
            slice.area.width.saturating_sub(1),
            slice.area.height,
        )
    } else if matches!(panel.title.as_str(), "Projects" | "Sessions") {
        Rect::new(
            slice.area.x + 1,
            slice.area.y,
            slice.area.width.saturating_sub(1),
            slice.area.height,
        )
    } else {
        slice.area
    };
    Paragraph::new(panel.render_lines(usize::from(text_area.width)))
        .style(Style::new().fg(palette::text()))
        .scroll((u16::try_from(slice.source_row).unwrap_or(u16::MAX), 0))
        .render(text_area, buf);
    render_hover(shell, panel, slice, text_area, pointer, buf);
}

fn render_hover(
    shell: &ShellState,
    panel: &DashboardPanel,
    slice: PanelSlice,
    text_area: Rect,
    pointer: Option<Position>,
    buf: &mut Buffer,
) {
    let Some(pointer) = pointer.filter(|pointer| slice.area.contains(*pointer)) else {
        return;
    };
    let panel_row = slice
        .source_row
        .saturating_add(usize::from(pointer.y.saturating_sub(slice.area.y)));
    if panel.title == "Navigation" {
        let tabs = DashboardTabs::new(slice.area.width);
        let Some(route) = tabs.route_at(pointer.x.saturating_sub(slice.area.x)) else {
            return;
        };
        let Some(columns) = tabs.column_range(route) else {
            return;
        };
        let x = slice.area.x.saturating_add(columns.start);
        let width = columns.end.saturating_sub(columns.start);
        if let Some(y) = visible_row(slice, /*row*/ 0) {
            buf.set_style(
                Rect::new(x, y, width, 1),
                Style::new().bg(palette::border()),
            );
        }
        if let Some(y) = visible_row(slice, /*row*/ 1) {
            buf.set_style(Rect::new(x, y, width, 1), Style::new().fg(palette::focus()));
        }
        return;
    }
    if !text_area.contains(pointer) {
        return;
    }
    if matches!(panel.title.as_str(), "Projects" | "Sessions") {
        let width = usize::from(text_area.width);
        let (interactive, more) = if panel.title == "Projects" {
            let rows = shell.projects.rows(width);
            let Some(row) = rows.get(panel_row) else {
                return;
            };
            (row.action.is_some(), row.more.is_some())
        } else {
            let thread = shell.session_list.thread_index_at_line(panel_row).is_some();
            (
                thread || shell.session_list.is_new_session_line(panel_row),
                thread && !shell.session_list.show_archived(),
            )
        };
        if !interactive {
            return;
        }
        let mut area = Rect::new(text_area.x, pointer.y, text_area.width, 1);
        let row_hover = super::sessions::row_highlight();
        if more {
            let start = text_area.x + more_button_start(width) as u16;
            if pointer.x >= start {
                area.x = start;
                area.width = text_area.right() - start;
                buf.set_style(area, row_hover);
                return;
            }
            area.width = start - text_area.x;
        }
        buf.set_style(area, row_hover);
        return;
    }
    let interactive = match (shell.dashboard_route, panel.title.as_str()) {
        (DashboardRoute::Agents, "Agents") | (DashboardRoute::Settings, "Settings") => true,
        (DashboardRoute::Status, "Edits") => shell.diff_store.has_session_edits(),
        (DashboardRoute::Sessions, _)
        | (DashboardRoute::Agents, _)
        | (DashboardRoute::Status, _)
        | (DashboardRoute::Settings, _) => false,
    };
    if panel.title == "Agents" && panel_row > 1 {
        let overview_height =
            agent_activity_overview_lines(&shell.agent_activity, usize::from(text_area.width))
                .len();
        let agent_line = panel_row
            .saturating_sub(1)
            .checked_sub(overview_height + 1)
            .and_then(|line| {
                agent_activity_thread_at_line(&shell.agent_activity, line, /*line_budget*/ 24)
            });
        if agent_line.is_none() {
            return;
        }
    }
    let interactive_row = match panel.title.as_str() {
        "Settings" => shell.settings.action_at_line(panel_row).is_some(),
        _ => panel_row > 0,
    };
    if interactive && interactive_row {
        buf.set_style(
            Rect::new(text_area.x, pointer.y, text_area.width, 1),
            Style::new().bg(palette::border()),
        );
    }
}

fn visible_row(slice: PanelSlice, row: usize) -> Option<u16> {
    let offset = row.checked_sub(slice.source_row)?;
    (offset < usize::from(slice.area.height)).then(|| {
        slice
            .area
            .y
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX))
    })
}
