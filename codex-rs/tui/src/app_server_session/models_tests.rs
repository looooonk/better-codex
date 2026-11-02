use super::*;
use pretty_assertions::assert_eq;
use std::collections::VecDeque;

fn model(name: &str) -> ApiModel {
    serde_json::from_value(serde_json::json!({
        "id": name,
        "model": name,
        "displayName": name,
        "description": "Catalog model",
        "hidden": false,
        "supportedReasoningEfforts": [{"reasoningEffort": "medium", "description": "Medium"}],
        "defaultReasoningEffort": "medium",
        "isDefault": false,
        "multiAgentVersion": null
    }))
    .expect("valid model fixture")
}

#[tokio::test]
async fn catalog_reads_all_pages_and_preserves_model_metadata() {
    let mut first = model("first");
    first.hidden = true;
    let mut last = model("new-default");
    last.is_default = true;
    last.default_service_tier = Some("priority".to_string());
    let expected = vec![
        model_preset_from_api_model(first.clone()),
        model_preset_from_api_model(last.clone()),
    ];
    let mut pages = VecDeque::from([
        ModelListResponse {
            data: vec![first],
            next_cursor: Some("next".to_string()),
        },
        ModelListResponse {
            data: vec![last],
            next_cursor: None,
        },
    ]);
    let mut requests = Vec::new();
    let models = read_catalog(|params| {
        requests.push(params);
        std::future::ready(Ok(pages.pop_front().expect("unexpected catalog request")))
    })
    .await
    .expect("complete catalog");
    assert_eq!(models, expected);
    assert_eq!(
        requests,
        vec![
            ModelListParams {
                cursor: None,
                limit: Some(100),
                include_hidden: Some(true),
            },
            ModelListParams {
                cursor: Some("next".to_string()),
                limit: Some(100),
                include_hidden: Some(true),
            },
        ]
    );
}

#[tokio::test]
async fn repeated_catalog_cursor_fails_without_hanging() {
    let error = read_catalog(|_| {
        std::future::ready(Ok(ModelListResponse {
            data: vec![model("same")],
            next_cursor: Some("same".to_string()),
        }))
    })
    .await
    .expect_err("repeated cursor must fail");
    assert!(error.to_string().contains("repeated model catalog cursor"));
}

#[tokio::test]
async fn oversized_catalog_page_fails_before_retaining_models() {
    let error = read_catalog(|_| {
        std::future::ready(Ok(ModelListResponse {
            data: vec![model("oversized"); 101],
            next_cursor: None,
        }))
    })
    .await
    .expect_err("oversized page must fail");
    assert!(error.to_string().contains("exceeded its requested size"));
}
