use std::fmt;
use std::fs::File;
use std::io::Read;
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
use crossterm::terminal::LeaveAlternateScreen;

const TERMINAL_RESTORE_PANIC_HELPER_ENV: &str = "CODEX_TUI_TERMINAL_RESTORE_PANIC_HELPER";
const TERMINAL_RESTORE_FATAL_DISCONNECT_HELPER_ENV: &str =
    "CODEX_TUI_TERMINAL_RESTORE_FATAL_DISCONNECT_HELPER";

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
    // SAFETY: openpty initializes the provided file descriptors on success.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
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

fn read_available_pty_output(master: OwnedFd) -> String {
    set_nonblocking(master.as_raw_fd());
    let mut master = File::from(master);
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(err) => panic!("read helper pty output: {err}"),
        }
    }
    String::from_utf8_lossy(&output).into_owned()
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
    let mut expected = String::new();
    command
        .write_ansi(&mut expected)
        .unwrap_or_else(|err| panic!("format {label} sequence: {err}"));
    assert!(
        output.contains(&expected),
        "missing {label} sequence {expected:?} in pty output {output:?}"
    );
}
