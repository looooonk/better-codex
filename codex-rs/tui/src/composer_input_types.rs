//! Shared data types for composer input and restored thread drafts.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalImageAttachment {
    pub(crate) placeholder: String,
    pub(crate) path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MentionBinding {
    /// Visible mention sigil (`$` or `@`).
    pub(crate) sigil: char,
    /// Mention token text without the leading sigil (`$` or `@`).
    pub(crate) mention: String,
    /// Canonical mention target (for example `app://...` or absolute SKILL.md path).
    pub(crate) path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueuedInputAction {
    Plain,
    ParseSlash,
    RunShell,
}
