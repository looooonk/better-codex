//! Prompt rendering for IDE context injected into TUI user turns.

use codex_app_server_protocol::ByteRange;
use codex_app_server_protocol::TextElement;
use codex_app_server_protocol::UserInput;

use super::IdeContext;

const MAX_PREFIX_BYTES: usize = 8_192;
const MAX_PATH_BYTES: usize = 2_048;
const MAX_SELECTION_RANGES: usize = 32;
const MAX_ACTIVE_SELECTION_CHARS: usize = 4_096;
const MAX_OPEN_TABS: usize = 20;
const MAX_OPEN_TABS_CHARS: usize = 2_048;
// Match the desktop app and IDE extension delimiter exactly. IDE context is serialized into the
// raw prompt before this marker, then transcript rendering strips back to the request after the last
// marker. Keeping the same marker and stripping semantics lets threads created with IDE context in
// one surface replay cleanly in the others.
const PROMPT_REQUEST_BEGIN: &str = "## My request for Codex:";

pub(crate) fn apply_ide_context_to_user_input(
    context: &IdeContext,
    items: &mut Vec<UserInput>,
) -> bool {
    let Some(context_text) = render_prompt_context(context) else {
        return false;
    };

    let suffix = format!("\n{PROMPT_REQUEST_BEGIN}\n");
    let truncation = "\n[IDE context truncated.]";
    let available = MAX_PREFIX_BYTES - suffix.len();
    let context_text = if context_text.len() > available {
        format!(
            "{}{truncation}",
            utf8_prefix(&context_text, available - truncation.len())
        )
    } else {
        context_text
    };
    let prefix = format!("{context_text}{suffix}");
    if let Some(text_index) = items
        .iter()
        .position(|item| matches!(item, UserInput::Text { .. }))
    {
        // Prefix the existing text item in place so image and text items keep
        // the same relative order they had in the user's original submission.
        let item = std::mem::replace(
            &mut items[text_index],
            UserInput::Text {
                text: String::new(),
                text_elements: Vec::new(),
            },
        );
        let UserInput::Text {
            text,
            text_elements,
        } = item
        else {
            unreachable!("position matched a text item");
        };
        items[text_index] = prefixed_text_input(prefix, text, text_elements);
    } else {
        items.insert(
            0,
            UserInput::Text {
                text: prefix,
                text_elements: Vec::new(),
            },
        );
    }

    true
}

pub(crate) fn has_prompt_context(context: &IdeContext) -> bool {
    render_prompt_context(context).is_some()
}

pub(crate) fn extract_prompt_request_with_offset(message: &str) -> (&str, usize) {
    let Some((before_request, request)) = message.rsplit_once(PROMPT_REQUEST_BEGIN) else {
        return (message, 0);
    };

    let request_start = before_request.len() + PROMPT_REQUEST_BEGIN.len();
    let trimmed_request = request.trim();
    let leading_trimmed_len = request.len() - request.trim_start().len();
    (trimmed_request, request_start + leading_trimmed_len)
}

fn utf8_prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn prefixed_text_input(prefix: String, text: String, text_elements: Vec<TextElement>) -> UserInput {
    let prefix_len = prefix.len();
    UserInput::Text {
        text: format!("{prefix}{text}"),
        text_elements: text_elements
            .into_iter()
            .map(|element| {
                let range = element.byte_range.clone();
                TextElement::new(
                    ByteRange {
                        start: range.start + prefix_len,
                        end: range.end + prefix_len,
                    },
                    element.placeholder().map(str::to_string),
                )
            })
            .collect(),
    }
}

fn render_prompt_context(context: &IdeContext) -> Option<String> {
    let mut ide_context_section = String::new();

    if let Some(active_file) = &context.active_file {
        ide_context_section.push_str(&format!(
            "\n## Active file: {}\n",
            utf8_prefix(&active_file.descriptor.path, MAX_PATH_BYTES)
        ));
    }

    if let Some(active_file) = &context.active_file {
        let selected_ranges = if active_file.selections.is_empty() {
            std::slice::from_ref(&active_file.selection)
        } else {
            active_file.selections.as_slice()
        }
        .iter()
        .filter(|range| range.start != range.end)
        .take(MAX_SELECTION_RANGES)
        .collect::<Vec<_>>();

        if !selected_ranges.is_empty()
            && (active_file.active_selection_content.is_empty() || selected_ranges.len() > 1)
        {
            if selected_ranges.len() == 1 {
                ide_context_section.push_str("\n## Active selection range:\n");
            } else {
                ide_context_section.push_str("\n## Active selection ranges:\n");
            }
            for range in selected_ranges {
                // Render ranges as 1-based positions for the prompt.
                let start_line = range.start.line.saturating_add(1);
                let start_column = range.start.character.saturating_add(1);
                let end_line = range.end.line.saturating_add(1);
                let end_column = range.end.character.saturating_add(1);
                ide_context_section.push_str(&format!(
                    "- {}: line {start_line}, column {start_column} to line {end_line}, column {end_column}\n",
                    utf8_prefix(&active_file.descriptor.path, MAX_PATH_BYTES)
                ));
            }
        }
    }

    if let Some(active_file) = &context.active_file
        && !active_file.active_selection_content.is_empty()
    {
        ide_context_section.push_str("\n## Active selection of the file:\n");
        let selection = active_file.active_selection_content.as_str();
        if let Some((truncate_at, _)) = selection.char_indices().nth(MAX_ACTIVE_SELECTION_CHARS) {
            ide_context_section.push_str(&selection[..truncate_at]);
            ide_context_section.push_str(&format!(
                "\n[Selection truncated to {MAX_ACTIVE_SELECTION_CHARS} characters.]\n"
            ));
        } else {
            ide_context_section.push_str(selection);
        }
    }

    if !context.open_tabs.is_empty() {
        ide_context_section.push_str("\n## Open tabs:\n");
        let mut rendered_tabs = 0;
        let mut rendered_tab_chars = 0;
        for tab in &context.open_tabs {
            if rendered_tabs >= MAX_OPEN_TABS {
                break;
            }

            let tab_line = format!(
                "- {}: {}\n",
                utf8_prefix(&tab.label, MAX_PATH_BYTES),
                utf8_prefix(&tab.path, MAX_PATH_BYTES)
            );
            if rendered_tab_chars + tab_line.len() > MAX_OPEN_TABS_CHARS {
                break;
            }

            ide_context_section.push_str(&tab_line);
            rendered_tabs += 1;
            rendered_tab_chars += tab_line.len();
        }

        let omitted_tabs = context.open_tabs.len() - rendered_tabs;
        if omitted_tabs > 0 {
            ide_context_section.push_str(&format!("[{omitted_tabs} open tabs omitted.]\n"));
        }
    }

    if ide_context_section.is_empty() {
        None
    } else {
        Some(format!(
            "# Context from my IDE setup:\n{ide_context_section}"
        ))
    }
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
