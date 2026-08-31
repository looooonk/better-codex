use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use axum::Json;
use axum::Router;
use axum::routing::get;
use axum::routing::post;
use codex_config::types::AuthKeyringBackendKind;
use codex_config::types::OAuthCredentialsStoreMode;
use codex_exec_server::ReqwestHttpClient;
use pretty_assertions::assert_eq;
use reqwest::Url;
use serde_json::json;
use tokio::net::TcpListener;

use super::super::load_oauth_tokens_from_file;
use super::super::test_support::TempCodexHome;
use crate::perform_oauth_login_return_url_with_http_client;

async fn spawn_oauth_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind OAuth listener");
    let base_url = format!(
        "http://{}",
        listener.local_addr().expect("OAuth listener address")
    );
    let metadata = json!({
        "issuer": format!("{base_url}/mcp"),
        "authorization_endpoint": format!("{base_url}/oauth/authorize"),
        "token_endpoint": format!("{base_url}/oauth/token"),
        "scopes_supported": [""],
    });
    let path_metadata = metadata.clone();
    let token_requests = Arc::new(AtomicUsize::new(0));
    let token_request_count = Arc::clone(&token_requests);
    let app = Router::new()
        .route(
            "/.well-known/oauth-authorization-server/mcp",
            get(move || {
                let metadata = path_metadata.clone();
                async move { Json(metadata) }
            }),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(move || {
                let metadata = metadata.clone();
                async move { Json(metadata) }
            }),
        )
        .route(
            "/oauth/token",
            post(move || {
                let token_request_count = Arc::clone(&token_request_count);
                async move {
                    token_request_count.fetch_add(1, Ordering::SeqCst);
                    Json(json!({
                        "access_token": "callback-access-token",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                    }))
                }
            }),
        );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve OAuth test");
    });
    (base_url, token_requests)
}

#[tokio::test(flavor = "current_thread")]
async fn finish_rejects_mismatched_callback_issuer_before_saving_tokens() {
    let _codex_home = TempCodexHome::new();
    let (base_url, token_requests) = spawn_oauth_server().await;
    let server_name = "callback-issuer-mismatch";
    let server_url = format!("{base_url}/mcp");
    let handle = perform_oauth_login_return_url_with_http_client(
        server_name,
        &server_url,
        OAuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
        /*http_headers*/ None,
        /*env_http_headers*/ None,
        &[],
        Some("configured-client"),
        /*oauth_resource*/ None,
        Some(/*timeout_secs*/ 5),
        /*callback_port*/ None,
        /*callback_url*/ None,
        Arc::new(ReqwestHttpClient),
    )
    .await
    .expect("start OAuth callback flow");
    let authorization_url = Url::parse(handle.authorization_url()).expect("authorization URL");
    let query = authorization_url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<std::collections::HashMap<_, _>>();
    let mut callback_url = Url::parse(
        query
            .get("redirect_uri")
            .expect("authorization URL redirect URI"),
    )
    .expect("callback URL");
    callback_url
        .query_pairs_mut()
        .append_pair("code", "authorization-code")
        .append_pair(
            "state",
            query.get("state").expect("authorization URL state"),
        )
        .append_pair("iss", &format!("{base_url}/different-issuer"));

    let response = reqwest::get(callback_url)
        .await
        .expect("send OAuth callback");
    assert!(response.status().is_success());
    let error = handle
        .wait()
        .await
        .expect_err("a mismatched callback issuer must fail the login");

    assert!(format!("{error:#}").contains("issuer"));
    assert_eq!(token_requests.load(Ordering::SeqCst), 0);
    assert_eq!(
        load_oauth_tokens_from_file(server_name, &server_url).expect("read OAuth token store"),
        None
    );
}
