use super::*;
use pretty_assertions::assert_eq;

#[test]
fn foreign_read_preserves_target_native_path_and_other_command_actions() {
    #[cfg(windows)]
    let cwd = PathUri::parse("file:///usr/local/src").expect("valid foreign POSIX cwd");
    #[cfg(not(windows))]
    let cwd = PathUri::parse("file:///C:/src").expect("valid foreign Windows cwd");
    let parsed_cmd = vec![
        ParsedCommand::Read {
            cmd: "cat file.txt".to_string(),
            name: "file.txt".to_string(),
            path: PathBuf::from("file.txt"),
        },
        ParsedCommand::ListFiles {
            cmd: "ls".to_string(),
            path: Some("subdir".to_string()),
        },
        ParsedCommand::Search {
            cmd: "rg needle".to_string(),
            query: Some("needle".to_string()),
            path: Some("src".to_string()),
        },
    ];
    let read_path = LegacyAppPathString::from(cwd.join("file.txt").expect("resolvable read path"));

    assert_eq!(
        command_actions_for_path_uri(&parsed_cmd, &cwd),
        vec![
            CommandAction::Read {
                command: "cat file.txt".to_string(),
                name: "file.txt".to_string(),
                path: read_path,
            },
            CommandAction::ListFiles {
                command: "ls".to_string(),
                path: Some("subdir".to_string()),
            },
            CommandAction::Search {
                command: "rg needle".to_string(),
                query: Some("needle".to_string()),
                path: Some("src".to_string()),
            },
        ]
    );
}
