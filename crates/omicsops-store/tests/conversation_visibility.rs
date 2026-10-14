use chrono::{TimeZone, Utc};
use omicsops_core::workspace::{
    Conversation, ConversationStatus, Message, MessageRole, Project, ProjectTemplate,
};
use omicsops_dto::*;
use omicsops_store::{Store, StoreError};
use sqlx::{Row, TypeInfo, ValueRef};
use uuid::Uuid;

fn time(n: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_millis_opt(1_790_000_000_000 + n).unwrap()
}
fn project(n: u128) -> Project {
    Project::new(
        Uuid::from_u128(n),
        "fixture",
        r"C:\data\visibility",
        ProjectTemplate::Blank,
        time(0),
    )
}
fn conversation(n: u128, p: Uuid) -> Conversation {
    Conversation::new(Uuid::from_u128(n), p, "same title", time(n as i64))
}
fn set(
    c: &Conversation,
    revision: u64,
    visibility: ConversationVisibilityV1,
) -> SetConversationVisibilityRequestV1 {
    SetConversationVisibilityRequestV1 {
        request_id: Uuid::new_v4(),
        project_id: c.project_id,
        conversation_id: c.id,
        expected_revision: revision,
        visibility,
    }
}
fn list(
    p: Uuid,
    visibility: ConversationVisibilityFilterV1,
) -> ConversationVisibilityListRequestV1 {
    ConversationVisibilityListRequestV1 {
        project_id: p,
        visibility,
        query: String::new(),
        offset: 0,
        limit: 50,
    }
}
async fn fixture() -> (Store, Project, Conversation) {
    let s = Store::open_in_memory().await.unwrap();
    let p = project(1);
    let c = conversation(10, p.id);
    s.save_project(&p).await.unwrap();
    s.save_conversation(&c).await.unwrap();
    (s, p, c)
}
fn error(e: StoreError, tag: &str) {
    assert!(
        matches!(e, StoreError::InvalidInput(ref v) if v == tag),
        "{e:?}"
    );
}
// Capture actual SQLite values of every legacy column, including timestamps and JSON bytes.
async fn legacy_bytes(s: &Store) -> Vec<Vec<Vec<String>>> {
    let mut all = Vec::new();
    for table in [
        "projects",
        "frames",
        "conversation_records",
        "messages",
        "conversation_groups",
        "conversation_group_members",
        "agent_runs_v4",
        "conversation_branches_v4",
    ] {
        let rows = sqlx::query(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .fetch_all(s.pool())
            .await
            .unwrap();
        all.push(
            rows.iter()
                .map(|row| {
                    row.columns()
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            let value = row.try_get_raw(i).unwrap();
                            if value.is_null() {
                                "NULL".to_owned()
                            } else if value.type_info().name() == "INTEGER" {
                                row.get::<i64, _>(i).to_string()
                            } else {
                                row.get::<String, _>(i)
                            }
                        })
                        .collect()
                })
                .collect(),
        );
    }
    all
}
async fn related(s: &Store, p: &Project, c: &Conversation) {
    let m = Message::markdown(
        Uuid::from_u128(1000),
        p.id,
        c.id,
        0,
        MessageRole::User,
        "original evidence",
        time(90),
    );
    s.save_message(&m).await.unwrap();
    sqlx::query("INSERT INTO conversation_groups VALUES(?,?,?,1,2)")
        .bind(Uuid::from_u128(1001).to_string())
        .bind(p.id.to_string())
        .bind("group")
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO conversation_group_members VALUES(?,?,?)")
        .bind(c.id.to_string())
        .bind(p.id.to_string())
        .bind(Uuid::from_u128(1001).to_string())
        .execute(s.pool())
        .await
        .unwrap();
    s.save_agent_run_v4(Uuid::from_u128(1002),p.id,c.id,"running",&serde_json::json!({"run_id":Uuid::from_u128(1002),"project_id":p.id,"conversation_id":c.id})).await.unwrap();
    let branch = conversation(20, p.id);
    s.save_conversation(&branch).await.unwrap();
    sqlx::query("INSERT INTO conversation_branches_v4 VALUES(?,?,?,?,?,'after_response',0,0,?,?,'merged',1,2)").bind(Uuid::from_u128(1003).to_string()).bind(branch.id.to_string()).bind(p.id.to_string()).bind(c.id.to_string()).bind(m.id.to_string()).bind("a".repeat(64)).bind("b".repeat(64)).execute(s.pool()).await.unwrap();
}

#[tokio::test]
async fn visibility_migration_preserves_legacy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let s = Store::open(&path).await.unwrap();
    let p = project(1);
    let mut c = conversation(10, p.id);
    c.status = ConversationStatus::Archived;
    s.save_project(&p).await.unwrap();
    s.save_conversation(&c).await.unwrap();
    related(&s, &p, &c).await;
    let before = legacy_bytes(&s).await;
    // Existing v4 database without this additive table/index is the legacy migration path.
    sqlx::query("DROP TABLE IF EXISTS conversation_visibility_v4")
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("DROP INDEX IF EXISTS idx_conversation_records_frame_project")
        .execute(s.pool())
        .await
        .unwrap();
    s.pool().close().await;
    for _ in 0..2 {
        let s = Store::open(&path).await.unwrap();
        assert_eq!(legacy_bytes(&s).await, before);
        let default_state = s
            .workspace_get_conversation_visibility(p.id, c.id)
            .await
            .unwrap();
        assert_eq!(default_state.visibility, ConversationVisibilityV1::Visible);
        assert_eq!(default_state.revision, 0);
        assert_eq!(default_state.changed_at, None);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversation_visibility_v4")
                .fetch_one(s.pool())
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            s.workspace_conversations_for_presentation(p.id)
                .await
                .unwrap()
                .len(),
            2
        );
        s.pool().close().await;
    }
}
#[tokio::test]
async fn visibility_filter_before_pagination() {
    let (s, p, _) = fixture().await;
    for i in 100..251 {
        let mut c = conversation(i, p.id);
        c.title = format!("find {i}");
        s.save_conversation(&c).await.unwrap();
        if i >= 150 {
            s.workspace_set_conversation_visibility(&set(&c, 0, ConversationVisibilityV1::Shelved))
                .await
                .unwrap();
        }
    }
    let mut q = list(p.id, ConversationVisibilityFilterV1::Visible);
    q.query = "find".into();
    let page = s.workspace_list_conversation_visibility(&q).await.unwrap();
    assert_eq!(page.items.len(), 50);
    assert_eq!(page.next_offset, None);
    assert!(
        page.items
            .iter()
            .all(|v| v.state.visibility == ConversationVisibilityV1::Visible)
    );
    q.visibility = ConversationVisibilityFilterV1::Shelved;
    let page = s.workspace_list_conversation_visibility(&q).await.unwrap();
    assert_eq!(page.items.len(), 50);
    assert_eq!(page.next_offset, Some(50));
    q.offset = 100;
    let page = s.workspace_list_conversation_visibility(&q).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next_offset, None);
    let other = project(2);
    s.save_project(&other).await.unwrap();
    q.project_id = other.id;
    q.offset = 0;
    q.visibility = ConversationVisibilityFilterV1::All;
    assert!(
        s.workspace_list_conversation_visibility(&q)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}
#[tokio::test]
async fn visibility_getter_by_owner_and_id() {
    let (s, p, c) = fixture().await;
    for i in 11..72 {
        s.save_conversation(&conversation(i, p.id)).await.unwrap();
    }
    assert!(
        !s.workspace_list_conversation_visibility(&list(p.id, ConversationVisibilityFilterV1::All))
            .await
            .unwrap()
            .items
            .iter()
            .any(|v| v.state.conversation_id == c.id)
    );
    assert_eq!(
        s.workspace_get_conversation_visibility(p.id, c.id)
            .await
            .unwrap()
            .revision,
        0
    );
    let other = project(2);
    s.save_project(&other).await.unwrap();
    error(
        s.workspace_get_conversation_visibility(other.id, c.id)
            .await
            .unwrap_err(),
        "conversation_visibility_ownership_mismatch",
    );
    error(
        s.workspace_get_conversation_visibility(p.id, Uuid::new_v4())
            .await
            .unwrap_err(),
        "conversation_visibility_not_found",
    );
    error(
        s.workspace_get_conversation_visibility(Uuid::new_v4(), c.id)
            .await
            .unwrap_err(),
        "conversation_visibility_not_found",
    );
}
#[tokio::test]
async fn visibility_replay_after_restore() {
    let (s, p, c) = fixture().await;
    let first = set(&c, 0, ConversationVisibilityV1::Shelved);
    let first_receipt = s
        .workspace_set_conversation_visibility(&first)
        .await
        .unwrap();
    let restored = s
        .workspace_set_conversation_visibility(&set(&c, 1, ConversationVisibilityV1::Visible))
        .await
        .unwrap();
    assert_eq!(restored.state.revision, 2);
    let replay = s
        .workspace_set_conversation_visibility(&first)
        .await
        .unwrap();
    assert_eq!(replay, first_receipt);
    assert_eq!(replay.state.revision, first_receipt.state.revision);
    let current = s
        .workspace_get_conversation_visibility(p.id, c.id)
        .await
        .unwrap();
    assert_eq!(current.visibility, ConversationVisibilityV1::Visible);
    assert_eq!(current.revision, 2);
    let mut conflict = first.clone();
    conflict.visibility = ConversationVisibilityV1::Visible;
    error(
        s.workspace_set_conversation_visibility(&conflict)
            .await
            .unwrap_err(),
        "conversation_visibility_request_conflict",
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM workspace_request_ledger")
            .fetch_one(s.pool())
            .await
            .unwrap(),
        2
    );
}
#[tokio::test]
async fn visibility_noop_and_revision_conflict() {
    let (s, p, mut c) = fixture().await;
    c.status = ConversationStatus::Running;
    s.save_conversation(&c).await.unwrap();
    related(&s, &p, &c).await;
    let before = legacy_bytes(&s).await;
    let noop = s
        .workspace_set_conversation_visibility(&set(&c, 0, ConversationVisibilityV1::Visible))
        .await
        .unwrap();
    assert!(!noop.changed);
    assert_eq!(noop.state.revision, 0);
    assert_eq!(noop.state.changed_at, None);
    let first = s
        .workspace_set_conversation_visibility(&set(&c, 0, ConversationVisibilityV1::Shelved))
        .await
        .unwrap();
    assert!(first.changed);
    assert_eq!(first.state.revision, 1);
    chrono::DateTime::parse_from_rfc3339(first.state.changed_at.as_ref().unwrap()).unwrap();
    let noop = s
        .workspace_set_conversation_visibility(&set(&c, 1, ConversationVisibilityV1::Shelved))
        .await
        .unwrap();
    assert!(!noop.changed);
    assert_eq!(noop.state, first.state);
    error(
        s.workspace_set_conversation_visibility(&set(&c, 0, ConversationVisibilityV1::Shelved))
            .await
            .unwrap_err(),
        "conversation_visibility_revision_conflict",
    );
    assert_eq!(legacy_bytes(&s).await, before);
    let conversation_after = s
        .conversations_for_project(p.id)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == c.id)
        .unwrap();
    assert_eq!(conversation_after.updated_at, c.updated_at);
    s.save_message(&Message::markdown(
        Uuid::new_v4(),
        p.id,
        c.id,
        1,
        MessageRole::User,
        "new turn",
        time(100),
    ))
    .await
    .unwrap();
    assert_eq!(
        s.workspace_get_conversation_visibility(p.id, c.id)
            .await
            .unwrap(),
        first.state
    );
}
#[tokio::test]
async fn visibility_composite_owner_fk() {
    let (s, p, c) = fixture().await;
    let other = project(2);
    s.save_project(&other).await.unwrap();
    for (owner, v, revision) in [
        (other.id, "shelved", 1),
        (p.id, "archived", 1),
        (p.id, "visible", 0),
    ] {
        assert!(
            sqlx::query("INSERT INTO conversation_visibility_v4 VALUES(?,?,?,?,1)")
                .bind(c.id.to_string())
                .bind(owner.to_string())
                .bind(v)
                .bind(revision)
                .execute(s.pool())
                .await
                .is_err()
        );
    }
    sqlx::query("INSERT INTO conversation_visibility_v4 VALUES(?,?,'visible',1,1)")
        .bind(c.id.to_string())
        .bind(p.id.to_string())
        .execute(s.pool())
        .await
        .unwrap();
    let mut foreign = set(&c, 0, ConversationVisibilityV1::Shelved);
    foreign.project_id = other.id;
    error(
        s.workspace_set_conversation_visibility(&foreign)
            .await
            .unwrap_err(),
        "conversation_visibility_ownership_mismatch",
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM workspace_request_ledger")
            .fetch_one(s.pool())
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn visibility_latest_and_delete_cascade() {
    let (s, p, c) = fixture().await;
    let newer = conversation(11, p.id);
    let draft = conversation(12, p.id);
    s.save_conversation(&newer).await.unwrap();
    s.save_conversation(&draft).await.unwrap();
    for (conv, n) in [(&c, 100), (&newer, 200)] {
        s.save_message(&Message::markdown(
            Uuid::new_v4(),
            p.id,
            conv.id,
            0,
            MessageRole::User,
            "",
            time(n),
        ))
        .await
        .unwrap();
    }
    assert_eq!(
        s.workspace_latest_visible_conversation(p.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        newer.id
    );
    s.workspace_set_conversation_visibility(&set(&newer, 0, ConversationVisibilityV1::Shelved))
        .await
        .unwrap();
    assert_eq!(
        s.workspace_latest_visible_conversation(p.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        c.id
    );
    assert_eq!(
        s.latest_used_conversation(p.id).await.unwrap().unwrap().id,
        newer.id
    );
    assert_eq!(s.conversations_for_project(p.id).await.unwrap().len(), 3);
    assert_eq!(
        s.workspace_conversations_for_presentation(p.id)
            .await
            .unwrap()
            .len(),
        2
    );
    s.workspace_set_conversation_visibility(&set(&c, 0, ConversationVisibilityV1::Shelved))
        .await
        .unwrap();
    assert_eq!(
        s.workspace_latest_visible_conversation(p.id).await.unwrap(),
        None
    );
    assert!(s.delete_conversation(p.id, newer.id).await.unwrap());
    error(
        s.workspace_get_conversation_visibility(p.id, newer.id)
            .await
            .unwrap_err(),
        "conversation_visibility_not_found",
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversation_visibility_v4")
            .fetch_one(s.pool())
            .await
            .unwrap(),
        1
    );
    s.delete_project(p.id).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversation_visibility_v4")
            .fetch_one(s.pool())
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn visibility_validates_bounds_and_literal_public_query() {
    let (s, p, mut c) = fixture().await;
    c.title = "Research API_KEY=fixture-only%secret".into();
    s.save_conversation(&c).await.unwrap();
    for (query, offset, limit) in [
        ("a".repeat(201), 0, 50),
        (String::new(), 10001, 50),
        (String::new(), 0, 0),
        (String::new(), 0, 101),
    ] {
        let mut q = list(p.id, ConversationVisibilityFilterV1::All);
        q.query = query;
        q.offset = offset;
        q.limit = limit;
        error(
            s.workspace_list_conversation_visibility(&q)
                .await
                .unwrap_err(),
            "conversation_visibility_invalid_input",
        );
    }
    let mut q = list(p.id, ConversationVisibilityFilterV1::All);
    q.query = "Research".into();
    assert_eq!(
        s.workspace_list_conversation_visibility(&q)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    q.query = "fixture-only".into();
    assert!(
        s.workspace_list_conversation_visibility(&q)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    q.query = "%".into();
    assert!(
        s.workspace_list_conversation_visibility(&q)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    q.query = "[REDACTED]".into();
    let page = s.workspace_list_conversation_visibility(&q).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(!page.items[0].title.contains("fixture-only"));
}
#[tokio::test]
async fn visibility_concurrent_cas_and_ledger_operation_conflict() {
    let (s, p, c) = fixture().await;
    let a = set(&c, 0, ConversationVisibilityV1::Shelved);
    let b = set(&c, 0, ConversationVisibilityV1::Shelved);
    let (a, b) = tokio::join!(
        s.workspace_set_conversation_visibility(&a),
        s.workspace_set_conversation_visibility(&b)
    );
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|v| **v).count(), 1);
    error(
        a.err().or_else(|| b.err()).unwrap(),
        "conversation_visibility_revision_conflict",
    );
    let req = set(&c, 1, ConversationVisibilityV1::Visible);
    sqlx::query("INSERT INTO workspace_request_ledger VALUES(?,'create_file_baseline',?,'{}',1)")
        .bind(req.request_id.to_string())
        .bind("a".repeat(64))
        .execute(s.pool())
        .await
        .unwrap();
    error(
        s.workspace_set_conversation_visibility(&req)
            .await
            .unwrap_err(),
        "conversation_visibility_request_conflict",
    );
    assert_eq!(
        s.workspace_get_conversation_visibility(p.id, c.id)
            .await
            .unwrap()
            .revision,
        1
    );
}

#[tokio::test]
async fn visibility_latest_ranks_user_time_and_stable_project_identity() {
    let (s, p, c) = fixture().await;
    let newer = conversation(11, p.id);
    let tie = conversation(12, p.id);
    let draft = conversation(13, p.id);
    let other = project(2);
    s.save_project(&other).await.unwrap();
    let foreign = conversation(14, other.id);
    for conv in [&newer, &tie, &draft, &foreign] {
        s.save_conversation(conv).await.unwrap();
    }
    for (conv, role, n) in [
        (&c, MessageRole::User, 100),
        (&newer, MessageRole::User, 200),
        (&tie, MessageRole::User, 200),
        (&draft, MessageRole::System, 900),
        (&foreign, MessageRole::User, 1000),
    ] {
        s.save_message(&Message::markdown(
            Uuid::new_v4(),
            conv.project_id,
            conv.id,
            0,
            role,
            "",
            time(n),
        ))
        .await
        .unwrap();
    }
    s.save_message(&Message::markdown(
        Uuid::new_v4(),
        p.id,
        c.id,
        1,
        MessageRole::Tool,
        "late tool",
        time(800),
    ))
    .await
    .unwrap();
    assert_eq!(
        s.workspace_latest_visible_conversation(p.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        newer.id
    );
    assert_eq!(
        s.latest_used_conversation(p.id).await.unwrap().unwrap().id,
        c.id
    );
}

#[tokio::test]
async fn visibility_receipt_failure_rolls_back_and_original_request_retries() {
    let (s, p, c) = fixture().await;
    let req = set(&c, 0, ConversationVisibilityV1::Shelved);
    sqlx::query("CREATE TEMP TRIGGER fail_visibility_receipt BEFORE INSERT ON workspace_request_ledger BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END").execute(s.pool()).await.unwrap();
    assert!(matches!(
        s.workspace_set_conversation_visibility(&req)
            .await
            .unwrap_err(),
        StoreError::Database(_)
    ));
    assert_eq!(
        s.workspace_get_conversation_visibility(p.id, c.id)
            .await
            .unwrap()
            .revision,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM workspace_request_ledger")
            .fetch_one(s.pool())
            .await
            .unwrap(),
        0
    );
    sqlx::query("DROP TRIGGER fail_visibility_receipt")
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(
        s.workspace_set_conversation_visibility(&req)
            .await
            .unwrap()
            .state
            .revision,
        1
    );
}
#[tokio::test]
async fn visibility_receipts_and_restored_row_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("visibility.sqlite");
    let s = Store::open(&path).await.unwrap();
    let p = project(1);
    let c = conversation(10, p.id);
    s.save_project(&p).await.unwrap();
    s.save_conversation(&c).await.unwrap();
    let req = set(&c, 0, ConversationVisibilityV1::Shelved);
    let receipt = s.workspace_set_conversation_visibility(&req).await.unwrap();
    s.workspace_set_conversation_visibility(&set(&c, 1, ConversationVisibilityV1::Visible))
        .await
        .unwrap();
    s.pool().close().await;
    for _ in 0..2 {
        let s = Store::open(&path).await.unwrap();
        assert_eq!(
            s.workspace_set_conversation_visibility(&req).await.unwrap(),
            receipt
        );
        let state = s
            .workspace_get_conversation_visibility(p.id, c.id)
            .await
            .unwrap();
        assert_eq!(state.visibility, ConversationVisibilityV1::Visible);
        assert_eq!(state.revision, 2);
        assert!(state.changed_at.is_some());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversation_visibility_v4")
                .fetch_one(s.pool())
                .await
                .unwrap(),
            1
        );
        s.pool().close().await;
    }
}
