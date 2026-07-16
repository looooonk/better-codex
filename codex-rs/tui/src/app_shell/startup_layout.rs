use ratatui::layout::Rect;

const MAIN_MIN_WIDTH: u16 = 56;
const SIDEBAR_MIN_WIDTH: u16 = 24;
const SIDEBAR_MAX_WIDTH: u16 = 36;
const SIDEBAR_WIDTH_PERCENT: u16 = 30;

pub(super) const STARTUP_FOOTER_HEIGHT: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StartupPanes {
    pub(super) main: Rect,
    pub(super) sidebar: Option<Rect>,
}

pub(super) fn startup_panes(area: Rect) -> StartupPanes {
    if area.width < MAIN_MIN_WIDTH.saturating_add(SIDEBAR_MIN_WIDTH) {
        return StartupPanes {
            main: area,
            sidebar: None,
        };
    }

    let sidebar_width = u32::from(area.width)
        .saturating_mul(u32::from(SIDEBAR_WIDTH_PERCENT))
        .div_ceil(100)
        .try_into()
        .unwrap_or(u16::MAX)
        .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
    let main_width = area.width.saturating_sub(sidebar_width);
    StartupPanes {
        main: Rect::new(area.x, area.y, main_width, area.height),
        sidebar: Some(Rect::new(
            area.x.saturating_add(main_width),
            area.y,
            sidebar_width,
            area.height,
        )),
    }
}
