use super::*;
use crate::app_shell::render::ShellView;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn context() -> IdeContext {
    serde_json::from_value(serde_json::json!({"activeFile": null, "openTabs": [{"label": "lib.rs", "path": "src/lib.rs"}]})).unwrap()
}

fn input() -> Vec<UserInput> {
    vec![UserInput::Text {
        text: "Explain this function".to_string(),
        text_elements: Vec::new(),
    }]
}

#[test]
fn only_enabled_submissions_receive_context_and_failures_leave_input_intact() {
    let mut shell = ShellState::snapshot_fixture();
    let mut items = input();
    shell.apply_ide_result(Ok(context()), &mut items);
    assert_eq!(items, input());
    shell.ide.enabled = true;
    shell.apply_ide_result(Err("Open the project in your IDE.".to_string()), &mut items);
    assert_eq!(items, input());
    assert!(shell.ide.warned);
    shell.apply_ide_result(Ok(context()), &mut items);
    assert!(!shell.ide.warned);
    assert_eq!(items, vec![UserInput::Text { text: "# Context from my IDE setup:\n\n## Open tabs:\n- lib.rs: src/lib.rs\n\n## My request for Codex:\nExplain this function".to_string(), text_elements: Vec::new() }]);
}

#[test]
fn disabled_ide_status_and_repeated_failure_notice_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.run_ide_command("off");
    shell.ide.enabled = true;
    for _ in 0..2 {
        shell.apply_ide_result(
            Err(
                "Open this project in VS Code or Cursor with the Codex extension active."
                    .to_string(),
            ),
            &mut input(),
        );
    }
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 24,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let rendered = buffer
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
    assert_eq!(rendered.matches("IDE context was skipped").count(), 1);
    insta::assert_snapshot!(rendered);
}
