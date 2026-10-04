use super::ShellState;
use super::selector::SelectorOption;
use super::selector::SelectorState;
use super::selector::SelectorValue;
use super::workspace_requests::workspace_request_id;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::FuzzyFileSearchParams;
use codex_app_server_protocol::FuzzyFileSearchResponse;
use codex_app_server_protocol::FuzzyFileSearchResult;
use color_eyre::Result;
use std::time::Duration;

pub(super) async fn search(
    client: AppServerRequestHandle,
    cwd: String,
    query: String,
) -> Result<Vec<FuzzyFileSearchResult>> {
    let response: FuzzyFileSearchResponse = tokio::time::timeout(
        Duration::from_secs(/*secs*/ 10),
        client.request_typed(ClientRequest::FuzzyFileSearch {
            request_id: workspace_request_id("file-mention"),
            params: FuzzyFileSearchParams {
                query,
                roots: vec![cwd],
                cancellation_token: None,
            },
        }),
    )
    .await??;
    Ok(response.files.into_iter().take(100).collect())
}

impl ShellState {
    pub(super) fn open_file_mentions(&mut self, files: Vec<FuzzyFileSearchResult>) {
        if files.is_empty() {
            self.push_status("No matching files. Use /mention <search> to try another name.");
            return;
        }
        let options = files
            .into_iter()
            .map(|file| {
                let path = std::path::Path::new(&file.root)
                    .join(&file.path)
                    .to_string_lossy()
                    .into_owned();
                SelectorOption::new(SelectorValue::FileMention(path), file.path, file.root)
            })
            .collect();
        self.open_selector(SelectorState::new("Mention a file", options));
    }

    pub(super) fn insert_file_mention(&mut self, path: &str) {
        let path = if path
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '"' | '\\'))
        {
            serde_json::to_string(path).expect("a string can be serialized")
        } else {
            path.to_string()
        };
        let draft = self.composer.text();
        let separator = if draft.is_empty() || draft.ends_with(char::is_whitespace) {
            ""
        } else {
            " "
        };
        self.composer
            .set_text(format!("{draft}{separator}@{path} "));
    }
}

#[cfg(test)]
#[path = "file_mentions_tests.rs"]
mod tests;
