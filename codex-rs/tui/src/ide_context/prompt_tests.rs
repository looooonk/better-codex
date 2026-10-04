use super::super::ActiveFile;
use super::super::FileDescriptor;
use super::super::IdeContext;
use super::super::Position;
use super::super::Range;
use super::*;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn descriptor(label: &str, path: &str) -> FileDescriptor {
    FileDescriptor {
        label: label.to_string(),
        path: path.to_string(),
    }
}

#[test]
fn render_prompt_context_matches_app_format() {
    let context = IdeContext {
        active_file: Some(ActiveFile {
            descriptor: descriptor("lib.rs", "src/lib.rs"),
            selection: Range {
                start: Position {
                    line: 4,
                    character: 0,
                },
                end: Position {
                    line: 6,
                    character: 1,
                },
            },
            active_selection_content: "fn selected() {}".to_string(),
            selections: Vec::new(),
        }),
        open_tabs: vec![
            descriptor("lib.rs", "src/lib.rs"),
            descriptor("main.rs", "src/main.rs"),
        ],
    };

    assert_eq!(
            render_prompt_context(&context),
            Some(
                "# Context from my IDE setup:\n\n## Active file: src/lib.rs\n\n## Active selection of the file:\nfn selected() {}\n## Open tabs:\n- lib.rs: src/lib.rs\n- main.rs: src/main.rs\n"
                    .to_string()
            )
        );
}

#[test]
fn render_prompt_context_omits_empty_context() {
    let context = IdeContext {
        active_file: None,
        open_tabs: Vec::new(),
    };

    assert_eq!(render_prompt_context(&context), None);
}

#[test]
fn apply_ide_context_uses_desktop_prompt_request_delimiter() {
    let context = IdeContext {
        active_file: Some(ActiveFile {
            descriptor: descriptor("lib.rs", "src/lib.rs"),
            selection: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            active_selection_content: String::new(),
            selections: Vec::new(),
        }),
        open_tabs: Vec::new(),
    };
    let text = "Ask $figma".to_string();
    let mut items = vec![
        UserInput::LocalImage {
            path: PathBuf::from("/tmp/screenshot.png"),
            detail: None,
        },
        UserInput::Text {
            text,
            text_elements: vec![TextElement::new(
                ByteRange { start: 4, end: 10 },
                Some("$figma".to_string()),
            )],
        },
    ];

    assert!(apply_ide_context_to_user_input(&context, &mut items));

    let expected_prefix =
        "# Context from my IDE setup:\n\n## Active file: src/lib.rs\n\n## My request for Codex:\n";
    let prefix_len = expected_prefix.len();
    assert_eq!(
        items,
        vec![
            UserInput::LocalImage {
                path: PathBuf::from("/tmp/screenshot.png"),
                detail: None,
            },
            UserInput::Text {
                text: format!("{expected_prefix}Ask $figma"),
                text_elements: vec![TextElement::new(
                    ByteRange {
                        start: prefix_len + 4,
                        end: prefix_len + 10,
                    },
                    Some("$figma".to_string()),
                )],
            },
        ]
    );
}

#[test]
fn extract_prompt_request_returns_text_after_last_delimiter() {
    let message =
        "# Context\n## My request for Codex:\nFirst\n## My request for Codex:\n  Second\n";

    assert_eq!(
        extract_prompt_request_with_offset(message),
        ("Second", message.find("Second").expect("request offset"))
    );
}

#[test]
fn render_prompt_context_includes_selection_ranges_without_content() {
    let first_range = Range {
        start: Position {
            line: 1,
            character: 2,
        },
        end: Position {
            line: 1,
            character: 5,
        },
    };
    let second_range = Range {
        start: Position {
            line: 3,
            character: 0,
        },
        end: Position {
            line: 4,
            character: 1,
        },
    };
    let context = IdeContext {
        active_file: Some(ActiveFile {
            descriptor: descriptor("lib.rs", "src/lib.rs"),
            selection: first_range.clone(),
            active_selection_content: String::new(),
            selections: vec![first_range, second_range],
        }),
        open_tabs: Vec::new(),
    };

    assert_eq!(
            render_prompt_context(&context),
            Some(
                "# Context from my IDE setup:\n\n## Active file: src/lib.rs\n\n## Active selection ranges:\n- src/lib.rs: line 2, column 3 to line 2, column 6\n- src/lib.rs: line 4, column 1 to line 5, column 2\n"
                    .to_string()
            )
        );
}

#[test]
fn render_prompt_context_truncates_large_selection() {
    let context = IdeContext {
        active_file: Some(ActiveFile {
            descriptor: descriptor("large.txt", "large.txt"),
            selection: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 1,
                },
            },
            active_selection_content: format!("{}tail", "a".repeat(MAX_ACTIVE_SELECTION_CHARS)),
            selections: Vec::new(),
        }),
        open_tabs: Vec::new(),
    };

    let rendered = render_prompt_context(&context).expect("rendered IDE context");
    assert!(rendered.contains(&format!(
        "[Selection truncated to {MAX_ACTIVE_SELECTION_CHARS} characters.]"
    )));
    assert!(!rendered.contains("tail"));
}

#[test]
fn render_prompt_context_omits_excess_open_tabs() {
    let open_tabs = (0..MAX_OPEN_TABS + 2)
        .map(|index| descriptor(&format!("file-{index}.rs"), &format!("src/file-{index}.rs")))
        .collect::<Vec<_>>();
    let context = IdeContext {
        active_file: None,
        open_tabs,
    };

    let rendered = render_prompt_context(&context).expect("rendered IDE context");
    assert!(rendered.contains("- file-19.rs: src/file-19.rs\n"));
    assert!(!rendered.contains("- file-20.rs: src/file-20.rs\n"));
    assert!(rendered.contains("[2 open tabs omitted.]\n"));
}

#[test]
fn unicode_context_has_a_total_byte_cap_without_changing_request_or_attachments() {
    let context = IdeContext {
        active_file: Some(ActiveFile {
            descriptor: descriptor(
                "文件.rs",
                &format!("/workspace/{}/文件.rs", "目录/".repeat(2_000)),
            ),
            selection: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: u32::MAX,
                    character: u32::MAX,
                },
            },
            active_selection_content: "🦀界".repeat(10_000),
            selections: Vec::new(),
        }),
        open_tabs: vec![descriptor("标签", &"目录/".repeat(4_000)); 200],
    };
    let request = format!("é $skill\n{}", "user input stays complete\n".repeat(1_000));
    let image = UserInput::LocalImage {
        path: "/tmp/diagram.png".into(),
        detail: None,
    };
    let elements = vec![TextElement::new(
        ByteRange { start: 3, end: 9 },
        Some("$skill".to_string()),
    )];
    let mut items = vec![
        image.clone(),
        UserInput::Text {
            text: request.clone(),
            text_elements: elements,
        },
    ];
    assert!(apply_ide_context_to_user_input(&context, &mut items));
    let UserInput::Text {
        text,
        text_elements,
    } = &items[1]
    else {
        panic!("text input");
    };
    let prefix_len = text.len() - request.len();
    assert!(prefix_len <= MAX_PREFIX_BYTES);
    assert!(text.is_char_boundary(prefix_len));
    assert!(text[..prefix_len].contains("[IDE context truncated.]"));
    assert_eq!(&text[prefix_len..], request);
    assert_eq!(items[0], image);
    assert_eq!(
        text_elements,
        &vec![TextElement::new(
            ByteRange {
                start: prefix_len + 3,
                end: prefix_len + 9
            },
            Some("$skill".to_string())
        )]
    );
    assert_eq!(
        extract_prompt_request_with_offset(text),
        (request.trim(), prefix_len)
    );
}

#[test]
fn image_only_submission_keeps_its_attachment_after_the_context() {
    let context = IdeContext {
        active_file: None,
        open_tabs: vec![descriptor("lib.rs", "src/lib.rs")],
    };
    let image = UserInput::LocalImage {
        path: "/tmp/diagram.png".into(),
        detail: None,
    };
    let mut items = vec![image.clone()];
    assert!(apply_ide_context_to_user_input(&context, &mut items));
    assert_eq!(items, vec![UserInput::Text { text: "# Context from my IDE setup:\n\n## Open tabs:\n- lib.rs: src/lib.rs\n\n## My request for Codex:\n".to_string(), text_elements: Vec::new() }, image]);
}

#[test]
fn selection_ranges_are_bounded_and_maximum_positions_do_not_overflow() {
    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: u32::MAX,
            character: u32::MAX,
        },
    };
    let context = IdeContext {
        active_file: Some(ActiveFile {
            descriptor: descriptor("lib.rs", "src/lib.rs"),
            selection: range.clone(),
            selections: vec![range; 10_000],
            active_selection_content: String::new(),
        }),
        open_tabs: Vec::new(),
    };
    let rendered = render_prompt_context(&context).unwrap();
    assert_eq!(
        rendered.matches("- src/lib.rs:").count(),
        MAX_SELECTION_RANGES
    );
    assert!(rendered.contains("line 4294967295, column 4294967295"));
}
