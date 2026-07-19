use std::collections::HashMap;

use codex_network_proxy::ManagedNetworkSandboxContext;
#[cfg(unix)]
use codex_protocol::models::PermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

use super::prepare_exec_request;
use crate::ExecParams;
#[cfg(unix)]
use crate::ExecServerRuntimePaths;
#[cfg(unix)]
use crate::FileSystemSandboxContext;
use crate::ProcessId;

#[cfg(unix)]
#[test]
fn sandbox_request_wraps_native_argv_on_executor() {
    let cwd: AbsolutePathBuf = std::env::current_dir()
        .expect("current directory")
        .try_into()
        .expect("absolute cwd");
    let cwd_uri = PathUri::from_abs_path(&cwd);
    let self_exe = std::env::current_exe().expect("current executable");
    let runtime_paths =
        ExecServerRuntimePaths::new(self_exe.clone(), Some(self_exe)).expect("runtime paths");
    let sandbox = FileSystemSandboxContext::from_permission_profile_with_cwd(
        PermissionProfile::workspace_write(),
        cwd_uri.clone(),
    );
    let params = ExecParams {
        process_id: ProcessId::from("process-1"),
        argv: vec![
            "/bin/bash".to_string(),
            "-lc".to_string(),
            "pwd".to_string(),
        ],
        cwd: cwd_uri,
        env_policy: None,
        env: HashMap::new(),
        tty: false,
        pipe_stdin: false,
        arg0: None,
        sandbox: Some(sandbox),
        enforce_managed_network: false,
        managed_network: None,
    };

    let prepared = prepare_exec_request(&params, HashMap::new(), Some(&runtime_paths))
        .expect("prepare sandboxed request");

    assert_ne!(prepared.command, params.argv);
    assert_eq!(prepared.cwd, cwd);
    #[cfg(target_os = "linux")]
    {
        assert_eq!(
            prepared.command.first(),
            Some(&runtime_paths.codex_self_exe.to_string_lossy().into_owned())
        );
        let permission_profile_json = prepared
            .command
            .iter()
            .position(|arg| arg == "--permission-profile")
            .and_then(|index| prepared.command.get(index + 1))
            .expect("sandbox wrapper permission profile");
        let permission_profile: PermissionProfile =
            serde_json::from_str(permission_profile_json).expect("permission profile JSON");
        assert_eq!(
            permission_profile,
            PermissionProfile::workspace_write()
                .materialize_project_roots_with_workspace_roots(std::slice::from_ref(&cwd))
        );
    }
    #[cfg(target_os = "macos")]
    assert_eq!(
        prepared.command.first().map(String::as_str),
        Some("/usr/bin/sandbox-exec")
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sandbox_request_preserves_custom_arg0_for_inner_command() {
    let cwd: AbsolutePathBuf = std::env::current_dir()
        .expect("current directory")
        .try_into()
        .expect("absolute cwd");
    let cwd_uri = PathUri::from_abs_path(&cwd);
    let self_exe = std::env::current_exe().expect("current executable");
    let runtime_paths =
        ExecServerRuntimePaths::new(self_exe.clone(), Some(self_exe)).expect("runtime paths");
    let params = ExecParams {
        process_id: ProcessId::from("process-custom-arg0"),
        argv: vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "exit 0".to_string(),
        ],
        cwd: cwd_uri.clone(),
        env_policy: None,
        env: HashMap::new(),
        tty: false,
        pipe_stdin: false,
        arg0: Some("custom-arg0".to_string()),
        sandbox: Some(FileSystemSandboxContext::from_permission_profile_with_cwd(
            PermissionProfile::workspace_write(),
            cwd_uri,
        )),
        enforce_managed_network: false,
        managed_network: None,
    };

    let prepared = prepare_exec_request(&params, HashMap::new(), Some(&runtime_paths))
        .expect("prepare sandboxed request");

    #[cfg(target_os = "linux")]
    assert!(
        prepared
            .command
            .contains(&"--command-arg0=custom-arg0".to_string())
    );
    #[cfg(target_os = "macos")]
    assert!(prepared.command.ends_with(&[
        "/bin/sh".to_string(),
        "-c".to_string(),
        "exec -a \"$0\" -- \"$@\"".to_string(),
        "custom-arg0".to_string(),
        "/bin/sh".to_string(),
        "-c".to_string(),
        "exit 0".to_string(),
    ]));
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_request_allows_prepared_managed_proxy_port() {
    let cwd: AbsolutePathBuf = std::env::current_dir()
        .expect("current directory")
        .try_into()
        .expect("absolute cwd");
    let cwd_uri = PathUri::from_abs_path(&cwd);
    let self_exe = std::env::current_exe().expect("current executable");
    let runtime_paths =
        ExecServerRuntimePaths::new(self_exe.clone(), Some(self_exe)).expect("runtime paths");
    let sandbox = FileSystemSandboxContext::from_permission_profile_with_cwd(
        PermissionProfile::workspace_write(),
        cwd_uri.clone(),
    );
    let params = ExecParams {
        process_id: ProcessId::from("process-managed-network"),
        argv: vec!["/usr/bin/true".to_string()],
        cwd: cwd_uri,
        env_policy: None,
        env: HashMap::new(),
        tty: false,
        pipe_stdin: false,
        arg0: None,
        sandbox: Some(sandbox),
        enforce_managed_network: true,
        managed_network: Some(ManagedNetworkSandboxContext {
            loopback_ports: vec![43123],
            allow_local_binding: false,
        }),
    };

    let prepared = prepare_exec_request(&params, HashMap::new(), Some(&runtime_paths))
        .expect("prepare managed-network sandbox request");
    let policy = prepared
        .command
        .windows(2)
        .find_map(|args| (args[0] == "-p").then_some(args[1].as_str()))
        .expect("Seatbelt policy argument");

    assert!(policy.contains("(allow network-outbound (remote ip \"localhost:43123\"))"));
}

#[test]
fn native_request_preserves_native_launch_fields() {
    let cwd: AbsolutePathBuf = std::env::current_dir()
        .expect("current directory")
        .try_into()
        .expect("absolute cwd");
    let cwd_uri = PathUri::from_abs_path(&cwd);
    let env = HashMap::from([("TEST_ENV".to_string(), "value".to_string())]);
    let params = ExecParams {
        process_id: ProcessId::from("process-1"),
        argv: vec!["echo".to_string(), "hello".to_string()],
        cwd: cwd_uri,
        env_policy: None,
        env: HashMap::new(),
        tty: false,
        pipe_stdin: false,
        arg0: Some("custom-arg0".to_string()),
        sandbox: None,
        enforce_managed_network: false,
        managed_network: None,
    };

    let prepared = prepare_exec_request(&params, env.clone(), /*runtime_paths*/ None)
        .expect("prepare native request");

    assert_eq!(prepared.command, params.argv);
    assert_eq!(prepared.cwd, cwd);
    assert_eq!(prepared.env, env);
    assert_eq!(prepared.arg0, params.arg0);
}

#[test]
fn native_request_rejects_managed_network_enforcement() {
    let cwd: AbsolutePathBuf = std::env::current_dir()
        .expect("current directory")
        .try_into()
        .expect("absolute cwd");
    let cwd_uri = PathUri::from_abs_path(&cwd);

    for (process_id, managed_network) in [
        ("without-details", None),
        (
            "with-details",
            Some(ManagedNetworkSandboxContext {
                loopback_ports: vec![43123],
                allow_local_binding: false,
            }),
        ),
    ] {
        let params = ExecParams {
            process_id: ProcessId::from(process_id),
            argv: vec!["true".to_string()],
            cwd: cwd_uri.clone(),
            env_policy: None,
            env: HashMap::new(),
            tty: false,
            pipe_stdin: false,
            arg0: None,
            sandbox: None,
            enforce_managed_network: true,
            managed_network,
        };

        let Err(error) = prepare_exec_request(&params, HashMap::new(), /*runtime_paths*/ None)
        else {
            panic!("managed network enforcement should fail without sandbox context");
        };
        assert_eq!(error.code, -32602);
        assert_eq!(
            error.message,
            "managed network enforcement requires sandbox context"
        );
    }
}
