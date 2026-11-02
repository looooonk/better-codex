use super::super::test_support::scripted_server;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn app_permissions_keep_literal_tool_names_and_validate_modes() {
    assert_eq!(
        AppsCommand::parse("tool app.foo write.file approval prompt").unwrap(),
        AppsCommand::Configure {
            app_id: "app.foo".into(),
            key: "tools.\"write.file\".approval_mode".into(),
            value: json!("prompt"),
        }
    );
    assert!(AppsCommand::parse("approval app.foo unrestricted").is_err());
}

#[tokio::test]
async fn access_uses_native_app_metadata_and_preserves_connection_link() {
    let (client, server) = scripted_server(vec![("app/read", json!({"apps":[{
        "id":"app.docs","name":"Docs","description":"Documents","iconUrl":null,"iconUrlDark":null,
        "distributionChannel":null,"installUrl":"https://chatgpt.com/apps/docs","pluginDisplayNames":[],"toolSummaries":[]
    }],"missingAppIds":[]}))]).await;
    let response = execute(
        AppServerRequestHandle::Remote(client.request_handle()),
        "thread-1".into(),
        AppsCommand::Access("app.docs".into()),
    )
    .await
    .unwrap();
    let WorkspaceResponse::Notice(message) = response else {
        panic!("expected app access")
    };
    insta::assert_snapshot!("app_access", message);
    client.shutdown().await.unwrap();
    assert_eq!(
        server.await.unwrap(),
        vec![json!({"appIds":["app.docs"],"threadId":"thread-1","includeTools":true})]
    );
}

#[tokio::test]
async fn app_catalog_follows_pages_and_rejects_cycles() {
    let (client, server) = scripted_server(vec![
        ("app/list", json!({"data":[],"nextCursor":"page2"})),
        ("app/list", json!({"data":[],"nextCursor":"page2"})),
    ])
    .await;
    let error = execute(
        AppServerRequestHandle::Remote(client.request_handle()),
        "thread-1".into(),
        AppsCommand::List(String::new()),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("repeated cursor"));
    client.shutdown().await.unwrap();
    let requests = server.await.unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|request| request["cursor"].clone())
            .collect::<Vec<_>>(),
        vec![Value::Null, json!("page2")]
    );
}
