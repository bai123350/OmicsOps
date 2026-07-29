use std::sync::{Arc, Mutex};

use omicsops_core::domain::{ConnectionProfile, RunEvent};
use rusqlite::{Connection, params};
use uuid::Uuid;

use crate::AdapterResult;

#[derive(Clone)]
pub struct Repository {
    connection: Arc<Mutex<Connection>>,
}

impl Repository {
    pub fn open(path: impl AsRef<std::path::Path>) -> AdapterResult<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> AdapterResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> AdapterResult<Self> {
        connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS connections (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL,
                profile_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS run_events (
                run_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                event_json TEXT NOT NULL,
                PRIMARY KEY (run_id, sequence)
            );
            CREATE TABLE IF NOT EXISTS app_objects (
                kind TEXT NOT NULL,
                id TEXT NOT NULL,
                value_json TEXT NOT NULL,
                PRIMARY KEY (kind, id)
            );
            ",
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn save_connection(&self, profile: &ConnectionProfile) -> AdapterResult<()> {
        let json = serde_json::to_string(profile)?;
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO connections (id, label, profile_json)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                   label = excluded.label,
                   profile_json = excluded.profile_json",
            params![profile.id.to_string(), profile.label, json],
        )?;
        Ok(())
    }

    pub fn list_connections(&self) -> AdapterResult<Vec<ConnectionProfile>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare("SELECT profile_json FROM connections ORDER BY label, id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            Ok(serde_json::from_str(&json)?)
        })
        .collect()
    }

    pub fn append_event(&self, event: &RunEvent) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT OR REPLACE INTO run_events (run_id, sequence, event_json)
                 VALUES (?1, ?2, ?3)",
            params![
                event.run_id.to_string(),
                event.sequence,
                serde_json::to_string(event)?
            ],
        )?;
        Ok(())
    }

    pub fn events_for_run(&self, run_id: Uuid) -> AdapterResult<Vec<RunEvent>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare(
            "SELECT event_json FROM run_events
             WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([run_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            Ok(serde_json::from_str(&json)?)
        })
        .collect()
    }

    pub fn put_json<T: serde::Serialize>(
        &self,
        kind: &str,
        id: &str,
        value: &T,
    ) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT OR REPLACE INTO app_objects (kind, id, value_json)
                 VALUES (?1, ?2, ?3)",
            params![kind, id, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    pub fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        kind: &str,
        id: &str,
    ) -> AdapterResult<Option<T>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare("SELECT value_json FROM app_objects WHERE kind = ?1 AND id = ?2")?;
        let mut rows = statement.query(params![kind, id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let json: String = row.get(0)?;
        Ok(Some(serde_json::from_str(&json)?))
    }
}
