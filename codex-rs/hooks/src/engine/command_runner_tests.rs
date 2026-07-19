use std::collections::HashMap;

use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookSource;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::*;

#[cfg(unix)]
#[tokio::test]
async fn run_command_bounds_stdout_and_stderr_capture() {
    let output_bytes = MAX_HOOK_OUTPUT_BYTES_PER_STREAM + 8192;
    let handler = ConfiguredHandler {
        event_name: HookEventName::Stop,
        matcher: None,
        command: format!("head -c {output_bytes} /dev/zero; head -c {output_bytes} /dev/zero >&2"),
        timeout_sec: 10,
        status_message: None,
        source_path: test_path_buf("/hooks.json").abs(),
        source: HookSource::User,
        display_order: 0,
        env: HashMap::new(),
    };
    let shell = CommandShell {
        program: "/bin/sh".to_string(),
        args: vec!["-c".to_string()],
    };
    let cwd = tempdir().expect("create temp dir");

    let result = run_command(
        &shell,
        &handler,
        /*configured_order*/ 0,
        "{}",
        cwd.path(),
    )
    .await;

    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout.len(), MAX_HOOK_OUTPUT_BYTES_PER_STREAM);
    assert_eq!(result.stderr.len(), MAX_HOOK_OUTPUT_BYTES_PER_STREAM);
    assert_eq!(
        result.error,
        Some(format!(
            "hook stdout and stderr exceeded the {MAX_HOOK_OUTPUT_BYTES_PER_STREAM}-byte capture limit"
        ))
    );
}
