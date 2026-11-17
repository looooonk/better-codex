use super::super::test_support::scripted_server;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn provider_parser_preserves_quoted_values_and_explicit_clears() {
    assert_eq!(
        ProviderCommand::parse("configure gateway name='Company Gateway' env_key=").unwrap(),
        ProviderCommand::Configure {
            id: "gateway".into(),
            fields: vec![
                ("name".into(), Some("Company Gateway".into())),
                ("env_key".into(), None)
            ],
        }
    );
    assert!(ProviderCommand::parse("configure gateway token").is_err());
}

#[tokio::test]
async fn managed_provider_default_cannot_be_changed() {
    let (client, server) = scripted_server(vec![(
        "modelProvider/list",
        json!({"data":[],"defaultProviderId":"openai","canChangeDefault":false}),
    )])
    .await;
    let error = execute(
        AppServerRequestHandle::Remote(client.request_handle()),
        ProviderCommand::Use {
            id: "openai".into(),
            model: None,
        },
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("controlled"));
    client.shutdown().await.unwrap();
    assert_eq!(server.await.unwrap(), vec![json!({})]);
}

#[tokio::test]
async fn gateway_edits_patch_only_requested_fields() {
    let provider = json!({"id":"gateway","name":"Company","canEdit":true,"baseUrl":"https://example.test/v1","settings":{"type":"customGateway","auth":{"mode":"externallyConfigured","envKey":null}}});
    let (client, server) = scripted_server(vec![
        (
            "modelProvider/list",
            json!({"data":[provider.clone()],"defaultProviderId":"openai","canChangeDefault":true}),
        ),
        ("modelProvider/config/write", json!({"provider":provider})),
    ])
    .await;
    execute(
        AppServerRequestHandle::Remote(client.request_handle()),
        ProviderCommand::parse("configure gateway name=Company").unwrap(),
    )
    .await
    .unwrap();
    client.shutdown().await.unwrap();
    assert_eq!(
        server.await.unwrap()[1],
        json!({"settings":{"type":"customGateway","providerId":"gateway","name":"Company"}})
    );
}

#[tokio::test]
async fn provider_selection_writes_the_default_and_any_model_pin_together() {
    let provider = json!({"id":"gateway","name":"Gateway","canEdit":true,"settings":{"type":"customGateway","auth":{"mode":"externallyConfigured","envKey":null}}});
    let catalog = json!({"data":[provider],"defaultProviderId":"openai","canChangeDefault":true});
    let saved = json!({"status":"ok","version":"1","filePath":"/tmp/config.toml"});
    for (pin, requested_model, method) in [
        (None, None, "modelProvider/default/set"),
        (Some("old"), None, "config/batchWrite"),
        (None, Some("selected"), "config/batchWrite"),
    ] {
        let mut script = vec![("modelProvider/list", catalog.clone())];
        if requested_model.is_none() {
            script.push(("config/read", json!({"config":{"model":pin},"origins":{}})));
        }
        script.push((method, saved.clone()));
        let (client, server) = scripted_server(script).await;
        execute(
            AppServerRequestHandle::Remote(client.request_handle()),
            ProviderCommand::Use {
                id: "gateway".into(),
                model: requested_model.map(str::to_owned),
            },
        )
        .await
        .unwrap();
        client.shutdown().await.unwrap();
        let requests = server.await.unwrap();
        let write = requests.last().unwrap();
        if method == "modelProvider/default/set" {
            assert_eq!(write, &json!({"providerId":"gateway"}));
        } else {
            assert_eq!(
                write["edits"],
                json!([
                    {"keyPath":"model_provider","value":"gateway","mergeStrategy":"replace"},
                    {"keyPath":"model","value":requested_model,"mergeStrategy":"replace"},
                ])
            );
        }
    }
}
