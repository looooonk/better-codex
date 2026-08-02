use super::*;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
fn fake_vim(body: &str) -> (tempfile::TempDir, PathBuf) {
    let temp_dir = tempfile::tempdir().expect("fake Vim directory");
    let program = temp_dir.path().join("fake-vim");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\nset -eu\ninput=$BETTER_CODEX_VIM_INPUT\nbridge=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = -S ]; then shift; bridge=$1; fi\n  shift\ndone\ntest -f \"$bridge\"\n{body}\n"
        ),
    )
    .expect("fake Vim script");
    let mut permissions = fs::metadata(&program)
        .expect("fake Vim metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&program, permissions).expect("executable fake Vim");
    (temp_dir, program)
}

#[cfg(unix)]
#[tokio::test]
async fn process_exit_and_marker_contract_controls_the_outcome() {
    let scripts = [
        "printf 'first\n界  \n\n' > \"$input\"\nprintf 'submit\n' > \"$BETTER_CODEX_VIM_ACTION\"",
        "printf 'edited draft\n' > \"$input\"",
        ":",
        "printf 'must not send\n' > \"$input\"\nprintf 'submit\n' > \"$BETTER_CODEX_VIM_ACTION\"\nexit 7",
    ];
    let mut outcomes = Vec::new();
    for script in scripts {
        let (_temp_dir, program) = fake_vim(script);
        outcomes.push(
            run_with_program(VimInputRequest::empty(ThreadId::new()), &program)
                .await
                .expect("fake Vim should run"),
        );
    }
    assert_eq!(
        outcomes,
        vec![
            VimInputOutcome::Submit("first\n界  \n".to_string()),
            VimInputOutcome::ReturnDraft("edited draft".to_string()),
            VimInputOutcome::ReturnDraft(String::new()),
            VimInputOutcome::Cancelled,
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn real_vim_bridge_defines_ctrl_j_and_submits_with_codex_command() {
    let Ok(program) = resolve_vim_program() else {
        return;
    };
    let temp_dir = tempfile::tempdir().expect("Vim bridge directory");
    let input = temp_dir.path().join("input.md");
    let marker = temp_dir.path().join("submit");
    let mapping = temp_dir.path().join("mapping");
    let bridge = temp_dir.path().join("bridge.vim");
    fs::write(&input, "before\n").expect("Vim input fixture");
    fs::write(&bridge, VIM_BRIDGE).expect("Vim bridge fixture");

    let status = Command::new(&program)
        .args(["-u", "NONE", "-n", "-i", "NONE", "-es", "-S"])
        .arg(&bridge)
        .args([
            "-c",
            "call writefile([maparg('<C-J>', 'n')], $BETTER_CODEX_VIM_MAPPING)",
            "-c",
            "call setline(1, ['from bridge', 'second line'])",
            "-c",
            "Codex",
        ])
        .env(VIM_ACTION_ENV, &marker)
        .env(VIM_INPUT_ENV, &input)
        .env("BETTER_CODEX_VIM_MAPPING", &mapping)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .expect("real Vim bridge should launch");
    assert_eq!(
        (
            status.success(),
            fs::read_to_string(mapping).expect("Ctrl-J mapping should be recorded"),
            read_vim_input(&input).expect("bridge should save UTF-8 input"),
            is_submit_marker(&marker).expect("bridge marker should be readable"),
        ),
        (
            true,
            ":Codex<CR>\n".to_string(),
            "from bridge\nsecond line".to_string(),
            true,
        )
    );
}

#[test]
fn input_reader_preserves_text_and_rejects_invalid_content() {
    let temp_dir = tempfile::tempdir().expect("Vim input directory");
    let input = temp_dir.path().join("input.md");
    let oversized = temp_dir.path().join("oversized.md");
    let invalid = temp_dir.path().join("invalid.md");
    fs::write(&input, b"first  \r\nsecond \r\n\r\n").expect("Vim input fixture");
    fs::write(&oversized, vec![b'x'; MAX_COMPOSER_BYTES.saturating_add(1)])
        .expect("oversized Vim input fixture");
    fs::write(&invalid, [0xff]).expect("invalid Vim input fixture");

    assert_eq!(
        (
            read_vim_input(&input).expect("valid Vim input"),
            read_vim_input(&oversized)
                .expect_err("oversized input should fail")
                .to_string(),
            read_vim_input(&invalid)
                .expect_err("invalid input should fail")
                .to_string()
                .contains("not valid UTF-8"),
        ),
        (
            "first  \nsecond \n".to_string(),
            input_too_large_message(MAX_COMPOSER_BYTES.saturating_add(1)),
            true,
        )
    );
}
