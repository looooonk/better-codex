use super::super::ShellState;
use super::super::render::ShellView;
use super::SlashCommandPopupKeyResult;
use super::SlashCommandSuggestions;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn composer_with_cursor(marked_text: &str) -> super::super::ComposerState {
    let (before, after) = marked_text
        .split_once('|')
        .expect("marked composer text should contain a cursor");
    let mut composer = super::super::ComposerState::default();
    composer.set_text(format!("{before}{after}"));
    composer.set_cursor(before.len());
    composer
}

fn matching_names(marked_text: &str) -> Option<Vec<&'static str>> {
    SlashCommandSuggestions::candidate(&composer_with_cursor(marked_text)).map(|suggestions| {
        suggestions
            .entries()
            .iter()
            .map(|definition| definition.name())
            .collect()
    })
}

#[test]
fn suggestions_follow_the_first_command_token_and_cursor() {
    assert_eq!(
        [
            matching_names("/|"),
            matching_names("/lo|"),
            matching_names("/cl|ear later"),
            matching_names("/goal |later"),
            matching_names(" /|"),
            matching_names("hello\n/|"),
        ],
        [
            Some(vec![
                "/clear", "/copy", "/goal", "/login", "/logout", "/vim", "/exit",
            ]),
            Some(vec!["/login", "/logout"]),
            Some(vec!["/clear"]),
            None,
            None,
            None,
        ]
    );
}

#[test]
fn navigation_wraps_and_completion_preserves_the_tail() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer = composer_with_cursor("/lo|");
    let mut navigation = Vec::new();
    for code in [KeyCode::Up, KeyCode::Down, KeyCode::Down] {
        let result = shell.handle_slash_command_popup_key(KeyEvent::new(code, KeyModifiers::NONE));
        let selected = shell
            .slash_command_suggestions()
            .expect("suggestions should remain open")
            .selected_definition()
            .name();
        navigation.push((result, selected));
    }
    assert_eq!(
        navigation,
        vec![
            (SlashCommandPopupKeyResult::Consumed, "/logout"),
            (SlashCommandPopupKeyResult::Consumed, "/login"),
            (SlashCommandPopupKeyResult::Consumed, "/logout"),
        ]
    );
    assert_eq!(
        shell.handle_slash_command_popup_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE,)),
        SlashCommandPopupKeyResult::Consumed
    );
    assert_eq!(
        (shell.composer.text(), shell.composer.cursor()),
        ("/logout ", 8)
    );

    shell.composer = composer_with_cursor("/cl| tail");
    shell.slash_command_popup.reset();
    shell.handle_slash_command_popup_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(
        (shell.composer.text(), shell.composer.cursor()),
        ("/clear tail", 7)
    );
}

#[test]
fn escape_dismisses_only_the_current_query() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer = composer_with_cursor("/|");

    assert_eq!(
        shell.handle_slash_command_popup_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE,)),
        SlashCommandPopupKeyResult::Consumed
    );
    assert!(shell.slash_command_suggestions().is_none());

    shell.slash_command_popup.reset();
    shell.composer = composer_with_cursor("/g|");
    assert!(shell.slash_command_suggestions().is_some());
}

#[test]
fn renders_slash_command_suggestions_wide_and_narrow() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer.set_text("/");
    insta::assert_snapshot!(
        "slash_command_suggestions_wide",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 28
            )
        )
    );

    shell.composer.set_text("/lo");
    shell.slash_command_popup.reset();
    shell.handle_slash_command_popup_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    insta::assert_snapshot!(
        "slash_command_suggestions_narrow",
        render_shell(
            &shell,
            Rect::new(
                /*x*/ 0, /*y*/ 0, /*width*/ 48, /*height*/ 18
            )
        )
    );
}

fn render_shell(shell: &ShellState, area: Rect) -> String {
    let mut buf = Buffer::empty(area);
    ShellView { shell }.render(area, &mut buf);
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .filter_map(|x| buf.cell((x, y)))
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
