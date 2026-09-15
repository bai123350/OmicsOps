use std::path::Path;

use crate::{Store, StoreError};
use chrono::Utc;
use omicsops_core::workspace::{
    Conversation, Message, MessageRole, Project, ProjectTemplate, SyncDirection, SyncEntry,
    SyncState,
};
use omicsops_protocol::{AgentEventKindV4, AgentEventV4, RunModeV4};
use rusqlite::{Connection, params};
use sqlx::Row;
use tempfile::tempdir;
use uuid::Uuid;

fn v3_fixture(path: &Path, malformed_project: bool) -> (Project, Conversation, Vec<Message>) {
    let project = Project::new(
        Uuid::new_v4(),
        "fixture",
        "C:\\data\\fixture",
        ProjectTemplate::SingleCellRnaSeq,
        Utc::now(),
    );
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "ordered transcript", Utc::now());
    let messages = vec![
        Message::markdown(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            2,
            MessageRole::Assistant,
            "second",
            Utc::now(),
        ),
        Message::markdown(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            1,
            MessageRole::User,
            "first",
            Utc::now(),
        ),
    ];
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            PRAGMA user_version = 3;
            CREATE TABLE projects (id TEXT PRIMARY KEY, updated_at TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE conversations (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, updated_at TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE messages (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, conversation_id TEXT NOT NULL, sequence INTEGER NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE agent_run_events_v3 (run_id TEXT NOT NULL, project_id TEXT NOT NULL, conversation_id TEXT NOT NULL, sequence INTEGER NOT NULL, previous_hash TEXT NOT NULL, event_hash TEXT NOT NULL UNIQUE, value_json TEXT NOT NULL, PRIMARY KEY(run_id, sequence));
            ",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO projects VALUES (?1, ?2, ?3)",
            params![
                project.id.to_string(),
                project.updated_at.to_rfc3339(),
                if malformed_project {
                    "{not-json".to_owned()
                } else {
                    serde_json::to_string(&project).unwrap()
                }
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO conversations VALUES (?1, ?2, ?3, ?4)",
            params![
                conversation.id.to_string(),
                project.id.to_string(),
                conversation.updated_at.to_rfc3339(),
                serde_json::to_string(&conversation).unwrap()
            ],
        )
        .unwrap();
    for message in &messages {
        connection
            .execute(
                "INSERT INTO messages VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    message.id.to_string(),
                    message.project_id.to_string(),
                    message.conversation_id.to_string(),
                    message.sequence,
                    serde_json::to_string(message).unwrap()
                ],
            )
            .unwrap();
    }
    (project, conversation, messages)
}

fn valid_sync_entry(project_id: Uuid) -> SyncEntry {
    SyncEntry {
        id: Uuid::new_v4(),
        project_id,
        relative_path: "results/counts.tsv".into(),
        local_relative_path: Some("local/counts.tsv".into()),
        remote_path: Some("remote/counts.tsv".into()),
        direction: SyncDirection::LocalToRemote,
        size_bytes: 42,
        sha256: "0123456789abcdef".into(),
        state: SyncState::Pending,
        transferred_bytes: 0,
        retry_count: 0,
        error: None,
        updated_at: Utc::now(),
    }
}

fn v3_fixture_with_sync(
    path: &Path,
    malformed_project: bool,
) -> (Project, Conversation, Vec<Message>, SyncEntry) {
    let (project, conversation, messages) = v3_fixture(path, malformed_project);
    let entry = valid_sync_entry(project.id);
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE sync_entries (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            ",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO sync_entries VALUES (?1, ?2, ?3)",
            params![
                entry.id.to_string(),
                entry.project_id.to_string(),
                serde_json::to_string(&entry).unwrap(),
            ],
        )
        .unwrap();
    (project, conversation, messages, entry)
}

fn v3_fixture_with_agent_events_without_occurred_at(
    path: &Path,
) -> (Project, Conversation, Vec<AgentEventV4>) {
    let (project, conversation, _) = v3_fixture(path, false);
    let run_id = Uuid::new_v4();
    let first = AgentEventV4::first(
        run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    let second = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::ModelText {
            text: "legacy event".into(),
        },
    );
    let events = vec![first, second];
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE agent_runs_v4 (
                run_id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                status TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE agent_events_v4 (
                run_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                previous_hash TEXT NOT NULL,
                event_hash TEXT NOT NULL UNIQUE,
                value_json TEXT NOT NULL,
                PRIMARY KEY(run_id, sequence)
            );
            CREATE TABLE notebook_entries (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            ",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_runs_v4
             (run_id,project_id,conversation_id,status,value_json)
             VALUES (?1,?2,?3,'planning','{}')",
            params![
                run_id.to_string(),
                project.id.to_string(),
                conversation.id.to_string(),
            ],
        )
        .unwrap();
    for event in &events {
        connection
            .execute(
                "INSERT INTO agent_events_v4
                 (run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    event.run_id.to_string(),
                    event.project_id.to_string(),
                    event.conversation_id.to_string(),
                    event.sequence,
                    event.previous_hash,
                    event.event_hash,
                    serde_json::to_string(event).unwrap(),
                ],
            )
            .unwrap();
    }
    (project, conversation, events)
}

#[test]
fn schema_is_clean_room_and_not_reference_ordered() {
    let sql = include_str!("../migrations/init.sql");
    assert!(!sql.to_ascii_lowercase().contains("wisp"));
    assert!(!sql.contains("xuzhougeng"));
    assert!(
        sql.find("CREATE TABLE IF NOT EXISTS projects")
            > sql.find("CREATE TABLE IF NOT EXISTS connections")
    );
}

#[test]
fn adapters_do_not_own_synchronous_persistence() {
    let adapters = Path::new(env!("CARGO_MANIFEST_DIR")).join("../omicsops-adapters/src");
    assert!(!adapters.join("persistence.rs").exists());
    for entry in std::fs::read_dir(adapters).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(entry.path()).unwrap();
        assert!(!source.contains("block_on"));
        assert!(!source.contains("Runtime::new"));
    }
}

#[tokio::test]
async fn store_owns_a_sqlite_pool_and_installs_v4_schema() -> Result<(), StoreError> {
    let store = Store::open_in_memory().await?;

    assert_eq!(store.schema_version().await?, 4);
    assert!(store.foreign_keys_enabled().await?);
    assert!(store.has_table("env_snapshots").await?);
    Ok(())
}

#[tokio::test]
async fn v3_upgrade_creates_named_backup_maps_rows_and_is_idempotent() -> Result<(), StoreError> {
    let directory = tempdir().unwrap();
    let path = directory
        .path()
        .join("空间 folder")
        .join("workspace.sqlite3");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let (project, conversation, messages) = v3_fixture(&path, false);

    let store = Store::open(&path).await?;
    assert_eq!(store.schema_version().await?, 4);
    let backups = backup_names(path.parent().unwrap());
    assert_eq!(backups.len(), 1);
    assert!(backups[0].starts_with("workspace.sqlite3.pre-store-v4."));
    assert!(backups[0].ends_with(".bak"));
    assert!(!backups.iter().any(|name| name.ends_with(".v3.bak")));
    assert_eq!(
        sqlx::query("SELECT COUNT(*) AS count FROM projects")
            .fetch_one(store.pool())
            .await?
            .get::<i64, _>("count"),
        1
    );
    assert_eq!(
        sqlx::query("SELECT COUNT(*) AS count FROM frames WHERE id = ?1")
            .bind(conversation.id.to_string())
            .fetch_one(store.pool())
            .await?
            .get::<i64, _>("count"),
        1
    );
    let rows = sqlx::query("SELECT seq, content FROM messages WHERE frame_id = ?1 ORDER BY seq")
        .bind(conversation.id.to_string())
        .fetch_all(store.pool())
        .await?;
    assert_eq!(rows.len(), messages.len());
    assert_eq!(rows[0].get::<i64, _>("seq"), 1);
    assert_eq!(rows[0].get::<String, _>("content"), "first");
    assert_eq!(rows[1].get::<i64, _>("seq"), 2);
    assert_eq!(rows[1].get::<String, _>("content"), "second");
    drop(store);

    let reopened = Store::open(&path).await?;
    assert_eq!(reopened.schema_version().await?, 4);
    assert_eq!(
        sqlx::query("SELECT COUNT(*) AS count FROM projects")
            .fetch_one(reopened.pool())
            .await?
            .get::<i64, _>("count"),
        1
    );
    assert_eq!(
        sqlx::query("SELECT COUNT(*) AS count FROM messages")
            .fetch_one(reopened.pool())
            .await?
            .get::<i64, _>("count"),
        2
    );
    assert_eq!(
        reopened.get_project(project.id).await?.unwrap().id,
        project.id
    );
    Ok(())
}

#[tokio::test]
async fn v3_sync_entries_migrate_and_support_restart_updates() -> Result<(), StoreError> {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sync-entry-v3.sqlite3");
    let (project, _, _, entry) = v3_fixture_with_sync(&path, false);

    let store = Store::open(&path).await?;
    let columns = sqlx::query("PRAGMA table_info(sync_entries)")
        .fetch_all(store.pool())
        .await?;
    assert!(
        columns
            .iter()
            .any(|row| { row.get::<String, _>(1) == "relative_path" && row.get::<i64, _>(3) == 1 })
    );
    assert_eq!(
        store.sync_entries_for_project(project.id).await?,
        vec![entry.clone()]
    );

    let mut terminal = entry.clone();
    terminal.relative_path = "results/counts.final.tsv".into();
    terminal.state = SyncState::Synced;
    terminal.transferred_bytes = terminal.size_bytes;
    terminal.updated_at = Utc::now();
    store.save_sync_entry(&terminal).await?;
    assert_eq!(
        store.sync_entries_for_project(project.id).await?,
        vec![terminal.clone()]
    );
    drop(store);

    let reopened = Store::open(&path).await?;
    assert_eq!(
        reopened.sync_entries_for_project(project.id).await?,
        vec![terminal]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_entries")
            .fetch_one(reopened.pool())
            .await?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn malformed_v3_sync_entry_json_rolls_back_without_defaulting_relative_path() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("malformed-sync-entry.sqlite3");
    let (project, _, _, entry) = v3_fixture_with_sync(&path, false);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE sync_entries SET value_json=?1 WHERE id=?2",
            params!["{not-json", entry.id.to_string()],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("malformed sync entry unexpectedly migrated"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("sync"), "{error}");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT value_json FROM sync_entries WHERE id=?1",
                [entry.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "{not-json"
    );
    let _ = project;
}

#[tokio::test]
async fn v3_sync_entry_identity_and_path_tampering_rolls_back() {
    for tamper in ["id", "project", "relative_path"] {
        let directory = tempdir().unwrap();
        let path = directory
            .path()
            .join(format!("sync-{tamper}-tamper.sqlite3"));
        let (project, _, _, entry) = v3_fixture_with_sync(&path, false);
        let connection = Connection::open(&path).unwrap();
        match tamper {
            "id" => {
                connection
                    .execute(
                        "UPDATE sync_entries SET id=?1 WHERE id=?2",
                        params![Uuid::new_v4().to_string(), entry.id.to_string()],
                    )
                    .unwrap();
            }
            "project" => {
                connection
                    .execute(
                        "UPDATE sync_entries SET project_id=?1 WHERE id=?2",
                        params![Uuid::new_v4().to_string(), entry.id.to_string()],
                    )
                    .unwrap();
            }
            "relative_path" => {
                let mut value = serde_json::to_value(&entry).unwrap();
                value["relative_path"] = serde_json::Value::String("   ".into());
                connection
                    .execute(
                        "UPDATE sync_entries SET value_json=?1 WHERE id=?2",
                        params![value.to_string(), entry.id.to_string()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(connection);

        let error = match Store::open(&path).await {
            Ok(_) => panic!("sync {tamper} tampering unexpectedly migrated"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("sync"), "{error}");
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('sync_entries')
                     WHERE name='relative_path'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0,
            "failed sync migration did not roll back the v4 table shape"
        );
        let _ = project;
    }
}

#[tokio::test]
async fn v3_sync_entry_unknown_project_is_rejected_by_fk_validation() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sync-unknown-project.sqlite3");
    let (project, _, _, entry) = v3_fixture_with_sync(&path, false);
    let unknown_project = Uuid::new_v4();
    let mut value = serde_json::to_value(&entry).unwrap();
    value["project_id"] = serde_json::Value::String(unknown_project.to_string());
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE sync_entries SET project_id=?1,value_json=?2 WHERE id=?3",
            params![
                unknown_project.to_string(),
                value.to_string(),
                entry.id.to_string()
            ],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("sync entry with an unknown project unexpectedly migrated"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("sync"), "{error}");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    let _ = project;
}

#[tokio::test]
async fn v3_sync_entry_target_count_and_set_tampering_rolls_back() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("sync-target-set-tamper.sqlite3");
    let (project, _, _, entry) = v3_fixture_with_sync(&path, false);
    let extra = valid_sync_entry(project.id);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "
            ALTER TABLE sync_entries RENAME TO sync_entries_v3_legacy;
            CREATE TABLE sync_entries (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            ",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO sync_entries VALUES (?1,?2,?3,?4)",
            params![
                extra.id.to_string(),
                extra.project_id.to_string(),
                extra.relative_path,
                serde_json::to_string(&extra).unwrap(),
            ],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("tampered sync target identifier set unexpectedly migrated"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("sync"), "{error}");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM sync_entries", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1,
        "failed sync migration left a second target row behind"
    );
    let _ = entry;
}

fn backup_names(directory: &Path) -> Vec<String> {
    std::fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            (name.contains("pre-store-v4") || name.ends_with(".v3.bak")).then_some(name)
        })
        .collect()
}

#[tokio::test]
async fn backup_never_reuses_a_stale_name_for_non_ascii_path() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("数据 set").join("work space.sqlite3");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let _ = v3_fixture(&path, false);
    let stale = path
        .parent()
        .unwrap()
        .join("work space.sqlite3.pre-store-v4.20000101-000000000-stale.bak");
    std::fs::write(&stale, b"stale backup").unwrap();

    Store::open(&path).await.unwrap();
    let backups = backup_names(path.parent().unwrap());
    assert_eq!(backups.len(), 2);
    assert_eq!(std::fs::read(&stale).unwrap(), b"stale backup");
}

#[tokio::test]
async fn future_schema_versions_are_rejected_without_downgrade() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("future.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA user_version = 5;")
        .unwrap();
    drop(connection);

    assert!(Store::open(&path).await.is_err());
    let connection = Connection::open(&path).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 5);
}

#[tokio::test]
async fn strict_mapping_rejects_unexpected_normalized_extension_rows() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("strict-count.sqlite3");
    let (project, _, _) = v3_fixture(&path, false);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE project_omicsops (
                project_id TEXT PRIMARY KEY,
                local_root TEXT NOT NULL,
                remote_root TEXT,
                connection_id TEXT,
                template TEXT NOT NULL,
                status TEXT NOT NULL,
                ollama_only INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO project_omicsops VALUES
                ('unexpected-project', 'C:/unexpected', NULL, NULL, 'blank', 'active', 0, 'now');
            ",
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("strict mapping unexpectedly succeeded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("project"));
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    let _ = project;
}

#[tokio::test]
async fn legacy_project_target_id_set_tampering_rolls_back() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("project-id-set-tamper.sqlite3");
    let (project, _, _) = v3_fixture(&path, false);
    let replacement = Uuid::new_v4();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("DELETE FROM messages; DELETE FROM conversations;")
        .unwrap();
    connection
        .execute_batch("ALTER TABLE projects RENAME TO projects_v3_legacy;")
        .unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                workspace_dir TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE project_omicsops (
                project_id TEXT PRIMARY KEY,
                local_root TEXT NOT NULL,
                remote_root TEXT,
                connection_id TEXT,
                template TEXT NOT NULL,
                status TEXT NOT NULL,
                ollama_only INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            ",
        )
        .unwrap();
    connection
        .execute_batch(&format!(
            "
            CREATE TRIGGER tamper_project_target AFTER INSERT ON project_omicsops
            WHEN NEW.project_id = '{project}'
            BEGIN
                UPDATE project_omicsops SET project_id = '{replacement}'
                    WHERE project_id = '{project}';
                UPDATE projects SET id = '{replacement}' WHERE id = '{project}';
            END;
            ",
            project = project.id,
            replacement = replacement,
        ))
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("tampered project target identifier set unexpectedly migrated"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("project"), "{error}");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0,
        "failed migration left normalized project rows behind"
    );
}

#[tokio::test]
async fn legacy_conversation_target_project_id_tampering_rolls_back() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("conversation-project-tamper.sqlite3");
    let (project, conversation, _) = v3_fixture(&path, false);
    let unknown_project = Uuid::new_v4();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE conversations SET project_id=?1 WHERE id=?2",
            params![unknown_project.to_string(), conversation.id.to_string()],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("tampered conversation target project unexpectedly migrated"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("conversation"), "{error}");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='frames'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0,
        "failed migration left normalized conversation rows behind"
    );
    let _ = project;
}

#[tokio::test]
async fn legacy_message_target_conversation_id_tampering_rolls_back() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("message-conversation-tamper.sqlite3");
    let (project, conversation, messages) = v3_fixture(&path, false);
    let unknown_conversation = Uuid::new_v4();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE messages SET conversation_id=?1 WHERE id=?2",
            params![unknown_conversation.to_string(), messages[0].id.to_string()],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("tampered message target conversation unexpectedly migrated"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("message"), "{error}");
    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='frames'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0,
        "failed migration left normalized conversation rows behind"
    );
    let _ = (project, conversation);
}

#[tokio::test]
async fn unknown_project_or_conversation_is_rejected_without_null_frame_bypass() {
    let store = Store::open_in_memory().await.unwrap();
    let message = Message::markdown(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        1,
        MessageRole::User,
        "must reject",
        Utc::now(),
    );
    let error = store.save_message(&message).await.unwrap_err();
    assert!(error.to_string().contains("project") || error.to_string().contains("conversation"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM frames")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn corrupted_event_sql_columns_are_rejected_with_event_context() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("corrupt-event.sqlite3");
    let store = Store::open(&path).await.unwrap();
    let run_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    store
        .save_project(&Project::new(
            project_id,
            "event project",
            "C:\\event-data",
            ProjectTemplate::Blank,
            Utc::now(),
        ))
        .await
        .unwrap();
    store
        .save_conversation(&Conversation::new(
            conversation_id,
            project_id,
            "event conversation",
            Utc::now(),
        ))
        .await
        .unwrap();
    let event = AgentEventV4::first(
        run_id,
        project_id,
        conversation_id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    store.append_agent_event_v4(&event).await.unwrap();
    drop(store);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE agent_events_v4 SET sequence = sequence + 1 WHERE run_id = ?1",
            [run_id.to_string()],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("corrupted event unexpectedly succeeded"),
        Err(error) => error,
    };
    let text = error.to_string();
    assert!(text.contains(&run_id.to_string()), "{text}");
    assert!(text.contains("event"), "{text}");
}

#[tokio::test]
async fn zero_occurred_at_is_rejected_even_when_json_is_valid() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("zero-occurred-at.sqlite3");
    let store = Store::open(&path).await.unwrap();
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    store
        .save_project(&Project::new(
            project_id,
            "occurred-at project",
            "C:\\occurred-at-data",
            ProjectTemplate::Blank,
            Utc::now(),
        ))
        .await
        .unwrap();
    store
        .save_conversation(&Conversation::new(
            conversation_id,
            project_id,
            "occurred-at conversation",
            Utc::now(),
        ))
        .await
        .unwrap();
    let run_id = Uuid::new_v4();
    let event = AgentEventV4::first(
        run_id,
        project_id,
        conversation_id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    store.append_agent_event_v4(&event).await.unwrap();
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE agent_events_v4 SET occurred_at=0 WHERE run_id=?1",
            [run_id.to_string()],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("zero occurred_at unexpectedly bypassed validation"),
        Err(error) => error,
    };
    let text = error.to_string();
    assert!(text.contains("event"), "{text}");
    assert!(
        text.contains("occurred") || text.contains("durable"),
        "{text}"
    );
}

#[tokio::test]
async fn v3_agent_events_without_occurred_at_are_backfilled_and_reopen_idempotently() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("legacy-agent-events.sqlite3");
    let (_, _, events) = v3_fixture_with_agent_events_without_occurred_at(&path);

    let store = Store::open(&path).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), 4);
    assert_eq!(
        store.agent_events_v4(events[0].run_id).await.unwrap(),
        events
    );
    let stored_times = sqlx::query_scalar::<_, i64>(
        "SELECT occurred_at FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence",
    )
    .bind(events[0].run_id.to_string())
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(
        stored_times,
        events
            .iter()
            .map(|event| event.occurred_at.timestamp_millis())
            .collect::<Vec<_>>()
    );
    for column in ["created_at", "updated_at"] {
        assert!(table_has_column(store.pool(), "agent_runs_v4", column).await);
    }
    drop(store);

    let reopened = Store::open(&path).await.unwrap();
    assert_eq!(
        reopened.agent_events_v4(events[0].run_id).await.unwrap(),
        events
    );
    assert_eq!(backup_names(directory.path()).len(), 1);
}

#[tokio::test]
async fn malformed_v3_agent_event_rolls_back_timestamp_and_index_extensions() {
    let directory = tempdir().unwrap();
    let path = directory
        .path()
        .join("malformed-legacy-agent-event.sqlite3");
    let (_, _, events) = v3_fixture_with_agent_events_without_occurred_at(&path);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE agent_events_v4 SET value_json='{not-json' WHERE run_id=?1 AND sequence=1",
            [events[0].run_id.to_string()],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("malformed legacy Agent event unexpectedly migrated"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("backfilling occurred_at"));

    let connection = Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    for (table, column) in [
        ("agent_events_v4", "occurred_at"),
        ("agent_runs_v4", "updated_at"),
        ("notebook_entries", "updated_at"),
    ] {
        assert_eq!(
            connection
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='{column}'"
                    ),
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0,
            "failed migration left {table}.{column} behind"
        );
    }
}

async fn table_has_column(pool: &sqlx::SqlitePool, table: &str, column: &str) -> bool {
    let rows = sqlx::query(&format!("PRAGMA table_info(\"{table}\")"))
        .fetch_all(pool)
        .await
        .unwrap();
    rows.iter().any(|row| row.get::<String, _>(1) == column)
}

#[tokio::test]
async fn archiving_an_unknown_run_is_rejected_without_writing_a_row() {
    let store = Store::open_in_memory().await.unwrap();
    let checkpoint = omicsops_protocol::ContextCheckpointV4 {
        schema_version: 4,
        through_sequence: 0,
        completion_criteria: vec![],
        unresolved_errors: vec![],
        recent_steps: vec![],
        scientific_state: serde_json::json!({}),
        task_shape: None,
        phase: None,
        task_revision: None,
        tasks: vec![],
        cycle_id: None,
    };

    let error = store
        .archive_agent_context_v4(Uuid::new_v4(), "[]", &checkpoint)
        .await
        .expect_err("archive API accepted an unknown run");
    assert!(error.to_string().contains("run"), "{error}");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_context_archives_v4")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
}

#[test]
fn backup_create_new_collision_retries_with_the_next_candidate() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("collision.sqlite3");
    std::fs::write(&path, b"source database").unwrap();
    let timestamp = 1_700_000_000_000_i64;
    let collided = directory
        .path()
        .join("collision.sqlite3.pre-store-v4.1700000000000-collision.bak");
    std::fs::write(&collided, b"pre-existing backup").unwrap();

    let mut ids = ["collision", "fresh"].into_iter();
    let backup = super::create_backup_with_id_factory(&path, timestamp, || {
        ids.next()
            .expect("test supplied too few backup ids")
            .to_owned()
    })
    .unwrap();

    assert_eq!(
        backup.file_name().unwrap().to_string_lossy(),
        "collision.sqlite3.pre-store-v4.1700000000000-fresh.bak"
    );
    assert_eq!(std::fs::read(&collided).unwrap(), b"pre-existing backup");
    assert_eq!(std::fs::read(&backup).unwrap(), b"source database");
}

#[tokio::test]
async fn failed_upgrade_rolls_back_schema_and_rows() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("broken.sqlite3");
    let (project, _, _) = v3_fixture(&path, true);

    assert!(Store::open(&path).await.is_err());
    let connection = Connection::open(&path).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 3);
    let value: String = connection
        .query_row(
            "SELECT value_json FROM projects WHERE id = ?1",
            [project.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, "{not-json");
    let frame_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='frames'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(frame_count, 0);
}

#[tokio::test]
async fn injected_upgrade_failure_is_atomic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("injected.sqlite3");
    let (project, conversation, messages) = v3_fixture(&path, false);

    let before = Connection::open(&path).unwrap();
    let schema_before = before
        .prepare(
            "SELECT name,sql FROM sqlite_master
             WHERE type='table' AND name IN ('projects','conversations','messages')
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let rows_before = [
        before
            .query_row(
                "SELECT value_json FROM projects WHERE id=?1",
                [project.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        before
            .query_row(
                "SELECT value_json FROM conversations WHERE id=?1",
                [conversation.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        before
            .query_row(
                "SELECT value_json FROM messages WHERE id=?1",
                [messages[0].id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
    ];
    drop(before);

    // The second checkpoint fails only after the first project row was copied,
    // proving rollback covers actual DML as well as the preceding table renames
    // and schema creation.
    assert!(Store::open_with_migration_failure(&path, 1).await.is_err());
    let connection = Connection::open(&path).unwrap();
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 3);
    let project_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE id = ?1",
            [project.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(project_count, 1);
    let schema_after = connection
        .prepare(
            "SELECT name,sql FROM sqlite_master
             WHERE type='table' AND name IN ('projects','conversations','messages')
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(schema_after, schema_before);
    let rows_after = [
        connection
            .query_row(
                "SELECT value_json FROM projects WHERE id=?1",
                [project.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT value_json FROM conversations WHERE id=?1",
                [conversation.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT value_json FROM messages WHERE id=?1",
                [messages[0].id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
    ];
    assert_eq!(rows_after, rows_before);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE '%_v3_legacy'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn foreign_keys_and_event_hashes_are_checked_before_commit() -> Result<(), StoreError> {
    let store = Store::open_in_memory().await?;
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    store
        .save_project(&Project::new(
            project_id,
            "project",
            "C:\\project",
            ProjectTemplate::Blank,
            Utc::now(),
        ))
        .await?;
    let conversation = Conversation::new(conversation_id, project_id, "FK", Utc::now());
    store.save_conversation(&conversation).await?;
    let first = AgentEventV4::first(
        Uuid::new_v4(),
        project_id,
        conversation_id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    store.append_agent_event_v4(&first).await?;
    assert_eq!(store.agent_events_v4(first.run_id).await?, vec![first]);
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(store.pool())
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn backup_name_is_exact_and_reopen_does_not_create_a_second_copy() {
    let directory = tempdir().unwrap();
    let path = directory
        .path()
        .join("数据 folder")
        .join("workspace file.sqlite3");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let _ = v3_fixture(&path, false);

    Store::open(&path).await.unwrap();
    let backups = backup_names(path.parent().unwrap());
    assert_eq!(backups.len(), 1, "expected one pre-store-v4 backup");
    let prefix = format!(
        "{}.pre-store-v4.",
        path.file_name().unwrap().to_string_lossy()
    );
    let backup = &backups[0];
    let suffix = backup
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(".bak"))
        .unwrap_or_else(|| panic!("backup name has wrong shape: {backup}"));
    let (timestamp, id) = suffix
        .split_once('-')
        .unwrap_or_else(|| panic!("backup name lacks timestamp/id separator: {backup}"));
    assert!(!timestamp.is_empty() && timestamp.chars().all(|c| c.is_ascii_digit()));
    assert!(!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()));

    Store::open(&path).await.unwrap();
    assert_eq!(backup_names(path.parent().unwrap()).len(), 1);
}

#[tokio::test]
async fn project_paths_preserve_spaces_non_ascii_windows_and_macos_forms() -> Result<(), StoreError>
{
    let store = Store::open_in_memory().await?;
    let now = Utc::now();
    let roots = [
        "C:\\Research Data\\样本集",
        "/Users/研究者/Research Data/样本集",
    ];
    for root in roots {
        let project = Project::new(
            Uuid::new_v4(),
            "path fixture",
            root,
            ProjectTemplate::Blank,
            now,
        );
        store.save_project(&project).await?;
        assert_eq!(
            store.get_project(project.id).await?.unwrap().local_root,
            root
        );
    }
    let whitespace_only = Project::new(
        Uuid::new_v4(),
        "invalid path",
        "   \t  ",
        ProjectTemplate::Blank,
        now,
    );
    assert!(store.save_project(&whitespace_only).await.is_err());
    Ok(())
}

#[tokio::test]
async fn v4_event_columns_must_match_the_json_payload() -> Result<(), StoreError> {
    let directory = tempdir().unwrap();
    let path = directory.path().join("event-column-integrity.sqlite3");
    let store = Store::open(&path).await?;
    let project = Project::new(
        Uuid::new_v4(),
        "event project",
        "/Users/研究者/Event Data",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await?;
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "event conversation", Utc::now());
    store.save_conversation(&conversation).await?;
    let event = AgentEventV4::first(
        Uuid::new_v4(),
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Plan,
        },
    );
    store.append_agent_event_v4(&event).await?;
    let row = sqlx::query(
        "SELECT sequence,project_id,conversation_id,previous_hash,event_hash,value_json
         FROM agent_events_v4 WHERE run_id=?1",
    )
    .bind(event.run_id.to_string())
    .fetch_one(store.pool())
    .await?;
    let payload: AgentEventV4 = serde_json::from_str(row.get::<String, _>("value_json").as_str())?;
    assert_eq!(row.get::<i64, _>("sequence"), payload.sequence as i64);
    assert_eq!(
        row.get::<String, _>("project_id"),
        payload.project_id.to_string()
    );
    assert_eq!(
        row.get::<String, _>("conversation_id"),
        payload.conversation_id.to_string()
    );
    assert_eq!(row.get::<String, _>("previous_hash"), payload.previous_hash);
    assert_eq!(row.get::<String, _>("event_hash"), payload.event_hash);
    let other_project = Project::new(
        Uuid::new_v4(),
        "other event project",
        "/Users/研究者/Other Data",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&other_project).await?;
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE agent_events_v4 SET project_id=?1 WHERE run_id=?2",
            params![other_project.id.to_string(), event.run_id.to_string()],
        )
        .unwrap();
    drop(connection);
    let error = match Store::open(&path).await {
        Ok(_) => panic!("corrupted event unexpectedly opened"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("event"), "{error}");
    Ok(())
}

#[tokio::test]
async fn v4_context_columns_must_match_checkpoint_and_transcript() -> Result<(), StoreError> {
    let directory = tempdir().unwrap();
    let path = directory.path().join("context-column-integrity.sqlite3");
    let store = Store::open(&path).await?;
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    store
        .save_project(&Project::new(
            project_id,
            "context project",
            "/Users/研究者/context data",
            ProjectTemplate::Blank,
            Utc::now(),
        ))
        .await?;
    store
        .save_conversation(&Conversation::new(
            conversation_id,
            project_id,
            "context conversation",
            Utc::now(),
        ))
        .await?;
    store
        .save_agent_run_v4(
            run_id,
            project_id,
            conversation_id,
            "archiving",
            &serde_json::json!({}),
        )
        .await?;
    let checkpoint = omicsops_protocol::ContextCheckpointV4 {
        schema_version: 4,
        through_sequence: 7,
        completion_criteria: vec!["complete".into()],
        unresolved_errors: vec![],
        recent_steps: vec!["inspect".into()],
        scientific_state: serde_json::json!({"path":"/Users/研究者/context data"}),
        task_shape: None,
        phase: None,
        task_revision: None,
        tasks: vec![],
        cycle_id: None,
    };
    let transcript = "[ { \"sequence\": 7 } ]";
    let archive = store
        .archive_agent_context_v4(run_id, transcript, &checkpoint)
        .await?;
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE agent_context_archives_v4 SET through_sequence=through_sequence+1 WHERE archive_id=?1",
            [archive.archive_id.to_string()],
        )
        .unwrap();
    drop(connection);
    let error = match Store::open(&path).await {
        Ok(_) => panic!("corrupted context unexpectedly opened"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("context") || error.to_string().contains("archive"),
        "{error}"
    );
    Ok(())
}

#[tokio::test]
async fn v4_context_archive_owner_is_validated_on_reopen() -> Result<(), StoreError> {
    let directory = tempdir().unwrap();
    let path = directory.path().join("context-owner-integrity.sqlite3");
    let store = Store::open(&path).await?;
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    store
        .save_project(&Project::new(
            project_id,
            "context owner project",
            "C:\\context-owner-data",
            ProjectTemplate::Blank,
            Utc::now(),
        ))
        .await?;
    store
        .save_conversation(&Conversation::new(
            conversation_id,
            project_id,
            "context owner conversation",
            Utc::now(),
        ))
        .await?;
    store
        .save_agent_run_v4(
            run_id,
            project_id,
            conversation_id,
            "archiving",
            &serde_json::json!({}),
        )
        .await?;
    let checkpoint = omicsops_protocol::ContextCheckpointV4 {
        schema_version: 4,
        through_sequence: 0,
        completion_criteria: vec![],
        unresolved_errors: vec![],
        recent_steps: vec![],
        scientific_state: serde_json::json!({}),
        task_shape: None,
        phase: None,
        task_revision: None,
        tasks: vec![],
        cycle_id: None,
    };
    let archive = store
        .archive_agent_context_v4(run_id, "[]", &checkpoint)
        .await?;
    drop(store);

    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    connection
        .execute(
            "UPDATE agent_context_archives_v4 SET run_id=?1 WHERE archive_id=?2",
            params![Uuid::new_v4().to_string(), archive.archive_id.to_string()],
        )
        .unwrap();
    drop(connection);

    let error = match Store::open(&path).await {
        Ok(_) => panic!("archive with an unknown owner run unexpectedly opened"),
        Err(error) => error,
    };
    let text = error.to_string();
    assert!(
        text.contains("context") || text.contains("archive"),
        "{text}"
    );
    assert!(
        text.contains("inconsistent") || text.contains("unknown"),
        "{text}"
    );
    Ok(())
}

#[tokio::test]
async fn generic_json_delete_removes_only_the_exact_kind_and_id() -> Result<(), StoreError> {
    let store = Store::open_in_memory().await?;
    store
        .put_json(
            "quick_action_v1",
            "same-id",
            &serde_json::json!({ "value": "delete" }),
        )
        .await?;
    store
        .put_json(
            "specialist_template_v1",
            "same-id",
            &serde_json::json!({ "value": "keep" }),
        )
        .await?;

    assert!(store.delete_json("quick_action_v1", "same-id").await?);
    assert!(!store.delete_json("quick_action_v1", "same-id").await?);
    assert_eq!(
        store
            .get_json::<serde_json::Value>("specialist_template_v1", "same-id")
            .await?,
        Some(serde_json::json!({ "value": "keep" }))
    );
    Ok(())
}
