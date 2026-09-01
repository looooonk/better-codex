use std::ffi::OsStr;
use std::net::IpAddr;

use clap::Args;
use clap::builder::TypedValueParser;
use clap::error::ErrorKind;
use url::Url;

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
}

/// Process-scoped transport used to reach the code-mode host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CodeModeHostTransport {
    /// Start and own the default local code-mode host.
    #[default]
    Local,
    /// Share an HTTP/2 gRPC connection to the specified host.
    Grpc(Url),
}

impl From<AppServerCodeModeHostArgs> for CodeModeHostTransport {
    fn from(args: AppServerCodeModeHostArgs) -> Self {
        match args.code_mode_host {
            Some(url) => Self::Grpc(url),
            None => Self::Local,
        }
    }
}

impl CodeModeHostTransport {
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self {
            Self::Local => Ok(()),
            Self::Grpc(url) => parse_host_url(url.as_str()).map(|_| ()),
        }
    }
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
