use super::super::test_support::scripted_server;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn switching_accounts_validates_workspace_before_mutation() {
    let (client, server) = scripted_server(vec![(
        "accountSession/list",
        json!({"activeSessionId":null,"sessions":[]}),
    )])
    .await;
    let error = execute(
        AppServerRequestHandle::Remote(client.request_handle()),
        AccountCommand::Switch {
            session_id: "missing".into(),
            account_id: "workspace".into(),
        },
        "openai",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("unknown saved account"));
    client.shutdown().await.unwrap();
    assert_eq!(
        server.await.unwrap(),
        vec![json!({"refreshWorkspaceMetadata":true})]
    );
}

#[tokio::test]
async fn saved_workspace_can_require_browser_login() {
    let (client, server) = scripted_server(vec![
        ("accountSession/list", json!({"activeSessionId":"session-1","sessions":[{
            "sessionId":"session-1","email":"user@example.test","userId":null,"displayName":null,"imageUrl":null,
            "lastUsedAt":0,"isActive":true,"selectedWorkspaceAccountId":"workspace-1",
            "workspaces":[{"accountId":"workspace-1","name":"Work","imageUrl":null,"kind":"workspace"}]
        }]})),
        ("accountSession/switch", json!({"loginId":"login-1","authUrl":"https://auth.example.test/work"})),
    ]).await;
    let response = execute(
        AppServerRequestHandle::Remote(client.request_handle()),
        AccountCommand::parse("switch session-1 workspace-1").unwrap(),
        "openai",
    )
    .await
    .unwrap();
    let WorkspaceResponse::AccountLogin { login_id, auth_url } = response else {
        panic!("expected browser login")
    };
    assert_eq!(
        (login_id, auth_url),
        ("login-1".into(), "https://auth.example.test/work".into())
    );
    client.shutdown().await.unwrap();
    assert_eq!(
        server.await.unwrap()[1],
        json!({"sessionId":"session-1","accountId":"workspace-1","allowBrowserLogin":true})
    );
}

#[tokio::test]
async fn cloud_refresh_uses_the_cloud_catalog_without_optional_requirements() {
    use codex_app_server_client::AppServerClient;
    use codex_app_server_client::CloudClient;
    use codex_app_server_client::CloudOperation;
    use codex_app_server_protocol::JSONRPCErrorError;

    let (client, mut commands, _events) = CloudClient::channel(/*capacity*/ 1);
    let handle = AppServerClient::Cloud(client).request_handle();
    let worker = tokio::spawn(async move {
        let command = commands.recv().await.unwrap();
        let CloudOperation::Request(request) = command.operation else {
            panic!("expected model/list");
        };
        let ClientRequest::ModelList { params, .. } = *request else {
            panic!("expected model/list");
        };
        assert_eq!(params.model_provider, None);
        let catalog = json!({"data":[{
            "id":"cloud-model","model":"cloud-model","displayName":"Cloud model","description":"",
            "hidden":false,"supportedReasoningEfforts":[],"defaultReasoningEffort":"medium","isDefault":true
        }],"nextCursor":null});
        command.response.send(Ok(Ok(catalog))).unwrap();

        let command = commands.recv().await.unwrap();
        assert!(
            matches!(command.operation, CloudOperation::Request(request) if matches!(*request, ClientRequest::ConfigRequirementsRead { .. }))
        );
        command
            .response
            .send(Ok(Err(JSONRPCErrorError {
                code: -32601,
                message: "method not found".into(),
                data: None,
            })))
            .unwrap();
    });
    let refreshed = execute(handle, AccountCommand::Refresh, "local-provider")
        .await
        .unwrap();
    let WorkspaceResponse::AccountChanged { models: Ok(models) } = refreshed else {
        panic!("expected cloud models after refresh");
    };
    assert_eq!(
        models
            .iter()
            .map(|model| model.model.as_str())
            .collect::<Vec<_>>(),
        ["cloud-model"]
    );
    worker.await.unwrap();
}
