//! IDE context data model and public helpers for TUI `/ide` support.

mod ipc;
mod prompt;
#[cfg(windows)]
mod windows_pipe;

pub(crate) use ipc::fetch_ide_context;
pub(crate) use prompt::apply_ide_context_to_user_input;
pub(crate) use prompt::extract_prompt_request_with_offset;
pub(crate) use prompt::has_prompt_context;

pub(crate) fn visible_request(message: &str) -> &str {
    if message.starts_with("# Context from my IDE setup:\n") {
        extract_prompt_request_with_offset(message).0
    } else {
        message
    }
}

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdeContext {
    active_file: Option<ActiveFile>,
    #[serde(default)]
    open_tabs: Vec<FileDescriptor>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ActiveFile {
    #[serde(flatten)]
    descriptor: FileDescriptor,
    selection: Range,
    #[serde(default)]
    active_selection_content: String,
    #[serde(default)]
    selections: Vec<Range>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct FileDescriptor {
    label: String,
    path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct Range {
    start: Position,
    end: Position,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct Position {
    line: u32,
    character: u32,
}

#[cfg(test)]
#[path = "ide_context/model_tests.rs"]
mod tests;
