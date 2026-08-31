//! Authoritative estimated credit and dollar usage for an individual Codex thread.

use super::Client;
use super::PathStyle;
use super::RequestError;
use anyhow::anyhow;
use reqwest::Method;
use reqwest::header::CONTENT_TYPE;
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde::Serialize;

const MAX_THREAD_ID_BYTES: usize = 256;
const MAX_THREAD_USAGE_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_THREAD_USAGE_ROWS: usize = 16;
const MAX_THREAD_USAGE_GROUPS: usize = 256;
const MAX_BREAKDOWN_LABEL_BYTES: usize = 256;

/// Backend usage grouped by model, reasoning effort, and response speed.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ThreadUsageBreakdownGroup {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub speed: Option<String>,
    pub estimated_usage_credits_micros: i64,
    pub net_new_input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

/// Backend-estimated usage totals expressed in integer millionths.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ThreadUsage {
    pub thread_id: String,
    pub estimated_usage_credits_micros: i64,
    pub estimated_usage_usd_micros: Option<i64>,
    #[serde(default)]
    pub groups: Vec<ThreadUsageBreakdownGroup>,
}

#[derive(Serialize)]
struct ThreadUsageQueryRequest<'a> {
    thread_ids: [&'a str; 1],
}

#[derive(Deserialize)]
struct ThreadUsageQueryResponse {
    threads: Vec<ThreadUsage>,
}

impl Client {
    /// Reads authoritative estimated totals without maintaining a second usage ledger.
    pub async fn get_thread_usage(&self, thread_id: &str) -> Result<ThreadUsage, RequestError> {
        validate_thread_id(thread_id)?;
        let url = self.thread_usage_url();
        let request = self
            .http
            .request(Method::POST, &url)
            .headers(self.headers())
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .json(&ThreadUsageQueryRequest {
                thread_ids: [thread_id],
            });
        let response = request.send().await.map_err(anyhow::Error::from)?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = read_bounded_body(response).await?;
        if !status.is_success() {
            return Err(RequestError::UnexpectedStatus {
                method: "POST".to_string(),
                url,
                status,
                content_type,
                body: String::from_utf8_lossy(&body).into_owned(),
            });
        }
        let body = String::from_utf8(body)
            .map_err(|error| RequestError::from(anyhow!("thread usage response is not UTF-8: {error}")))?;
        let response = self
            .decode_json::<ThreadUsageQueryResponse>(&url, &content_type, &body)
            .map_err(RequestError::from)?;
        if response.threads.len() > MAX_THREAD_USAGE_ROWS {
            return Err(RequestError::from(anyhow!(
                "thread usage response contains more than {MAX_THREAD_USAGE_ROWS} rows"
            )));
        }

        let usage = response
            .threads
            .into_iter()
            .find(|usage| usage.thread_id == thread_id)
            .ok_or_else(|| {
                RequestError::from(anyhow!(
                    "thread usage response did not contain requested thread {thread_id}"
                ))
            })?;
        validate_usage(&usage)?;
        Ok(usage)
    }

    fn thread_usage_url(&self) -> String {
        match self.path_style {
            PathStyle::CodexApi => {
                format!("{}/api/codex/usage/thread_usage/query", self.base_url)
            }
            PathStyle::ChatGptApi => {
                format!("{}/wham/usage/thread_usage/query", self.base_url)
            }
        }
    }
}

fn validate_thread_id(thread_id: &str) -> Result<(), RequestError> {
    if thread_id.is_empty() || thread_id.len() > MAX_THREAD_ID_BYTES {
        return Err(RequestError::from(anyhow!(
            "thread ID must contain between 1 and {MAX_THREAD_ID_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_usage(usage: &ThreadUsage) -> Result<(), RequestError> {
    if usage.groups.len() > MAX_THREAD_USAGE_GROUPS {
        return Err(RequestError::from(anyhow!(
            "thread usage response contains more than {MAX_THREAD_USAGE_GROUPS} breakdown groups"
        )));
    }
    for label in usage.groups.iter().flat_map(|group| {
        [
            group.model.as_deref(),
            group.reasoning_effort.as_deref(),
            group.speed.as_deref(),
        ]
        .into_iter()
        .flatten()
    }) {
        if label.len() > MAX_BREAKDOWN_LABEL_BYTES {
            return Err(RequestError::from(anyhow!(
                "thread usage breakdown label exceeds {MAX_BREAKDOWN_LABEL_BYTES} bytes"
            )));
        }
    }
    Ok(())
}

async fn read_bounded_body(
    mut response: reqwest::Response,
) -> Result<Vec<u8>, RequestError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(anyhow::Error::from)? {
        if body.len().saturating_add(chunk.len()) > MAX_THREAD_USAGE_RESPONSE_BYTES {
            return Err(RequestError::from(anyhow!(
                "thread usage response exceeds {MAX_THREAD_USAGE_RESPONSE_BYTES} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
#[path = "thread_usage_tests.rs"]
mod tests;
