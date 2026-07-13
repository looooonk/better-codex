/// Split structured reasoning-summary parts into the status header and renderable content.
pub(crate) fn split_reasoning_summary_parts(reasoning_parts: &[String]) -> (String, String) {
    let mut leading_empty_part_header = None;
    let mut content_parts = Vec::with_capacity(reasoning_parts.len());

    for part in reasoning_parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let header_end = part.strip_prefix("**").and_then(|after_open| {
            after_open
                .find("**")
                .and_then(|close| (close > 0).then_some(close + 4))
        });
        let body = header_end.map_or(part, |header_end| &part[header_end..]);
        if body.trim() == "<!-- -->" {
            if content_parts.is_empty()
                && leading_empty_part_header.is_none()
                && let Some(header_end) = header_end
            {
                leading_empty_part_header = Some(part[..header_end].to_string());
            }
            continue;
        }

        content_parts.push(part);
    }

    let content = content_parts.join("\n\n");
    if content.is_empty() {
        return (leading_empty_part_header.unwrap_or_default(), content);
    }

    if let Some(after_open) = content.strip_prefix("**")
        && let Some(close) = after_open.find("**")
    {
        let after_close_idx = 2 + close + 2;
        let after_close = &content[after_close_idx..];
        if after_close.starts_with('\n') || after_close.starts_with('\r') {
            return (
                content[..after_close_idx].to_string(),
                after_close.to_string(),
            );
        }
    }

    (leading_empty_part_header.unwrap_or_default(), content)
}
