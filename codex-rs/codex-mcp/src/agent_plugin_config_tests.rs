use super::*;
use codex_config::McpServerTransportConfig;
use codex_utils_path_uri::LegacyAppPathString;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn agent_plugin_http_config_accepts_meta_and_filters_client_headers() {
    let root = tempdir().expect("plugin root");
    let data = tempdir().expect("plugin data root");
    let outcome = parse_agent_plugin_mcp_config(
        root.path(),
        data.path(),
        r#"{
          "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
          "_meta":{"publisher":"example"},
          "mcpServers":{"docs":{
            "type":"streamable-http",
            "url":"https://example.com/mcp",
            "headers":{
              "Authorization":"Bearer ignored",
              "Content-Type":"application/json",
              "X-Plugin-Name":"café"
            },
            "_meta":{"purpose":"documentation"}
          }}
        }"#,
    )
    .expect("parse Agent Plugins HTTP config");

    assert_eq!(outcome.errors, Vec::<PluginMcpServerParseError>::new());
    let McpServerTransportConfig::StreamableHttp {
        url,
        bearer_token_env_var,
        http_headers,
        env_http_headers,
    } = &outcome.servers["docs"].transport
    else {
        panic!("expected streamable HTTP transport");
    };
    assert_eq!(
        (url, bearer_token_env_var, http_headers, env_http_headers,),
        (
            &"https://example.com/mcp".to_string(),
            &None,
            &Some(HashMap::from([(
                "X-Plugin-Name".to_string(),
                "café".to_string(),
            )])),
            &None,
        )
    );
}

#[test]
fn agent_plugin_http_config_keeps_valid_siblings() {
    let root = tempdir().expect("plugin root");
    let outcome = parse_agent_plugin_mcp_config(
        root.path(),
        root.path(),
        r#"{
          "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
          "mcpServers":{
            "docs":{"type":"streamable-http","url":"https://example.com/mcp"},
            "insecure":{"type":"streamable-http","url":"http://example.com/mcp"},
            "legacy":{"type":"sse","url":"https://example.com/sse"}
          }
        }"#,
    )
    .expect("parse Agent Plugins HTTP config");

    assert_eq!(
        outcome.servers.into_keys().collect::<Vec<_>>(),
        vec!["docs".to_string()]
    );
    assert_eq!(
        outcome.errors,
        vec![
            PluginMcpServerParseError {
                name: "insecure".to_string(),
                message: "non-loopback Agent Plugins MCP endpoints must use HTTPS".to_string(),
            },
            PluginMcpServerParseError {
                name: "legacy".to_string(),
                message: "Agent Plugins legacy SSE transport is not supported by Codex".to_string(),
            },
        ]
    );
}

#[test]
fn agent_plugin_mcp_config_rejects_unsupported_schema() {
    let root = tempdir().expect("plugin root");
    let error = parse_agent_plugin_mcp_config(
        root.path(),
        root.path(),
        r#"{"$schema":"https://agent-plugins.org/schemas/2.0.0/mcp.schema.json","mcpServers":{}}"#,
    )
    .expect_err("unsupported schema should fail");

    assert!(
        error
            .to_string()
            .contains("unsupported Agent Plugins MCP schema")
    );
}

#[test]
fn agent_plugin_stdio_config_expands_contained_paths() {
    let root = tempdir().expect("plugin root");
    let data = tempdir().expect("plugin data root");
    let root_path = std::fs::canonicalize(root.path()).expect("canonical plugin root");
    let data_path = std::fs::canonicalize(data.path()).expect("canonical plugin data root");
    let outcome = parse_agent_plugin_mcp_config(
        root.path(),
        data.path(),
        r#"{
          "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
          "mcpServers":{"demo":{
            "type":"stdio",
            "command":"./bin/server",
            "args":["--root=${PLUGIN_ROOT}","--data=${PLUGIN_DATA}"],
            "env":{"CUSTOM":"${PLUGIN_DATA}/custom"},
            "cwd":"${PLUGIN_DATA}/state",
            "_meta":{"purpose":"demo"}
          }}
        }"#,
    )
    .expect("parse Agent Plugins stdio config");

    assert_eq!(outcome.errors, Vec::<PluginMcpServerParseError>::new());
    let McpServerTransportConfig::Stdio {
        command,
        args,
        env,
        env_vars,
        cwd,
    } = &outcome.servers["demo"].transport
    else {
        panic!("expected stdio transport");
    };
    assert_eq!(
        (command, args, env, env_vars, cwd),
        (
            &root_path.join("bin/server").display().to_string(),
            &vec![
                format!("--root={}", root_path.display()),
                format!("--data={}", data_path.display()),
            ],
            &Some(HashMap::from([
                (
                    "CUSTOM".to_string(),
                    format!("{}/custom", data_path.display()),
                ),
                ("PLUGIN_DATA".to_string(), data_path.display().to_string()),
                ("PLUGIN_ROOT".to_string(), root_path.display().to_string()),
            ])),
            &Vec::new(),
            &Some(LegacyAppPathString::from_path(&data_path.join("state"))),
        )
    );
}

#[test]
fn agent_plugin_stdio_config_rejects_unsafe_paths_and_reserved_env() {
    let root = tempdir().expect("plugin root");
    for (server, expected) in [
        (
            serde_json::json!({"type":"stdio","command":"/bin/sh"}),
            "bare executable name or a contained `./` path",
        ),
        (
            serde_json::json!({
                "type":"stdio",
                "command":"python",
                "env":{"PLUGIN_ROOT":"override"}
            }),
            "cannot override reserved variable `PLUGIN_ROOT`",
        ),
        (
            serde_json::json!({
                "type":"stdio",
                "command":"python",
                "cwd":"./../outside"
            }),
            "must remain within",
        ),
    ] {
        let contents = serde_json::json!({
            "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
            "mcpServers": {"demo": server},
        })
        .to_string();
        let outcome = parse_agent_plugin_mcp_config(root.path(), root.path(), &contents)
            .expect("parse Agent Plugins stdio config");

        assert!(outcome.servers.is_empty());
        assert_eq!(outcome.errors.len(), 1);
        assert!(outcome.errors[0].message.contains(expected));
    }
}

#[test]
fn agent_plugin_placeholder_expansion_is_single_pass() {
    let parent = tempdir().expect("plugin parent");
    let root = parent.path().join("${PLUGIN_DATA}");
    let data = parent.path().join("data");
    std::fs::create_dir_all(&root).expect("create plugin root");
    std::fs::create_dir_all(&data).expect("create plugin data root");
    let root_path = std::fs::canonicalize(&root).expect("canonical plugin root");
    let data_path = std::fs::canonicalize(&data).expect("canonical plugin data root");
    let outcome = parse_agent_plugin_mcp_config(
        &root,
        &data,
        r#"{
          "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
          "mcpServers":{"demo":{
            "type":"stdio",
            "command":"python",
            "args":["${PLUGIN_ROOT}:${PLUGIN_DATA}"]
          }}
        }"#,
    )
    .expect("parse Agent Plugins stdio config");

    let McpServerTransportConfig::Stdio { args, .. } = &outcome.servers["demo"].transport else {
        panic!("expected stdio transport");
    };
    assert_eq!(
        args,
        &vec![format!("{}:{}", root_path.display(), data_path.display())]
    );
}

#[cfg(unix)]
#[test]
fn agent_plugin_stdio_cwd_rejects_symlink_escape() {
    let root = tempdir().expect("plugin root");
    let outside = tempdir().expect("outside root");
    std::os::unix::fs::symlink(outside.path(), root.path().join("outside"))
        .expect("create symlink");
    let contents = r#"{
      "$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
      "mcpServers":{"demo":{"type":"stdio","command":"python","cwd":"./outside"}}
    }"#;

    let outcome = parse_agent_plugin_mcp_config(root.path(), root.path(), contents)
        .expect("parse Agent Plugins stdio config");

    assert!(outcome.servers.is_empty());
    assert_eq!(outcome.errors.len(), 1);
    assert!(outcome.errors[0].message.contains("must remain within"));
}
