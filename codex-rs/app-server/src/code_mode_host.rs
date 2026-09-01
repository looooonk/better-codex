use std::ffi::OsStr;
use std::ffi::OsString;
use std::net::IpAddr;

use clap::Args;
use clap::builder::TypedValueParser;
use clap::error::ErrorKind;
use url::Url;
use codex_code_mode::GrpcCodeModeHostCapability;

const MAX_HOST_URL_BYTES: usize = 2_048;

/// Selects the code-mode host for a single app-server process.
#[derive(Args, Debug, Clone, Default, PartialEq, Eq)]
pub struct AppServerCodeModeHostArgs {
    /// Connect to a gRPC code-mode host instead of starting a local host.
    #[arg(
        long = "code-mode-host",
        value_name = "URL",
        value_parser = RedactedHostUrlParser
    )]
    pub code_mode_host: Option<Url>,

    /// Read the server-issued code-mode host capability from this environment variable.
    #[arg(
        long = "code-mode-host-token-env",
        value_name = "ENV",
        requires = "code_mode_host",
        value_parser = parse_env_name
    )]
    pub code_mode_host_token_env: Option<String>,
}

/// Process-scoped transport used to reach the code-mode host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CodeModeHostTransport {
    /// Start and own the default local code-mode host.
    #[default]
    Local,
    /// Share an HTTP/2 gRPC connection to the specified host.
    Grpc(Url),
    /// Share a capability-authenticated HTTP/2 gRPC connection.
    AuthenticatedGrpc {
        url: Url,
        capability: GrpcCodeModeHostCapability,
    },
}

impl AppServerCodeModeHostArgs {
    /// Resolves the process transport and reads any capability from its named environment.
    pub fn try_into_transport(self) -> Result<CodeModeHostTransport, String> {
        self.resolve_with(std::env::var_os)
    }

    fn resolve_with(
        self,
        lookup: impl FnOnce(&str) -> Option<OsString>,
    ) -> Result<CodeModeHostTransport, String> {
        match (self.code_mode_host, self.code_mode_host_token_env) {
            (None, None) => Ok(CodeModeHostTransport::Local),
            (None, Some(_)) => Err(
                "code-mode host capability environment requires --code-mode-host".to_string(),
            ),
            (Some(url), None) if matches!(url.scheme(), "unix" | "https") => {
                Ok(CodeModeHostTransport::Grpc(url))
            }
            (Some(_), None) => Err(
                "plaintext HTTP code-mode hosts require --code-mode-host-token-env".to_string(),
            ),
            (Some(url), Some(environment)) => {
                let value = lookup(&environment).ok_or_else(|| {
                    format!("code-mode host capability environment {environment} is not set")
                })?;
                let value = value.into_string().map_err(|_| {
                    format!(
                        "code-mode host capability environment {environment} is not valid UTF-8"
                    )
                })?;
                let capability = GrpcCodeModeHostCapability::new(value).map_err(|_| {
                    format!("code-mode host capability environment {environment} is invalid")
                })?;
                Ok(CodeModeHostTransport::AuthenticatedGrpc { url, capability })
            }
        }
    }
}

impl CodeModeHostTransport {
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::Local => Ok(()),
            Self::Grpc(url) => {
                let url = parse_host_url(url.as_str())?;
                if !matches!(url.scheme(), "unix" | "https") {
                    return Err(
                        "plaintext HTTP code-mode hosts require a server-issued capability"
                            .to_string(),
                    );
                }
                Ok(())
            }
            Self::AuthenticatedGrpc { url, .. } => parse_host_url(url.as_str()).map(|_| ()),
        }
    }
}

fn parse_env_name(value: &str) -> Result<String, String> {
    let valid = value.len() <= 256
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| byte == b'_' || byte.is_ascii_alphanumeric() && (index > 0 || !byte.is_ascii_digit()));
    if !valid || value.is_empty() {
        return Err("code-mode host capability environment name is invalid".to_string());
    }
    Ok(value.to_string())
}

#[derive(Clone)]
struct RedactedHostUrlParser;

impl TypedValueParser for RedactedHostUrlParser {
    type Value = Url;

    fn parse_ref(
        &self,
        command: &clap::Command,
        _argument: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value = value.to_str().ok_or_else(|| {
            clap::Error::raw(
                ErrorKind::InvalidUtf8,
                "code-mode host URL must contain valid UTF-8",
            )
            .with_cmd(command)
        })?;
        parse_host_url(value)
            .map_err(|error| clap::Error::raw(ErrorKind::ValueValidation, error).with_cmd(command))
    }
}

fn parse_host_url(value: &str) -> Result<Url, String> {
    if value.len() > MAX_HOST_URL_BYTES {
        return Err("code-mode host URL is too long".to_string());
    }
    let url = Url::parse(value).map_err(|_| "invalid code-mode host URL".to_string())?;
    if url.scheme() == "unix" {
        if url.host_str().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || !url.path().starts_with('/')
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("code-mode Unix socket URL requires an absolute path".to_string());
        }
        return Ok(url);
    }
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(
            "code-mode host URL must use http://, https://, or unix: transport".to_string(),
        );
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("code-mode host URL must not contain credentials".to_string());
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(
            "code-mode host URL must not contain a path, query, or fragment".to_string(),
        );
    }
    if url.scheme() == "http"
        && !url
            .host_str()
            .and_then(|host| {
                host.trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| ip.is_loopback())
    {
        return Err(
            "plaintext code-mode hosts must use a loopback IP address".to_string(),
        );
    }
    Ok(url)
}

#[cfg(test)]
#[path = "code_mode_host_tests.rs"]
mod tests;
