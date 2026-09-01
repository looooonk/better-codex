use super::*;

impl ThreadHistoryBuilder {
    pub(super) fn handle_web_search_begin(&mut self, payload: &WebSearchBeginEvent) {
        let item = ThreadItem::WebSearch(WebSearchItem {
            id: payload.call_id.clone(),
            query: String::new(),
            action: None,
            results: None,
        });
        self.upsert_item_in_current_turn(item);
    }

    pub(super) fn handle_web_search_end(&mut self, payload: &WebSearchEndEvent) {
        let item = ThreadItem::WebSearch(WebSearchItem {
            id: payload.call_id.clone(),
            query: payload.query.clone(),
            action: Some(web_search_action_from_core(payload.action.clone())),
            results: payload.results.clone(),
        });
        self.upsert_item_in_current_turn(item);
    }

    pub(super) fn handle_exec_command_begin(&mut self, payload: &ExecCommandBeginEvent) {
        let item = build_command_execution_begin_item(payload);
        self.upsert_item_in_turn_id(&payload.turn_id, item);
    }

    pub(super) fn handle_exec_command_end(&mut self, payload: &ExecCommandEndEvent) {
        let item = build_command_execution_end_item(payload);
        // Command completions can arrive out of order. Unified exec may return
        // while a PTY is still running, then emit ExecCommandEnd later from a
        // background exit watcher when that process finally exits. By then, a
        // newer user turn may already have started. Route by event turn_id so
        // replay preserves the original turn association.
        self.upsert_item_in_turn_id(&payload.turn_id, item);
    }

    pub(super) fn handle_guardian_assessment(&mut self, payload: &GuardianAssessmentEvent) {
        let status = match payload.status {
            GuardianAssessmentStatus::InProgress => CommandExecutionStatus::InProgress,
            GuardianAssessmentStatus::Denied | GuardianAssessmentStatus::Aborted => {
                CommandExecutionStatus::Declined
            }
            GuardianAssessmentStatus::TimedOut => CommandExecutionStatus::Failed,
            GuardianAssessmentStatus::Approved => return,
        };
        let Some(item) = build_item_from_guardian_event(payload, status) else {
            return;
        };
        if payload.turn_id.is_empty() {
            self.upsert_item_in_current_turn(item);
        } else {
            self.upsert_item_in_turn_id(&payload.turn_id, item);
        }
    }

    pub(super) fn handle_apply_patch_approval_request(
        &mut self,
        payload: &ApplyPatchApprovalRequestEvent,
    ) {
        let item = build_file_change_approval_request_item(payload);
        if payload.turn_id.is_empty() {
            self.upsert_item_in_current_turn(item);
        } else {
            self.upsert_item_in_turn_id(&payload.turn_id, item);
        }
    }

    pub(super) fn handle_patch_apply_begin(&mut self, payload: &PatchApplyBeginEvent) {
        let item = build_file_change_begin_item(payload);
        if payload.turn_id.is_empty() {
            self.upsert_item_in_current_turn(item);
        } else {
            self.upsert_item_in_turn_id(&payload.turn_id, item);
        }
    }

    pub(super) fn handle_patch_apply_end(&mut self, payload: &PatchApplyEndEvent) {
        let item = build_file_change_end_item(payload);
        if payload.turn_id.is_empty() {
            self.upsert_item_in_current_turn(item);
        } else {
            self.upsert_item_in_turn_id(&payload.turn_id, item);
        }
    }

    pub(super) fn handle_dynamic_tool_call_request(
        &mut self,
        payload: &codex_protocol::dynamic_tools::DynamicToolCallRequest,
    ) {
        let item = ThreadItem::DynamicToolCall {
            id: payload.call_id.clone(),
            namespace: payload.namespace.clone(),
            tool: payload.tool.clone(),
            arguments: payload.arguments.clone(),
            status: DynamicToolCallStatus::InProgress,
            content_items: None,
            success: None,
            duration_ms: None,
        };
        if payload.turn_id.is_empty() {
            self.upsert_item_in_current_turn(item);
        } else {
            self.upsert_item_in_turn_id(&payload.turn_id, item);
        }
    }

    pub(super) fn handle_dynamic_tool_call_response(
        &mut self,
        payload: &DynamicToolCallResponseEvent,
    ) {
        let status = if payload.success {
            DynamicToolCallStatus::Completed
        } else {
            DynamicToolCallStatus::Failed
        };
        let duration_ms = i64::try_from(payload.duration.as_millis()).ok();
        let item = ThreadItem::DynamicToolCall {
            id: payload.call_id.clone(),
            namespace: payload.namespace.clone(),
            tool: payload.tool.clone(),
            arguments: payload.arguments.clone(),
            status,
            content_items: Some(convert_dynamic_tool_content_items(&payload.content_items)),
            success: Some(payload.success),
            duration_ms,
        };
        if payload.turn_id.is_empty() {
            self.upsert_item_in_current_turn(item);
        } else {
            self.upsert_item_in_turn_id(&payload.turn_id, item);
        }
    }

    pub(super) fn handle_mcp_tool_call_begin(&mut self, payload: &McpToolCallBeginEvent) {
        let item = ThreadItem::McpToolCall {
            id: payload.call_id.clone(),
            server: payload.invocation.server.clone(),
            tool: payload.invocation.tool.clone(),
            status: McpToolCallStatus::InProgress,
            arguments: payload
                .invocation
                .arguments
                .clone()
                .unwrap_or(serde_json::Value::Null),
            app_context: payload
                .connector_id
                .clone()
                .map(|connector_id| McpToolCallAppContext {
                    connector_id,
                    link_id: payload.link_id.clone(),
                    resource_uri: payload.mcp_app_resource_uri.clone(),
                    app_name: payload.app_name.clone(),
                    template_id: payload.template_id.clone(),
                    action_name: payload.action_name.clone(),
                }),
            mcp_app_resource_uri: payload.mcp_app_resource_uri.clone(),
            plugin_id: payload.plugin_id.clone(),
            result: None,
            error: None,
            duration_ms: None,
        };
        self.upsert_item_in_current_turn(item);
    }

    pub(super) fn handle_mcp_tool_call_end(&mut self, payload: &McpToolCallEndEvent) {
        let status = if payload.is_success() {
            McpToolCallStatus::Completed
        } else {
            McpToolCallStatus::Failed
        };
        let duration_ms = i64::try_from(payload.duration.as_millis()).ok();
        let (result, error) = match &payload.result {
            Ok(value) => (
                Some(Box::new(McpToolCallResult {
                    content: value.content.clone(),
                    structured_content: value.structured_content.clone(),
                    meta: value.meta.clone(),
                })),
                None,
            ),
            Err(message) => (
                None,
                Some(McpToolCallError {
                    message: message.clone(),
                }),
            ),
        };
        let item = ThreadItem::McpToolCall {
            id: payload.call_id.clone(),
            server: payload.invocation.server.clone(),
            tool: payload.invocation.tool.clone(),
            status,
            arguments: payload
                .invocation
                .arguments
                .clone()
                .unwrap_or(serde_json::Value::Null),
            app_context: payload
                .connector_id
                .clone()
                .map(|connector_id| McpToolCallAppContext {
                    connector_id,
                    link_id: payload.link_id.clone(),
                    resource_uri: payload.mcp_app_resource_uri.clone(),
                    app_name: payload.app_name.clone(),
                    template_id: payload.template_id.clone(),
                    action_name: payload.action_name.clone(),
                }),
            mcp_app_resource_uri: payload.mcp_app_resource_uri.clone(),
            plugin_id: payload.plugin_id.clone(),
            result,
            error,
            duration_ms,
        };
        self.upsert_item_in_current_turn(item);
    }

    pub(super) fn handle_view_image_tool_call(&mut self, payload: &ViewImageToolCallEvent) {
        let item = ThreadItem::ImageView {
            id: payload.call_id.clone(),
            path: payload.path.clone().into(),
        };
        self.upsert_item_in_current_turn(item);
    }

    pub(super) fn handle_image_generation_begin(&mut self, payload: &ImageGenerationBeginEvent) {
        let item = ThreadItem::ImageGeneration(ImageGenerationItem {
            id: payload.call_id.clone(),
            status: String::new(),
            revised_prompt: None,
            result: String::new(),
            saved_path: None,
        });
        self.upsert_item_in_current_turn(item);
    }

    pub(super) fn handle_image_generation_end(&mut self, payload: &ImageGenerationEndEvent) {
        let item = ThreadItem::ImageGeneration(ImageGenerationItem {
            id: payload.call_id.clone(),
            status: payload.status.clone(),
            revised_prompt: payload.revised_prompt.clone(),
            result: payload.result.clone(),
            saved_path: payload.saved_path.clone(),
        });
        self.upsert_item_in_current_turn(item);
    }
}
