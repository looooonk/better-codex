use super::SettingsPage;
use crate::app_shell::design::palette;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;

const EXTRA_WIDTH_ORDER: [usize; SettingsPage::ALL.len()] = [1, 2, 0, 3];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SettingsTabCell {
    page: SettingsPage,
    start: usize,
    width: usize,
}

pub(super) struct SettingsTabs {
    width: usize,
    cells: [SettingsTabCell; SettingsPage::ALL.len()],
}

impl SettingsTabs {
    pub(super) fn new(width: usize) -> Self {
        let label_widths = SettingsPage::ALL.map(|page| page.label().chars().count());
        let minimum_width = label_widths
            .iter()
            .copied()
            .sum::<usize>()
            .saturating_add(SettingsPage::ALL.len().saturating_sub(1));
        let extra_width = width.saturating_sub(minimum_width);
        let base_extra_width = extra_width / SettingsPage::ALL.len();
        let extra_width_remainder = extra_width % SettingsPage::ALL.len();
        let use_label_widths = width >= minimum_width;
        let mut start = 0;
        let cells = std::array::from_fn(|index| {
            let page = SettingsPage::ALL[index];
            let cell_width = if use_label_widths {
                // Give middle tabs the first spare cells so their labels keep breathing room on
                // both sides of the separators in the narrow dashboard pane.
                let receives_remainder =
                    EXTRA_WIDTH_ORDER[..extra_width_remainder].contains(&index);
                label_widths[index]
                    + usize::from(index + 1 < SettingsPage::ALL.len())
                    + base_extra_width
                    + usize::from(receives_remainder)
            } else {
                width / SettingsPage::ALL.len()
                    + usize::from(index < width % SettingsPage::ALL.len())
            };
            let cell = SettingsTabCell {
                page,
                start,
                width: cell_width,
            };
            start = start.saturating_add(cell_width);
            cell
        });
        Self { width, cells }
    }

    pub(super) fn lines(self, active_page: SettingsPage) -> [Line<'static>; 2] {
        let mut labels = Vec::new();
        let mut underline = Vec::new();
        for (index, cell) in self.cells.into_iter().enumerate() {
            if cell.width == 0 {
                continue;
            }

            let has_separator = index + 1 < SettingsPage::ALL.len() && cell.width > 1;
            let label_width = cell.width.saturating_sub(usize::from(has_separator));
            let label = crate::text_formatting::truncate_text(cell.page.label(), label_width);
            let label_padding = label_width.saturating_sub(label.chars().count());
            let label = if label_padding == 1 && index == 0 {
                format!("{label:<label_width$}")
            } else if label_padding == 1 {
                format!("{label:>label_width$}")
            } else {
                format!("{label:^label_width$}")
            };
            let active = cell.page == active_page;
            labels.push(if active {
                label.fg(palette::FOCUS).bg(palette::ELEVATED).bold()
            } else {
                label.fg(palette::MUTED).bg(palette::SURFACE)
            });
            if has_separator {
                labels.push("│".fg(palette::BORDER).bg(palette::SURFACE));
            }

            let rule = if active { "━" } else { "─" };
            let color = if active {
                palette::FOCUS
            } else {
                palette::BORDER
            };
            underline.push(
                rule.repeat(cell.width)
                    .set_style(Style::new().fg(color).bg(palette::SURFACE)),
            );
        }

        [
            Line::from(labels).bg(palette::SURFACE),
            Line::from(underline).bg(palette::SURFACE),
        ]
    }

    pub(super) fn page_at(self, column: usize) -> Option<SettingsPage> {
        if column >= self.width {
            return None;
        }
        self.cells
            .into_iter()
            .rev()
            .find(|cell| cell.width > 0 && column >= cell.start)
            .map(|cell| cell.page)
    }
}
