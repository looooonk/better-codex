use super::*;
use codex_app_server_protocol::ConfigWriteResponse;
use codex_app_server_protocol::WriteStatus;

#[test_case("config/value/write", "\"model_provider\""; "value writes")]
#[test_case("config/batchWrite", "model_provider"; "batch writes")]
#[tokio::test]
async fn config_writes_refresh_provider_catalogs(write_method: &str, provider_key: &str) -> Result<()> {
    let endpoint = MockServer::start().await;
    let home = TempDir::new()?;
    let base = codex_models_manager::bundled_models_response()?.models.remove(0);
    let mut models = Vec::new();
    for suffix in ["a", "b", "c", "openai", "file"] {
        let mut model = base.clone();
        model.slug = format!("catalog-{suffix}");
        model.visibility = codex_protocol::openai_models::ModelVisibility::List;
        model.supported_in_api = true;
        Mock::given(method("GET"))
            .and(path(format!("/{suffix}/models")))
            .respond_with(ResponseTemplate::new(/*s*/ 200).set_body_json(ModelsResponse {
                models: vec![model.clone()],
            }))
            .mount(&endpoint)
            .await;
        models.push(model);
    }
    let uri = endpoint.uri();
    let mut config = String::from("model_provider = 'provider-a'\n[features]\napi_key_model_discovery = true\n");
    for (id, name, suffix) in [
        ("provider-a", "Provider A", "a"),
        ("provider-b", "Provider B", "b"),
        ("openai", "OpenAI", "openai"),
    ] {
        config.push_str(&format!(
            "[model_providers.{id}]\nname = '{name}'\nbase_url = '{uri}/{suffix}'\nmodel_catalog_url = '{uri}/{suffix}/models'\nenv_key = 'CATALOG_TEST_KEY'\n"
        ));
    }
    std::fs::write(home.path().join("config.toml"), config)?;
    let catalog_path = home.path().join("models.json");
    std::fs::write(&catalog_path, serde_json::to_string(&ModelsResponse {
        models: vec![models[4].clone()],
    })?)?;
    let mut server = TestAppServer::builder()
        .with_codex_home(home.path())
        .with_env_overrides(&[
            ("OPENAI_API_KEY", None),
            ("CODEX_API_KEY", None),
            ("CATALOG_TEST_KEY", Some("test-key")),
        ])
        .build_initialized()
        .await?;
    let changes = [
        None,
        Some((provider_key, json!("provider-b"))),
        Some(("\"model_providers\".\"provider-b\".\"model_catalog_url\"", json!(format!("{uri}/c/models")))),
        Some((provider_key, json!("openai"))),
        Some(("model_catalog_json", json!(catalog_path))),
    ];
    for (change, model) in changes.into_iter().zip(models) {
        if let Some((key, value)) = change {
            let edit = json!({"keyPath":key, "value":value, "mergeStrategy":"replace"});
            let params = if write_method == "config/batchWrite" {
                json!({"edits":[edit], "reloadUserConfig":true})
            } else {
                edit
            };
            let request = server.send_request(write_method, Some(params)).await?;
            let response: ConfigWriteResponse = server.read_response(request).await?;
            assert_eq!(response.status, WriteStatus::Ok);
        }
        let response: ModelListResponse = server.request(|request_id| ClientRequest::ModelList {
            request_id,
            params: ModelListParams { include_hidden: Some(true), ..Default::default() },
        }).await?;
        let mut expected = vec![ModelPreset::from(model)];
        ModelPreset::mark_default_by_picker_visibility(&mut expected);
        assert_eq!(response, ModelListResponse {
            data: expected.iter().map(model_from_preset).collect(),
            next_cursor: None,
        });
    }
    Ok(())
}
