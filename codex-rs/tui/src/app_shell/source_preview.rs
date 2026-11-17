//! Read-only source navigation for file citations in the active conversation.

use super::scrollback_view::render_scrollback_frame;
use crate::markdown_render::FileLink;
use crate::markdown_render::SourceLocation;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::FsReadFileParams;
use codex_app_server_protocol::FsReadFileResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadEnvironment;
use codex_app_server_protocol::ThreadReadParams;
use codex_protocol::ThreadId;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::LegacyAppPathString;
use codex_utils_path_uri::PathConvention;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::ensure;
use color_eyre::eyre::eyre;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use serde::Deserialize;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_LINES: usize = 10_000;

#[derive(Debug)]
pub(super) struct SourcePreview {
    path: String,
    lines: Vec<Line<'static>>,
    target_x: usize,
    target: SourceLocation,
    top: usize,
}

pub(super) async fn read(
    client: AppServerRequestHandle,
    thread_id: ThreadId,
    link: FileLink,
) -> Result<SourcePreview> {
    #[derive(Deserialize)]
    struct Workspace {
        environments: Option<Vec<ThreadEnvironment>>,
    }
    #[derive(Deserialize)]
    struct Response {
        thread: Workspace,
    }
    let response: Response = client
        .request_typed(ClientRequest::ThreadRead {
            request_id: RequestId::String(Uuid::new_v4().to_string()),
            params: ThreadReadParams {
                thread_id: thread_id.to_string(),
                include_turns: false,
            },
        })
        .await?;
    // fs/readFile is served by the app-server host. A same-named path on a separate
    // execution host can refer to an entirely different file.
    let environments = response.thread.environments.unwrap_or_default();
    let cwd = environments
        .first()
        .filter(|_| {
            environments
                .iter()
                .all(|env| env.environment_id == codex_exec_server::LOCAL_ENVIRONMENT_ID)
        })
        .and_then(|env| env.cwd.to_inferred_path_uri())
        .ok_or_else(|| {
            eyre!("Source previews require a workspace on the connected app-server host")
        })?;
    let resolved = LegacyAppPathString::from_string(link.path.clone())
        .resolve_against(&cwd, /*user_home_dir*/ None)?;
    ensure!(
        resolved.infer_path_convention() == Some(PathConvention::Posix)
            && !resolved.to_url().has_host(),
        "Source previews require a macOS or Linux workspace"
    );
    let response: FsReadFileResponse = client
        .request_typed(ClientRequest::FsReadFile {
            request_id: RequestId::String(Uuid::new_v4().to_string()),
            params: FsReadFileParams {
                path: AbsolutePathBuf::from_absolute_path(resolved.inferred_native_path_string())?,
            },
        })
        .await?;
    ensure!(
        response.data_base64.len() <= MAX_SOURCE_BYTES.div_ceil(3) * 4,
        "Source preview exceeds 1 MiB"
    );
    let bytes = STANDARD
        .decode(response.data_base64)
        .wrap_err("Invalid file contents")?;
    SourcePreview::new(link, bytes)
}

impl SourcePreview {
    pub(super) fn new(link: FileLink, bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_SOURCE_BYTES,
            "Source preview exceeds 1 MiB"
        );
        let source = String::from_utf8(bytes).wrap_err("Source preview requires UTF-8 text")?;
        ensure!(
            !source
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')),
            "This file contains binary data"
        );
        let line_count = source.lines().count();
        ensure!(
            line_count <= MAX_SOURCE_LINES,
            "Source preview exceeds 10,000 lines"
        );
        let mut target = link
            .location
            .unwrap_or(SourceLocation { line: 1, column: 1 });
        target.line = target.line.clamp(1, line_count.max(1));
        // Citation columns count Unicode characters. Snap a column inside a grapheme to its start.
        let line = source.lines().nth(target.line - 1).unwrap_or_default();
        let byte = line
            .char_indices()
            .nth(target.column.saturating_sub(1))
            .map_or(line.len(), |(i, _)| i);
        let target_x = line
            .grapheme_indices(true)
            .take_while(|(i, g)| i + g.len() <= byte)
            .map(|(_, g)| if g == "\t" { 4 } else { g.width() })
            .sum();
        let filename = link.path.rsplit('/').next().unwrap_or(&link.path);
        let language = filename.rsplit_once('.').map_or(filename, |(_, ext)| ext);
        let source = source
            .replace("\r\n", "\n")
            .replace('\r', "")
            .replace('\t', "    ");
        let lines = crate::render::highlight::highlight_code_to_lines(&source, language);
        let top = target.line.saturating_sub(4);
        Ok(Self {
            path: link.path,
            lines,
            target,
            target_x,
            top,
        })
    }

    pub(super) fn scroll(&mut self, key: KeyCode) {
        self.top = match key {
            KeyCode::Up | KeyCode::Char('k') => self.top.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.top.saturating_add(1),
            KeyCode::PageUp => self.top.saturating_sub(12),
            KeyCode::PageDown => self.top.saturating_add(12),
            KeyCode::Home => 0,
            KeyCode::End => self.lines.len(),
            _ => self.top,
        }
        .min(self.lines.len().saturating_sub(1));
    }

    pub(super) fn render(&self, area: Rect, buf: &mut Buffer) {
        let frame = render_scrollback_frame(area, " Source ", buf);
        Paragraph::new(format!(
            "{}:{}:{}",
            self.path, self.target.line, self.target.column
        ))
        .render(frame.header, buf);
        let rows = self
            .lines
            .iter()
            .enumerate()
            .skip(self.top)
            .take(usize::from(frame.body.height));
        let width = usize::from(frame.body.width).saturating_sub(7);
        let mut visible = Vec::new();
        for (index, text) in rows {
            let number = index + 1;
            let line_number = Span::from(format!("{number:>5}  "));
            let mut spans = vec![if number == self.target.line {
                line_number.bold()
            } else {
                line_number.dim()
            }];
            if number == self.target.line {
                let left = if self.target_x >= width {
                    self.target_x.saturating_sub(width / 2)
                } else {
                    0
                };
                let mut x = 0;
                for grapheme in text.styled_graphemes(Style::default()) {
                    if x >= left.saturating_add(width) {
                        break;
                    }
                    if x >= left {
                        let span = Span::styled(grapheme.symbol.to_owned(), grapheme.style);
                        spans.push(if x == self.target_x {
                            span.reversed()
                        } else {
                            span
                        });
                    }
                    x += grapheme.symbol.width();
                }
                if x == self.target_x {
                    spans.push(" ".reversed());
                }
            } else {
                spans.extend(text.spans.iter().cloned());
            }
            visible.push(Line::from(spans));
        }
        Paragraph::new(visible).render(frame.body, buf);
        Paragraph::new("↑/↓ scroll   Esc close")
            .dim()
            .render(frame.footer, buf);
    }
}

#[cfg(test)]
#[path = "source_preview_tests.rs"]
mod tests;
