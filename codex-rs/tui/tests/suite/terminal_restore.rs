use std::fmt;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::process::Command as ProcessCommand;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use crossterm::Command;
use crossterm::cursor::SetCursorStyle;
use crossterm::cursor::Show;
use crossterm::event::DisableBracketedPaste;
use crossterm::event::DisableFocusChange;
use crossterm::event::DisableMouseCapture;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;

const TERMINAL_RESTORE_PANIC_HELPER_ENV: &str = "CODEX_TUI_TERMINAL_RESTORE_PANIC_HELPER";
const TERMINAL_RESTORE_FATAL_DISCONNECT_HELPER_ENV: &str =
    "CODEX_TUI_TERMINAL_RESTORE_FATAL_DISCONNECT_HELPER";
const INTERACTIVE_CHILD_HANDOFF_HELPER_ENV: &str = "CODEX_TUI_INTERACTIVE_CHILD_HANDOFF_HELPER";

#[test]
fn panic_hook_restores_terminal_modes_under_pty() {
    let output = run_restore_helper_under_pty(TERMINAL_RESTORE_PANIC_HELPER_ENV);
    assert_restored_terminal_sequences(&output);
}

#[test]
fn fatal_disconnect_restores_terminal_modes_under_pty() {
    let output = run_restore_helper_under_pty(TERMINAL_RESTORE_FATAL_DISCONNECT_HELPER_ENV);
    assert_restored_terminal_sequences(&output);
    assert!(
        output.contains("ERROR: app-server disconnected"),
        "missing fatal disconnect message in pty output {output:?}"
    );
}

#[test]
fn interactive_child_handoff_releases_and_reacquires_the_pty() {
    let pty = open_pty();
    let original_termios = termios(pty.slave.as_raw_fd());
    let codex_tui = codex_utils_cargo_bin::cargo_bin("codex-tui").expect("codex-tui binary");
    let mut command = ProcessCommand::new(codex_tui);
    command
        .env(INTERACTIVE_CHILD_HANDOFF_HELPER_ENV, "1")
        .stdin(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stdout(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stderr(Stdio::from(dup_file(pty.slave.as_raw_fd())));

    let mut child = command.spawn().expect("spawn interactive child helper");
    let mut reader = dup_file(pty.master.as_raw_fd());
    set_nonblocking(reader.as_raw_fd());
    let mut writer = dup_file(pty.master.as_raw_fd());
    let mut output = Vec::new();
    wait_for_pty_text(&mut child, &mut reader, &mut output, "CHILD_READY");
    writer
        .write_all(b"child-input\n")
        .expect("write child input");
    writer.flush().expect("flush child input");
    wait_for_pty_text(&mut child, &mut reader, &mut output, "TUI_RESUMED");
    writer.write_all(b"r").expect("write resumed TUI input");
    writer.flush().expect("flush resumed TUI input");
    wait_for_pty_text(&mut child, &mut reader, &mut output, "TUI_GOT:r");
    let status = wait_for_child_with_pty_output(&mut child, &mut reader, &mut output);
    assert!(status.success(), "interactive child helper should succeed");
    assert_termios_restored(&original_termios, &termios(pty.slave.as_raw_fd()));

    let output = String::from_utf8_lossy(&output);
    let enter = command_sequence(EnterAlternateScreen, "enter alternate screen");
    let leave = command_sequence(LeaveAlternateScreen, "leave alternate screen");
    let first_enter = output.find(&enter).expect("initial alternate screen");
    let handoff_leave = output
        .find(&leave)
        .expect("handoff leaves alternate screen");
    let child_ready = output.find("CHILD_READY").expect("child ready marker");
    let child_got = output
        .find("CHILD_GOT:child-input")
        .expect("child input marker");
    let resumed_enter = output[handoff_leave + leave.len()..]
        .find(&enter)
        .map(|index| index + handoff_leave + leave.len())
        .expect("handoff restores alternate screen");
    let resumed = output.find("TUI_RESUMED").expect("TUI resume marker");
    assert!(
        first_enter < handoff_leave
            && handoff_leave < child_ready
            && child_ready < child_got
            && child_got < resumed_enter
            && resumed_enter < resumed,
        "unexpected handoff ordering in PTY output {output:?}"
    );
}

#[test]
fn cancelled_interactive_child_handoff_restores_the_pty() {
    let pty = open_pty();
    let original_termios = termios(pty.slave.as_raw_fd());
    let codex_tui = codex_utils_cargo_bin::cargo_bin("codex-tui").expect("codex-tui binary");
    let mut command = ProcessCommand::new(codex_tui);
    command
        .env(INTERACTIVE_CHILD_HANDOFF_HELPER_ENV, "cancel")
        .stdin(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stdout(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stderr(Stdio::from(dup_file(pty.slave.as_raw_fd())));

    let mut child = command.spawn().expect("spawn cancellation handoff helper");
    let mut reader = dup_file(pty.master.as_raw_fd());
    set_nonblocking(reader.as_raw_fd());
    let mut writer = dup_file(pty.master.as_raw_fd());
    let mut output = Vec::new();
    wait_for_pty_text(&mut child, &mut reader, &mut output, "CANCEL_CHILD_READY");
    wait_for_pty_text(&mut child, &mut reader, &mut output, "TUI_CANCEL_RESUMED");
    writer.write_all(b"c").expect("write resumed TUI input");
    writer.flush().expect("flush resumed TUI input");
    wait_for_pty_text(&mut child, &mut reader, &mut output, "TUI_GOT:c");
    let status = wait_for_child_with_pty_output(&mut child, &mut reader, &mut output);
    assert!(
        status.success(),
        "cancellation handoff helper should succeed"
    );
    assert_termios_restored(&original_termios, &termios(pty.slave.as_raw_fd()));

    let output = String::from_utf8_lossy(&output);
    let enter = command_sequence(EnterAlternateScreen, "enter alternate screen");
    let leave = command_sequence(LeaveAlternateScreen, "leave alternate screen");
    let first_enter = output.find(&enter).expect("initial alternate screen");
    let handoff_leave = output
        .find(&leave)
        .expect("handoff leaves alternate screen");
    let child_ready = output
        .find("CANCEL_CHILD_READY")
        .expect("cancellation child ready marker");
    let resumed_enter = output[handoff_leave + leave.len()..]
        .find(&enter)
        .map(|index| index + handoff_leave + leave.len())
        .expect("cancelled handoff restores alternate screen");
    let resumed = output
        .find("TUI_CANCEL_RESUMED")
        .expect("TUI cancellation resume marker");
    assert!(
        first_enter < handoff_leave
            && handoff_leave < child_ready
            && child_ready < resumed_enter
            && resumed_enter < resumed,
        "unexpected cancellation handoff ordering in PTY output {output:?}"
    );
}

#[test]
fn unauthenticated_startup_honors_no_alt_screen_under_pty() {
    let pty = open_pty();
    let codex_tui = codex_utils_cargo_bin::cargo_bin("codex-tui").expect("codex-tui binary");
    let codex_home = tempfile::tempdir().expect("temporary CODEX_HOME");
    let log_dir = codex_home.path().join("logs");

    let mut command = ProcessCommand::new(codex_tui);
    command
        .arg("--no-alt-screen")
        .arg("-c")
        .arg(format!("log_dir={:?}", log_dir.display().to_string()))
        .env("CODEX_HOME", codex_home.path())
        .env("RUST_LOG", "trace")
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stdout(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stderr(Stdio::from(dup_file(pty.slave.as_raw_fd())));

    let mut child = command.spawn().expect("spawn inline startup under pty");
    let mut reader = dup_file(pty.master.as_raw_fd());
    set_nonblocking(reader.as_raw_fd());
    let mut output = Vec::new();
    wait_for_pty_text(&mut child, &mut reader, &mut output, "ACCOUNT LOGIN");

    let mut writer = dup_file(pty.master.as_raw_fd());
    writer.write_all(b"3").expect("select startup exit");
    writer.flush().expect("flush startup exit selection");
    std::thread::sleep(Duration::from_millis(/*millis*/ 25));
    writer.write_all(b"\r").expect("submit startup exit");
    writer.flush().expect("flush startup exit submission");
    let status = wait_for_child_with_pty_output(&mut child, &mut reader, &mut output);
    assert!(status.success(), "inline startup should exit cleanly");

    read_available_pty_bytes(&mut reader, &mut output);
    let output = String::from_utf8_lossy(&output);
    assert!(
        output.contains("ACCOUNT LOGIN") && output.contains("Sign in with ChatGPT"),
        "startup screen did not render in pty output {output:?}"
    );
    assert_missing_sequence(
        &output,
        crossterm::terminal::EnterAlternateScreen,
        "enter alternate screen",
    );
}

fn run_restore_helper_under_pty(helper_env: &str) -> String {
    let pty = open_pty();
    let original_termios = termios(pty.slave.as_raw_fd());
    let codex_tui = codex_utils_cargo_bin::cargo_bin("codex-tui").expect("codex-tui binary");

    let mut command = ProcessCommand::new(codex_tui);
    command
        .env(helper_env, "1")
        .stdin(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stdout(Stdio::from(dup_file(pty.slave.as_raw_fd())))
        .stderr(Stdio::from(dup_file(pty.slave.as_raw_fd())));

    let mut child = command.spawn().expect("spawn panic helper under pty");
    let status = wait_for_child(&mut child);
    assert!(!status.success(), "panic helper should fail");

    let restored_termios = termios(pty.slave.as_raw_fd());
    assert_termios_restored(&original_termios, &restored_termios);

    drop(pty.slave);
    read_available_pty_output(pty.master)
}

fn assert_restored_terminal_sequences(output: &str) {
    assert_contains_sequence(output, DisableBracketedPaste, "bracketed paste disable");
    assert_contains_sequence(output, DisableFocusChange, "focus-report disable");
    assert_contains_sequence(output, DisableMouseCapture, "mouse disable");
    assert_contains_sequence(output, DisableAlternateScroll, "alternate scroll disable");
    assert_contains_sequence(output, LeaveAlternateScreen, "leave alternate screen");
    assert_contains_sequence(output, Show, "cursor show");
    assert_contains_sequence(
        output,
        SetCursorStyle::DefaultUserShape,
        "default cursor shape",
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableAlternateScroll;

impl Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007l")
    }
}

struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
}

fn open_pty() -> Pty {
    let mut master = 0;
    let mut slave = 0;
    let window_size = libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty initializes the provided file descriptors on success.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &window_size,
        )
    };
    assert_eq!(
        result,
        0,
        "openpty failed: {}",
        std::io::Error::last_os_error()
    );

    // SAFETY: openpty returned owned descriptors on success.
    let master = unsafe { OwnedFd::from_raw_fd(master) };
    // SAFETY: openpty returned owned descriptors on success.
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    Pty { master, slave }
}

fn dup_file(fd: i32) -> File {
    // SAFETY: dup returns a fresh descriptor that File owns on success.
    let duplicated = unsafe { libc::dup(fd) };
    assert!(
        duplicated >= 0,
        "dup failed: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: duplicated is a fresh descriptor owned by this File.
    unsafe { File::from_raw_fd(duplicated) }
}

fn wait_for_child(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().expect("poll panic helper") {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("panic helper timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_child_with_pty_output(
    child: &mut std::process::Child,
    reader: &mut File,
    output: &mut Vec<u8>,
) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 15);
    loop {
        read_available_pty_bytes(reader, output);
        if let Some(status) = child.try_wait().expect("poll inline startup") {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let output = String::from_utf8_lossy(output);
            panic!("inline startup timed out while exiting: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(/*millis*/ 25));
    }
}

fn read_available_pty_output(master: OwnedFd) -> String {
    set_nonblocking(master.as_raw_fd());
    let mut master = File::from(master);
    let mut output = Vec::new();
    read_available_pty_bytes(&mut master, &mut output);
    String::from_utf8_lossy(&output).into_owned()
}

fn wait_for_pty_text(
    child: &mut std::process::Child,
    reader: &mut File,
    output: &mut Vec<u8>,
    expected: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(/*secs*/ 20);
    loop {
        read_available_pty_bytes(reader, output);
        if String::from_utf8_lossy(output).contains(expected) {
            return;
        }
        if let Some(status) = child.try_wait().expect("poll inline startup") {
            panic!("inline startup exited before rendering {expected:?}: {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let output = String::from_utf8_lossy(output);
            panic!("inline startup timed out waiting for {expected:?}: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(/*millis*/ 25));
    }
}

fn read_available_pty_bytes(reader: &mut File, output: &mut Vec<u8>) {
    let mut buffer = [0; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(err) => panic!("read helper pty output: {err}"),
        }
    }
}

fn set_nonblocking(fd: i32) {
    // SAFETY: fcntl is called with a valid pty master descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(
        flags >= 0,
        "fcntl F_GETFL failed: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: fcntl is called with a valid pty master descriptor and updated flag bitset.
    let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    assert!(
        result >= 0,
        "fcntl F_SETFL failed: {}",
        std::io::Error::last_os_error()
    );
}

fn termios(fd: i32) -> libc::termios {
    // SAFETY: zeroed is valid as an output buffer for tcgetattr.
    let mut termios = unsafe { std::mem::zeroed() };
    // SAFETY: fd is an open pty slave descriptor and termios points to writable memory.
    let result = unsafe { libc::tcgetattr(fd, &mut termios) };
    assert_eq!(
        result,
        0,
        "tcgetattr failed: {}",
        std::io::Error::last_os_error()
    );
    termios
}

fn assert_termios_restored(original: &libc::termios, restored: &libc::termios) {
    assert_flag_mask_restored(
        restored.c_iflag,
        original.c_iflag,
        libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON,
        "input flags",
    );
    assert_flag_mask_restored(
        restored.c_oflag,
        original.c_oflag,
        libc::OPOST,
        "output flags",
    );
    assert_flag_mask_restored(
        restored.c_cflag,
        original.c_cflag,
        libc::CSIZE | libc::PARENB,
        "control flags",
    );
    assert_flag_mask_restored(
        restored.c_lflag,
        original.c_lflag,
        libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG,
        "local flags",
    );
    assert_eq!(restored.c_cc[libc::VMIN], original.c_cc[libc::VMIN]);
    assert_eq!(restored.c_cc[libc::VTIME], original.c_cc[libc::VTIME]);
}

fn assert_flag_mask_restored<T>(restored: T, original: T, mask: T, label: &str)
where
    T: std::fmt::Debug + Copy + PartialEq + std::ops::BitAnd<Output = T>,
{
    assert_eq!(
        restored & mask,
        original & mask,
        "{label} were not restored"
    );
}

fn assert_contains_sequence(output: &str, command: impl Command, label: &str) {
    let expected = command_sequence(command, label);
    assert!(
        output.contains(&expected),
        "missing {label} sequence {expected:?} in pty output {output:?}"
    );
}

fn assert_missing_sequence(output: &str, command: impl Command, label: &str) {
    let unexpected = command_sequence(command, label);
    assert!(
        !output.contains(&unexpected),
        "unexpected {label} sequence {unexpected:?} in pty output {output:?}"
    );
}

fn command_sequence(command: impl Command, label: &str) -> String {
    let mut sequence = String::new();
    command
        .write_ansi(&mut sequence)
        .unwrap_or_else(|err| panic!("format {label} sequence: {err}"));
    sequence
}
