#[cfg(codex_bazel)]
pub use code_mode_proto::codex::code_mode::v1::*;

#[cfg(not(codex_bazel))]
tonic::include_proto!("codex.code_mode.v1");

use tonic::transport::Channel;

use self::code_mode_host_client::CodeModeHostClient;

pub const MAX_IDENTIFIER_BYTES: usize = 256;
pub const MAX_APPLICATION_MESSAGE_BYTES: usize = 1_024 * 1_024;
pub const MAX_CONTENT_ITEMS: usize = 1_024;
pub const MAX_TOOL_ERROR_BYTES: usize = 64 * 1_024;
pub const CLIENT_ID_METADATA_KEY: &str = "x-codex-code-mode-client-id";
pub const CAPABILITY_METADATA_KEY: &str = "x-codex-code-mode-capability";
pub const CAPABILITY_HEX_BYTES: usize = 64;

/// Builds a generated client with the gRPC application limit in both directions.
pub fn bounded_code_mode_host_client(channel: Channel) -> CodeModeHostClient<Channel> {
    CodeModeHostClient::new(channel)
        .max_decoding_message_size(MAX_APPLICATION_MESSAGE_BYTES)
        .max_encoding_message_size(MAX_APPLICATION_MESSAGE_BYTES)
}

#[cfg(test)]
#[path = "grpc_tests.rs"]
mod tests;
