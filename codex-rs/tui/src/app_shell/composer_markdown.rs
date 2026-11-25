//! Style source bytes, including delimiters, without changing the editable text.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::style::Style;

pub(super) struct SourceStyles {
    changes: std::iter::Peekable<std::vec::IntoIter<(usize, Style)>>,
    current: Style,
}

impl SourceStyles {
    pub(super) fn new(source: &str) -> Self {
        let mut changes = Vec::new();
        if source.contains(['*', '_', '`', '~']) {
            let styles = super::super::design::markdown_styles();
            let mut stack = vec![Style::new()];
            for (event, range) in Parser::new(source).into_offset_iter() {
                let parent = *stack.last().expect("base style");
                match event {
                    Event::Start(tag @ (Tag::Strong | Tag::Emphasis | Tag::CodeBlock(_))) => {
                        let style = parent.patch(match tag {
                            Tag::Strong => styles.strong,
                            Tag::Emphasis => styles.emphasis,
                            _ => styles.code,
                        });
                        changes.push((range.start, style));
                        stack.push(style);
                    }
                    Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::CodeBlock) => {
                        stack.pop();
                        changes.push((range.end, *stack.last().expect("base style")));
                    }
                    Event::Code(_) => {
                        changes.push((range.start, parent.patch(styles.code)));
                        changes.push((range.end, parent));
                    }
                    _ => {}
                }
            }
        }
        Self { changes: changes.into_iter().peekable(), current: Style::new() }
    }

    pub(super) fn at(&mut self, offset: usize) -> Style {
        while let Some((_, style)) = self.changes.next_if(|(start, _)| *start <= offset) {
            self.current = style;
        }
        self.current
    }
}
