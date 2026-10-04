use super::*;
use crate::app_shell::ShellState;
use crate::app_shell::render::ShellView;
use crate::legacy_core::config::ConfigBuilder;
use codex_config::LoaderOverrides;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn main_context() -> KeymapContextSet {
    KeymapContextSet::new(KeymapContext::Global)
        .with(KeymapContext::Chat)
        .with(KeymapContext::Composer)
        .with(KeymapContext::Editor)
}

fn configured(value: &str) -> ShellKeymap {
    ShellKeymap::from_config(&toml::from_str(value).unwrap()).unwrap()
}

#[test]
fn configured_chord_dispatches_once_and_replaced_submit_is_disabled() {
    let mut keys = configured("[composer]\nsubmit = 'ctrl-x ctrl-s'");
    assert!(matches!(
        keys.resolve(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            main_context()
        ),
        ResolvedKey::Consumed
    ));
    assert!(matches!(
        keys.resolve(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            main_context()
        ),
        ResolvedKey::Consumed
    ));
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL), main_context()), ResolvedKey::Key(key) if key == KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    );
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL), main_context()), ResolvedKey::Key(key) if key.code == KeyCode::Char('s'))
    );
}

#[test]
fn explicit_unbinding_and_global_fallback_work_in_the_composer() {
    let mut keys = configured("[global]\nsubmit = 'f18'\n[composer]\nsubmit = []");
    assert_eq!(keys.hint("composer", "submit", "Enter"), "unbound");
    assert!(matches!(
        keys.resolve(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            main_context()
        ),
        ResolvedKey::Consumed
    ));
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE), main_context()), ResolvedKey::Key(key) if key.code == KeyCode::F(18))
    );
    let mut keys = configured("[global]\nsubmit = 'f18'");
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE), main_context()), ResolvedKey::Key(key) if key.code == KeyCode::Enter)
    );
}

#[test]
fn mapped_editor_and_list_controls_are_scoped_to_their_current_surface() {
    let mut keys = configured("[editor]\nmove_left = 'f18'\n[list]\nmove_down = 'f19'");
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE), main_context()), ResolvedKey::Key(key) if key.code == KeyCode::Left)
    );
    let list = KeymapContextSet::new(KeymapContext::List);
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE), list), ResolvedKey::Key(key) if key.code == KeyCode::F(18))
    );
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::F(19), KeyModifiers::NONE), list), ResolvedKey::Key(key) if key.code == KeyCode::Down)
    );
    assert!(matches!(
        keys.resolve(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), list),
        ResolvedKey::Consumed
    ));
}

#[test]
fn invalid_and_conflicting_configuration_is_rejected_before_dispatch() {
    for value in [
        "[editor]\nmove_left = 'f18'\nmove_right = 'f18'",
        "[composer]\nsubmit = 'ctrl-p'",
        "[vim_normal]\nmove_left = 'f18'",
        "[composer]\nsubmit = 'f2'",
    ] {
        let config = toml::from_str(value).unwrap();
        assert!(ShellKeymap::from_config(&config).is_err());
    }
}

#[test]
fn editable_picker_conflicts_and_legacy_editor_unbinding_are_enforced() {
    let conflict = toml::from_str("[editor]\nmove_left = 'f18'\n[list]\naccept = 'f18'").unwrap();
    assert!(ShellKeymap::from_config(&conflict).is_err());
    let mut keys = configured("[editor]\nmove_line_start = []\nkill_line_start = []");
    for code in [KeyCode::Left, KeyCode::Backspace, KeyCode::Char('\u{007f}')] {
        assert!(matches!(
            keys.resolve(KeyEvent::new(code, KeyModifiers::SUPER), main_context()),
            ResolvedKey::Consumed
        ));
    }
    let conflict = toml::from_str("[global]\ncopy = 'f18'\n[pager]\nscroll_down = 'f18'").unwrap();
    assert!(ShellKeymap::from_config(&conflict).is_err());
    let mut keys = configured("[global]\ncopy = 'f18'");
    let pager = KeymapContextSet::new(KeymapContext::Pager).with_copy();
    assert!(
        matches!(keys.resolve(KeyEvent::new(KeyCode::F(18), KeyModifiers::NONE), pager), ResolvedKey::Key(key) if key == KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL))
    );
    assert!(matches!(
        keys.resolve(
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
            pager
        ),
        ResolvedKey::Consumed
    ));
}

#[tokio::test]
async fn saved_keymap_reads_back_selected_profile_and_cli_precedence() {
    let home = tempfile::tempdir().unwrap();
    let selected = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
        home.path().join("profile.toml"),
    )
    .unwrap();
    tokio::fs::write(
        home.path().join("config.toml"),
        "[tui]\nanimations = false\n",
    )
    .await
    .unwrap();
    tokio::fs::write(selected.as_path(), "[tui]\nshow_tooltips = false\n")
        .await
        .unwrap();
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .cli_overrides(vec![(
            "tui.keymap.composer.submit".to_string(),
            toml::Value::String("f18".to_string()),
        )])
        .loader_overrides(LoaderOverrides {
            user_config_path: Some(selected.clone()),
            user_config_profile: Some("work".parse().unwrap()),
            ..LoaderOverrides::without_managed_config_for_tests()
        })
        .build()
        .await
        .unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.client_config_path = selected.clone();
    shell.keybindings = ShellKeymap::from_config(&config.tui_keymap).unwrap();
    shell
        .run_keymap_command("composer.submit f19", &config)
        .await
        .unwrap();
    assert_eq!(
        shell.keybindings.source.composer.submit,
        config.tui_keymap.composer.submit
    );
    assert!(shell.transcript.back().unwrap().text.contains("overrides"));
    let saved: toml::Value =
        toml::from_str(&tokio::fs::read_to_string(selected.as_path()).await.unwrap()).unwrap();
    assert_eq!(
        saved["tui"]["keymap"]["composer"]["submit"].as_str(),
        Some("f19")
    );
    assert_eq!(
        tokio::fs::read_to_string(home.path().join("config.toml"))
            .await
            .unwrap(),
        "[tui]\nanimations = false\n"
    );
}

#[test]
fn configured_hints_and_keymap_picker_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.keybindings = configured(
        "[composer]\nsubmit = 'f18'\nqueue = []\n[editor]\ninsert_newline = 'f19'\n[list]\naccept = 'f20'",
    );
    shell.dashboard_route = crate::app_shell::navigation::DashboardRoute::Help;
    shell.composer.clear();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 110, /*height*/ 28,
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
    insta::assert_snapshot!(rendered);
}
