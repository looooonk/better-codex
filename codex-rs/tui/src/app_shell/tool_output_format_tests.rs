use super::*;
use pretty_assertions::assert_eq;

fn plain(lines: TranscriptLines) -> String {
    lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn expands_nested_execution_results_and_concatenated_json() {
    let output = r#"{"i":0,"value":{"exit_code":0,"output":"first line\n  second line\n"}}{"i":1,"value":{"output":"{\"ok\":true,\"count\":2}"}}"#;
    insta::assert_snapshot!(plain(document_lines(
        "functions.exec",
        output,
        /*width*/ 64,
        OutputFormat::Readable
    )));
    let original = plain(document_lines(
        "functions.exec",
        output,
        /*width*/ 500,
        OutputFormat::Original,
    ));
    assert!(original.ends_with(output));
}

#[test]
fn preserves_source_escapes_indentation_blank_lines_and_terminal_color() {
    let output = "    let message = \"hello\\nworld\";\n\n\u{1b}[32m  passed\u{1b}[0m";
    assert_eq!(
        readable_output(output, "exec cat source.rs").1,
        codex_ansi_escape::ansi_escape(output).lines,
    );
}

#[test]
fn decodes_only_valid_json_strings() {
    assert_eq!(
        plain(json_lines(r#""first\n\tsecond""#, /*depth*/ 0).unwrap().1),
        "first\n    second",
    );
    assert!(json_lines(r#"{"incomplete": "first\n"#, /*depth*/ 0).is_none());
    assert!(json_lines(r"literal\nsource", /*depth*/ 0).is_none());
}

#[test]
fn supports_search_locations_with_columns_spaces_and_windows_paths() {
    insta::assert_snapshot!(plain(document_lines(
        "exec rg -n warning .",
        "src/config.rs:304:    warning with a long explanation that wraps across the pane\nC:\\work dir\\config.rs:12:9:  another match\n36: a match without a filename\n17:\nfinal diagnostic",
        /*width*/ 48,
        OutputFormat::Readable,
    )));
}

#[test]
fn leaves_non_search_numeric_output_alone() {
    let output = "12:30:45 job started\n1:2:3";
    assert_eq!(
        readable_output(output, "exec logs").1,
        codex_ansi_escape::ansi_escape(output).lines
    );
}

#[test]
fn oversized_pretty_json_falls_back_without_losing_content() {
    let output = serde_json::to_string(&vec![0; MAX_FORMATTED_LINES]).unwrap();
    assert!(json_lines(&output, /*depth*/ 0).is_none());
    assert_eq!(
        readable_output(&output, "exec report").1,
        codex_ansi_escape::ansi_escape(&output).lines
    );
}

#[test]
fn long_source_lines_keep_their_indentation_when_wrapped() {
    insta::assert_snapshot!(plain(document_lines(
        "exec cat source.rs",
        "fn example() {\n    let result = process_configuration_with_a_long_name(first_argument, second_argument);\n\n    result\n}",
        /*width*/ 42,
        OutputFormat::Readable,
    )));
}
