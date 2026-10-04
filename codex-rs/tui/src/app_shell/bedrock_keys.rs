//! Fixed shortcuts used before users have had a chance to configure Codex.

use crossterm::event::KeyCode;

use crate::key_hint;
use crate::key_hint::KeyBinding;

pub(crate) const MOVE_UP: [KeyBinding; 2] = [
    key_hint::plain(KeyCode::Up),
    key_hint::plain(KeyCode::Char('k')),
];
pub(crate) const MOVE_DOWN: [KeyBinding; 2] = [
    key_hint::plain(KeyCode::Down),
    key_hint::plain(KeyCode::Char('j')),
];
pub(crate) const CONFIRM: [KeyBinding; 1] = [key_hint::plain(KeyCode::Enter)];
pub(crate) const CANCEL: [KeyBinding; 1] = [key_hint::plain(KeyCode::Esc)];
