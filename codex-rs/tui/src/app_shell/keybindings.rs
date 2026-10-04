//! Adapt native configured shortcuts to the existing full-screen controls.

use crate::key_hint::KeyBinding;
use crate::key_hint::KeyBindingListExt;
use crate::keymap::KeyChordMatch;
use crate::keymap::KeyChordMatcher;
use crate::keymap::KeymapActionId;
use crate::keymap::KeymapContext;
use crate::keymap::KeymapContextSet;
use crate::keymap::RuntimeKeymap;
use crate::keymap::configured_binding_for_action;
use crate::keymap::keymap_action_ids;
use crate::keymap::runtime_action_bindings;
use codex_config::types::TuiKeymap;
use crossterm::event::KeyEvent;

mod actions;
mod settings;
pub(super) use actions::ShellShortcut;

#[derive(Clone, Debug)]
pub(super) struct ShellKeymap {
    source: TuiKeymap,
    runtime: RuntimeKeymap,
    matcher: KeyChordMatcher,
}

pub(super) enum ResolvedKey {
    Key(KeyEvent),
    Navigation(KeyEvent),
    Shortcut(ShellShortcut),
    Consumed,
}

impl Default for ShellKeymap {
    fn default() -> Self {
        Self {
            source: TuiKeymap::default(),
            runtime: RuntimeKeymap::defaults(),
            matcher: KeyChordMatcher::default(),
        }
    }
}

impl ShellKeymap {
    pub(super) fn from_config(source: &TuiKeymap) -> Result<Self, String> {
        let runtime = RuntimeKeymap::from_config(source)?;
        for id in keymap_action_ids() {
            let Some(binding) = configured_binding_for_action(source, id).and_then(Option::as_ref)
            else {
                continue;
            };
            if !binding.specs().is_empty() && actions::target(id).is_none() {
                return Err(format!(
                    "{} is unavailable in the full-screen interface. Use /keymap to see supported actions.",
                    id.config_path()
                ));
            }
            for spec in binding.specs() {
                let first = spec.as_str().split(' ').next().unwrap_or_default();
                if matches!(
                    id.context,
                    KeymapContext::Global
                        | KeymapContext::Chat
                        | KeymapContext::Composer
                        | KeymapContext::Editor
                ) && matches!(
                    first,
                    "ctrl-d" | "ctrl-n" | "ctrl-p" | "ctrl-v" | "alt-m" | "alt-e" | "f1" | "f3"
                ) {
                    return Err(format!(
                        "{} conflicts with the full-screen shortcut `{first}`. Choose another key.",
                        id.config_path()
                    ));
                }
            }
            let configured_keys = runtime_action_bindings(&runtime)
                .filter(|binding| binding.id == id)
                .flat_map(|binding| {
                    crate::keymap::user_bindings(binding.bindings)
                        .iter()
                        .copied()
                })
                .chain(
                    runtime
                        .chords
                        .bindings
                        .iter()
                        .filter(|binding| binding.action == id)
                        .map(|binding| binding.chord.prefix),
                );
            for key in configured_keys {
                let (code, modifiers) = key.parts();
                let event = KeyEvent::new(code, modifiers);
                if let Some(other) = keymap_action_ids().find(|other| {
                    let editor_list = matches!(
                        (id.context, other.context),
                        (KeymapContext::Editor, KeymapContext::List)
                            | (KeymapContext::List, KeymapContext::Editor)
                    ) && !crate::key_hint::is_plain_text_key_event(event);
                    let pager_copy = (id.context == KeymapContext::Global
                        && id.action == "copy"
                        && other.context == KeymapContext::Pager)
                        || (other.context == KeymapContext::Global
                            && other.action == "copy"
                            && id.context == KeymapContext::Pager);
                    let fork_overlap = editor_list || pager_copy;
                    *other != id
                        && (other.context.overlaps(id.context) || fork_overlap)
                        && match configured_binding_for_action(source, *other) {
                            Some(None) => actions::defaults(*other).is_pressed(event),
                            Some(Some(_)) if fork_overlap => {
                                runtime_action_bindings(&runtime).any(|binding| {
                                    binding.id == *other && binding.bindings.is_pressed(event)
                                }) || runtime.chords.bindings.iter().any(|binding| {
                                    binding.action == *other && binding.chord.prefix.is_press(event)
                                })
                            }
                            Some(Some(_)) | None => false,
                        }
                }) {
                    return Err(format!(
                        "{} conflicts with {}. Rebind or explicitly unbind the other action first.",
                        id.config_path(),
                        other.config_path()
                    ));
                }
            }
        }
        Ok(Self {
            source: source.clone(),
            runtime,
            matcher: KeyChordMatcher::default(),
        })
    }

    pub(super) fn configured(&self, context: &str, action: &str) -> bool {
        crate::keymap::keymap_action_id(context, action)
            .and_then(|id| configured_binding_for_action(&self.source, id))
            .is_some_and(Option::is_some)
    }

    pub(super) fn hint(&self, context: &str, action: &str, fallback: &str) -> String {
        if !self.configured(context, action) {
            return fallback.to_string();
        }
        crate::keymap::keymap_action_id(context, action)
            .and_then(|id| self.runtime.primary_hint(id.context, id.action))
            .map_or_else(
                || "unbound".to_string(),
                super::super::key_hint::ShortcutHint::display_label,
            )
    }

    pub(super) fn has_overrides(&self) -> bool {
        self.source != TuiKeymap::default()
    }

    pub(super) fn resolve(&mut self, key: KeyEvent, contexts: KeymapContextSet) -> ResolvedKey {
        let chord_contexts = if contexts.contains(KeymapContext::Editor)
            && crate::key_hint::is_plain_text_key_event(key)
            && !self.matcher.is_pending()
        {
            contexts.without(KeymapContext::List)
        } else {
            contexts
        };
        let key = match self
            .matcher
            .advance(key, &self.runtime.chords, chord_contexts)
        {
            KeyChordMatch::PassThrough => key,
            KeyChordMatch::Completed(key) => key,
            KeyChordMatch::Pending(_) | KeyChordMatch::Cancelled | KeyChordMatch::Ignored => {
                return ResolvedKey::Consumed;
            }
        };
        let active = |id: KeymapActionId| {
            contexts.contains_action(id)
                && !(id.context == KeymapContext::List
                    && contexts.contains(KeymapContext::Editor)
                    && crate::key_hint::is_plain_text_key_event(key))
                && !(id.context == KeymapContext::Editor
                    && id.action == "insert_newline"
                    && contexts.contains(KeymapContext::List))
                && configured_binding_for_action(&self.source, id).is_some_and(Option::is_some)
        };
        for runtime_binding in
            runtime_action_bindings(&self.runtime).filter(|binding| active(binding.id))
        {
            if (runtime_binding.bindings.is_pressed(key)
                || crate::keymap::dispatch_binding(runtime_binding.id)
                    .is_some_and(|binding| binding.is_press(key)))
                && let Some(target) = actions::target(runtime_binding.id)
            {
                return match target {
                    ShellShortcut::Key(binding) => {
                        let (code, modifiers) = binding.parts();
                        let mapped = KeyEvent {
                            code,
                            modifiers,
                            ..key
                        };
                        if runtime_binding.id.context == KeymapContext::List
                            && contexts.contains(KeymapContext::Editor)
                        {
                            ResolvedKey::Navigation(mapped)
                        } else {
                            ResolvedKey::Key(mapped)
                        }
                    }
                    action => ResolvedKey::Shortcut(action),
                };
            }
        }
        if keymap_action_ids()
            .filter(|id| active(*id))
            .any(|id| actions::defaults(id).is_pressed(key))
        {
            ResolvedKey::Consumed
        } else {
            ResolvedKey::Key(key)
        }
    }
}

pub(crate) fn validate_keymap(config: &TuiKeymap) -> Result<(), String> {
    ShellKeymap::from_config(config).map(|_| ())
}

#[cfg(test)]
#[path = "keybindings_tests.rs"]
mod tests;
