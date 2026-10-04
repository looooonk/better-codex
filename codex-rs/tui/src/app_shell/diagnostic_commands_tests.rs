use super::*;
use crate::app_shell::render::ShellView;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn retained_warnings_are_bounded_and_adjacent_duplicates_collapse() {
    let mut shell = ShellState::snapshot_fixture();
    for index in 0..102 {
        shell.retain_warning(format!("warning {index}"));
    }
    shell.retain_warning("warning 101".to_string());
    assert_eq!(
        shell.warnings.0,
        (2..102)
            .map(|index| format!("warning {index}"))
            .collect::<VecDeque<_>>()
    );
    shell.retain_warning("é".repeat(20_000));
    assert_eq!(
        shell.warnings.0.back().unwrap(),
        &format!("{}\n[warning truncated]", "é".repeat(16_384))
    );
}

#[test]
fn warnings_keep_diagnostic_details_visible() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.retain_warning("Project configuration ignored\n/workspace/.codex/config.toml\nTrust this workspace to load its settings.".to_string());
    shell.push_system(shell.warnings.0.front().unwrap().clone());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 22,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let text = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(text);
}
