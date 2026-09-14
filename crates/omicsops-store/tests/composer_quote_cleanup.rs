use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_store::Store;
use serde_json::{Value, json};
use uuid::Uuid;

const QUOTE_KIND: &str = "composer_quote_v1";

async fn project(store: &Store, name: &str) -> Project {
    let project = Project::new(
        Uuid::new_v4(),
        name,
        format!("C:/data/{name}"),
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    project
}

async fn conversation(store: &Store, project: &Project, title: &str) -> Conversation {
    let conversation = Conversation::new(Uuid::new_v4(), project.id, title, Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    conversation
}

fn quote(project_id: Uuid, conversation_id: Uuid) -> Value {
    json!({
        "id": Uuid::new_v4(),
        "project_id": project_id,
        "conversation_id": conversation_id,
        "backend_id": "local",
        "relative_path": "results/notes.md",
        "sha256": "a".repeat(64),
        "text": "selected",
    })
}

fn quote_with_id(project_id: Uuid, conversation_id: Uuid, id: Uuid, path: &str) -> Value {
    json!({
        "id": id,
        "project_id": project_id,
        "conversation_id": conversation_id,
        "backend_id": "local",
        "relative_path": path,
        "sha256": "a".repeat(64),
        "text": "selected",
    })
}

#[tokio::test]
async fn deleting_conversation_removes_only_its_owned_quote_snapshots() {
    let store = Store::open_in_memory().await.unwrap();
    let project = project(&store, "quote-cleanup").await;
    let deleted = conversation(&store, &project, "deleted").await;
    let retained = conversation(&store, &project, "retained").await;
    let deleted_quote_id = Uuid::new_v4();
    let retained_quote_id = Uuid::new_v4();
    store
        .put_json(
            QUOTE_KIND,
            &deleted_quote_id.to_string(),
            &quote(project.id, deleted.id),
        )
        .await
        .unwrap();
    store
        .put_json(
            QUOTE_KIND,
            &retained_quote_id.to_string(),
            &quote(project.id, retained.id),
        )
        .await
        .unwrap();

    assert!(
        store
            .delete_conversation(project.id, deleted.id)
            .await
            .unwrap()
    );
    assert!(
        store
            .get_json::<Value>(QUOTE_KIND, &deleted_quote_id.to_string())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_json::<Value>(QUOTE_KIND, &retained_quote_id.to_string())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn deleting_project_removes_its_quotes_but_keeps_other_project_quotes() {
    let store = Store::open_in_memory().await.unwrap();
    let deleted_project = project(&store, "deleted-project").await;
    let retained_project = project(&store, "retained-project").await;
    let deleted_conversation = conversation(&store, &deleted_project, "deleted").await;
    let retained_conversation = conversation(&store, &retained_project, "retained").await;
    let deleted_quote_id = Uuid::new_v4();
    let retained_quote_id = Uuid::new_v4();
    store
        .put_json(
            QUOTE_KIND,
            &deleted_quote_id.to_string(),
            &quote(deleted_project.id, deleted_conversation.id),
        )
        .await
        .unwrap();
    store
        .put_json(
            QUOTE_KIND,
            &retained_quote_id.to_string(),
            &quote(retained_project.id, retained_conversation.id),
        )
        .await
        .unwrap();

    assert!(store.delete_project(deleted_project.id).await.unwrap());
    assert!(
        store
            .get_json::<Value>(QUOTE_KIND, &deleted_quote_id.to_string())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_json::<Value>(QUOTE_KIND, &retained_quote_id.to_string())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn scoped_json_insert_deduplicates_and_enforces_project_cap_atomically() {
    let store = Store::open_in_memory().await.unwrap();
    let project = project(&store, "atomic-cap").await;
    let conversation = conversation(&store, &project, "quotes").await;
    let first_id = Uuid::new_v4();
    let first = quote_with_id(project.id, conversation.id, first_id, "results/first.md");
    let first_write = store
        .put_scoped_json_deduplicated(
            QUOTE_KIND,
            &first_id.to_string(),
            project.id,
            conversation.id,
            1,
            &[
                ("backend_id", "local"),
                ("relative_path", "results/first.md"),
                ("sha256", "a".repeat(64).as_str()),
                ("text", "selected"),
            ],
            &first,
        )
        .await
        .unwrap();
    assert!(first_write.inserted);
    assert_eq!(first_write.id, first_id.to_string());

    let duplicate_id = Uuid::new_v4();
    let duplicate = quote_with_id(
        project.id,
        conversation.id,
        duplicate_id,
        "results/first.md",
    );
    let duplicate_write = store
        .put_scoped_json_deduplicated(
            QUOTE_KIND,
            &duplicate_id.to_string(),
            project.id,
            conversation.id,
            1,
            &[
                ("backend_id", "local"),
                ("relative_path", "results/first.md"),
                ("sha256", "a".repeat(64).as_str()),
                ("text", "selected"),
            ],
            &duplicate,
        )
        .await
        .unwrap();
    assert!(!duplicate_write.inserted);
    assert_eq!(duplicate_write.id, first_id.to_string());

    let second_id = Uuid::new_v4();
    let second = quote_with_id(project.id, conversation.id, second_id, "results/second.md");
    let error = store
        .put_scoped_json_deduplicated(
            QUOTE_KIND,
            &second_id.to_string(),
            project.id,
            conversation.id,
            1,
            &[
                ("backend_id", "local"),
                ("relative_path", "results/second.md"),
                ("sha256", "a".repeat(64).as_str()),
                ("text", "selected"),
            ],
            &second,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("maximum"), "{error}");
    assert_eq!(store.list_json::<Value>(QUOTE_KIND).await.unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_scoped_inserts_cannot_bypass_project_cap() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("atomic.sqlite"))
        .await
        .unwrap();
    let project = project(&store, "concurrent-cap").await;
    let conversation = conversation(&store, &project, "quotes").await;
    let left_id = Uuid::new_v4();
    let right_id = Uuid::new_v4();
    let left = quote_with_id(project.id, conversation.id, left_id, "results/left.md");
    let right = quote_with_id(project.id, conversation.id, right_id, "results/right.md");
    let left_id_string = left_id.to_string();
    let right_id_string = right_id.to_string();
    let left_hash = "a".repeat(64);
    let right_hash = "a".repeat(64);
    let left_dedup = [
        ("backend_id", "local"),
        ("relative_path", "results/left.md"),
        ("sha256", left_hash.as_str()),
        ("text", "selected"),
    ];
    let right_dedup = [
        ("backend_id", "local"),
        ("relative_path", "results/right.md"),
        ("sha256", right_hash.as_str()),
        ("text", "selected"),
    ];
    let left_store = store.clone();
    let right_store = store.clone();
    let (left_result, right_result) = tokio::join!(
        left_store.put_scoped_json_deduplicated(
            QUOTE_KIND,
            &left_id_string,
            project.id,
            conversation.id,
            1,
            &left_dedup,
            &left,
        ),
        right_store.put_scoped_json_deduplicated(
            QUOTE_KIND,
            &right_id_string,
            project.id,
            conversation.id,
            1,
            &right_dedup,
            &right,
        ),
    );
    let inserted = [left_result.as_ref(), right_result.as_ref()]
        .into_iter()
        .filter(|result| result.as_ref().is_ok_and(|write| write.inserted))
        .count();
    assert_eq!(inserted, 1, "left={left_result:?} right={right_result:?}");
    assert_eq!(store.list_json::<Value>(QUOTE_KIND).await.unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_quote_insert_and_conversation_delete_leave_no_orphan() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().join("atomic-delete.sqlite"))
        .await
        .unwrap();
    let project = project(&store, "concurrent-delete").await;
    let conversation = conversation(&store, &project, "quotes").await;
    let quote_id = Uuid::new_v4();
    let value = quote_with_id(project.id, conversation.id, quote_id, "results/race.md");
    let quote_id_string = quote_id.to_string();
    let quote_hash = "a".repeat(64);
    let quote_dedup = [
        ("backend_id", "local"),
        ("relative_path", "results/race.md"),
        ("sha256", quote_hash.as_str()),
        ("text", "selected"),
    ];
    let insert_store = store.clone();
    let delete_store = store.clone();
    let (insert_result, delete_result) = tokio::join!(
        insert_store.put_scoped_json_deduplicated(
            QUOTE_KIND,
            &quote_id_string,
            project.id,
            conversation.id,
            100,
            &quote_dedup,
            &value,
        ),
        delete_store.delete_conversation(project.id, conversation.id),
    );
    if delete_result.is_ok_and(|deleted| deleted) {
        assert!(
            store
                .get_json::<Value>(QUOTE_KIND, &quote_id_string)
                .await
                .unwrap()
                .is_none(),
            "insert result: {insert_result:?}"
        );
    }
}
