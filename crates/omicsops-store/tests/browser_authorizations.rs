use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_protocol::{
    BrowserApprovalBindingV4, BrowserApprovalScopeV4, BrowserAuthorizationV4, BrowserSessionKindV4,
};
use omicsops_store::Store;
use uuid::Uuid;

#[tokio::test]
async fn browser_settings_round_trip_as_validated_global_configuration() {
    let store = Store::open_in_memory().await.unwrap();
    assert_eq!(store.browser_settings().await.unwrap(), None);
    let settings = serde_json::json!({
        "auto_launch": false,
        "auto_close_turn_tabs": true,
        "browser_path": null,
        "default_search_provider": "bing",
        "disabled_domains": ["blocked.example"],
        "preferred_domains": [],
    });
    store.save_browser_settings(&settings).await.unwrap();
    assert_eq!(store.browser_settings().await.unwrap(), Some(settings));
    assert!(
        store
            .save_browser_settings(&serde_json::json!([]))
            .await
            .is_err()
    );
}

async fn fixture() -> (Store, Project, Conversation, Project, Conversation) {
    let store = Store::open_in_memory().await.unwrap();
    let first = Project::new(
        Uuid::new_v4(),
        "first",
        r"C:\data\first",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let second = Project::new(
        Uuid::new_v4(),
        "second",
        r"C:\data\second",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&first).await.unwrap();
    store.save_project(&second).await.unwrap();
    let first_conversation = Conversation::new(Uuid::new_v4(), first.id, "first", Utc::now());
    let second_conversation = Conversation::new(Uuid::new_v4(), second.id, "second", Utc::now());
    store.save_conversation(&first_conversation).await.unwrap();
    store.save_conversation(&second_conversation).await.unwrap();
    (
        store,
        first,
        first_conversation,
        second,
        second_conversation,
    )
}

fn binding() -> BrowserApprovalBindingV4 {
    BrowserApprovalBindingV4 {
        capability: "web_open_tab".into(),
        target_host: "example.org".into(),
        session: BrowserSessionKindV4::Shared,
        protocol_version: 1,
    }
}

fn authorization(
    id: &str,
    scope: BrowserApprovalScopeV4,
    project_id: Option<Uuid>,
    conversation_id: Option<Uuid>,
) -> BrowserAuthorizationV4 {
    BrowserAuthorizationV4 {
        id: id.into(),
        scope,
        binding: binding(),
        project_id,
        conversation_id,
        created_at_ms: Utc::now().timestamp_millis(),
    }
}

#[tokio::test]
async fn authorization_scopes_are_idempotent_isolated_and_revocable() {
    let (store, first, conversation, second, second_conversation) = fixture().await;
    let project = authorization(
        "project-grant",
        BrowserApprovalScopeV4::Project,
        Some(first.id),
        None,
    );
    store.save_browser_authorization_v4(&project).await.unwrap();
    store.save_browser_authorization_v4(&project).await.unwrap();
    let conflicting = authorization("project-grant", BrowserApprovalScopeV4::Global, None, None);
    store
        .save_browser_authorization_v4(&conflicting)
        .await
        .unwrap();
    assert_eq!(
        store.list_browser_authorizations_v4().await.unwrap().len(),
        1
    );
    assert_eq!(
        store.list_browser_authorizations_v4().await.unwrap()[0].scope,
        BrowserApprovalScopeV4::Project
    );
    assert!(
        store
            .consume_browser_authorization_v4(first.id, conversation.id, &binding())
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_browser_authorization_v4(second.id, second_conversation.id, &binding())
            .await
            .unwrap()
    );
    assert!(
        store
            .revoke_browser_authorization_v4(&project.id)
            .await
            .unwrap()
    );
    assert!(
        !store
            .revoke_browser_authorization_v4(&project.id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn once_is_consumed_and_binding_changes_invalidate_authority() {
    let (store, project, conversation, _, _) = fixture().await;
    let once = authorization(
        "once-grant",
        BrowserApprovalScopeV4::Once,
        Some(project.id),
        Some(conversation.id),
    );
    store.save_browser_authorization_v4(&once).await.unwrap();
    assert!(
        store
            .consume_browser_authorization_v4(project.id, conversation.id, &binding())
            .await
            .unwrap()
    );
    assert!(
        !store
            .consume_browser_authorization_v4(project.id, conversation.id, &binding())
            .await
            .unwrap()
    );

    let conversation_grant = authorization(
        "conversation-grant",
        BrowserApprovalScopeV4::Conversation,
        Some(project.id),
        Some(conversation.id),
    );
    store
        .save_browser_authorization_v4(&conversation_grant)
        .await
        .unwrap();
    let mut changed = binding();
    changed.protocol_version = 2;
    assert!(
        !store
            .consume_browser_authorization_v4(project.id, conversation.id, &changed)
            .await
            .unwrap()
    );
    changed = binding();
    changed.session = BrowserSessionKindV4::Workspace;
    assert!(
        !store
            .consume_browser_authorization_v4(project.id, conversation.id, &changed)
            .await
            .unwrap()
    );
    assert!(
        store
            .revoke_browser_authorization_v4(&conversation_grant.id)
            .await
            .unwrap()
    );

    let concurrent = authorization(
        "concurrent-once",
        BrowserApprovalScopeV4::Once,
        Some(project.id),
        Some(conversation.id),
    );
    store
        .save_browser_authorization_v4(&concurrent)
        .await
        .unwrap();
    let first = store.clone();
    let second = store.clone();
    let concurrent_binding = binding();
    let (left, right) = tokio::join!(
        first.consume_browser_authorization_v4(project.id, conversation.id, &concurrent_binding,),
        second.consume_browser_authorization_v4(project.id, conversation.id, &concurrent_binding,),
    );
    assert_ne!(left.unwrap(), right.unwrap());
}
