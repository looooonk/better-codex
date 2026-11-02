use super::*;
use super::super::test_support::scripted_server;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn switching_accounts_validates_workspace_before_mutation() {
    let (client, server) = scripted_server(vec![("accountSession/list", json!({"activeSessionId":null,"sessions":[]}))]).await;
    let error = execute(AppServerRequestHandle::Remote(client.request_handle()), AccountCommand::Switch { session_id:"missing".into(),account_id:"workspace".into() }).await.unwrap_err();
    assert!(error.to_string().contains("unknown saved account"));
    client.shutdown().await.unwrap();
    assert_eq!(server.await.unwrap(), vec![json!({"refreshWorkspaceMetadata":true})]);
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
    let response = execute(AppServerRequestHandle::Remote(client.request_handle()), AccountCommand::parse("switch session-1 workspace-1").unwrap()).await.unwrap();
    let WorkspaceResponse::AccountLogin { login_id, auth_url } = response else { panic!("expected browser login") };
    assert_eq!((login_id,auth_url),("login-1".into(),"https://auth.example.test/work".into()));
    client.shutdown().await.unwrap();
    assert_eq!(server.await.unwrap()[1],json!({"sessionId":"session-1","accountId":"workspace-1","allowBrowserLogin":true}));
}
