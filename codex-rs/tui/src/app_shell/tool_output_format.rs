use super::super::design::palette;
use super::super::terminal_output;
use crate::render::highlight::highlight_code_to_lines;
use crate::render::line_utils::prefix_lines;
use crate::session_transcript::TranscriptLines;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use ratatui::style::Stylize;
use ratatui::text::Line;
use regex_lite::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const MAX_FORMATTED_BYTES: usize = 512 * 1024;
const MAX_FORMATTED_LINES: usize = 8_000;

static SEARCH_MATCH: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(?:(.+?):)?([0-9]+)(?::([0-9]+))?:(.*)$").ok());

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum OutputFormat {
    #[default]
    Readable,
    Original,
}

pub(super) fn document_lines(
    title: &str,
    output: &str,
    width: usize,
    format: OutputFormat,
) -> TranscriptLines {
    let width = width.max(/*other*/ 1);
    let mut lines = vec![Line::from("Tool call").fg(palette::cyan()).bold()];
    let title = terminal_output::normalize(title);
    let call = if title.starts_with("exec ") {
        highlight_code_to_lines(&title, "bash")
    } else {
        codex_ansi_escape::ansi_escape(&title).lines
    };
    lines.extend(
        word_wrap_lines(
            call,
            RtOptions::new(width)
                .initial_indent(Line::from("│ ".fg(palette::cyan())))
                .subsequent_indent(Line::from("│ ".fg(palette::cyan()))),
        )
        .into_iter()
        .map(|line| line.bg(palette::elevated())),
    );
    lines.push(Line::default());
    let (label, output_lines) = match format {
        OutputFormat::Readable => readable_output(output, &title),
        OutputFormat::Original => ("Original", codex_ansi_escape::ansi_escape(output).lines),
    };
    lines.push(Line::from(vec![
        "Output".fg(palette::cyan()).bold(),
        format!(" · {label}").fg(palette::muted()),
    ]));
    if output.is_empty() {
        lines.push(Line::from("No output captured.").fg(palette::muted()));
    } else {
        for line in output_lines {
            let indent = line
                .spans
                .iter()
                .flat_map(|span| span.content.chars())
                .take_while(|character| *character == ' ')
                .count()
                .min(width / 3);
            let continuation = format!("↪ {}", " ".repeat(indent));
            lines.extend(word_wrap_lines(
                [line],
                RtOptions::new(width)
                    .word_splitter(textwrap::WordSplitter::NoHyphenation)
                    .subsequent_indent(Line::from(continuation.fg(palette::muted()))),
            ));
        }
    }
    lines
}

fn readable_output(output: &str, title: &str) -> (&'static str, TranscriptLines) {
    if let Some((label, lines)) = json_lines(output, /*depth*/ 0) {
        return (label, lines);
    }
    let search = title
        .split([' ', '\n', '\'', '"', ';', '|'])
        .any(|word| matches!(word.rsplit('/').next(), Some("rg" | "grep")));
    let mut lines = Vec::new();
    let mut formatted_search = false;
    for line in codex_ansi_escape::ansi_escape(output).lines {
        let text = line.to_string();
        if search
            && let Some(captures) = SEARCH_MATCH
                .as_ref()
                .and_then(|pattern| pattern.captures(&text))
        {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            let path = captures.get(1).map_or("", |value| value.as_str());
            let row = &captures[2];
            let column = captures.get(3).map(|value| value.as_str());
            let location = if path.is_empty() {
                format!("Line {row}")
            } else {
                format!("{path}:{row}")
            };
            let location =
                column.map_or_else(|| location.clone(), |column| format!("{location}:{column}"));
            lines.push(Line::from(location).fg(palette::cyan()).bold());
            lines.push(Line::from(format!("  {}", captures[4].trim_start())));
            formatted_search = true;
        } else if let Some((_, json)) = json_lines(&text, /*depth*/ 0) {
            lines.extend(json);
        } else {
            lines.push(line);
        }
        if lines.len() > MAX_FORMATTED_LINES {
            return ("Text", codex_ansi_escape::ansi_escape(output).lines);
        }
    }
    (
        if formatted_search {
            "Search results"
        } else {
            "Text"
        },
        lines,
    )
}

fn json_lines(text: &str, depth: usize) -> Option<(&'static str, TranscriptLines)> {
    if depth >= 3 || !text.trim_start().starts_with(['{', '[', '"']) {
        return None;
    }
    let values = serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .take(/*n*/ 257)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.is_empty() || values.len() > 256 {
        return None;
    }
    let mut lines = Vec::new();
    let mut label = "JSON";
    let mut total_bytes = 0;
    for value in values {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        if let Value::String(decoded) = value {
            let (inner_label, inner_lines) = json_lines(&decoded, depth + 1).unwrap_or_else(|| {
                (
                    "Decoded text",
                    codex_ansi_escape::ansi_escape(&terminal_output::normalize(&decoded)).lines,
                )
            });
            label = inner_label;
            lines.extend(inner_lines);
        } else {
            let mut bytes = Vec::new();
            serde_json::to_writer_pretty(LimitedWriter(&mut bytes), &value).ok()?;
            total_bytes += bytes.len();
            if total_bytes > MAX_FORMATTED_BYTES {
                return None;
            }
            let pretty = String::from_utf8(bytes).ok()?;
            if pretty.lines().count() > MAX_FORMATTED_LINES {
                return None;
            }
            let highlighted = highlight_code_to_lines(&pretty, "json");
            for (source, styled) in pretty.lines().zip(highlighted) {
                let start = source.find("\": ").map_or_else(
                    || source.len() - source.trim_start().len(),
                    |index| index + 3,
                );
                // Only decode complete JSON string values, not source-code escapes.
                let literal = source[start..]
                    .strip_suffix(',')
                    .unwrap_or(&source[start..]);
                let decoded = serde_json::from_str::<String>(literal).ok();
                let expanded = decoded.as_deref().and_then(|decoded| {
                    json_lines(decoded, depth + 1)
                        .map(|(_, lines)| lines)
                        .or_else(|| {
                            decoded.contains(['\n', '\r', '\t']).then(|| {
                                codex_ansi_escape::ansi_escape(&terminal_output::normalize(decoded))
                                    .lines
                            })
                        })
                });
                if let Some(expanded) = expanded {
                    label = "JSON · expanded text";
                    lines.push(Line::from(vec![
                        source[..start].to_string().fg(palette::cyan()),
                        "Text".fg(palette::muted()),
                    ]));
                    let indent = source.len() - source.trim_start().len();
                    let prefix = format!("{}│ ", " ".repeat(indent + 2)).fg(palette::muted());
                    lines.extend(prefix_lines(expanded, prefix.clone(), prefix));
                } else {
                    lines.push(styled);
                }
                if lines.len() > MAX_FORMATTED_LINES {
                    return None;
                }
            }
        }
        if lines.len() > MAX_FORMATTED_LINES {
            return None;
        }
    }
    Some((label, lines))
}

struct LimitedWriter<'a>(&'a mut Vec<u8>);

impl std::io::Write for LimitedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_FORMATTED_BYTES {
            return Err(std::io::Error::other("formatted output limit reached"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[path = "tool_output_format_tests.rs"]
mod tests;
