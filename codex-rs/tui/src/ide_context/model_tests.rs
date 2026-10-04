use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn deserializes_existing_ide_context_shape() {
    let value = json!({
        "activeFile": {
            "label": "lib.rs",
            "path": "src/lib.rs",
            "fsPath": "/repo/src/lib.rs",
            "selection": {
                "start": { "line": 1, "character": 2 },
                "end": { "line": 3, "character": 4 }
            },
            "activeSelectionContent": "selected",
            "selections": []
        },
        "openTabs": [
            {
                "label": "main.rs",
                "path": "src/main.rs",
                "fsPath": "/repo/src/main.rs",
                "startLine": 2,
                "endLine": 10
            }
        ],
        "processEnv": {
            "path": "/usr/bin"
        }
    });

    let context: IdeContext = serde_json::from_value(value).expect("deserialize ide context");
    assert_eq!(
        context,
        IdeContext {
            active_file: Some(ActiveFile {
                descriptor: FileDescriptor {
                    label: "lib.rs".to_string(),
                    path: "src/lib.rs".to_string(),
                },
                selection: Range {
                    start: Position {
                        line: 1,
                        character: 2,
                    },
                    end: Position {
                        line: 3,
                        character: 4,
                    },
                },
                active_selection_content: "selected".to_string(),
                selections: Vec::new(),
            }),
            open_tabs: vec![FileDescriptor {
                label: "main.rs".to_string(),
                path: "src/main.rs".to_string(),
            }],
        }
    );
}
