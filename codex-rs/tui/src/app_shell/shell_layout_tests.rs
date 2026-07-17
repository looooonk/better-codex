use super::*;
use pretty_assertions::assert_eq;

#[test]
fn responsive_layout_uses_sidebar_overlay_and_minimum_width_boundaries() {
    let shell = ShellState::snapshot_fixture();

    assert_eq!(
        (terminal_width_supported(39), terminal_width_supported(40)),
        (false, true)
    );
    assert_eq!(
        calculate(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 39, /*height*/ 24,
            ),
        ),
        None
    );
    assert_eq!(
        calculate(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 24,
            ),
        ),
        Some(ShellLayout {
            header: Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 3
            ),
            transcript: Rect::new(
                /*x*/ 0, /*y*/ 3, /*width*/ 40, /*height*/ 15
            ),
            input: Rect::new(
                /*x*/ 0, /*y*/ 18, /*width*/ 40, /*height*/ 6
            ),
            dashboard: Some(DashboardPlacement::Overlay(Rect::new(
                /*x*/ 0, /*y*/ 3, /*width*/ 40, /*height*/ 15,
            ))),
        })
    );
    assert_eq!(
        calculate(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 99, /*height*/ 24,
            ),
        ),
        Some(ShellLayout {
            header: Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 99, /*height*/ 3
            ),
            transcript: Rect::new(
                /*x*/ 0, /*y*/ 3, /*width*/ 99, /*height*/ 15
            ),
            input: Rect::new(
                /*x*/ 0, /*y*/ 18, /*width*/ 99, /*height*/ 6
            ),
            dashboard: Some(DashboardPlacement::Overlay(Rect::new(
                /*x*/ 49, /*y*/ 3, /*width*/ 50, /*height*/ 15,
            ))),
        })
    );
    assert_eq!(
        calculate(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28,
            ),
        ),
        Some(ShellLayout {
            header: Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 50, /*height*/ 3
            ),
            transcript: Rect::new(
                /*x*/ 0, /*y*/ 3, /*width*/ 50, /*height*/ 19
            ),
            input: Rect::new(
                /*x*/ 0, /*y*/ 22, /*width*/ 50, /*height*/ 6
            ),
            dashboard: Some(DashboardPlacement::Sidebar(Rect::new(
                /*x*/ 50, /*y*/ 0, /*width*/ 50, /*height*/ 28,
            ))),
        })
    );
}

#[test]
fn hiding_the_dashboard_reclaims_the_full_width_layout() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;

    assert_eq!(
        calculate(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 24,
            ),
        ),
        Some(ShellLayout {
            header: Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 78, /*height*/ 3
            ),
            transcript: Rect::new(
                /*x*/ 0, /*y*/ 3, /*width*/ 78, /*height*/ 15
            ),
            input: Rect::new(
                /*x*/ 0, /*y*/ 18, /*width*/ 78, /*height*/ 6
            ),
            dashboard: None,
        })
    );
}

#[test]
fn compact_help_preserves_the_overlay_height() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_route = DashboardRoute::Help;

    assert_eq!(
        calculate(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 16,
            ),
        ),
        Some(ShellLayout {
            header: Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 2
            ),
            transcript: Rect::new(
                /*x*/ 0, /*y*/ 2, /*width*/ 48, /*height*/ 10
            ),
            input: Rect::new(
                /*x*/ 0, /*y*/ 12, /*width*/ 48, /*height*/ 4
            ),
            dashboard: Some(DashboardPlacement::Overlay(Rect::new(
                /*x*/ 0, /*y*/ 2, /*width*/ 48, /*height*/ 10,
            ))),
        })
    );
}
