use super::super::ShellState;
use super::super::render::ShellView;
use super::*;
use crate::app_theme;
use crate::app_theme::ThemePalette;
use crate::app_theme::palette as active_palette;
use codex_config::types::TuiAppTheme;
use itertools::Itertools;
use pretty_assertions::assert_eq;
use ratatui::layout::Position;
use ratatui::style::Style;

#[test]
fn only_transitions_to_max_and_ultra_have_a_ripple_tone() {
    let cases = [
        (
            Some(&ReasoningEffort::High),
            Some(&ReasoningEffort::Max),
            Some(ReasoningRippleTone::Max),
        ),
        (
            Some(&ReasoningEffort::Max),
            Some(&ReasoningEffort::Ultra),
            Some(ReasoningRippleTone::Ultra),
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
            ReasoningRippleTone::for_transition(current, target)
        }),
        cases.map(|(_current, _target, expected)| expected),
    );
}

#[test]
fn ripple_frames_advance_at_the_refresh_cadence_and_stop_at_the_deadline() {
    let now = Instant::now();
    let ripple = ReasoningRipple::new(ReasoningRippleTone::Max, now);
    let initial_frame = Some(ReasoningRippleFrame {
        tone: ReasoningRippleTone::Max,
        progress: 0.0,
    });

    assert_eq!(
        (
            ripple.frame(now),
            ripple.frame(now + FRAME_INTERVAL - Duration::from_millis(/*millis*/ 1)),
            ripple.frame(now + FRAME_INTERVAL),
            ripple.frame(now + RIPPLE_DURATION),
            ripple.is_expired(now + RIPPLE_DURATION),
        ),
        (
            initial_frame,
            initial_frame,
            Some(ReasoningRippleFrame {
                tone: ReasoningRippleTone::Max,
                progress: FRAME_INTERVAL.as_secs_f32() / RIPPLE_DURATION.as_secs_f32(),
            }),
            None,
            true,
        ),
    );
}

#[test]
fn themed_max_and_ultra_ripple_gradients_snapshot() {
    let snapshots = [
        (
            "tokyo-night",
            TuiAppTheme::TokyoNight,
            ReasoningRippleTone::Max,
        ),
        (
            "tokyo-night",
            TuiAppTheme::TokyoNight,
            ReasoningRippleTone::Ultra,
        ),
        (
            "gruvbox-dark",
            TuiAppTheme::GruvboxDark,
            ReasoningRippleTone::Max,
        ),
        (
            "gruvbox-dark",
            TuiAppTheme::GruvboxDark,
            ReasoningRippleTone::Ultra,
        ),
        (
            "catppuccin-mocha",
            TuiAppTheme::CatppuccinMocha,
            ReasoningRippleTone::Max,
        ),
        (
            "catppuccin-mocha",
            TuiAppTheme::CatppuccinMocha,
            ReasoningRippleTone::Ultra,
        ),
    ]
    .map(|(theme_name, theme, tone)| {
        let _active_theme = app_theme::activate(theme);
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 31, /*height*/ 3,
        );
        let origin = Rect::new(
            /*x*/ 13, /*y*/ 1, /*width*/ 5, /*height*/ 1,
        );
        let mut buf = labeled_header_buffer(area, active_palette());
        ReasoningRippleFrame {
            tone,
            progress: 0.45,
        }
        .render(area, origin, &mut buf);
        format!("{theme_name} {tone:?}\n{}", buffer_backgrounds(&buf, area))
    })
    .join("\n\n");

    insta::assert_snapshot!("themed_max_and_ultra_ripple_gradients", snapshots);
}

#[test]
fn shell_render_limits_ripple_to_top_bar_snapshot() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 24,
    );
    let mut shell = ShellState::snapshot_fixture();
    shell.app_theme = TuiAppTheme::GruvboxDark;
    let mut baseline = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut baseline);

    let starts_at = Instant::now() + Duration::from_secs(/*secs*/ 60);
    shell.reasoning_ripple = Some(ReasoningRipple {
        tone: ReasoningRippleTone::Ultra,
        started_at: starts_at,
        expires_at: starts_at + RIPPLE_DURATION,
    });
    let mut rippled = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut rippled);

    let changed = changed_backgrounds(&baseline, &rippled, area);
    assert!(changed.iter().all(|position| position.y < 3));
    insta::assert_snapshot!(
        "shell_render_limits_ripple_to_top_bar",
        changed_rows(&changed, area)
    );
}

fn labeled_header_buffer(area: Rect, theme: ThemePalette) -> Buffer {
    let mut buf = Buffer::empty(area);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buf[(x, y)]
                .set_symbol("·")
                .set_style(Style::new().fg(theme.text).bg(theme.dark));
        }
    }
    buf
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

fn changed_backgrounds(before: &Buffer, after: &Buffer, area: Rect) -> Vec<Position> {
    area.positions()
        .filter(|position| before[*position].style().bg != after[*position].style().bg)
        .collect()
}

fn changed_rows(changed: &[Position], area: Rect) -> String {
    (area.y..area.bottom())
        .filter(|y| changed.iter().any(|position| position.y == *y))
        .map(|y| {
            let cells = (area.x..area.right())
                .map(|x| {
                    if changed.contains(&Position::new(x, y)) {
                        '▓'
                    } else {
                        '·'
                    }
                })
                .collect::<String>();
            format!("{y:02}: {cells}")
        })
        .join("\n")
}
