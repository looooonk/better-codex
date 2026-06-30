use codex_app_server_protocol::RequestId as AppServerRequestId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedAppServerRequest {
    ExecApproval {
        id: String,
    },
    FileChangeApproval {
        id: String,
    },
    PermissionsApproval {
        id: String,
    },
    UserInput {
        call_id: String,
    },
    McpElicitation {
        server_name: String,
        request_id: AppServerRequestId,
    },
}
