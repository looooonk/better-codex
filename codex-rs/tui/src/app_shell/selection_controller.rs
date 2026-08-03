use super::ShellState;
use super::backend::AppShellBackend;
use super::composer_render;
use super::render::PointerPane;
use super::render::ShellView;
use super::text_selection::NormalizedVisualRange;
use super::text_selection::VisualGraphemeHit;
use super::transcript_view;
use crate::clipboard_copy::ClipboardLease;
use crate::legacy_core::config::Config;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use std::ops::Range;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TextSelectionState {
    transcript: Option<TranscriptTextSelection>,
    drag: Option<TextSelectionDrag>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TranscriptTextSelection {
    range: NormalizedVisualRange,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TextSelectionDrag {
    Transcript {
        anchor: VisualGraphemeHit,
        origin: Position,
        moved: bool,
    },
    Message {
        anchor: Range<usize>,
        origin: Position,
        moved: bool,
    },
}

impl TextSelectionDrag {
    fn origin(&self) -> Position {
        match self {
            Self::Transcript { origin, .. } | Self::Message { origin, .. } => *origin,
        }
    }

    fn moved(&self) -> bool {
        match self {
            Self::Transcript { moved, .. } | Self::Message { moved, .. } => *moved,
        }
    }
}

impl ShellState {
    pub(super) fn transcript_text_selection(&self) -> Option<NormalizedVisualRange> {
        self.text_selection
            .transcript
            .as_ref()
            .map(|selection| selection.range)
    }

    pub(super) fn has_text_selection(&self) -> bool {
        self.composer.selected_text().is_some() || self.text_selection.transcript.is_some()
    }

    pub(super) fn clear_transcript_text_selection(&mut self) {
        self.text_selection.transcript = None;
        if matches!(
            self.text_selection.drag.as_ref(),
            Some(TextSelectionDrag::Transcript { .. })
        ) {
            self.text_selection.drag = None;
        }
    }

    pub(super) fn clear_text_selections(&mut self) {
        self.composer.clear_selection();
        self.text_selection = TextSelectionState::default();
    }

    pub(super) fn copy_text_selection_with(
        &mut self,
        copy_fn: impl FnOnce(&str) -> Result<Option<ClipboardLease>, String>,
    ) -> bool {
        let text = self
            .composer
            .selected_text()
            .map(str::to_string)
            .or_else(|| {
                self.text_selection
                    .transcript
                    .as_ref()
                    .map(|selection| selection.text.clone())
            });
        let Some(text) = text else {
            return false;
        };

        match copy_fn(&text) {
            Ok(lease) => {
                self.clipboard_lease = lease;
                self.push_status("copied selected text");
            }
            Err(error) => self.push_error(format!("Copy failed: {error}")),
        }
        true
    }

    pub(super) fn handle_text_copy_shortcut_with(
        &mut self,
        key: KeyEvent,
        copy_fn: impl FnOnce(&str) -> Result<Option<ClipboardLease>, String>,
    ) -> bool {
        if !is_text_copy_shortcut(key) {
            return false;
        }
        self.copy_text_selection_with(copy_fn) || key.modifiers.contains(KeyModifiers::SUPER)
    }

    pub(super) async fn handle_mouse_selection_down<S>(
        &mut self,
        area: Rect,
        position: Position,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        if self.rewind.is_forking() {
            return Ok(());
        }
        self.exit_confirmation_pending = false;
        self.set_pointer_position(position);
        self.text_selection.drag = None;

        let selection_blocked = self.diff_view.is_some()
            || self.tool_output.is_some()
            || self.agent_log.is_some()
            || self.selector.is_some()
            || self.command_palette.is_some()
            || self.pending_account_auth.is_some()
            || self.pending_approval.is_some()
            || self.pending_session_delete.is_some()
            || self.pending_elicitation.is_some()
            || self.pending_external_agent_import.is_some()
            || self.pending_mcp_management.is_some()
            || self.pending_plugin_management.is_some()
            || self.pending_user_input.is_some()
            || self.safety_buffering_modal_lines().is_some();
        if !selection_blocked {
            match (ShellView { shell: self }).pointer_pane_at(area, position) {
                Some(PointerPane::Transcript) => {
                    let transcript = (ShellView { shell: self }).transcript_area(area);
                    if let Some(anchor) =
                        transcript_view::transcript_text_hit_at(self, transcript, position)
                    {
                        self.composer.clear_selection();
                        self.clear_transcript_selection();
                        self.text_selection.transcript = None;
                        self.text_selection.drag = Some(TextSelectionDrag::Transcript {
                            anchor,
                            origin: position,
                            moved: false,
                        });
                        return Ok(());
                    }
                }
                Some(PointerPane::Input) => {
                    let input = (ShellView { shell: self }).input_area(area);
                    let display = self.composer.display();
                    if let Some(hit) = composer_render::composer_text_hit_inside(
                        input,
                        display.text(),
                        display.cursor(),
                        position,
                    ) {
                        let anchor = hit.grapheme_range();
                        let caret = hit.caret_range();
                        self.clear_transcript_text_selection();
                        self.handle_mouse_click(area, position, config, app_server)
                            .await?;
                        self.composer.set_cursor_from_display_range(caret);
                        self.text_selection.drag = Some(TextSelectionDrag::Message {
                            anchor,
                            origin: position,
                            moved: false,
                        });
                        return Ok(());
                    }
                }
                Some(PointerPane::Header | PointerPane::Dashboard) | None => {}
            }
        }

        self.clear_text_selections();
        self.handle_mouse_click(area, position, config, app_server)
            .await
    }

    pub(super) fn handle_mouse_selection_drag(&mut self, area: Rect, position: Position) {
        let Some(drag) = self.text_selection.drag.clone() else {
            return;
        };
        self.set_pointer_position(position);
        match drag {
            TextSelectionDrag::Transcript {
                anchor,
                origin,
                moved,
            } => {
                let moved = moved || origin != position;
                self.text_selection.drag = Some(TextSelectionDrag::Transcript {
                    anchor,
                    origin,
                    moved,
                });
                if (ShellView { shell: self }).pointer_pane_at(area, position)
                    != Some(PointerPane::Transcript)
                {
                    return;
                }
                let transcript = (ShellView { shell: self }).transcript_area(area);
                let Some(focus) =
                    transcript_view::transcript_text_hit_at(self, transcript, position)
                else {
                    return;
                };
                let range = NormalizedVisualRange::from_hits(anchor, focus);
                let Some(text) = transcript_view::transcript_selected_text(self, transcript, range)
                else {
                    return;
                };
                self.text_selection.transcript = Some(TranscriptTextSelection { range, text });
                self.text_selection.drag = Some(TextSelectionDrag::Transcript {
                    anchor,
                    origin,
                    moved,
                });
            }
            TextSelectionDrag::Message {
                anchor,
                origin,
                moved,
            } => {
                let moved = moved || origin != position;
                self.text_selection.drag = Some(TextSelectionDrag::Message {
                    anchor: anchor.clone(),
                    origin,
                    moved,
                });
                let input = (ShellView { shell: self }).input_area(area);
                let display = self.composer.display();
                let Some(focus) = composer_render::composer_text_hit_clamped_to_visible_viewport(
                    input,
                    display.text(),
                    display.cursor(),
                    position,
                ) else {
                    return;
                };
                self.composer
                    .set_selection_from_display_ranges(anchor.clone(), focus.grapheme_range());
                self.text_selection.drag = Some(TextSelectionDrag::Message {
                    anchor,
                    origin,
                    moved,
                });
            }
        }
    }

    pub(super) async fn handle_mouse_selection_release<S>(
        &mut self,
        area: Rect,
        position: Position,
        config: &Config,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let Some(pending) = self.text_selection.drag.clone() else {
            return Ok(());
        };
        if pending.moved() || pending.origin() != position {
            self.handle_mouse_selection_drag(area, position);
        }
        let Some(completed) = self.text_selection.drag.take() else {
            return Ok(());
        };
        if matches!(&completed, TextSelectionDrag::Transcript { .. })
            && !completed.moved()
            && completed.origin() == position
        {
            self.text_selection.transcript = None;
            self.handle_mouse_click(area, completed.origin(), config, app_server)
                .await?;
        }
        Ok(())
    }
}

pub(super) fn is_text_copy_shortcut(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c'))
        && key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
}

#[cfg(test)]
#[path = "selection_controller_tests.rs"]
mod tests;
