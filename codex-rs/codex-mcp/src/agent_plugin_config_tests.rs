use super::*;
use codex_config::McpServerTransportConfig;
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
        (
            url,
            bearer_token_env_var,
            http_headers,
            env_http_headers,
        ),
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
                message: "Agent Plugins legacy SSE transport is not supported by Codex"
                    .to_string(),
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
