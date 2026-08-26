use omicsops_core::workspace::{Project, ProjectTemplate};
use omicsops_store::Store;
use rusqlite::{Connection, params};
use uuid::Uuid;

#[tokio::test]
async fn opening_an_existing_database_preserves_retired_runtime_tables_and_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "
            PRAGMA user_version = 3;
            CREATE TABLE agent_run_events_v3 (
                run_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                previous_hash TEXT NOT NULL,
                event_hash TEXT NOT NULL UNIQUE,
                value_json TEXT NOT NULL,
                PRIMARY KEY(run_id, sequence)
            );
            CREATE TABLE agent_run_snapshots_v3 (
                run_id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                last_sequence INTEGER NOT NULL,
                last_event_hash TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            ",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_run_events_v3 VALUES (?1, ?2, ?3, 1, '', 'event-hash', '{}')",
            params!["legacy-run", "legacy-project", "legacy-conversation"],
        )
        .unwrap();
    drop(connection);

    drop(Store::open(&path).await.unwrap());

    let connection = Connection::open(path).unwrap();
    let rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_run_events_v3 WHERE run_id = 'legacy-run'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'agent_run_snapshots_v3'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);
}

#[tokio::test]
async fn deleting_a_current_project_does_not_clear_retired_runtime_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("delete.sqlite3");
    let project = Project::new(
        Uuid::new_v4(),
        "current",
        directory.path().display().to_string(),
        ProjectTemplate::Blank,
        chrono::Utc::now(),
    );
    {
        let store = Store::open(&path).await.unwrap();
        store.save_project(&project).await.unwrap();
    }
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO agent_run_events_v3 VALUES (?1, ?2, ?3, 1, '', 'retired-hash', '{}')",
            params![
                "retired-run",
                project.id.to_string(),
                "retired-conversation"
            ],
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&path).await.unwrap();
    assert!(store.delete_project(project.id).await.unwrap());
    drop(store);

    let connection = Connection::open(path).unwrap();
    let rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_run_events_v3 WHERE run_id = 'retired-run'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
}
