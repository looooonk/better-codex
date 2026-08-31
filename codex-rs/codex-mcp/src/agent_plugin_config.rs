use super::PluginMcpConfigParseOutcome;
use super::PluginMcpServerParseError;
use codex_config::McpServerConfig;
use serde::Deserialize;
use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use url::Host;

const AGENT_PLUGIN_MCP_SCHEMA_URI: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";
const CLIENT_OWNED_HTTP_HEADERS: &[&str] = &[
    "accept",
    "authorization",
    "connection",
    "content-encoding",
    "content-length",
    "content-type",
    "host",
    "last-event-id",
    "mcp-protocol-version",
    "mcp-session-id",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AgentPluginMcpFile {
    #[serde(rename = "$schema")]
    schema: String,
    mcp_servers: BTreeMap<String, JsonValue>,
    #[serde(default, rename = "_meta")]
    _meta: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum AgentPluginMcpServer {
    #[serde(rename = "stdio")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        cwd: Option<String>,
        #[serde(default, rename = "_meta")]
        _meta: Option<JsonValue>,
    },
    #[serde(rename = "streamable-http")]
    StreamableHttp {
        url: String,
        headers: Option<BTreeMap<String, String>>,
        #[serde(default, rename = "_meta")]
        _meta: Option<JsonValue>,
    },
    #[serde(rename = "sse")]
    Sse {
        #[serde(rename = "url")]
        _url: String,
        #[serde(rename = "headers")]
        _headers: Option<BTreeMap<String, String>>,
        #[serde(default, rename = "_meta")]
        _meta: Option<JsonValue>,
    },
}

/// Translates an Agent Plugins `mcp.json` into Codex MCP configuration.
pub fn parse_agent_plugin_mcp_config(
    plugin_root: &Path,
    plugin_data_root: &Path,
    contents: &str,
) -> Result<PluginMcpConfigParseOutcome, serde_json::Error> {
    let AgentPluginMcpFile {
        schema,
        mcp_servers,
        ..
    } = serde_json::from_str(contents)?;
    if schema != AGENT_PLUGIN_MCP_SCHEMA_URI {
        return Err(plugin_mcp_json_error(format!(
            "unsupported Agent Plugins MCP schema `{schema}`; supported schema: {AGENT_PLUGIN_MCP_SCHEMA_URI}"
        )));
    }

    let mut outcome = PluginMcpConfigParseOutcome::default();
    for (name, value) in mcp_servers {
        match normalize_agent_plugin_mcp_server(value, plugin_root, plugin_data_root) {
            Ok(config) => {
                outcome.servers.insert(name, config);
            }
            Err(message) => outcome
                .errors
                .push(PluginMcpServerParseError { name, message }),
        }
    }
    Ok(outcome)
}

fn normalize_agent_plugin_mcp_server(
    value: JsonValue,
    plugin_root: &Path,
    plugin_data_root: &Path,
) -> Result<McpServerConfig, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "Agent Plugins MCP server must be an object".to_string())?;
    match object.get("type").and_then(JsonValue::as_str) {
        Some("stdio") => reject_explicit_null(object, "cwd")?,
        Some("streamable-http" | "sse") => reject_explicit_null(object, "headers")?,
        _ => {}
    }
    let server =
        serde_json::from_value::<AgentPluginMcpServer>(value).map_err(|err| err.to_string())?;
    let normalized = match server {
        AgentPluginMcpServer::Stdio {
            command,
            args,
            env,
            cwd,
            ..
        } => super::agent_plugin_paths::normalize_agent_plugin_stdio_server(
            command,
            args,
            env,
            cwd,
            plugin_root,
            plugin_data_root,
        )?,
        AgentPluginMcpServer::StreamableHttp { url, headers, .. } => {
            normalize_agent_plugin_http_server(url, headers)?
        }
        AgentPluginMcpServer::Sse { .. } => {
            return Err("Agent Plugins legacy SSE transport is not supported by Codex".to_string());
        }
    };
    serde_json::from_value(JsonValue::Object(normalized)).map_err(|err| err.to_string())
}

fn normalize_agent_plugin_http_server(
    url: String,
    mut headers: Option<BTreeMap<String, String>>,
) -> Result<JsonMap<String, JsonValue>, String> {
    validate_agent_plugin_url(&url)?;
    if let Some(headers) = headers.as_mut() {
        validate_agent_plugin_headers(headers)?;
        headers.retain(|name, _| {
            !CLIENT_OWNED_HTTP_HEADERS
                .iter()
                .any(|owned| name.eq_ignore_ascii_case(owned))
        });
    }
    let mut normalized = JsonMap::from_iter([("url".to_string(), JsonValue::String(url))]);
    if let Some(headers) = headers.filter(|headers| !headers.is_empty()) {
        normalized.insert("http_headers".to_string(), string_map_value(headers));
    }
    Ok(normalized)
}

fn validate_agent_plugin_url(raw_url: &str) -> Result<(), String> {
    if raw_url.is_empty() {
        return Err("Agent Plugins HTTP server requires a non-empty `url`".to_string());
    }
    let parsed = url::Url::parse(raw_url)
        .map_err(|err| format!("invalid Agent Plugins MCP URL `{raw_url}`: {err}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("Agent Plugins MCP URL must be absolute HTTP or HTTPS".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() || parsed.fragment().is_some() {
        return Err(
            "Agent Plugins MCP URL must not contain user information or a fragment".to_string(),
        );
    }
    let is_loopback = match parsed.host() {
        Some(Host::Domain(host)) => host == "localhost",
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    if parsed.scheme() == "http" && !is_loopback {
        return Err("non-loopback Agent Plugins MCP endpoints must use HTTPS".to_string());
    }
    Ok(())
}

fn validate_agent_plugin_headers(headers: &BTreeMap<String, String>) -> Result<(), String> {
    let mut seen = HashSet::new();
    for (name, value) in headers {
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "duplicate case-insensitive Agent Plugins HTTP header `{name}`"
            ));
        }
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        {
            return Err(format!("invalid Agent Plugins HTTP header name `{name}`"));
        }
        if value
            .bytes()
            .any(|byte| (byte < 32 && byte != b'\t') || byte == 127)
        {
            return Err(format!(
                "invalid Agent Plugins HTTP header value for `{name}`"
            ));
        }
    }
    Ok(())
}

fn reject_explicit_null(object: &JsonMap<String, JsonValue>, field: &str) -> Result<(), String> {
    if object.get(field).is_some_and(JsonValue::is_null) {
        return Err(format!(
            "Agent Plugins MCP `{field}` must use its declared type when present"
        ));
    }
    Ok(())
}

fn string_map_value(values: BTreeMap<String, String>) -> JsonValue {
    JsonValue::Object(
        values
            .into_iter()
            .map(|(name, value)| (name, JsonValue::String(value)))
            .collect(),
    )
}

fn plugin_mcp_json_error(message: impl Into<String>) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}

#[cfg(test)]
#[path = "agent_plugin_config_tests.rs"]
mod tests;
