use super::design::fill_rect;
use super::design::palette;
use super::design::pane_content_rect;
use super::design::pane_style;
use super::reasoning_ripple::ReasoningRippleFrame;
use crate::line_truncation::line_width;
use crate::text_formatting::truncate_text;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const CONTROL_GAP: u16 = 1;
const DASHBOARD_BRAND_GAP: u16 = 1;
const BRAND_CONTROL_GAP: u16 = 2;
const STATUS_SPINNER_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HeaderControl {
    Dashboard,
    Model,
    ReasoningEffort,
    ServiceTier,
}

pub(super) struct HeaderView<'a> {
    pub(super) cwd: &'a str,
    pub(super) model: &'a str,
    pub(super) reasoning_effort: &'a str,
    pub(super) service_tier: &'a str,
    pub(super) status: &'a str,
    pub(super) status_spinner_frame: Option<usize>,
    pub(super) turn_elapsed_seconds: Option<u64>,
    pub(super) dashboard_visible: bool,
    pub(super) reasoning_ripple: Option<ReasoningRippleFrame>,
}

impl HeaderView<'_> {
    pub(super) fn render(&self, area: Rect, hovered: Option<HeaderControl>, buf: &mut Buffer) {
        fill_rect(buf, area, palette::dark());
        let content = pane_content_rect(area);
        let layout = self.control_layout(area);
        if let Some(layout) = layout {
            Paragraph::new(self.dashboard_button(hovered)).render(layout.dashboard, buf);
            Paragraph::new(self.brand_line(layout.compact_brand))
                .style(pane_style(palette::dark()))
                .render(layout.brand, buf);
            if let Some(model) = layout.model {
                Paragraph::new(self.model_label())
                    .style(
                        Style::new()
                            .fg(palette::text())
                            .bg(control_background(hovered, HeaderControl::Model)),
                    )
                    .render(model, buf);
            }
            if let Some(effort) = layout.effort {
                Paragraph::new(self.effort_label())
                    .style(
                        Style::new()
                            .fg(palette::purple())
                            .bg(control_background(hovered, HeaderControl::ReasoningEffort)),
                    )
                    .render(effort, buf);
            }
            if let Some(service_tier) = layout.service_tier {
                Paragraph::new(self.service_tier_label())
                    .style(
                        Style::new()
                            .fg(palette::purple())
                            .bg(control_background(hovered, HeaderControl::ServiceTier)),
                    )
                    .render(service_tier, buf);
            }
            if let Some(status) = layout.status {
                let line = match layout.status_display {
                    HeaderStatusDisplay::Full => self.status_line(),
                    HeaderStatusDisplay::TimerOnly => self.turn_timer_line(),
                    HeaderStatusDisplay::None => Line::default(),
                };
                Paragraph::new(line)
                    .style(pane_style(palette::dark()))
                    .render(status, buf);
            }
        } else {
            Paragraph::new(self.brand_line(/*compact*/ true))
                .style(pane_style(palette::dark()))
                .render(Rect::new(content.x, content.y, content.width, 1), buf);
        }

        Paragraph::new(
            Line::from("─".repeat(usize::from(area.width)))
                .style(Style::new().fg(palette::border()).bg(palette::dark())),
        )
        .render(
            Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
            buf,
        );
        if let (Some(ripple), Some(origin)) = (
            self.reasoning_ripple,
            layout.and_then(|layout| layout.effort),
        ) {
            ripple.render(area, origin, buf);
        }
    }

    pub(super) fn control_at(&self, area: Rect, position: Position) -> Option<HeaderControl> {
        let layout = self.control_layout(area)?;
        if layout.dashboard.contains(position) {
            Some(HeaderControl::Dashboard)
        } else if layout.model.is_some_and(|area| area.contains(position)) {
            Some(HeaderControl::Model)
        } else if layout.effort.is_some_and(|area| area.contains(position)) {
            Some(HeaderControl::ReasoningEffort)
        } else if layout
            .service_tier
            .is_some_and(|area| area.contains(position))
        {
            Some(HeaderControl::ServiceTier)
        } else {
            None
        }
    }

    fn control_layout(&self, area: Rect) -> Option<HeaderControlLayout> {
        let content = pane_content_rect(area);
        let dashboard_width = u16::try_from(text_width(&self.dashboard_label())).ok()?;
        let model_width = text_width(&self.model_label());
        let effort_width = text_width(&self.effort_label());
        let service_tier_width = text_width(&self.service_tier_label());
        let status_width = line_width(&self.status_line());
        let timer_width = line_width(&self.turn_timer_line());
        let model_width = u16::try_from(model_width).ok()?;
        let effort_width = u16::try_from(effort_width).ok()?;
        let service_tier_width = u16::try_from(service_tier_width).ok()?;
        let status_width = u16::try_from(status_width).ok()?;
        let timer_width = u16::try_from(timer_width).ok()?;
        let control_sets: &[(bool, HeaderControlSet)] = if self.turn_elapsed_seconds.is_some() {
            &[
                (false, HeaderControlSet::AllWithStatus),
                (true, HeaderControlSet::AllWithStatus),
                (true, HeaderControlSet::ModelWithStatus),
                (true, HeaderControlSet::ModelWithTimer),
                (true, HeaderControlSet::StatusOnly),
                (true, HeaderControlSet::TimerOnly),
                (true, HeaderControlSet::AllSelectors),
                (true, HeaderControlSet::ModelAndEffort),
                (true, HeaderControlSet::ModelOnly),
                (true, HeaderControlSet::None),
            ]
        } else {
            &[
                (false, HeaderControlSet::AllWithStatus),
                (true, HeaderControlSet::AllWithStatus),
                (true, HeaderControlSet::AllSelectors),
                (true, HeaderControlSet::ModelAndEffort),
                (true, HeaderControlSet::ModelOnly),
                (true, HeaderControlSet::None),
            ]
        };
        for (compact_brand, controls) in control_sets.iter().copied() {
            let controls_width = match controls {
                HeaderControlSet::AllWithStatus => model_width
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(effort_width)
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(service_tier_width)
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(status_width),
                HeaderControlSet::ModelWithStatus => model_width
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(status_width),
                HeaderControlSet::ModelWithTimer => model_width
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(timer_width),
                HeaderControlSet::StatusOnly => status_width,
                HeaderControlSet::TimerOnly => timer_width,
                HeaderControlSet::AllSelectors => model_width
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(effort_width)
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(service_tier_width),
                HeaderControlSet::ModelAndEffort => model_width
                    .saturating_add(CONTROL_GAP)
                    .saturating_add(effort_width),
                HeaderControlSet::ModelOnly => model_width,
                HeaderControlSet::None => 0,
            };
            let brand_width = u16::try_from(line_width(&self.brand_line(compact_brand))).ok()?;
            let controls_and_gap = if controls_width == 0 {
                0
            } else {
                BRAND_CONTROL_GAP.saturating_add(controls_width)
            };
            let required_width = dashboard_width
                .saturating_add(DASHBOARD_BRAND_GAP)
                .saturating_add(brand_width)
                .saturating_add(controls_and_gap);
            if required_width > content.width {
                continue;
            }
            let controls_x = content.right().saturating_sub(controls_width);
            let shows_model = matches!(
                controls,
                HeaderControlSet::AllWithStatus
                    | HeaderControlSet::ModelWithStatus
                    | HeaderControlSet::ModelWithTimer
                    | HeaderControlSet::AllSelectors
                    | HeaderControlSet::ModelAndEffort
                    | HeaderControlSet::ModelOnly
            );
            let shows_effort = matches!(
                controls,
                HeaderControlSet::AllWithStatus
                    | HeaderControlSet::AllSelectors
                    | HeaderControlSet::ModelAndEffort
            );
            let shows_service_tier = matches!(
                controls,
                HeaderControlSet::AllWithStatus | HeaderControlSet::AllSelectors
            );
            let status_display = match controls {
                HeaderControlSet::AllWithStatus
                | HeaderControlSet::ModelWithStatus
                | HeaderControlSet::StatusOnly => HeaderStatusDisplay::Full,
                HeaderControlSet::ModelWithTimer | HeaderControlSet::TimerOnly => {
                    HeaderStatusDisplay::TimerOnly
                }
                HeaderControlSet::AllSelectors
                | HeaderControlSet::ModelAndEffort
                | HeaderControlSet::ModelOnly
                | HeaderControlSet::None => HeaderStatusDisplay::None,
            };
            let mut next_control_x = controls_x;
            let mut place_control = |visible: bool, width: u16| {
                visible.then(|| {
                    let rect = Rect::new(next_control_x, content.y, width, 1);
                    next_control_x = next_control_x
                        .saturating_add(width)
                        .saturating_add(CONTROL_GAP);
                    rect
                })
            };
            let model = place_control(shows_model, model_width);
            let effort = place_control(shows_effort, effort_width);
            let service_tier = place_control(shows_service_tier, service_tier_width);
            let status = place_control(
                status_display != HeaderStatusDisplay::None,
                if status_display == HeaderStatusDisplay::TimerOnly {
                    timer_width
                } else {
                    status_width
                },
            );
            return Some(HeaderControlLayout {
                dashboard: Rect::new(content.x, content.y, dashboard_width, 1),
                brand: Rect::new(
                    content
                        .x
                        .saturating_add(dashboard_width)
                        .saturating_add(DASHBOARD_BRAND_GAP),
                    content.y,
                    brand_width,
                    1,
                ),
                model,
                effort,
                service_tier,
                status,
                status_display,
                compact_brand,
            });
        }
        None
    }

    fn dashboard_button(&self, hovered: Option<HeaderControl>) -> Line<'static> {
        let hovered = hovered == Some(HeaderControl::Dashboard);
        let background = if hovered {
            palette::border()
        } else if self.dashboard_visible {
            palette::focus()
        } else {
            palette::elevated()
        };
        let label = self.dashboard_label();
        let label = if self.dashboard_visible {
            label
                .fg(if hovered {
                    palette::text()
                } else {
                    palette::dark()
                })
                .bold()
        } else {
            label.fg(palette::cyan())
        };
        Line::from(label).style(Style::new().bg(background))
    }

    fn brand_line(&self, compact: bool) -> Line<'static> {
        if compact {
            return Line::from(vec![
                "◆".fg(palette::purple()).bold(),
                " BC".fg(palette::text()).bold(),
            ]);
        }
        let workspace = std::path::Path::new(self.cwd)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(self.cwd);
        Line::from(vec![
            "◆".fg(palette::purple()).bold(),
            " BETTER CODEX".fg(palette::text()).bold(),
            "  ".into(),
            truncate_text(workspace, /*max_graphemes*/ 18).fg(palette::muted()),
        ])
    }

    fn model_label(&self) -> String {
        format!(" {} ▾ ", truncate_text(self.model, /*max_graphemes*/ 18))
    }

    fn dashboard_label(&self) -> String {
        " Dashboard ".to_string()
    }

    fn effort_label(&self) -> String {
        format!(
            " {} ▾ ",
            truncate_text(self.reasoning_effort, /*max_graphemes*/ 12)
        )
    }

    fn service_tier_label(&self) -> String {
        format!(
            " {} ▾ ",
            truncate_text(self.service_tier, /*max_graphemes*/ 12)
        )
    }

    fn status_line(&self) -> Line<'static> {
        let color = match self.status {
            "ready" => palette::success(),
            "failed" | "error" | "disconnected" => palette::error(),
            "interrupted" => palette::warning(),
            _ => palette::cyan(),
        };
        let indicator = self
            .status_spinner_frame
            .map(|frame| STATUS_SPINNER_FRAMES[frame % STATUS_SPINNER_FRAMES.len()])
            .unwrap_or("●");
        let mut spans = vec![
            format!("{indicator} ").fg(color),
            self.status.to_string().fg(palette::text()).bold(),
        ];
        if let Some(seconds) = self.turn_elapsed_seconds {
            spans.extend([
                " · turn ".fg(palette::muted()),
                format_turn_elapsed(seconds).fg(palette::text()).bold(),
            ]);
        }
        Line::from(spans)
    }

    fn turn_timer_line(&self) -> Line<'static> {
        Line::from(vec![
            "turn ".fg(palette::muted()),
            format_turn_elapsed(self.turn_elapsed_seconds.unwrap_or_default())
                .fg(palette::text())
                .bold(),
        ])
    }
}

fn format_turn_elapsed(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn control_background(hovered: Option<HeaderControl>, control: HeaderControl) -> Color {
    if hovered == Some(control) {
        palette::border()
    } else {
        palette::elevated()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HeaderControlLayout {
    dashboard: Rect,
    brand: Rect,
    model: Option<Rect>,
    effort: Option<Rect>,
    service_tier: Option<Rect>,
    status: Option<Rect>,
    status_display: HeaderStatusDisplay,
    compact_brand: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderStatusDisplay {
    None,
    Full,
    TimerOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderControlSet {
    AllWithStatus,
    ModelWithStatus,
    ModelWithTimer,
    StatusOnly,
    TimerOnly,
    AllSelectors,
    ModelAndEffort,
    ModelOnly,
    None,
}

fn text_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

#[cfg(test)]
#[path = "header_tests.rs"]
mod tests;
