use super::*;
use codex_config::types::TuiAppTheme;
use pretty_assertions::assert_eq;

fn option(index: usize) -> SelectorOption<usize> {
    SelectorOption::new(
        index,
        format!("Choice {index}"),
        format!("Detailed explanation for choice {index}"),
    )
}

fn current_option(index: usize) -> SelectorOption<usize> {
    option(index).current(true)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn initial_selection_uses_the_current_option() {
    let state = SelectorState::new("Choices", vec![option(0), current_option(1), option(2)]);

    assert_eq!(state.selected, 1);
    assert_eq!(state.options[state.selected].value, 1);
}

#[test]
fn reasoning_default_is_an_explicit_typed_choice() {
    let state = SelectorState::reasoning_efforts(
        &ReasoningEffortValue::Default,
        &[ReasoningEffortPreset {
            effort: ReasoningEffort::High,
            description: "Deep reasoning for difficult tasks.".to_string(),
        }],
    );

    assert_eq!(state.selected, 0);
    assert_eq!(
        &state.options[0].value,
        &SelectorValue::ReasoningEffort(ReasoningEffortValue::Default)
    );
    assert_eq!(
        &state.options[1].value,
        &SelectorValue::ReasoningEffort(ReasoningEffortValue::Explicit(ReasoningEffort::High))
    );
}

#[test]
fn app_theme_selector_renders_all_themes_and_marks_the_current_one() {
    let state = SelectorState::app_themes(TuiAppTheme::CatppuccinMocha);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 20,
    );
    let geometry = selector_geometry(area, state.options.len());
    let mut buffer = Buffer::empty(area);

    state.render(area, /*pointer*/ None, &mut buffer);

    assert_eq!(state.selected, 2);
    insta::assert_snapshot!(buffer_text(&buffer, geometry.modal));
}

#[test]
fn movement_keys_clamp_and_keep_the_selection_visible() {
    let mut state = SelectorState::new("Choices", (0..8).map(option).collect());

    for _ in 0..5 {
        assert_eq!(
            state.handle_key(key(KeyCode::Char('j'))),
            SelectorOutcome::Pending
        );
    }
    assert_eq!((state.selected, state.visible_scroll(3)), (5, 3));

    assert_eq!(
        state.handle_key(key(KeyCode::Char('g'))),
        SelectorOutcome::Pending
    );
    assert_eq!((state.selected, state.visible_scroll(3)), (0, 0));

    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)),
        SelectorOutcome::Pending
    );
    assert_eq!((state.selected, state.visible_scroll(3)), (7, 5));

    assert_eq!(state.handle_key(key(KeyCode::Up)), SelectorOutcome::Pending);
    assert_eq!((state.selected, state.visible_scroll(3)), (6, 4));
}

#[test]
fn selection_keys_return_typed_outcomes() {
    let mut state = SelectorState::new("Choices", vec![option(10), option(20), option(30)]);

    assert_eq!(
        state.handle_key(key(KeyCode::Char('2'))),
        SelectorOutcome::Selected(20)
    );
    assert_eq!(state.selected, 1);
    assert_eq!(
        state.handle_key(key(KeyCode::Enter)),
        SelectorOutcome::Selected(20)
    );
    assert_eq!(
        state.handle_key(key(KeyCode::Esc)),
        SelectorOutcome::Cancelled
    );
    assert_eq!(
        state.handle_key(key(KeyCode::Char('9'))),
        SelectorOutcome::Pending
    );
}

#[test]
fn modal_geometry_is_centered_and_bounded() {
    let state = SelectorState::new("Choices", (0..20).map(option).collect());
    let area = Rect::new(
        /*x*/ 7, /*y*/ 3, /*width*/ 120, /*height*/ 50,
    );

    let geometry = selector_geometry(area, state.options.len());

    assert_eq!(geometry.modal.width, MAX_MODAL_WIDTH);
    assert_eq!(geometry.modal.height, MAX_MODAL_HEIGHT);
    assert_eq!(
        geometry.modal.x.saturating_sub(area.x),
        area.width.saturating_sub(geometry.modal.width) / 2
    );
    assert_eq!(
        geometry.modal.y.saturating_sub(area.y),
        area.height.saturating_sub(geometry.modal.height) / 2
    );
    assert_eq!(geometry.visible_options, 10);
}

#[test]
fn narrow_geometry_stays_inside_its_bounds() {
    let state = SelectorState::new("Choices", vec![option(0)]);
    let area = Rect::new(
        /*x*/ 4, /*y*/ 2, /*width*/ 24, /*height*/ 8,
    );

    let geometry = selector_geometry(area, state.options.len());

    assert!(area.contains(Position::new(geometry.modal.x, geometry.modal.y)));
    assert!(geometry.modal.right() <= area.right());
    assert!(geometry.modal.bottom() <= area.bottom());
    assert_eq!(geometry.modal.width, 20);
    assert_eq!(geometry.modal.height, 4);
    assert_eq!(geometry.visible_options, 1);
}

#[test]
fn wheel_page_keys_reach_choices_beyond_the_visible_window() {
    let mut state = SelectorState::new("Choices", (0..12).map(option).collect());

    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
        SelectorOutcome::Pending
    );
    assert_eq!(state.selected, 5);
    assert_eq!(
        state.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
        SelectorOutcome::Pending
    );
    assert_eq!(state.selected, 10);
}

#[test]
fn clicking_outside_the_modal_cancels_selection() {
    let mut state = SelectorState::new("Choices", vec![option(0)]);

    assert_eq!(
        state.select_at(Rect::new(0, 0, 80, 20), Position::new(0, 0)),
        SelectorOutcome::Cancelled
    );
}

#[test]
fn option_hit_testing_uses_both_rows_and_the_visible_scroll_window() {
    let mut state = SelectorState::new("Choices", (0..8).map(option).collect());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 13,
    );
    let geometry = selector_geometry(area, state.options.len());
    state.set_selected(7);
    let scroll = state.visible_scroll(geometry.visible_options);

    assert_eq!(
        state.option_at(area, Position::new(geometry.options.x, geometry.options.y)),
        Some(scroll)
    );
    assert_eq!(
        state.option_at(
            area,
            Position::new(geometry.options.x, geometry.options.y.saturating_add(1))
        ),
        Some(scroll)
    );
    assert_eq!(
        state.option_at(
            area,
            Position::new(geometry.options.x, geometry.options.y.saturating_add(2))
        ),
        Some(scroll + 1)
    );
    assert_eq!(
        state.option_at(area, Position::new(geometry.footer.x, geometry.footer.y)),
        None
    );
}

#[test]
fn clicking_an_option_selects_and_returns_its_value() {
    let mut state = SelectorState::new("Choices", vec![option(10), option(20), option(30)]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 20,
    );
    let geometry = selector_geometry(area, state.options.len());
    let position = Position::new(
        geometry.options.x,
        geometry.options.y.saturating_add(OPTION_HEIGHT),
    );

    assert_eq!(
        state.select_at(area, position),
        SelectorOutcome::Selected(20)
    );
    assert_eq!(state.selected, 1);
}

#[test]
fn rendering_uses_two_rows_and_marks_current_and_selected_choices() {
    let state = SelectorState::new("Choices", vec![option(0), current_option(1), option(2)]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 20,
    );
    let geometry = selector_geometry(area, state.options.len());
    let mut buf = Buffer::empty(area);

    state.render(area, /*pointer*/ None, &mut buf);

    let rendered = buffer_text(&buf, geometry.modal);
    assert!(rendered.contains("Choice 1  current"));
    assert!(rendered.contains("Detailed explanation for choice 1"));
    assert_eq!(
        buf[(geometry.options.x, geometry.options.y.saturating_add(2))]
            .style()
            .fg,
        Some(palette::FOCUS)
    );
    assert_eq!(
        buf[(geometry.options.x, geometry.options.y.saturating_add(2))]
            .style()
            .bg,
        Some(palette::ELEVATED)
    );
}

fn buffer_text(buf: &Buffer, area: Rect) -> String {
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
