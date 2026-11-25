//! A temporary single-width glyph lets the Markdown renderer wrap each formula as a word.
//! Only the transcript's visual copy uses these glyphs. Each resulting run owns the TeX source.
use super::CellSize;
use super::Formula;
use super::raster;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::width::display_width;
use lru::LruCache;
use ratatui::style::Color;
use std::borrow::Cow;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

pub(crate) struct Measurements(LruCache<(Arc<str>, bool), Option<raster::Metrics>>);
impl Default for Measurements {
    fn default() -> Self {
        Self(LruCache::new(const { NonZeroUsize::new(256).unwrap() }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MathRun {
    pub formula: Arc<Formula>,
    pub x: u16,
    pub row: u16,
    pub marker: char,
}

pub(crate) struct MarkedText<'a> {
    pub text: Cow<'a, str>,
    formulas: HashMap<char, Arc<Formula>>,
    width: u16,
}

impl Measurements {
    pub fn mark<'a>(
        &mut self,
        text: &'a str,
        cell: CellSize,
        width: u16,
        color: Color,
    ) -> MarkedText<'a> {
        let mut marked = MarkedText {
            text: Cow::Borrowed(text),
            formulas: HashMap::new(),
            width,
        };
        if !(text.contains('$') || text.contains("\\(") || text.contains("\\[")) {
            return marked;
        }
        let Color::Rgb(r, g, b) = color else {
            return marked;
        };
        let Color::Rgb(br, bg, bb) = crate::app_theme::palette().base else {
            return marked;
        };
        let colors = raster::MathColors {
            foreground: [r, g, b],
            background: [br, bg, bb],
        };
        if text
            .chars()
            .any(|c| (0xf0000..0xffffe).contains(&(c as u32)))
            || text.contains("&#")
        {
            return marked;
        }
        let mut out = String::new();
        let mut last = 0;
        // Pick markers outside all literal source characters (including adjacent equations).
        let mut markers = (0xf0000..0xffffe).filter_map(char::from_u32);
        for (range, display) in crate::markdown_render::formula_ranges(text) {
            let source: Arc<str> = text[range.clone()].into();
            let key = (source.clone(), display);
            let Some(metrics) = *self
                .0
                .get_or_insert_ref(&key, || raster::metrics(&source, display))
            else {
                continue;
            };
            let Some(formula) = Formula::measure(metrics, source, display, cell, width, colors)
            else {
                continue;
            };
            let Some(marker) = markers.next() else {
                break;
            };
            out.push_str(&text[last..range.start]);
            if display {
                // Markdown list continuations need their original indentation after the
                // paragraph break, or a nested equation will terminate the list.
                let prefix = text[..range.start].rsplit('\n').next().unwrap_or_default();
                out.push_str("\n\n");
                if prefix.chars().all(|ch| ch == ' ' || ch == '\t') {
                    out.push_str(prefix);
                }
            }
            out.extend(std::iter::repeat_n(marker, usize::from(formula.columns)));
            if display {
                out.push_str("\n\n");
            }
            last = range.end;
            marked.formulas.insert(marker, Arc::new(formula));
        }
        if last > 0 {
            out.push_str(&text[last..]);
            marked.text = Cow::Owned(out);
        }
        marked
    }
}

impl MarkedText<'_> {
    /// If a narrow Markdown container split a formula, use the normal text layout for the item.
    pub fn expand(&self, lines: Vec<HyperlinkLine>) -> Option<Vec<HyperlinkLine>> {
        if self.formulas.is_empty() {
            return Some(lines);
        }
        let mut out = Vec::with_capacity(lines.len());
        let mut accounted = 0;
        for mut line in lines {
            let mut runs = Vec::new();
            let mut col = 0usize;
            for span in &line.line.spans {
                let mut chars = span.content.char_indices().peekable();
                let mut start = 0;
                while let Some((offset, ch)) = chars.next() {
                    let Some(formula) = self.formulas.get(&ch) else {
                        continue;
                    };
                    col += display_width(&span.content[start..offset]);
                    let mut count = 1;
                    while chars.next_if(|(_, next)| *next == ch).is_some() {
                        count += 1;
                    }
                    if count != usize::from(formula.columns) {
                        return None;
                    }
                    runs.push(MathRun {
                        formula: formula.clone(),
                        x: col.try_into().ok()?,
                        row: formula.above,
                        marker: ch,
                    });
                    accounted += 1;
                    start = offset + count * ch.len_utf8();
                    col += count;
                }
                col += display_width(&span.content[start..]);
            }
            if runs.is_empty() {
                out.push(line);
                continue;
            }
            // Display math owns a paragraph. Align its entire box within the current
            // text body and keep the added margin out of semantic selection and copy.
            if let [run] = runs.as_mut_slice()
                && run.formula.display
            {
                let centered = self.width.saturating_sub(run.formula.columns) / 2;
                let padding = usize::from(centered.saturating_sub(run.x));
                if padding > 0 {
                    line.line.spans.insert(0, " ".repeat(padding).into());
                    line.synthetic_prefix_width += padding;
                    if let Some(source) = &mut line.source {
                        source.prefix_bytes += padding;
                    }
                    for link in &mut line.hyperlinks {
                        link.columns = link.columns.start + padding..link.columns.end + padding;
                    }
                    run.x += padding as u16;
                }
            }
            let above = runs.iter().map(|r| r.formula.above).max().unwrap_or(0);
            let below = runs.iter().map(|r| r.formula.below).max().unwrap_or(0);
            for y in 0..=above + below {
                let mut row = if y == above {
                    line.clone()
                } else {
                    HyperlinkLine::default()
                };
                row.math = Some(
                    runs.iter()
                        .filter_map(|run| {
                            let local = (y + run.formula.above).checked_sub(above)?;
                            (local < run.formula.rows()).then(|| MathRun {
                                row: local,
                                ..run.clone()
                            })
                        })
                        .collect::<Vec<_>>()
                        .into(),
                );
                out.push(row);
            }
        }
        (accounted == self.formulas.len()).then_some(out)
    }
}

/// Map selected render-only tokens back to their original delimited Markdown.
pub(crate) fn copy_source(text: String, runs: &[&MathRun]) -> String {
    if runs.is_empty() {
        return text;
    }
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut remaining = runs.iter().peekable();
    while let Some(ch) = chars.next() {
        if let Some(run) = remaining.next_if(|run| run.marker == ch) {
            while chars.next_if_eq(&ch).is_some() {}
            out.push_str(&run.formula.source);
        } else {
            out.push(ch);
        }
    }
    out
}
