use omicsops_store::Store;
use serde_json::json;

#[tokio::test]
async fn iteration_settings_persist_and_reject_invalid_limits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.sqlite");
    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.agent_iteration_settings().await.unwrap(), None);
    for limit in [100, 0, 250] {
        store
            .save_agent_iteration_settings(&json!({"max_iterations": limit}))
            .await
            .unwrap();
        assert_eq!(
            store.agent_iteration_settings().await.unwrap(),
            Some(json!({"max_iterations": limit}))
        );
    }
    for value in [
        json!([]),
        json!({}),
        json!({"max_iterations": -1}),
        json!({"max_iterations": 1.5}),
        json!({"max_iterations": 4294967296_u64}),
    ] {
        assert!(store.save_agent_iteration_settings(&value).await.is_err());
    }
    drop(store);
    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(
        reopened.agent_iteration_settings().await.unwrap(),
        Some(json!({"max_iterations": 250}))
    );
}

#[tokio::test]
async fn session_settings_validate_new_fields_and_preserve_values() {
    let store = Store::open_in_memory().await.unwrap();
    for value in [
        json!({"max_iterations":100,"auto_continue":"true"}),
        json!({"max_iterations":100,"auto_continue_limit":-1}),
        json!({"max_iterations":100,"auto_continue_limit":1.5}),
        json!({"max_iterations":100,"auto_continue_limit":4294967296_u64}),
        json!({"max_iterations":100,"auto_compact":null}),
        json!({"max_iterations":100,"follow_up_questions":1}),
    ] {
        assert!(
            store.save_agent_iteration_settings(&value).await.is_err(),
            "{value}"
        );
    }
    let value = json!({"max_iterations":0,"auto_continue":true,"auto_continue_limit":0,"auto_compact":false,"follow_up_questions":false});
    store.save_agent_iteration_settings(&value).await.unwrap();
    assert_eq!(store.agent_iteration_settings().await.unwrap(), Some(value));
}
