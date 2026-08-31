use super::RemotePluginCatalogError;
use super::RemotePluginListResponse;
use super::RemotePluginScope;
use super::RemotePluginServiceConfig;
use super::RemotePluginSummary;
use super::authenticated_request;
use super::build_remote_plugin_summary;
use super::ensure_chatgpt_auth;
use super::send_and_decode;
use codex_login::CodexAuth;
use codex_login::default_client::build_reqwest_client;
use tracing::instrument;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemotePluginSearchRequest<'a> {
    pub query: &'a str,
    pub scope: Option<RemotePluginScope>,
    pub limit: u32,
    pub page_token: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemotePluginSearchPage {
    pub plugins: Vec<RemotePluginSummary>,
    pub next_page_token: Option<String>,
}

#[instrument(
    level = "debug",
    skip_all,
    fields(plugin.scope = ?search.scope, plugin.limit = search.limit)
)]
pub async fn search_remote_plugins(
    config: &RemotePluginServiceConfig,
    auth: Option<&CodexAuth>,
    search: RemotePluginSearchRequest<'_>,
) -> Result<RemotePluginSearchPage, RemotePluginCatalogError> {
    let auth = ensure_chatgpt_auth(auth)?;
    let base_url = config.chatgpt_base_url.trim_end_matches('/');
    let mut url = Url::parse(&format!("{base_url}/ps/plugins/search"))
        .map_err(RemotePluginCatalogError::InvalidBaseUrl)?;
    let url_for_error = url.to_string();
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("q", search.query);
        if let Some(scope) = search.scope {
            query.append_pair("scope", scope.api_value());
        }
        query.append_pair("limit", &search.limit.to_string());
        if let Some(page_token) = search.page_token {
            query.append_pair("pageToken", page_token);
        }
    }

    let url = url.to_string();
    let request = authenticated_request(build_reqwest_client().get(&url), auth)?;
    let response: RemotePluginListResponse = send_and_decode(request, &url_for_error)
        .await
        .map_err(|error| match error {
            RemotePluginCatalogError::Request { url, source } => {
                RemotePluginCatalogError::Request {
                    url,
                    source: source.without_url(),
                }
            }
            other => other,
        })?;
    let plugins = response
        .plugins
        .iter()
        .take(search.limit as usize)
        .map(|plugin| build_remote_plugin_summary(plugin, /*installed_plugin*/ None))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RemotePluginSearchPage {
        plugins,
        next_page_token: response.pagination.next_page_token,
    })
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
