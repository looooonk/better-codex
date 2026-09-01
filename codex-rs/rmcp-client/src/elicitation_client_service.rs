use std::sync::Arc;

use rmcp::RoleClient;
use rmcp::model::ClientInfo;
use rmcp::model::ClientResult;
use rmcp::model::CustomRequest;
use rmcp::model::CustomResult;
use rmcp::model::ElicitResult;
use rmcp::model::ElicitationAction;
use rmcp::model::MetaObject;
use rmcp::model::ProtocolVersion;
use rmcp::model::RequestMetaObject;
use rmcp::model::RequestParamsMeta;
use rmcp::model::ServerNotification;
use rmcp::model::ServerRequest;
use rmcp::service::NotificationContext;
use rmcp::service::RequestContext;
use rmcp::service::Service;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::logging_client_handler::LoggingClientHandler;
use crate::rmcp_client::Elicitation;
use crate::rmcp_client::ElicitationPauseState;
use crate::rmcp_client::ElicitationResponse;
use crate::rmcp_client::SendElicitation;
use crate::serialized_size::serialized_size_exceeds;

const MCP_PROGRESS_TOKEN_META_KEY: &str = "progressToken";
const MCP_ELICITATION_CREATE_METHOD: &str = "elicitation/create";
const OPENAI_FORM_METHOD: &str = "openai/form";
const MAX_MCP_MRTR_ELICITATION_FIELD_BYTES: usize = 4 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenAiFormRequestParams {
    #[serde(rename = "_meta")]
    meta: Option<Value>,
    message: String,
    requested_schema: Value,
}

#[derive(Clone)]
pub(crate) struct ElicitationClientService {
    handler: LoggingClientHandler,
    supports_openai_form: bool,
    send_elicitation: Arc<SendElicitation>,
    pause_state: ElicitationPauseState,
}

impl ElicitationClientService {
    pub(crate) fn new(
        client_info: ClientInfo,
        send_elicitation: SendElicitation,
        pause_state: ElicitationPauseState,
    ) -> Self {
        let supports_openai_form = client_info
            .capabilities
            .extensions
            .as_ref()
            .is_some_and(|extensions| extensions.contains_key(OPENAI_FORM_METHOD));
        let send_elicitation = Arc::new(send_elicitation);
        Self {
            handler: LoggingClientHandler::new(
                client_info,
                clone_send_elicitation(Arc::clone(&send_elicitation)),
            ),
            supports_openai_form,
            send_elicitation,
            pause_state,
        }
    }

    async fn create_elicitation(
        &self,
        request: Elicitation,
        context: RequestContext<RoleClient>,
        enforce_modern_bounds: bool,
    ) -> Result<ElicitationResponse, rmcp::ErrorData> {
        let RequestContext { id, meta, .. } = context;
        let request = restore_context_meta(request, meta);
        if enforce_modern_bounds {
            validate_elicitation_request_bounds(&request)?;
        }
        let _pause = self.pause_state.enter();
        let response = (self.send_elicitation)(id, request)
            .await
            .map_err(|err| rmcp::ErrorData::internal_error(err.to_string(), None))?;
        if enforce_modern_bounds {
            validate_elicitation_response_bounds(&response)?;
        }
        Ok(response)
    }
}

fn clone_send_elicitation(send_elicitation: Arc<SendElicitation>) -> SendElicitation {
    Box::new(move |request_id, request| send_elicitation(request_id, request))
}

impl Service<RoleClient> for ElicitationClientService {
    async fn handle_request(
        &self,
        request: ServerRequest,
        context: RequestContext<RoleClient>,
    ) -> Result<ClientResult, rmcp::ErrorData> {
        match request {
            ServerRequest::ElicitRequest(request) => {
                let modern_session = context
                    .peer
                    .peer_info()
                    .is_some_and(|info| info.protocol_version >= ProtocolVersion::V_2026_07_28);
                let response = self
                    .create_elicitation(Elicitation::Mcp(request.params), context, modern_session)
                    .await?;
                if modern_session {
                    Ok(ClientResult::ElicitResult(typed_elicitation_result(
                        response,
                    )?))
                } else {
                    Ok(ClientResult::CustomResult(elicitation_response_result(
                        response,
                    )?))
                }
            }
            ServerRequest::CustomRequest(request)
                if request.method == MCP_ELICITATION_CREATE_METHOD =>
            {
                let modern_session = context
                    .peer
                    .peer_info()
                    .is_some_and(|info| info.protocol_version >= ProtocolVersion::V_2026_07_28);
                let response = self
                    .create_elicitation(custom_mcp_elicitation(request)?, context, modern_session)
                    .await?;
                if modern_session {
                    Ok(ClientResult::ElicitResult(typed_elicitation_result(
                        response,
                    )?))
                } else {
                    Ok(ClientResult::CustomResult(elicitation_response_result(
                        response,
                    )?))
                }
            }
            ServerRequest::CustomRequest(request)
                if request.method == OPENAI_FORM_METHOD && self.supports_openai_form =>
            {
                let response = self
                    .create_elicitation(
                        openai_form_elicitation(request)?,
                        context,
                        /*enforce_modern_bounds*/ false,
                    )
                    .await?;
                Ok(ClientResult::CustomResult(elicitation_response_result(
                    response,
                )?))
            }
            request => {
                <LoggingClientHandler as Service<RoleClient>>::handle_request(
                    &self.handler,
                    request,
                    context,
                )
                .await
            }
        }
    }

    async fn handle_notification(
        &self,
        notification: ServerNotification,
        context: NotificationContext<RoleClient>,
    ) -> Result<(), rmcp::ErrorData> {
        <LoggingClientHandler as Service<RoleClient>>::handle_notification(
            &self.handler,
            notification,
            context,
        )
        .await
    }

    fn get_info(&self) -> ClientInfo {
        <LoggingClientHandler as Service<RoleClient>>::get_info(&self.handler)
    }
}

fn custom_mcp_elicitation(request: CustomRequest) -> Result<Elicitation, rmcp::ErrorData> {
    let raw_params = request
        .params
        .ok_or_else(|| rmcp::ErrorData::invalid_params("missing params", None))?;
    let params = serde_json::from_value(raw_params)
        .map_err(|err| rmcp::ErrorData::invalid_params(err.to_string(), None))?;
    Ok(Elicitation::Mcp(params))
}

fn openai_form_elicitation(request: CustomRequest) -> Result<Elicitation, rmcp::ErrorData> {
    let params = request
        .params_as::<OpenAiFormRequestParams>()
        .map_err(|err| rmcp::ErrorData::invalid_params(err.to_string(), None))?
        .ok_or_else(|| rmcp::ErrorData::invalid_params("missing params", None))?;
    Ok(Elicitation::OpenAiForm {
        meta: params.meta,
        message: params.message,
        requested_schema: params.requested_schema,
    })
}

fn restore_context_meta(
    mut request: Elicitation,
    mut context_meta: RequestMetaObject,
) -> Elicitation {
    // RMCP lifts JSON-RPC `_meta` into RequestContext before invoking services.
    context_meta.remove(MCP_PROGRESS_TOKEN_META_KEY);
    if context_meta.is_empty() {
        return request;
    }

    match &mut request {
        Elicitation::Mcp(request) => request
            .meta_mut()
            .get_or_insert_with(RequestMetaObject::new)
            .extend(context_meta),
        Elicitation::OpenAiForm { meta, .. } => {
            let meta = meta
                .get_or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut();
            if let Some(meta) = meta {
                meta.extend(context_meta.0.0);
            }
        }
    }
    request
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyElicitationResultWithMeta {
    action: ElicitationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    meta: Option<Value>,
}

fn elicitation_response_result(
    response: ElicitationResponse,
) -> Result<CustomResult, rmcp::ErrorData> {
    let ElicitationResponse {
        action,
        content,
        meta,
    } = response;
    let result = LegacyElicitationResultWithMeta {
        action,
        content,
        meta,
    };

    serde_json::to_value(result)
        .map(CustomResult)
        .map_err(|err| rmcp::ErrorData::internal_error(err.to_string(), None))
}

fn typed_elicitation_result(
    response: ElicitationResponse,
) -> Result<ElicitResult, rmcp::ErrorData> {
    let ElicitationResponse {
        action,
        content,
        meta,
    } = response;
    let mut result = ElicitResult::new(action);
    result.content = content;
    result.meta = match meta {
        None => None,
        Some(Value::Object(meta)) => Some(MetaObject::from(meta)),
        Some(meta) => {
            return Err(rmcp::ErrorData::invalid_params(
                format!("MCP elicitation response _meta must be an object, got {meta}"),
                None,
            ));
        }
    };
    Ok(result)
}

fn validate_elicitation_request_bounds(request: &Elicitation) -> Result<(), rmcp::ErrorData> {
    if let Some(meta) = request.meta() {
        validate_serialized_field_size("elicitation request _meta", meta)?;
    }
    Ok(())
}

fn validate_elicitation_response_bounds(
    response: &ElicitationResponse,
) -> Result<(), rmcp::ErrorData> {
    if let Some(content) = response.content.as_ref() {
        validate_serialized_field_size("elicitation response content", content)?;
    }
    if let Some(meta) = response.meta.as_ref() {
        validate_serialized_field_size("elicitation response _meta", meta)?;
    }
    Ok(())
}

fn validate_serialized_field_size(
    field: &str,
    value: &impl Serialize,
) -> Result<(), rmcp::ErrorData> {
    if serialized_size_exceeds(value, MAX_MCP_MRTR_ELICITATION_FIELD_BYTES)
        .map_err(|error| rmcp::ErrorData::invalid_params(error.to_string(), None))?
    {
        return Err(rmcp::ErrorData::invalid_params(
            format!("MCP {field} exceeds {MAX_MCP_MRTR_ELICITATION_FIELD_BYTES} bytes"),
            None,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rmcp::model::BooleanSchema;
    use rmcp::model::ElicitRequestParams;
    use rmcp::model::ElicitationSchema;
    use rmcp::model::PrimitiveSchemaDefinition;
    use serde_json::Value;
    use serde_json::json;

    use super::*;

    #[test]
    fn restore_context_meta_adds_elicitation_meta_and_removes_progress_token() {
        let request = restore_context_meta(
            Elicitation::Mcp(form_request(/*meta*/ None)),
            meta(json!({
                "progressToken": "progress-token",
                "persist": ["session", "always"],
            })),
        );

        assert_eq!(
            request,
            Elicitation::Mcp(form_request(Some(meta(json!({
                "persist": ["session", "always"],
            })))))
        );
    }

    #[test]
    fn parses_openai_form_custom_requests() {
        let elicitation = openai_form_elicitation(CustomRequest::new(
            OPENAI_FORM_METHOD,
            Some(json!({
                "message": "Select a template",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "template": {
                            "type": "openai/imagePicker",
                            "items": [{
                                "id": "monthly-review",
                                "title": "Monthly review",
                                "image": "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciLz4="
                            }]
                        }
                    }
                }
            })),
        ))
        .expect("valid openai/form request");

        assert_eq!(
            elicitation,
            Elicitation::OpenAiForm {
                meta: None,
                message: "Select a template".to_string(),
                requested_schema: json!({
                    "type": "object",
                    "properties": {
                        "template": {
                            "type": "openai/imagePicker",
                            "items": [{
                                "id": "monthly-review",
                                "title": "Monthly review",
                                "image": "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciLz4="
                            }]
                        }
                    }
                }),
            }
        );
    }

    #[test]
    fn elicitation_response_result_serializes_response_meta() {
        let result = rmcp::model::ClientResult::CustomResult(
            elicitation_response_result(ElicitationResponse {
                action: ElicitationAction::Accept,
                content: Some(json!({ "confirmed": true })),
                meta: Some(json!({ "persist": "always" })),
            })
            .expect("elicitation response should serialize"),
        );

        assert_eq!(
            serde_json::to_value(result).expect("client result should serialize"),
            json!({
                "action": "accept",
                "content": { "confirmed": true },
                "_meta": { "persist": "always" },
            })
        );
    }

    #[test]
    fn modern_elicitation_request_metadata_is_bounded() {
        let at_limit = form_request(Some(meta(json!({
            "v": "x".repeat(MAX_MCP_MRTR_ELICITATION_FIELD_BYTES - 8),
        }))));
        validate_elicitation_request_bounds(&Elicitation::Mcp(at_limit))
            .expect("metadata at the serialized limit must be accepted");

        let oversized = form_request(Some(meta(json!({
            "v": "x".repeat(MAX_MCP_MRTR_ELICITATION_FIELD_BYTES - 7),
        }))));
        let error = validate_elicitation_request_bounds(&Elicitation::Mcp(oversized))
            .expect_err("oversized metadata must be rejected");
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn modern_elicitation_response_fields_are_bounded() {
        validate_elicitation_response_bounds(&ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(json!("x".repeat(MAX_MCP_MRTR_ELICITATION_FIELD_BYTES - 2))),
            meta: Some(json!({})),
        })
        .expect("content at the serialized limit must be accepted");

        let error = validate_elicitation_response_bounds(&ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(json!("x".repeat(MAX_MCP_MRTR_ELICITATION_FIELD_BYTES - 1))),
            meta: Some(json!({})),
        })
        .expect_err("oversized content must be rejected");
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);

        let error = validate_elicitation_response_bounds(&ElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(json!({})),
            meta: Some(json!({
                "v": "x".repeat(MAX_MCP_MRTR_ELICITATION_FIELD_BYTES - 7),
            })),
        })
        .expect_err("oversized response metadata must be rejected");
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    fn form_request(meta: Option<RequestMetaObject>) -> ElicitRequestParams {
        ElicitRequestParams::FormElicitationParams {
            meta,
            message: "Confirm?".to_string(),
            requested_schema: ElicitationSchema::builder()
                .required_property(
                    "confirmed",
                    PrimitiveSchemaDefinition::Boolean(BooleanSchema::new()),
                )
                .build()
                .expect("schema should build"),
        }
    }

    fn meta(value: Value) -> RequestMetaObject {
        let Value::Object(map) = value else {
            panic!("meta must be an object");
        };
        RequestMetaObject::from(map)
    }
}
