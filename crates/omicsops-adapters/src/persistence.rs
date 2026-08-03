use std::sync::{Arc, Mutex};

use omicsops_core::{
    audit::RunEventV2,
    domain::{AnalysisPlan, ConnectionProfile, RunEvent},
    plan_v2::{ApprovedPlan, EnvironmentLock, StepAttempt, migrate_v1_plan},
};
use rusqlite::{Connection, params};
use uuid::Uuid;

use crate::AdapterResult;

#[derive(Clone)]
pub struct Repository {
    connection: Arc<Mutex<Connection>>,
}

impl Repository {
    pub fn open(path: impl AsRef<std::path::Path>) -> AdapterResult<Self> {
        let path = path.as_ref();
        if path.exists() {
            let probe = Connection::open(path)?;
            let version: u32 = probe.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            drop(probe);
            if version < 2 {
                let backup = std::path::PathBuf::from(format!("{}.v1.bak", path.display()));
                if !backup.exists() {
                    std::fs::copy(path, backup)?;
                }
            }
        }
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> AdapterResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> AdapterResult<Self> {
        Self::migrate(&mut connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn migrate(connection: &mut Connection) -> AdapterResult<()> {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
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
            CREATE TABLE IF NOT EXISTS approved_plans (
                id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL,
                plan_hash TEXT NOT NULL,
                approved_plan_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_events_v2 (
                run_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                event_hash TEXT NOT NULL UNIQUE,
                event_json TEXT NOT NULL,
                PRIMARY KEY (run_id, sequence)
            );
            CREATE TABLE IF NOT EXISTS step_attempts_v2 (
                run_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                attempt INTEGER NOT NULL,
                attempt_json TEXT NOT NULL,
                PRIMARY KEY (run_id, step_id, attempt)
            );
            CREATE TABLE IF NOT EXISTS environment_locks_v2 (
                run_id TEXT NOT NULL,
                environment_id TEXT NOT NULL,
                lock_json TEXT NOT NULL,
                PRIMARY KEY (run_id, environment_id)
            );
            PRAGMA user_version = 2;
            ",
        )?;
        let legacy_plans = {
            let mut statement = transaction
                .prepare("SELECT id, value_json FROM app_objects WHERE kind = 'analysis_plan'")?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (id, json) in legacy_plans {
            let Ok(plan) = serde_json::from_str::<AnalysisPlan>(&json) else {
                continue;
            };
            let migrated = migrate_v1_plan(&plan);
            transaction.execute(
                "INSERT OR IGNORE INTO app_objects (kind, id, value_json) VALUES ('analysis_plan_v2', ?1, ?2)",
                params![id, serde_json::to_string(&migrated)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> AdapterResult<u32> {
        Ok(self.connection.lock().expect("database lock").query_row(
            "PRAGMA user_version",
            [],
            |row| row.get(0),
        )?)
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
            "INSERT INTO run_events (run_id, sequence, event_json)
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

    pub fn save_approved_plan(&self, approved: &ApprovedPlan) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO approved_plans (id, plan_id, plan_hash, approved_plan_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                approved.id.to_string(),
                approved.plan_id.to_string(),
                approved.plan_hash,
                serde_json::to_string(approved)?
            ],
        )?;
        Ok(())
    }

    pub fn get_approved_plan(&self, id: Uuid) -> AdapterResult<Option<ApprovedPlan>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare("SELECT approved_plan_json FROM approved_plans WHERE id = ?1")?;
        let mut rows = statement.query([id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let json: String = row.get(0)?;
        Ok(Some(serde_json::from_str(&json)?))
    }

    pub fn append_audit_event(&self, event: &RunEventV2) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO audit_events_v2 (run_id, sequence, event_hash, event_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                event.run_id.to_string(),
                event.sequence,
                event.event_hash,
                serde_json::to_string(event)?
            ],
        )?;
        Ok(())
    }

    pub fn audit_events_for_run(&self, run_id: Uuid) -> AdapterResult<Vec<RunEventV2>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare(
            "SELECT event_json FROM audit_events_v2 WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([run_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            Ok(serde_json::from_str(&json)?)
        })
        .collect()
    }

    pub fn save_step_attempt(&self, attempt: &StepAttempt) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO step_attempts_v2 (run_id, step_id, attempt, attempt_json) VALUES (?1, ?2, ?3, ?4)",
            params![attempt.run_id.to_string(), attempt.step_id, attempt.attempt, serde_json::to_string(attempt)?],
        )?;
        Ok(())
    }

    pub fn step_attempts_for_run(&self, run_id: Uuid) -> AdapterResult<Vec<StepAttempt>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare(
            "SELECT attempt_json FROM step_attempts_v2 WHERE run_id = ?1 ORDER BY step_id, attempt",
        )?;
        let rows = statement.query_map([run_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn save_environment_lock(&self, lock: &EnvironmentLock) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO environment_locks_v2 (run_id, environment_id, lock_json) VALUES (?1, 'micromamba', ?2)",
            params![lock.run_id.to_string(), serde_json::to_string(lock)?],
        )?;
        Ok(())
    }

    pub fn environment_lock_for_run(&self, run_id: Uuid) -> AdapterResult<Option<EnvironmentLock>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare("SELECT lock_json FROM environment_locks_v2 WHERE run_id = ?1 AND environment_id = 'micromamba'")?;
        let mut rows = statement.query([run_id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&row.get::<_, String>(0)?)?))
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

    pub fn list_json<T: serde::de::DeserializeOwned>(&self, kind: &str) -> AdapterResult<Vec<T>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare(
            "SELECT value_json FROM app_objects
             WHERE kind = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([kind], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            Ok(serde_json::from_str(&json)?)
        })
        .collect()
    }
}
