use super::*;
use pretty_assertions::assert_eq;

#[test]
fn oversized_destinations_remain_plain_text() {
    let prefix = "https://example.com/";
    let destination = format!(
        "{prefix}{}",
        "a".repeat(MAX_HYPERLINK_DESTINATION_BYTES - prefix.len())
    );
    assert_eq!(web_destination(&destination), Some(destination.clone()));
    let oversized = format!("{destination}é");
    assert_eq!(web_destination(&oversized), None);
    assert_eq!(osc8_hyperlink(&oversized, "visible"), "visible");

    // Explicit Markdown links can reach the cell marker without bare-URL detection.
    let line = HyperlinkLine {
        source: None,
        synthetic_prefix_width: 0,
        line: Line::from("visible"),
        hyperlinks: vec![TerminalHyperlink::web(/*columns*/ 0..7, oversized)],
    };
    let mut buf = Buffer::with_lines(["visible"]);
    let expected = buf.clone();
    let area = buf.area;
    mark_buffer_hyperlinks(&mut buf, area, &[line], /*scroll_rows*/ 0);
    assert_eq!(buf, expected);
}

#[test]
fn discovers_many_web_urls_with_incremental_columns() {
    let destination = "https://x.io";
    let token = format!("👩‍💻 ({destination}).\u{2003}");
    let text = token.repeat(/*n*/ 50_000);
    let expected = (0..50_000)
        .map(|index| {
            let start = index * display_width(&token) + 4;
            TerminalHyperlink::web(start..start + destination.len(), destination.to_string())
        })
        .collect::<Vec<_>>();
    assert_eq!(web_links_in_text(&text), expected);
}

#[test]
fn trims_long_runs_of_unmatched_closing_delimiters() {
    let destination = "https://example.com/a_(b)[c]{d}<e>";
    let text = format!("{destination}{}", ")]}>".repeat(/*n*/ 50_000));
    assert_eq!(
        web_links_in_text(&text),
        vec![TerminalHyperlink::web(
            /*columns*/ 0..destination.len(),
            destination.to_string(),
        )]
    );
}

#[test]
fn decorated_hyperlinks_preserve_joined_emoji_columns() {
    let destination = "https://example.test/👩‍💻/path";
    let line = annotate_web_urls_in_line(Line::from(format!("👩‍💻 {destination} tail")));
    assert_eq!(
        decorate_spans(&line),
        vec![Span::from(format!(
            "👩‍💻 {} tail",
            osc8_hyperlink(destination, destination)
        ))]
    );
}

#[test]
fn source_wrapping_clips_a_cursor_sentinel_at_the_input_boundary() {
    let destination = "https://example.test/pull/9410";
    let text = format!("› [configuration editor]({destination})");
    let source = annotate_web_urls_in_line(Line::from(text.clone()));
    let mut wrapped = crate::wrapping::word_wrap_line_with_source(&source.line, text.len());
    wrapped.last_mut().expect("wrapped line").range.end += 1;
    let actual = remap_source_wrapped_line(&source, wrapped);
    let mut expected = source;
    expected.source = Some(LogicalLineSource::from_line(&expected.line));

    assert!(lines_with_sources_eq(&actual, &[expected]));
}

#[test]
fn cursor_sentinels_preserve_wrapped_unicode_links_and_copy_gutters() {
    let mut source = annotate_web_urls_in_line(Line::from("  é界 https://example.test/👩‍💻/path"));
    source.synthetic_prefix_width = 2;
    let options = RtOptions::new(12).subsequent_indent(Line::from("    "));
    let expected = remap_source_wrapped_line(
        &source,
        crate::wrapping::word_wrap_line_with_source(&source.line, options.clone()),
    );
    let mut wrapped = crate::wrapping::word_wrap_line_with_source(&source.line, options);
    wrapped.last_mut().expect("wrapped line").range.end += 1;
    let actual = remap_source_wrapped_line(&source, wrapped);

    assert!(lines_with_sources_eq(&actual, &expected));
    assert_eq!(
        actual
            .iter()
            .map(|line| line.synthetic_prefix_width)
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|line| line.synthetic_prefix_width)
            .collect::<Vec<_>>()
    );
    assert_eq!(actual[0].synthetic_prefix_width, 2);
}

#[test]
fn cursor_only_wrapped_row_has_empty_source_and_no_hyperlinks() {
    let source = HyperlinkLine::from("");
    let actual = remap_source_wrapped_line(
        &source,
        vec![crate::wrapping::WrappedLine {
            line: Line::from("  "),
            range: 1..1,
            prefix_bytes: 2,
        }],
    );
    let mut expected = HyperlinkLine::from("  ");
    let mut logical = LogicalLineSource::from_line(&source.line);
    logical.prefix_bytes = 2;
    expected.source = Some(logical);
    assert!(lines_with_sources_eq(&actual, &[expected]));
    assert_eq!(actual[0].synthetic_prefix_width, 0);
}
