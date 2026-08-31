use super::ShellState;
use super::TranscriptKind;
use crate::clipboard_copy::ClipboardLease;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

const RESPONSE_ORDINAL_LABELS: [&str; 9] = [
    "latest",
    "2nd latest",
    "3rd latest",
    "4th latest",
    "5th latest",
    "6th latest",
    "7th latest",
    "8th latest",
    "9th latest",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResponseOrdinal(u8);

impl ResponseOrdinal {
    pub(super) const LATEST: Self = Self(1);

    pub(super) fn from_ascii_digit(digit: char) -> Option<Self> {
        let one_based = u8::try_from(digit.to_digit(10)?).ok()?;
        (1..=9).contains(&one_based).then_some(Self(one_based))
    }

    fn index(self) -> usize {
        usize::from(self.0 - 1)
    }

    fn label(self) -> &'static str {
        RESPONSE_ORDINAL_LABELS[self.index()]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CopyResponseRequest {
    Response(ResponseOrdinal),
    Invalid,
}

impl CopyResponseRequest {
    pub(super) fn parse_args(args: &str) -> Self {
        let args = args.trim();
        if args.is_empty() {
            return Self::Response(ResponseOrdinal::LATEST);
        }
        let mut chars = args.chars();
        let ordinal = chars.next().and_then(ResponseOrdinal::from_ascii_digit);
        match (ordinal, chars.next()) {
            (Some(ordinal), None) => Self::Response(ordinal),
            (Some(_), Some(_)) | (None, Some(_)) | (None, None) => Self::Invalid,
        }
    }
}

pub(super) fn response_ordinal_from_alt_key(key: KeyEvent) -> Option<ResponseOrdinal> {
    if key.kind != KeyEventKind::Press || key.modifiers != KeyModifiers::ALT {
        return None;
    }
    match key.code {
        KeyCode::Char(digit) => ResponseOrdinal::from_ascii_digit(digit),
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::Esc
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => None,
    }
}

impl ShellState {
    pub(super) fn copy_response_request_with(
        &mut self,
        request: CopyResponseRequest,
        copy_fn: impl FnOnce(&str) -> Result<Option<ClipboardLease>, String>,
    ) {
        match request {
            CopyResponseRequest::Response(ordinal) => self.copy_response_with(ordinal, copy_fn),
            CopyResponseRequest::Invalid => self.push_error("Usage: /copy [1-9]"),
        }
    }

    pub(super) fn copy_response_with(
        &mut self,
        ordinal: ResponseOrdinal,
        copy_fn: impl FnOnce(&str) -> Result<Option<ClipboardLease>, String>,
    ) {
        let Some(text) = self.response_copy_text(ordinal).map(str::to_owned) else {
            if ordinal == ResponseOrdinal::LATEST {
                self.push_error("No Codex response to copy");
            } else {
                self.push_error(format!("No {} Codex response to copy", ordinal.label()));
            }
            return;
        };
        self.copy_text_with(
            &text,
            format!("copied {} Codex response", ordinal.label()),
            copy_fn,
        );
    }

    pub(super) fn copy_selected_transcript_with(
        &mut self,
        copy_fn: impl FnOnce(&str) -> Result<Option<ClipboardLease>, String>,
    ) {
        let Some((kind, text)) = self
            .selected_transcript_copy_text()
            .map(|(kind, text)| (kind, text.to_string()))
        else {
            self.copy_response_with(ResponseOrdinal::LATEST, copy_fn);
            return;
        };
        self.copy_text_with(
            &text,
            format!("copied {} transcript item", kind.label()),
            copy_fn,
        );
    }

    pub(super) fn selected_transcript_copy_text(&self) -> Option<(TranscriptKind, &str)> {
        let selected = self.transcript_selection?;
        self.transcript.get(selected).map(|line| {
            (
                line.kind,
                line.full_text.as_deref().unwrap_or(line.text.as_str()),
            )
        })
    }

    pub(super) fn selected_transcript_is_output(&self) -> bool {
        self.transcript_selection
            .and_then(|selected| self.transcript.get(selected))
            .is_some_and(|line| line.kind == TranscriptKind::Output)
    }

    pub(super) fn transcript_copy_text(&self) -> Option<(TranscriptKind, &str)> {
        self.selected_transcript_copy_text().or_else(|| {
            self.response_copy_text(ResponseOrdinal::LATEST)
                .map(|text| (TranscriptKind::Assistant, text))
        })
    }

    fn response_copy_text(&self, ordinal: ResponseOrdinal) -> Option<&str> {
        self.transcript
            .iter()
            .rev()
            .filter(|line| line.kind == TranscriptKind::Assistant)
            .nth(ordinal.index())
            .map(|line| line.text.as_str())
    }

    fn copy_text_with(
        &mut self,
        text: &str,
        success_message: String,
        copy_fn: impl FnOnce(&str) -> Result<Option<ClipboardLease>, String>,
    ) {
        match copy_fn(text) {
            Ok(lease) => {
                self.clipboard_lease = lease;
                self.push_status(success_message);
            }
            Err(error) => self.push_error(format!("Copy failed: {error}")),
        }
    }
}

#[cfg(test)]
#[path = "transcript_copy_tests.rs"]
mod tests;
