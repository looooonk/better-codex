use super::design::palette;
use super::design::tab_span;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use serde::Deserialize;
use serde::Serialize;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;

const STATE_FILE: &str = "app-shell-state.json";
const COMPACT_TAB_WIDTH: u16 = 48;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum DashboardRoute {
    #[default]
    Settings,
    Agents,
    Workspace,
    Sessions,
    Help,
}

impl DashboardRoute {
    pub(super) const ALL: [Self; 5] = [
        Self::Settings,
        Self::Agents,
        Self::Workspace,
        Self::Sessions,
        Self::Help,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Agents => "Agents",
            Self::Workspace => "Workspace",
            Self::Settings => "Settings",
            Self::Help => "Help",
        }
    }

    fn compact_label(self) -> &'static str {
        match self {
            Self::Sessions => "Threads",
            Self::Agents => "Agents",
            Self::Workspace => "Files",
            Self::Settings => "Config",
            Self::Help => "Help",
        }
    }

    pub(super) fn previous(self) -> Self {
        let index = route_index(self);
        Self::ALL[index.saturating_sub(1)]
    }

    pub(super) fn next(self) -> Self {
        let index = route_index(self);
        Self::ALL[index.saturating_add(1).min(Self::ALL.len() - 1)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DashboardTabCell {
    route: DashboardRoute,
    start: u16,
    width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DashboardTabs {
    width: u16,
    cells: [DashboardTabCell; DashboardRoute::ALL.len()],
}

impl DashboardTabs {
    pub(super) fn new(width: u16) -> Self {
        let tab_count = u16::try_from(DashboardRoute::ALL.len()).unwrap_or(u16::MAX);
        let label_widths = DashboardRoute::ALL
            .map(|route| u16::try_from(route.label().chars().count()).unwrap_or(u16::MAX));
        let separator_width = tab_count.saturating_sub(1);
        let minimum_width = label_widths
            .iter()
            .copied()
            .sum::<u16>()
            .saturating_add(separator_width);
        let extra_width = width.saturating_sub(minimum_width);
        let base_extra_width = extra_width / tab_count;
        let extra_width_remainder = extra_width % tab_count;
        let use_label_widths = width >= minimum_width.max(COMPACT_TAB_WIDTH);
        let mut start = 0;
        let cells = std::array::from_fn(|index| {
            let route = DashboardRoute::ALL[index];
            let cell_width = if use_label_widths {
                label_widths[index]
                    + u16::from(index + 1 < DashboardRoute::ALL.len())
                    + base_extra_width
                    + u16::from(index < usize::from(extra_width_remainder))
            } else {
                width / tab_count + u16::from(index < usize::from(width % tab_count))
            };
            let cell = DashboardTabCell {
                route,
                start,
                width: cell_width,
            };
            start = start.saturating_add(cell_width);
            cell
        });
        Self { width, cells }
    }

    pub(super) fn lines(self, active_route: DashboardRoute) -> [Line<'static>; 2] {
        let compact = self.width < COMPACT_TAB_WIDTH;
        let mut labels = Vec::new();
        let mut underline = Vec::new();
        for (index, cell) in self.cells.into_iter().enumerate() {
            if cell.width > 0 {
                let separator_width = cell.separator_width();
                let has_separator = separator_width > 0;
                let label_width = cell.content_width();
                let label = if compact {
                    cell.route.compact_label()
                } else {
                    cell.route.label()
                };
                let label = crate::text_formatting::truncate_text(label, usize::from(label_width));
                let label_padding = usize::from(label_width).saturating_sub(label.chars().count());
                let label = if index + 1 == DashboardRoute::ALL.len() && label_padding == 1 {
                    // Ratatui's surrounding pane already supplies the trailing outer margin. Put
                    // a single odd padding cell before the final label so it does not run into
                    // the preceding separator at the narrow edge of the wide-tab layout.
                    format!("{label:>width$}", width = usize::from(label_width))
                } else {
                    format!("{label:^width$}", width = usize::from(label_width))
                };
                labels.push(tab_span(label, cell.route == active_route));
                if has_separator {
                    labels.push("│".fg(palette::BORDER).bg(palette::DARK));
                }
                let rule = if cell.route == active_route {
                    "━"
                        .repeat(usize::from(label_width))
                        .set_style(Style::new().fg(palette::FOCUS).bg(palette::DARK).bold())
                } else {
                    "─"
                        .repeat(usize::from(label_width))
                        .set_style(Style::new().fg(palette::BORDER).bg(palette::DARK))
                };
                underline.push(rule);
                if has_separator {
                    underline.push("─".fg(palette::BORDER).bg(palette::DARK));
                }
            }
        }
        [
            Line::from(labels).bg(palette::DARK),
            Line::from(underline).bg(palette::DARK),
        ]
    }

    pub(super) fn route_at(self, column: u16) -> Option<DashboardRoute> {
        if column >= self.width {
            return None;
        }
        self.cells
            .into_iter()
            .rev()
            .find(|cell| {
                cell.content_width() > 0
                    && (cell.start..cell.start.saturating_add(cell.content_width()))
                        .contains(&column)
            })
            .map(|cell| cell.route)
    }

    pub(super) fn column_range(self, route: DashboardRoute) -> Option<Range<u16>> {
        self.cells
            .into_iter()
            .find(|cell| cell.route == route && cell.width > 0)
            .map(|cell| cell.start..cell.start.saturating_add(cell.content_width()))
    }
}

impl DashboardTabCell {
    fn separator_width(self) -> u16 {
        u16::from(self.route != DashboardRoute::Help && self.width > 1)
    }

    fn content_width(self) -> u16 {
        self.width.saturating_sub(self.separator_width())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AppShellRouteState {
    pub(super) route: DashboardRoute,
}

impl AppShellRouteState {
    pub(super) fn load(codex_home: &Path) -> Self {
        let path = state_path(codex_home);
        let Ok(bytes) = std::fs::read(path) else {
            return Self::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub(super) fn save(&self, codex_home: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(codex_home)?;
        let bytes = serde_json::to_vec_pretty(self)?;
        std::fs::write(state_path(codex_home), bytes)
    }
}

fn route_index(route: DashboardRoute) -> usize {
    DashboardRoute::ALL
        .iter()
        .position(|candidate| *candidate == route)
        .unwrap_or(0)
}

fn state_path(codex_home: &Path) -> PathBuf {
    codex_home.join(STATE_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn route_state_round_trips_through_codex_home() {
        let temp = tempfile::tempdir().expect("create temp codex home");
        let state = AppShellRouteState {
            route: DashboardRoute::Settings,
        };

        state.save(temp.path()).expect("save route state");

        assert_eq!(AppShellRouteState::load(temp.path()), state);
    }

    #[test]
    fn invalid_route_state_falls_back_to_settings() {
        let temp = tempfile::tempdir().expect("create temp codex home");
        std::fs::write(state_path(temp.path()), b"{\"route\":\"missing\"}")
            .expect("write invalid route state");

        assert_eq!(
            AppShellRouteState::load(temp.path()),
            AppShellRouteState::default()
        );
    }

    #[test]
    fn dashboard_tabs_fill_width_and_leave_delimiters_outside_hit_areas() {
        let tabs = DashboardTabs::new(/*width*/ 34);
        let rendered = tabs.lines(DashboardRoute::Sessions).map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        });

        assert_eq!(
            rendered
                .iter()
                .map(|line| line.chars().count())
                .collect::<Vec<_>>(),
            vec![34; 2]
        );
        let expected_routes = [
            (Some(DashboardRoute::Settings), 6),
            (None, 1),
            (Some(DashboardRoute::Agents), 6),
            (None, 1),
            (Some(DashboardRoute::Workspace), 6),
            (None, 1),
            (Some(DashboardRoute::Sessions), 6),
            (None, 1),
            (Some(DashboardRoute::Help), 6),
        ]
        .into_iter()
        .flat_map(|(route, width)| std::iter::repeat_n(route, width))
        .collect::<Vec<_>>();
        assert_eq!(
            (0..34)
                .map(|column| tabs.route_at(column))
                .collect::<Vec<_>>(),
            expected_routes
        );
    }

    #[test]
    fn dashboard_tab_ranges_exclude_their_trailing_delimiters() {
        let tabs = DashboardTabs::new(COMPACT_TAB_WIDTH);

        assert_eq!(
            DashboardRoute::ALL.map(|route| tabs.column_range(route)),
            [
                Some(0..10),
                Some(11..19),
                Some(20..31),
                Some(32..42),
                Some(43..48),
            ]
        );
    }

    #[test]
    fn dashboard_tabs_highlight_only_the_active_route() {
        insta::assert_debug_snapshot!(
            DashboardTabs::new(/*width*/ 46).lines(DashboardRoute::Workspace)
        );
    }

    #[test]
    fn final_wide_tab_keeps_space_after_the_separator() {
        let [labels, _underline] =
            DashboardTabs::new(COMPACT_TAB_WIDTH).lines(DashboardRoute::Sessions);
        let line = labels
            .spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>();

        assert_eq!(line, " Settings │ Agents │ Workspace │ Sessions │ Help");
    }
}
