use super::*;
use pretty_assertions::assert_eq;

#[test]
fn arrows_follow_soft_wrapped_rows_in_the_issue_example() {
    let text = "You should do items A, B, and C.";

    assert_eq!(
        ComposerLayout::new(text, /*width*/ 30)
            .vertical_target(text.len(), ComposerVerticalDirection::Up),
        ComposerVerticalTarget::Cursor(6)
    );
    assert_eq!(
        ComposerLayout::new(text, /*width*/ 30)
            .vertical_target(/*cursor*/ 6, ComposerVerticalDirection::Down),
        ComposerVerticalTarget::Cursor(text.len())
    );
}

#[test]
fn vertical_targets_use_display_columns_and_clamp_to_short_rows() {
    let text = "界界界\nab";
    let second_line = text.find("ab").expect("second line should exist");

    assert_eq!(
        ComposerLayout::new(text, /*width*/ 20)
            .vertical_target(text.len(), ComposerVerticalDirection::Up),
        ComposerVerticalTarget::Cursor("界".len())
    );
    assert_eq!(
        ComposerLayout::new(text, /*width*/ 20)
            .vertical_target("界".len(), ComposerVerticalDirection::Down),
        ComposerVerticalTarget::Cursor(second_line.saturating_add("ab".len()))
    );
}

#[test]
fn vertical_targets_report_the_outer_visual_boundaries() {
    let text = "one wrapped line";

    assert_eq!(
        (
            ComposerLayout::new(text, /*width*/ 10)
                .vertical_target(/*cursor*/ 0, ComposerVerticalDirection::Up),
            ComposerLayout::new(text, /*width*/ 10)
                .vertical_target(text.len(), ComposerVerticalDirection::Down),
        ),
        (
            ComposerVerticalTarget::Boundary,
            ComposerVerticalTarget::Boundary,
        )
    );
}
