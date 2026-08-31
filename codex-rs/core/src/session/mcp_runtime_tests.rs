use super::*;

#[tokio::test]
async fn reuses_binding_while_the_catalog_revision_is_stable() {
    let config = crate::config::test_config().await;
    let runtime = McpRuntimeSnapshot::new_uninitialized_for_test(&config);

    let first = runtime.binding().await;
    let repeated = runtime.binding().await;

    assert!(std::sync::Arc::ptr_eq(&first, &repeated));
}
