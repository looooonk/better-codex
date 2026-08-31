use codex_exec_server::HttpRedirectPolicy;
use reqwest::header::HeaderMap;
use rmcp::model::ClientJsonRpcMessage;
use rmcp::model::ConstString;
use rmcp::model::DiscoverRequestMethod;
use rmcp::model::ErrorCode;
use rmcp::model::ErrorData;
use rmcp::model::JsonRpcMessage;
use rmcp::model::ProtocolVersion;
use rmcp::model::ServerJsonRpcMessage;
use rmcp::model::ServerResult;
use rmcp::transport::common::http_header::HEADER_MCP_PROTOCOL_VERSION;

const LEGACY_HTTP_PREVALIDATION_ERROR_CODE: ErrorCode = ErrorCode(-32000);

pub(super) fn mcp_redirect_policy(headers: &HeaderMap) -> HttpRedirectPolicy {
    if headers
        .get(HEADER_MCP_PROTOCOL_VERSION)
        .and_then(|value| value.to_str().ok())
        == Some(ProtocolVersion::V_2026_07_28.as_str())
    {
        HttpRedirectPolicy::Stop
    } else {
        HttpRedirectPolicy::Follow
    }
}

/// Return a discovery response only when it belongs to the request.
///
/// The sole uncorrelated compatibility case is an id-less HTTP prevalidation
/// error with a deployed legacy rejection shape.
pub(super) fn correlated_discovery_response(
    request: &ClientJsonRpcMessage,
    response: ServerJsonRpcMessage,
    allow_idless_http_prevalidation: bool,
) -> Option<ServerJsonRpcMessage> {
    let JsonRpcMessage::Request(request_message) = request else {
        return Some(response);
    };
    if request_message.request.method() != DiscoverRequestMethod::VALUE {
        return Some(response);
    }

    let response_matches = match &response {
        JsonRpcMessage::Response(response) => response.id == request_message.id,
        JsonRpcMessage::Error(error) => error.id.as_ref() == Some(&request_message.id),
        JsonRpcMessage::Request(_) | JsonRpcMessage::Notification(_) => false,
    };
    let idless_prevalidation = allow_idless_http_prevalidation
        && matches!(
            &response,
            JsonRpcMessage::Error(error)
                if error.id.is_none()
                    && error.error.code == LEGACY_HTTP_PREVALIDATION_ERROR_CODE
                    && has_legacy_fallback_evidence(&error.error.message)
        );
    (response_matches || idless_prevalidation)
        .then(|| legacy_discovery_fallback_response(request, response, idless_prevalidation))
}

// rmcp's automatic lifecycle does not yet recognize deployed legacy discovery
// rejection shapes. Remove this compatibility shim once the SDK does:
// https://github.com/modelcontextprotocol/rust-sdk/issues/1040
pub(super) fn legacy_discovery_fallback_response(
    request: &ClientJsonRpcMessage,
    response: ServerJsonRpcMessage,
    allow_uncorrelated_http_rejection: bool,
) -> ServerJsonRpcMessage {
    let JsonRpcMessage::Request(request) = request else {
        return response;
    };
    if request.request.method() != DiscoverRequestMethod::VALUE {
        return response;
    }

    let requires_legacy_initialization = match &response {
        JsonRpcMessage::Response(response) if response.id == request.id => match &response.result {
            ServerResult::DiscoverResult(result) => {
                only_known_legacy_protocol_versions(&result.supported_versions)
            }
            _ => false,
        },
        JsonRpcMessage::Error(error) if error.id.as_ref() == Some(&request.id) => {
            (error.error.code == ErrorCode::UNSUPPORTED_PROTOCOL_VERSION
                && error
                    .error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("supported"))
                    .and_then(|supported| {
                        serde_json::from_value::<Vec<ProtocolVersion>>(supported.clone()).ok()
                    })
                    .is_some_and(|supported| only_known_legacy_protocol_versions(&supported)))
                || matches!(
                    error.error.code,
                    ErrorCode::UNSUPPORTED_PROTOCOL_VERSION
                        | ErrorCode::INVALID_REQUEST
                        | ErrorCode::INVALID_PARAMS
                ) && explicitly_rejects_modern_protocol_version(&error.error.message)
        }
        JsonRpcMessage::Error(error)
            if allow_uncorrelated_http_rejection
                && error.id.is_none()
                && error.error.code == LEGACY_HTTP_PREVALIDATION_ERROR_CODE =>
        {
            has_legacy_fallback_evidence(&error.error.message)
        }
        _ => false,
    };

    if requires_legacy_initialization {
        ServerJsonRpcMessage::error(
            ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                "MCP discovery requires legacy initialization",
                None,
            ),
            Some(request.id.clone()),
        )
    } else {
        response
    }
}

fn only_known_legacy_protocol_versions(versions: &[ProtocolVersion]) -> bool {
    !versions.is_empty()
        && versions.iter().all(|version| {
            ProtocolVersion::KNOWN_VERSIONS.contains(version)
                && version < &ProtocolVersion::V_2026_07_28
        })
}

fn explicitly_rejects_modern_protocol_version(message: &str) -> bool {
    message
        .trim()
        .eq_ignore_ascii_case("unsupported protocol version: 2026-07-28")
}

fn has_legacy_fallback_evidence(message: &str) -> bool {
    if message == "Bad Request: No valid session ID provided" {
        return true;
    }

    let Some(supported) = message
        .strip_prefix("Bad Request: Unsupported protocol version: 2026-07-28 (supported versions: ")
        .or_else(|| {
            message.strip_prefix("Bad Request: Unsupported protocol version (supported versions: ")
        })
        .and_then(|supported| supported.strip_suffix(')'))
    else {
        return false;
    };

    let versions = supported.split(',').map(str::trim).collect::<Vec<_>>();
    !versions.is_empty()
        && ProtocolVersion::KNOWN_VERSIONS
            .iter()
            .any(|known| versions.contains(&known.as_str()))
        && versions.iter().all(|version| {
            let bytes = version.as_bytes();
            bytes.len() == 10
                && bytes[4] == b'-'
                && bytes[7] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
                && *version < "2026-07-28"
        })
}
