use super::*;
use super::super::test_support::scripted_server;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn provider_parser_preserves_quoted_values_and_explicit_clears() {
    assert_eq!(ProviderCommand::parse("configure gateway name='Company Gateway' env_key=").unwrap(), ProviderCommand::Configure {
        id:"gateway".into(), fields:vec![("name".into(),Some("Company Gateway".into())),("env_key".into(),None)],
    });
    assert!(ProviderCommand::parse("configure gateway token").is_err());
}

#[tokio::test]
async fn managed_provider_default_cannot_be_changed() {
    let (client, server) = scripted_server(vec![("modelProvider/list",json!({"data":[],"defaultProviderId":"openai","canChangeDefault":false}))]).await;
    let error = execute(AppServerRequestHandle::Remote(client.request_handle()), ProviderCommand::Use { id:"openai".into(),model:None }).await.unwrap_err();
    assert!(error.to_string().contains("controlled"));
    client.shutdown().await.unwrap();
    assert_eq!(server.await.unwrap(),vec![json!({})]);
}

#[tokio::test]
async fn gateway_edits_patch_only_requested_fields() {
    let provider = json!({"id":"gateway","name":"Company","canEdit":true,"baseUrl":"https://example.test/v1","settings":{"type":"customGateway","auth":{"mode":"externallyConfigured","envKey":null}}});
    let (client, server) = scripted_server(vec![
        ("modelProvider/list",json!({"data":[provider.clone()],"defaultProviderId":"openai","canChangeDefault":true})),
        ("modelProvider/config/write",json!({"provider":provider})),
    ]).await;
    execute(AppServerRequestHandle::Remote(client.request_handle()), ProviderCommand::parse("configure gateway name=Company").unwrap()).await.unwrap();
    client.shutdown().await.unwrap();
    assert_eq!(server.await.unwrap()[1],json!({"settings":{"type":"customGateway","providerId":"gateway","name":"Company"}}));
}
