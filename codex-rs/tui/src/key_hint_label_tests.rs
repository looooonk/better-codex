//! Compact shortcut labels stay uniformly emphasized above secondary descriptions.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn modifier_combinations_stay_compact_inside_chords() {
    let cases = [
        (KeyModifiers::CONTROL, "⌃t"),
        (KeyModifiers::ALT, "⌥t"),
        (KeyModifiers::CONTROL | KeyModifiers::SHIFT, "⌃⇧t"),
        (KeyModifiers::CONTROL | KeyModifiers::ALT, "⌃⌥t"),
    ];
    for (modifiers, expected) in cases {
        let prefix = KeyBinding::new(KeyCode::Char('t'), modifiers);
        let chord = ShortcutHint::Chord {
            prefix,
            completion: plain(KeyCode::Enter),
        };
        assert_eq!(
            (prefix.display_label(), chord.display_label()),
            (expected.to_owned(), format!("{expected} enter")),
        );
    }
}
