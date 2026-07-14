use super::design::tab_span;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

const STATE_FILE: &str = "app-shell-state.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum DashboardRoute {
    #[default]
    Sessions,
    Workspace,
    Settings,
    Help,
}

impl DashboardRoute {
    pub(super) const ALL: [Self; 4] = [Self::Sessions, Self::Workspace, Self::Settings, Self::Help];

    pub(super) fn short_label(self) -> &'static str {
        match self {
            Self::Sessions => "S",
            Self::Workspace => "W",
            Self::Settings => "Set",
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
        let separator_count = u16::try_from(DashboardRoute::ALL.len().saturating_sub(1))
            .unwrap_or(u16::MAX)
            .min(width);
        let available_width = width.saturating_sub(separator_count);
        let tab_count = u16::try_from(DashboardRoute::ALL.len()).unwrap_or(u16::MAX);
        let base_width = available_width / tab_count;
        let remainder = available_width % tab_count;
        let mut start = 0;
        let cells = std::array::from_fn(|index| {
            let route = DashboardRoute::ALL[index];
            let cell_width = base_width + u16::from(index < usize::from(remainder));
            let cell = DashboardTabCell {
                route,
                start,
                width: cell_width,
            };
            start = start.saturating_add(cell_width);
            if index + 1 < DashboardRoute::ALL.len() && start < width {
                start = start.saturating_add(1);
            }
            cell
        });
        Self { width, cells }
    }

    pub(super) fn line(self, active_route: DashboardRoute) -> Line<'static> {
        let mut spans = Vec::new();
        for (index, cell) in self.cells.into_iter().enumerate() {
            if cell.width > 0 {
                let label = format!("{}{}", index + 1, cell.route.short_label());
                let label = crate::text_formatting::truncate_text(&label, usize::from(cell.width));
                spans.push(tab_span(
                    format!("{label:^width$}", width = usize::from(cell.width)),
                    cell.route == active_route,
                ));
            }
            if index + 1 < self.cells.len() && cell.start.saturating_add(cell.width) < self.width {
                spans.push(Span::from("|").dim());
            }
        }
        Line::from(spans)
    }

    pub(super) fn route_at(self, column: u16) -> Option<DashboardRoute> {
        self.cells
            .into_iter()
            .find(|cell| column >= cell.start && column < cell.start.saturating_add(cell.width))
            .map(|cell| cell.route)
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
    fn invalid_route_state_falls_back_to_sessions() {
        let temp = tempfile::tempdir().expect("create temp codex home");
        std::fs::write(state_path(temp.path()), b"{\"route\":\"missing\"}")
            .expect("write invalid route state");

        assert_eq!(
            AppShellRouteState::load(temp.path()),
            AppShellRouteState::default()
        );
    }

    #[test]
    fn dashboard_tabs_fill_width_and_leave_separators_unclickable() {
        let tabs = DashboardTabs::new(/*width*/ 28);
        let rendered = tabs
            .line(DashboardRoute::Sessions)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(rendered.chars().count(), 28);
        assert_eq!(rendered.matches('|').count(), 3);
        assert_eq!(
            (0..28)
                .map(|column| tabs.route_at(column))
                .collect::<Vec<_>>(),
            vec![
                Some(DashboardRoute::Sessions),
                Some(DashboardRoute::Sessions),
                Some(DashboardRoute::Sessions),
                Some(DashboardRoute::Sessions),
                Some(DashboardRoute::Sessions),
                Some(DashboardRoute::Sessions),
                Some(DashboardRoute::Sessions),
                None,
                Some(DashboardRoute::Workspace),
                Some(DashboardRoute::Workspace),
                Some(DashboardRoute::Workspace),
                Some(DashboardRoute::Workspace),
                Some(DashboardRoute::Workspace),
                Some(DashboardRoute::Workspace),
                None,
                Some(DashboardRoute::Settings),
                Some(DashboardRoute::Settings),
                Some(DashboardRoute::Settings),
                Some(DashboardRoute::Settings),
                Some(DashboardRoute::Settings),
                Some(DashboardRoute::Settings),
                None,
                Some(DashboardRoute::Help),
                Some(DashboardRoute::Help),
                Some(DashboardRoute::Help),
                Some(DashboardRoute::Help),
                Some(DashboardRoute::Help),
                Some(DashboardRoute::Help),
            ]
        );
    }
}
