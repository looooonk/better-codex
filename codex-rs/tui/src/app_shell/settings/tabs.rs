use super::SettingsPage;
use crate::app_shell::design::palette;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::ops::Range;

// Grow the outer delimiter gaps before the center one. Each internal gap contributes equally to
// the tabs on both sides of its delimiter, keeping cell widths balanced as space is added.
const EXTRA_GAP_ORDER: [usize; SettingsPage::ALL.len() - 1] = [1, 3, 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SettingsTabCell {
    page: SettingsPage,
    start: usize,
    width: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app_shell) struct SettingsTabs {
    width: usize,
    cells: [SettingsTabCell; SettingsPage::ALL.len()],
}

impl SettingsTabs {
    pub(in crate::app_shell) fn new(width: usize) -> Self {
        let label_widths = SettingsPage::ALL.map(|page| page.label().chars().count());
        let tab_count = SettingsPage::ALL.len();
        let minimum_width = label_widths
            .iter()
            .copied()
            .sum::<usize>()
            .saturating_add(tab_count.saturating_sub(1));
        let extra_width = width.saturating_sub(minimum_width);
        let width_pair_count = tab_count.saturating_mul(2);
        let base_gap_width = extra_width / width_pair_count;
        let extra_width_remainder = extra_width % width_pair_count;
        let mut gap_widths = [base_gap_width; SettingsPage::ALL.len() + 1];
        for gap in EXTRA_GAP_ORDER.into_iter().take(extra_width_remainder / 2) {
            gap_widths[gap] = gap_widths[gap].saturating_add(1);
        }
        gap_widths[0] = gap_widths[0].saturating_add(extra_width_remainder % 2);
        let use_label_widths = width >= minimum_width;
        let mut start = 0;
        let cells = std::array::from_fn(|index| {
            let page = SettingsPage::ALL[index];
            let cell_width = if use_label_widths {
                label_widths[index]
                    + usize::from(index + 1 < tab_count)
                    + gap_widths[index]
                    + gap_widths[index + 1]
            } else {
                width / tab_count + usize::from(index < width % tab_count)
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

    pub(in crate::app_shell) fn lines(self, active_page: SettingsPage) -> [Line<'static>; 2] {
        let mut labels = Vec::new();
        let mut underline = Vec::new();
        for cell in self.cells {
            if cell.width == 0 {
                continue;
            }

            let separator_width = cell.separator_width();
            let has_separator = separator_width > 0;
            let label_width = cell.content_width();
            let label = crate::text_formatting::truncate_text(cell.page.label(), label_width);
            let label_padding = label_width.saturating_sub(label.chars().count());
            let left_padding = label_padding / 2;
            let right_padding = label_padding.saturating_sub(left_padding);
            let label = format!(
                "{}{}{}",
                " ".repeat(left_padding),
                label,
                " ".repeat(right_padding)
            );
            let active = cell.page == active_page;
            labels.push(if active {
                label.fg(palette::focus()).bold()
            } else {
                label.fg(palette::muted())
            });
            if has_separator {
                labels.push("│".fg(palette::border()));
            }

            let rule = if active { "━" } else { "─" };
            let color = if active {
                palette::focus()
            } else {
                palette::border()
            };
            underline.push(rule.repeat(label_width).fg(color));
            if has_separator {
                underline.push("─".fg(palette::border()));
            }
        }

        [Line::from(labels), Line::from(underline)]
    }

    pub(in crate::app_shell) fn page_at(self, column: usize) -> Option<SettingsPage> {
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
            .map(|cell| cell.page)
    }

    pub(in crate::app_shell) fn column_range(self, page: SettingsPage) -> Option<Range<usize>> {
        self.cells
            .into_iter()
            .find(|cell| cell.page == page && cell.width > 0)
            .map(|cell| cell.start..cell.start.saturating_add(cell.content_width()))
    }
}

impl SettingsTabCell {
    fn separator_width(self) -> usize {
        usize::from(self.page != SettingsPage::Integrations && self.width > 1)
    }

    fn content_width(self) -> usize {
        self.width.saturating_sub(self.separator_width())
    }
}
