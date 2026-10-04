use super::*;
#[cfg(unix)]
use pretty_assertions::assert_eq;

#[cfg(unix)]
fn test_deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

#[cfg(unix)]
fn write_ide_context_response(
    stream: &mut impl std::io::Write,
    request_id: &str,
    active_selection_content: &str,
) {
    if let Err(err) = write_frame(
        stream,
        &json!({
            "type": "response",
            "requestId": request_id,
            "resultType": "success",
            "method": "ide-context",
            "handledByClientId": "vscode-client",
            "result": {
                "type": "broadcast",
                "ideContext": {
                    "activeFile": {
                        "label": "lib.rs",
                        "path": "src/lib.rs",
                        "fsPath": "/repo/src/lib.rs",
                        "selection": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 3 }
                        },
                        "activeSelectionContent": active_selection_content,
                        "selections": []
                    },
                    "openTabs": []
                }
            }
        }),
    ) {
        panic!("write ide-context response failed: {err}");
    }
}

fn spawn_ide_context_server(
    listener: std::os::unix::net::UnixListener,
    active_selection_content: &'static str,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            panic!("accept failed");
        };
        let request = match read_frame(&mut stream, test_deadline()) {
            Ok(request) => request,
            Err(err) => panic!("read ide-context failed: {err}"),
        };
        let Some(request_id) = request.get("requestId").and_then(Value::as_str) else {
            panic!("ide-context request did not include a request id");
        };
        write_ide_context_response(&mut stream, request_id, active_selection_content);
    })
}

fn fetch_test_ide_context(
    primary_socket_path: PathBuf,
    legacy_socket_path: PathBuf,
) -> Result<IdeContext, IdeContextError> {
    fetch_ide_context_from_unix_socket_paths(
        primary_socket_path,
        vec![legacy_socket_path],
        Path::new("/repo"),
        test_deadline(),
    )
}

fn assert_listener_unused(listener: &std::os::unix::net::UnixListener) {
    if let Err(err) = listener.set_nonblocking(true) {
        panic!("set listener nonblocking failed: {err}");
    }
    match listener.accept() {
        Err(err) => assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock),
        Ok(_) => panic!("listener should not receive a connection"),
    }
}

#[test]
fn primary_ipc_socket_path_uses_codex_home() {
    let codex_home = Path::new("/home/test/.codex");

    assert_eq!(
        primary_ipc_socket_path(codex_home),
        codex_home.join("ipc").join("ipc.sock")
    );
}

#[test]
fn fetch_ide_context_prefers_primary_socket() {
    use std::os::unix::net::UnixListener;

    let tempdir = tempfile::tempdir().expect("tempdir");
    let primary_socket_path = tempdir.path().join("primary.sock");
    let legacy_socket_path = tempdir.path().join("legacy.sock");
    let primary_listener = UnixListener::bind(&primary_socket_path).expect("bind primary");
    let legacy_listener = UnixListener::bind(&legacy_socket_path).expect("bind legacy");
    let server = spawn_ide_context_server(primary_listener, "primary");

    let context = fetch_test_ide_context(primary_socket_path, legacy_socket_path)
        .expect("fetch IDE context from primary socket");

    server.join().expect("server joins");
    assert_eq!(
        context
            .active_file
            .expect("active file")
            .active_selection_content,
        "primary"
    );
    assert_listener_unused(&legacy_listener);
}

#[test]
fn fetch_ide_context_falls_back_to_legacy_socket() {
    use std::os::unix::net::UnixListener;

    let tempdir = tempfile::tempdir().expect("tempdir");
    let primary_socket_path = tempdir.path().join("missing-primary.sock");
    let legacy_socket_path = tempdir.path().join("legacy.sock");
    let legacy_listener = UnixListener::bind(&legacy_socket_path).expect("bind legacy");
    let server = spawn_ide_context_server(legacy_listener, "legacy");

    let context = fetch_test_ide_context(primary_socket_path, legacy_socket_path)
        .expect("fetch IDE context from legacy socket");

    server.join().expect("server joins");
    assert_eq!(
        context
            .active_file
            .expect("active file")
            .active_selection_content,
        "legacy"
    );
}

#[test]
fn fetch_ide_context_falls_back_to_uid_zero_legacy_socket() {
    use std::os::unix::net::UnixListener;

    let tempdir = tempfile::tempdir().expect("tempdir");
    let primary_socket_path = tempdir.path().join("missing-primary.sock");
    let legacy_socket_path = legacy_ipc_socket_paths(tempdir.path(), /*uid*/ 0)
        .into_iter()
        .next()
        .expect("UID-0 legacy socket path");
    std::fs::create_dir(legacy_socket_path.parent().expect("legacy parent"))
        .expect("create legacy parent");
    let legacy_listener = UnixListener::bind(&legacy_socket_path).expect("bind legacy");
    let server = spawn_ide_context_server(legacy_listener, "legacy-root");

    let context = fetch_test_ide_context(primary_socket_path, legacy_socket_path)
        .expect("fetch IDE context from UID-0 legacy socket");

    server.join().expect("server joins");
    assert_eq!(
        context
            .active_file
            .expect("active file")
            .active_selection_content,
        "legacy-root"
    );
}

#[test]
fn fetch_ide_context_falls_back_to_pre_migration_uid_zero_legacy_socket() {
    use std::os::unix::net::UnixListener;

    let tempdir = tempfile::tempdir().expect("tempdir");
    let primary_socket_path = tempdir.path().join("missing-primary.sock");
    let legacy_socket_paths = legacy_ipc_socket_paths(tempdir.path(), /*uid*/ 0);
    let pre_migration_socket_path = legacy_socket_paths
        .last()
        .expect("pre-migration UID-0 legacy socket path");
    std::fs::create_dir(pre_migration_socket_path.parent().expect("legacy parent"))
        .expect("create legacy parent");
    let legacy_listener =
        UnixListener::bind(pre_migration_socket_path).expect("bind pre-migration legacy");
    let server = spawn_ide_context_server(legacy_listener, "legacy-root-pre-migration");

    let context = fetch_ide_context_from_unix_socket_paths(
        primary_socket_path,
        legacy_socket_paths,
        Path::new("/repo"),
        test_deadline(),
    )
    .expect("fetch IDE context from pre-migration UID-0 legacy socket");

    server.join().expect("server joins");
    assert_eq!(
        context
            .active_file
            .expect("active file")
            .active_selection_content,
        "legacy-root-pre-migration"
    );
}

#[test]
fn fetch_ide_context_does_not_fall_back_after_primary_timeout() {
    use std::os::unix::net::UnixListener;

    let tempdir = tempfile::tempdir().expect("tempdir");
    let primary_socket_path = tempdir.path().join("missing-primary.sock");
    let legacy_socket_path = tempdir.path().join("legacy.sock");
    let legacy_listener = UnixListener::bind(&legacy_socket_path).expect("bind legacy");

    let err = fetch_ide_context_from_unix_socket_paths(
        primary_socket_path,
        vec![legacy_socket_path],
        Path::new("/repo"),
        Instant::now(),
    )
    .expect_err("expired primary deadline should fail");

    assert!(matches!(
        err,
        IdeContextError::Connect(err) if err.kind() == std::io::ErrorKind::TimedOut
    ));
    assert_listener_unused(&legacy_listener);
}

#[test]
fn fetch_ide_context_does_not_fall_back_after_primary_protocol_error() {
    use std::os::unix::net::UnixListener;
    use std::thread;

    let tempdir = tempfile::tempdir().expect("tempdir");
    let primary_socket_path = tempdir.path().join("primary.sock");
    let legacy_socket_path = tempdir.path().join("legacy.sock");
    let primary_listener = UnixListener::bind(&primary_socket_path).expect("bind primary");
    let legacy_listener = UnixListener::bind(&legacy_socket_path).expect("bind legacy");
    let server = thread::spawn(move || {
        let (mut stream, _) = primary_listener.accept().expect("accept primary");
        read_frame(&mut stream, test_deadline()).expect("read ide-context");
        write_frame(&mut stream, &json!({ "type": "unexpected" })).expect("write invalid response");
    });

    let err = fetch_test_ide_context(primary_socket_path, legacy_socket_path)
        .expect_err("invalid primary response should fail");

    server.join().expect("server joins");
    assert!(matches!(err, IdeContextError::InvalidResponse(_)));
    assert_listener_unused(&legacy_listener);
}

#[cfg(unix)]
#[test]
fn unix_deadline_stream_uses_remaining_deadline_for_blocking_reads() {
    use std::os::unix::net::UnixStream;

    let (client, _server) = UnixStream::pair().expect("create unix stream pair");
    let mut stream = UnixDeadlineStream::new(client, Instant::now() + Duration::from_millis(50));
    let start = Instant::now();
    let mut buf = [0_u8; 1];

    let err = std::io::Read::read(&mut stream, &mut buf)
        .expect_err("read should time out at the request deadline");

    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn validate_unix_socket_path_rejects_unsafe_parent_directory() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(tempdir.path(), std::fs::Permissions::from_mode(0o777))
        .expect("set unsafe permissions");
    let socket_path = tempdir.path().join("codex-ipc.sock");
    let _listener = UnixListener::bind(&socket_path).expect("bind socket");

    let err = validate_unix_socket_path(&socket_path)
        .expect_err("world-writable parent directory should be rejected");

    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}

#[cfg(unix)]
#[test]
fn fetch_ide_context_uses_unregistered_request_route() {
    use std::os::unix::net::UnixListener;
    use std::thread;

    let tempdir = tempfile::tempdir().expect("tempdir");
    let socket_path = tempdir.path().join("codex-ipc.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind socket");

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");

        let ide_context = read_frame(&mut stream, test_deadline()).expect("read ide-context");
        assert_eq!(
            ide_context.get("method").and_then(Value::as_str),
            Some("ide-context")
        );
        assert_eq!(
            ide_context.get("sourceClientId").and_then(Value::as_str),
            Some(TUI_SOURCE_CLIENT_ID)
        );
        assert_eq!(
            ide_context
                .get("params")
                .and_then(|params| params.get("workspaceRoot"))
                .and_then(Value::as_str),
            Some("/repo")
        );
        let ide_context_request_id = ide_context
            .get("requestId")
            .and_then(Value::as_str)
            .expect("ide-context request id");
        write_frame(
            &mut stream,
            &json!({
                "type": "request",
                "requestId": "inbound-request",
                "sourceClientId": "vscode-client",
                "version": 0,
                "method": "unknown-method",
                "params": {}
            }),
        )
        .expect("write inbound request before ide-context response");
        let inbound_response = read_frame(&mut stream, test_deadline())
            .expect("read inbound request response before ide-context response");
        assert_eq!(
            inbound_response,
            json!({
                "type": "response",
                "requestId": "inbound-request",
                "resultType": "error",
                "error": "no-handler-for-request"
            })
        );

        write_frame(
            &mut stream,
            &json!({
                "type": "client-discovery-request",
                "requestId": "discovery-request",
                "request": ide_context.clone(),
            }),
        )
        .expect("write client discovery request");
        let discovery_response =
            read_frame(&mut stream, test_deadline()).expect("read client discovery response");
        assert_eq!(
            discovery_response.get("type").and_then(Value::as_str),
            Some("client-discovery-response")
        );
        assert_eq!(
            discovery_response.get("requestId").and_then(Value::as_str),
            Some("discovery-request")
        );
        assert_eq!(
            discovery_response
                .get("response")
                .and_then(|response| response.get("canHandle"))
                .and_then(Value::as_bool),
            Some(false)
        );

        write_frame(
            &mut stream,
            &json!({
                "type": "broadcast",
                "method": "thread-stream-state-changed",
                "params": "x".repeat(MAX_IPC_FRAME_BYTES / 2),
            }),
        )
        .expect("write large broadcast");
        write_ide_context_response(&mut stream, ide_context_request_id, "use");
    });

    let context = fetch_test_ide_context(socket_path, tempdir.path().join("missing-legacy.sock"))
        .expect("fetch ide context");

    server.join().expect("server joins");
    assert_eq!(
        context
            .active_file
            .expect("active file")
            .active_selection_content,
        "use"
    );
}
