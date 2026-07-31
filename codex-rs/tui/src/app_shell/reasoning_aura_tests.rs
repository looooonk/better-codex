use super::super::ShellState;
use super::super::render::ShellView;
use super::*;
use crate::app_theme;
use crate::app_theme::ThemePalette;
use crate::app_theme::palette as active_palette;
use crate::tui::FrameRequester;
use codex_config::types::TuiAppTheme;
use pretty_assertions::assert_eq;
use ratatui::layout::Position;
use ratatui::style::Style;
use tokio::sync::broadcast::error::TryRecvError;

#[test]
fn only_transitions_to_max_and_ultra_have_an_aura_tone() {
    let cases = [
        (
            Some(&ReasoningEffort::High),
            Some(&ReasoningEffort::Max),
            Some(ReasoningAuraTone::Max),
        ),
        (
            Some(&ReasoningEffort::Max),
            Some(&ReasoningEffort::Ultra),
            Some(ReasoningAuraTone::Ultra),
        ),
        (
            Some(&ReasoningEffort::Max),
            Some(&ReasoningEffort::Max),
            None,
        ),
        (
            Some(&ReasoningEffort::Ultra),
            Some(&ReasoningEffort::High),
            None,
        ),
        (Some(&ReasoningEffort::High), None, None),
    ];

    assert_eq!(
        cases.map(|(current, target, _expected)| {
            ReasoningAuraTone::for_transition(current, target)
        }),
        cases.map(|(_current, _target, expected)| expected),
    );
}

#[test]
fn aura_expires_at_the_deadline_without_changing_cell_content() {
    let _active_theme = app_theme::activate(TuiAppTheme::TokyoNight);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 7, /*height*/ 5,
    );
    let mut buf = labeled_buffer(area, active_palette());
    let unchanged = buf.clone();
    let now = Instant::now();
    let aura = ReasoningAura::new(ReasoningAuraTone::Max, now);

    assert_eq!(aura.expires_at(), now + AURA_DURATION);
    aura.render(area, &mut buf, aura.expires_at());
    assert_eq!(buf, unchanged);

    aura.render(area, &mut buf, now);
    assert_eq!(buffer_symbols(&buf, area), buffer_symbols(&unchanged, area));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn expiration_deadline_clears_aura_and_requests_a_real_frame() {
    let now = tokio::time::Instant::now().into_std();
    let mut aura = Some(ReasoningAura::new(ReasoningAuraTone::Max, now));
    let deadline = aura
        .as_ref()
        .map(ReasoningAura::expires_at)
        .expect("aura should have a deadline");
    let (draw_tx, mut draw_rx) = tokio::sync::broadcast::channel(1);
    let frame_requester = FrameRequester::new(draw_tx);
    let expiration = wait_for_expiration(Some(deadline));
    tokio::pin!(expiration);

    let initially_expired = tokio::select! {
        biased;
        expired_at = expiration.as_mut() => Some(expired_at),
        _ = tokio::task::yield_now() => None,
    };
    assert_eq!(initially_expired, None);

    tokio::time::advance(Duration::from_millis(/*millis*/ 799)).await;
    let expired_early = tokio::select! {
        biased;
        expired_at = expiration.as_mut() => Some(expired_at),
        _ = tokio::task::yield_now() => None,
    };
    let cleared_early = clear_expired_aura(
        &mut aura,
        tokio::time::Instant::now().into_std(),
        &frame_requester,
    );
    assert_eq!(
        (
            expired_early,
            cleared_early,
            aura.is_some(),
            draw_rx.try_recv(),
        ),
        (None, false, true, Err(TryRecvError::Empty)),
    );

    tokio::time::advance(Duration::from_millis(/*millis*/ 1)).await;
    let expired_at = expiration.as_mut().await;
    let cleared = clear_expired_aura(&mut aura, expired_at, &frame_requester);
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(/*millis*/ 1)).await;

    assert_eq!(
        (expired_at, cleared, aura.is_none(), draw_rx.try_recv()),
        (deadline, true, true, Ok(())),
    );
}

#[test]
fn themed_max_and_ultra_aura_backgrounds_snapshot() {
    let snapshots = [
        (
            "tokyo-night",
            TuiAppTheme::TokyoNight,
            ReasoningAuraTone::Max,
        ),
        (
            "tokyo-night",
            TuiAppTheme::TokyoNight,
            ReasoningAuraTone::Ultra,
        ),
        (
            "gruvbox-dark",
            TuiAppTheme::GruvboxDark,
            ReasoningAuraTone::Max,
        ),
        (
            "gruvbox-dark",
            TuiAppTheme::GruvboxDark,
            ReasoningAuraTone::Ultra,
        ),
        (
            "catppuccin-mocha",
            TuiAppTheme::CatppuccinMocha,
            ReasoningAuraTone::Max,
        ),
        (
            "catppuccin-mocha",
            TuiAppTheme::CatppuccinMocha,
            ReasoningAuraTone::Ultra,
        ),
    ]
    .map(|(theme_name, theme, tone)| {
        let _active_theme = app_theme::activate(theme);
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 7, /*height*/ 5,
        );
        let mut buf = labeled_buffer(area, active_palette());
        let now = Instant::now();
        ReasoningAura::new(tone, now).render(area, &mut buf, now);
        format!("{theme_name} {tone:?}\n{}", buffer_backgrounds(&buf, area))
    })
    .join("\n\n");

    insta::assert_snapshot!("themed_max_and_ultra_aura_backgrounds", snapshots);
}

#[test]
fn shell_render_applies_max_and_ultra_aura_snapshot() {
    let snapshots = [ReasoningAuraTone::Max, ReasoningAuraTone::Ultra]
        .map(|tone| {
            let mut shell = ShellState::snapshot_fixture();
            shell.app_theme = TuiAppTheme::GruvboxDark;
            shell.reasoning_aura = Some(ReasoningAura {
                tone,
                expires_at: Instant::now() + Duration::from_secs(/*secs*/ 60),
            });
            let area = Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 24,
            );
            let mut buf = Buffer::empty(area);
            ShellView { shell: &shell }.render(area, &mut buf);
            format!("{tone:?}\n{}", shell_style_samples(&buf, area))
        })
        .join("\n\n");

    insta::assert_snapshot!("shell_render_applies_max_and_ultra_aura", snapshots);
}

fn labeled_buffer(area: Rect, theme: ThemePalette) -> Buffer {
    let mut buf = Buffer::empty(area);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buf[(x, y)]
                .set_symbol("·")
                .set_style(Style::new().fg(theme.text).bg(theme.base));
        }
    }
    buf
}

fn buffer_symbols(buf: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .filter_map(|x| buf.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn buffer_backgrounds(buf: &Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .filter_map(|x| buf.cell((x, y)))
                .map(|cell| match cell.style().bg {
                    Some(Color::Rgb(red, green, blue)) => {
                        format!("#{red:02x}{green:02x}{blue:02x}")
                    }
                    background => format!("{background:?}"),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn shell_style_samples(buf: &Buffer, area: Rect) -> String {
    [
        (
            "outer header",
            Position::new(area.x + area.width / 2, area.y),
        ),
        (
            "inner header",
            Position::new(area.x + area.width / 2, area.y.saturating_add(1)),
        ),
        (
            "outer left",
            Position::new(area.x, area.y + area.height / 2),
        ),
        (
            "inner left",
            Position::new(area.x.saturating_add(1), area.y + area.height / 2),
        ),
        (
            "center",
            Position::new(area.x + area.width / 2, area.y + area.height / 2),
        ),
        (
            "outer composer",
            Position::new(area.x + area.width / 2, area.bottom().saturating_sub(1)),
        ),
    ]
    .map(|(label, position)| {
        let cell = buf.cell(position).expect("sample should be inside shell");
        format!(
            "{label}: {:?} {}",
            cell.symbol(),
            background_hex(cell.style().bg)
        )
    })
    .join("\n")
}

fn background_hex(background: Option<Color>) -> String {
    match background {
        Some(Color::Rgb(red, green, blue)) => format!("#{red:02x}{green:02x}{blue:02x}"),
        background => format!("{background:?}"),
    }
}
