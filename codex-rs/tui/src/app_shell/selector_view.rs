use super::super::design::fill_rect;
use super::super::design::palette;
use super::super::design::pane_style;
use super::SelectorOption;
use super::SelectorState;
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
pub(super) const OPTION_HEIGHT: u16 = 2;
const OPTION_PREFIX_WIDTH: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SelectorGeometry {
    pub(super) modal: Rect,
    pub(super) options: Rect,
    pub(super) footer: Rect,
    pub(super) visible_options: usize,
}

impl<T> SelectorState<T> {
    pub(in super::super) fn render(&self, area: Rect, pointer: Option<Position>, buf: &mut Buffer) {
        let geometry = selector_geometry(area, self.options.len());
        let hovered = pointer.and_then(|position| self.option_at(area, position));
        buf.set_style(area, Style::new().add_modifier(Modifier::DIM));
        Clear.render(geometry.modal, buf);
        fill_rect(buf, geometry.modal, palette::SURFACE);

        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(palette::FOCUS))
            .style(pane_style(palette::SURFACE))
            .title(Line::from(format!(" {} ", self.title)).bold())
            .render(geometry.modal, buf);

        let scroll = self.visible_scroll(geometry.visible_options);
        for (visible_index, option) in self
            .options
            .iter()
            .skip(scroll)
            .take(geometry.visible_options)
            .enumerate()
        {
            let index = scroll + visible_index;
            let area = Rect::new(
                geometry.options.x,
                geometry.options.y.saturating_add(
                    u16::try_from(visible_index).unwrap_or(u16::MAX) * OPTION_HEIGHT,
                ),
                geometry.options.width,
                OPTION_HEIGHT,
            );
            self.render_option(area, index, option, hovered == Some(index), buf);
        }
        Paragraph::new(selector_footer(
            self.selected,
            self.options.len(),
            scroll,
            geometry.visible_options,
            geometry.footer.width,
        ))
        .style(pane_style(palette::SURFACE))
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
            palette::BORDER
        } else if selected {
            palette::ELEVATED
        } else {
            palette::SURFACE
        };
        fill_rect(buf, area, background);
        let current_label = if option.current { "  current" } else { "" };
        let label_width = usize::from(area.width)
            .saturating_sub(OPTION_PREFIX_WIDTH + current_label.chars().count())
            .max(1);
        let label = first_wrapped_line(&option.label, label_width);
        let detail_width = usize::from(area.width)
            .saturating_sub(OPTION_PREFIX_WIDTH)
            .max(1);
        let detail = first_wrapped_line(&option.detail, detail_width);
        let pointer = if selected {
            "›".fg(palette::FOCUS).bold()
        } else {
            " ".into()
        };
        let shortcut = if index < 9 {
            (index + 1).to_string().fg(palette::MUTED)
        } else {
            "·".fg(palette::MUTED)
        };
        let current = if option.current {
            "●".fg(palette::SUCCESS)
        } else {
            "○".fg(palette::MUTED)
        };
        let label = if selected {
            label.fg(palette::TEXT).bold()
        } else {
            label.fg(palette::TEXT)
        };
        let current_label = if option.current {
            current_label.fg(palette::SUCCESS)
        } else {
            "".into()
        };
        let lines = vec![
            Line::from(vec![
                pointer,
                " ".into(),
                shortcut,
                " ".into(),
                current,
                " ".into(),
                label,
                current_label,
            ]),
            Line::from(vec![
                " ".repeat(OPTION_PREFIX_WIDTH).into(),
                detail.fg(palette::MUTED),
            ]),
        ];
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
    let hint = [
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
    .unwrap_or_default();
    let hint = if hint.is_empty() {
        String::new()
    } else {
        format!(" {hint}  ")
    };
    Line::from(vec![
        hint.fg(palette::MUTED),
        position.fg(palette::PURPLE).bold(),
    ])
}

pub(super) fn selector_geometry(area: Rect, option_count: usize) -> SelectorGeometry {
    let available_width = area.width.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let available_height = area.height.saturating_sub(MODAL_MARGIN.saturating_mul(2));
    let width = available_width.min(MAX_MODAL_WIDTH);
    let option_count = u16::try_from(option_count.max(1)).unwrap_or(u16::MAX);
    let desired_height = option_count.saturating_mul(OPTION_HEIGHT).saturating_add(3);
    let height = desired_height
        .min(MAX_MODAL_HEIGHT)
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
    let footer_height = u16::from(content.height > OPTION_HEIGHT);
    let option_height = content.height.saturating_sub(footer_height);
    let options = Rect::new(content.x, content.y, content.width, option_height);
    let footer = Rect::new(
        content.x,
        content.y.saturating_add(option_height),
        content.width,
        footer_height,
    );
    SelectorGeometry {
        modal,
        options,
        footer,
        visible_options: usize::from(options.height / OPTION_HEIGHT),
    }
}

fn first_wrapped_line(text: &str, width: usize) -> String {
    textwrap::wrap(text, textwrap::Options::new(width.max(1)))
        .first()
        .map_or_else(String::new, |line| line.to_string())
}
