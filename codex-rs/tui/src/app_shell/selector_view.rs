use super::super::design::fill_rect;
use super::super::design::palette;
use super::super::design::pane_style;
use super::SelectorOption;
use super::SelectorState;
use crate::render::line_utils::prefix_lines;
use ratatui::buffer::Buffer;
use ratatui::layout::Position;
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
pub(super) const MAX_MODAL_WIDTH: u16 = 68;
pub(super) const MAX_MODAL_HEIGHT: u16 = 24;
const OPTION_PREFIX_WIDTH: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SelectorGeometry {
    pub(super) modal: Rect,
    pub(super) options: Rect,
    pub(super) footer: Rect,
    pub(super) rows: Vec<(usize, Rect)>,
}

impl<T> SelectorState<T> {
    pub(in super::super) fn render(&self, area: Rect, pointer: Option<Position>, buf: &mut Buffer) {
        let geometry = selector_geometry(area, &self.options, self.selected);
        let hovered = pointer.and_then(|position| self.option_at(area, position));
        buf.set_style(area, Style::new().add_modifier(Modifier::DIM));
        Clear.render(geometry.modal, buf);
        fill_rect(buf, geometry.modal, palette::surface());

        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::focus()))
            .style(pane_style(palette::surface()))
            .title(Line::from(format!(" {} ", self.title)).bold())
            .render(geometry.modal, buf);

        let scroll = geometry.rows.first().map_or(0, |(index, _)| *index);
        for &(index, area) in &geometry.rows {
            self.render_option(
                area,
                index,
                &self.options[index],
                hovered == Some(index),
                buf,
            );
        }
        Paragraph::new(selector_footer(
            self.selected,
            self.options.len(),
            scroll,
            geometry.rows.len(),
            geometry.footer.width,
            self.key_hints.as_deref(),
        ))
        .style(pane_style(palette::surface()))
        .render(geometry.footer, buf);
    }

    fn render_option(
        &self,
        area: Rect,
        index: usize,
        option: &SelectorOption<T>,
        hovered: bool,
        buf: &mut Buffer,
    ) {
        let selected = index == self.selected;
        let background = if hovered {
            palette::border()
        } else if selected {
            palette::elevated()
        } else {
            palette::surface()
        };
        fill_rect(buf, area, background);
        let current_label = if option.current { "  current" } else { "" };
        let label_width = usize::from(area.width)
            .saturating_sub(OPTION_PREFIX_WIDTH + current_label.chars().count())
            .max(1);
        let label = first_wrapped_line(&option.label, label_width);
        let pointer = if selected {
            "›".fg(palette::focus()).bold()
        } else {
            " ".into()
        };
        let shortcut = if index < 9 {
            (index + 1).to_string().fg(palette::muted())
        } else {
            "·".fg(palette::muted())
        };
        let current = if option.current {
            "●".fg(palette::success())
        } else {
            "○".fg(palette::muted())
        };
        let label = if selected {
            label.fg(palette::text()).bold()
        } else {
            label.fg(palette::text())
        };
        let current_label = if option.current {
            current_label.fg(palette::success())
        } else {
            "".into()
        };
        let mut lines = vec![Line::from(vec![
            pointer,
            " ".into(),
            shortcut,
            " ".into(),
            current,
            " ".into(),
            label,
            current_label,
        ])];
        let indent = " ".repeat(OPTION_PREFIX_WIDTH).into();
        lines.extend(prefix_lines(
            wrapped_detail(&option.detail, area.width)
                .into_iter()
                .map(|line| Line::from(line.into_owned()).fg(palette::muted()))
                .collect(),
            indent,
            " ".repeat(OPTION_PREFIX_WIDTH).into(),
        ));
        Paragraph::new(lines)
            .style(pane_style(background))
            .render(area, buf);
    }
}

fn selector_footer(
    selected: usize,
    option_count: usize,
    scroll: usize,
    visible_options: usize,
    width: u16,
    key_hints: Option<&str>,
) -> Line<'static> {
    let before = if scroll > 0 { "↑ " } else { "" };
    let after = if scroll.saturating_add(visible_options) < option_count {
        " ↓"
    } else {
        ""
    };
    let position = format!(
        "{before}{}/{}{after}",
        selected.saturating_add(1).min(option_count),
        option_count
    );
    let width = usize::from(width);
    let hint = key_hints.unwrap_or_else(|| {
        [
            "wheel / j k  Enter select  Esc cancel",
            "wheel / j k  Enter  Esc",
            "j/k  Enter  Esc",
            "↑↓  ↵",
            "",
        ]
        .into_iter()
        .find(|hint| {
            let spacing = usize::from(!hint.is_empty()) * 3;
            hint.chars().count() + spacing + position.chars().count() <= width
        })
        .unwrap_or_default()
    });
    let hint = if hint.is_empty() {
        String::new()
    } else {
        format!(" {hint}  ")
    };
    Line::from(vec![
        hint.fg(palette::muted()),
        position.fg(palette::purple()).bold(),
    ])
}

pub(super) fn selector_geometry<T>(
    area: Rect,
    choices: &[SelectorOption<T>],
    selected: usize,
) -> SelectorGeometry {
    let available_width = area.width.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let width = available_width.min(MAX_MODAL_WIDTH);
    let content_width = width.saturating_sub(2);
    let content_width = content_width.saturating_sub(u16::from(content_width > 2) * 2);
    let heights = choices
        .iter()
        .map(|option| {
            u16::try_from(wrapped_detail(&option.detail, content_width).len())
                .unwrap_or(u16::MAX)
                .saturating_add(1)
        })
        .collect::<Vec<_>>();
    let desired_height = heights
        .iter()
        .fold(3u16, |sum, height| sum.saturating_add(*height));
    let selected_height = heights
        .get(selected)
        .copied()
        .unwrap_or_default()
        .saturating_add(3);
    let available_height = area
        .height
        .saturating_sub(MODAL_MARGIN.saturating_mul(2))
        .max(selected_height.min(area.height));
    let height = desired_height
        .min(MAX_MODAL_HEIGHT.max(selected_height))
        .min(available_height)
        .max(available_height.min(5));
    let modal = Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
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
    let footer_height = u16::from(content.height > 2);
    let option_height = content.height.saturating_sub(footer_height);
    let options = Rect::new(content.x, content.y, content.width, option_height);
    let footer = Rect::new(
        content.x,
        content.y.saturating_add(option_height),
        content.width,
        footer_height,
    );
    let mut scroll = selected.min(choices.len().saturating_sub(1));
    let mut used_height = heights.get(scroll).copied().unwrap_or_default();
    while scroll > 0 && used_height.saturating_add(heights[scroll - 1]) <= options.height {
        scroll -= 1;
        used_height = used_height.saturating_add(heights[scroll]);
    }
    let mut rows = Vec::new();
    let mut y = options.y;
    for (index, &height) in heights.iter().enumerate().skip(scroll) {
        let remaining = options.bottom().saturating_sub(y);
        if remaining == 0 || (height > remaining && !rows.is_empty()) {
            break;
        }
        let height = height.min(remaining);
        rows.push((index, Rect::new(options.x, y, options.width, height)));
        y = y.saturating_add(height);
    }
    SelectorGeometry {
        modal,
        options,
        footer,
        rows,
    }
}

fn wrapped_detail(text: &str, width: u16) -> Vec<std::borrow::Cow<'_, str>> {
    textwrap::wrap(
        text,
        usize::from(width)
            .saturating_sub(OPTION_PREFIX_WIDTH)
            .max(1),
    )
}

fn first_wrapped_line(text: &str, width: usize) -> String {
    textwrap::wrap(text, textwrap::Options::new(width.max(1)))
        .first()
        .map_or_else(String::new, std::string::ToString::to_string)
}
