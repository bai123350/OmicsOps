use std::sync::{Arc, Mutex};

use omicsops_agent::AgentEvent;
use omicsops_core::{
    audit::RunEventV2,
    domain::{AnalysisPlan, ConnectionProfile, RunEvent},
    plan_v2::{ApprovedPlan, EnvironmentLock, StepAttempt, migrate_v1_plan},
    workspace::{
        AgentTurn, Artifact, Conversation, Message, ModelProfile, NotebookEntry, Project,
        SkillPackage, SyncEntry,
    },
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
            if version < 3 {
                let suffix = if version < 2 { "v1" } else { "v2" };
                let backup = std::path::PathBuf::from(format!("{}.{suffix}.bak", path.display()));
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
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                updated_at TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                value_json TEXT NOT NULL,
                FOREIGN KEY(project_id) REFERENCES projects(id)
            );
            CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, conversation_id TEXT NOT NULL, sequence INTEGER NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS agent_turns (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, conversation_id TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS agent_events (turn_id TEXT NOT NULL, sequence INTEGER NOT NULL, value_json TEXT NOT NULL, PRIMARY KEY(turn_id, sequence));
            CREATE TABLE IF NOT EXISTS tool_calls (id TEXT PRIMARY KEY, turn_id TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS approvals_v3 (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS artifacts_v3 (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS notebook_entries (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS skill_packages (id TEXT PRIMARY KEY, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS model_profiles (id TEXT PRIMARY KEY, value_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS sync_entries (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, value_json TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS conversations_project_updated ON conversations(project_id, updated_at DESC);
            PRAGMA user_version = 3;
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

    pub fn save_project(&self, project: &Project) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO projects (id, updated_at, value_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at, value_json = excluded.value_json",
            params![project.id.to_string(), project.updated_at.to_rfc3339(), serde_json::to_string(project)?],
        )?;
        Ok(())
    }

    pub fn list_projects(&self) -> AdapterResult<Vec<Project>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare("SELECT value_json FROM projects ORDER BY updated_at DESC, id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn get_project(&self, id: Uuid) -> AdapterResult<Option<Project>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare("SELECT value_json FROM projects WHERE id = ?1")?;
        let mut rows = statement.query([id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&row.get::<_, String>(0)?)?))
    }

    pub fn save_conversation(&self, conversation: &Conversation) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO conversations (id, project_id, updated_at, value_json) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id, updated_at = excluded.updated_at, value_json = excluded.value_json",
            params![conversation.id.to_string(), conversation.project_id.to_string(), conversation.updated_at.to_rfc3339(), serde_json::to_string(conversation)?],
        )?;
        Ok(())
    }

    pub fn conversations_for_project(&self, project_id: Uuid) -> AdapterResult<Vec<Conversation>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare("SELECT value_json FROM conversations WHERE project_id = ?1 ORDER BY updated_at DESC, id")?;
        let rows = statement.query_map([project_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn save_message(&self, message: &Message) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO messages (id, project_id, conversation_id, sequence, value_json) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET sequence = excluded.sequence, value_json = excluded.value_json",
            params![message.id.to_string(), message.project_id.to_string(), message.conversation_id.to_string(), message.sequence, serde_json::to_string(message)?],
        )?;
        Ok(())
    }

    pub fn messages_for_conversation(&self, conversation_id: Uuid) -> AdapterResult<Vec<Message>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare(
            "SELECT value_json FROM messages WHERE conversation_id = ?1 ORDER BY sequence, id",
        )?;
        let rows =
            statement.query_map([conversation_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn append_agent_event(&self, event: &AgentEvent) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO agent_events (turn_id, sequence, value_json) VALUES (?1, ?2, ?3)",
            params![
                event.turn_id.to_string(),
                event.sequence,
                serde_json::to_string(event)?
            ],
        )?;
        Ok(())
    }

    pub fn agent_events_for_turn(&self, turn_id: Uuid) -> AdapterResult<Vec<AgentEvent>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection
            .prepare("SELECT value_json FROM agent_events WHERE turn_id = ?1 ORDER BY sequence")?;
        let rows = statement.query_map([turn_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn save_agent_turn(&self, turn: &AgentTurn) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO agent_turns (id, project_id, conversation_id, value_json) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET value_json = excluded.value_json",
            params![turn.id.to_string(), turn.project_id.to_string(), turn.conversation_id.to_string(), serde_json::to_string(turn)?],
        )?;
        Ok(())
    }

    pub fn agent_turns_for_conversation(
        &self,
        conversation_id: Uuid,
    ) -> AdapterResult<Vec<AgentTurn>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection
            .prepare("SELECT value_json FROM agent_turns WHERE conversation_id = ?1 ORDER BY id")?;
        let rows =
            statement.query_map([conversation_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn save_notebook_entry(&self, entry: &NotebookEntry) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO notebook_entries (id, project_id, value_json) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET value_json = excluded.value_json",
            params![entry.id.to_string(), entry.project_id.to_string(), serde_json::to_string(entry)?],
        )?;
        Ok(())
    }

    pub fn notebook_for_project(&self, project_id: Uuid) -> AdapterResult<Vec<NotebookEntry>> {
        self.project_json_rows("notebook_entries", project_id)
    }

    pub fn save_artifact_v3(&self, artifact: &Artifact) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO artifacts_v3 (id, project_id, value_json) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET value_json = excluded.value_json",
            params![artifact.id.to_string(), artifact.project_id.to_string(), serde_json::to_string(artifact)?],
        )?;
        Ok(())
    }

    pub fn artifacts_for_project(&self, project_id: Uuid) -> AdapterResult<Vec<Artifact>> {
        self.project_json_rows("artifacts_v3", project_id)
    }

    pub fn save_sync_entry(&self, entry: &SyncEntry) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO sync_entries (id, project_id, value_json) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET value_json = excluded.value_json",
            params![entry.id.to_string(), entry.project_id.to_string(), serde_json::to_string(entry)?],
        )?;
        Ok(())
    }

    pub fn sync_entries_for_project(&self, project_id: Uuid) -> AdapterResult<Vec<SyncEntry>> {
        self.project_json_rows("sync_entries", project_id)
    }

    pub fn save_model_profile(&self, profile: &ModelProfile) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO model_profiles (id, value_json) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET value_json = excluded.value_json",
            params![profile.id.to_string(), serde_json::to_string(profile)?],
        )?;
        Ok(())
    }

    pub fn list_model_profiles(&self) -> AdapterResult<Vec<ModelProfile>> {
        self.simple_json_rows("model_profiles")
    }

    pub fn get_model_profile(&self, id: Uuid) -> AdapterResult<Option<ModelProfile>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare("SELECT value_json FROM model_profiles WHERE id = ?1")?;
        let mut rows = statement.query([id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&row.get::<_, String>(0)?)?))
    }

    pub fn save_skill_package(&self, skill: &SkillPackage) -> AdapterResult<()> {
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO skill_packages (id, value_json) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET value_json = excluded.value_json",
            params![skill.id.to_string(), serde_json::to_string(skill)?],
        )?;
        Ok(())
    }

    pub fn list_skill_packages(&self) -> AdapterResult<Vec<SkillPackage>> {
        self.simple_json_rows("skill_packages")
    }

    pub fn delete_skill_package(&self, id: Uuid) -> AdapterResult<()> {
        self.connection
            .lock()
            .expect("database lock")
            .execute("DELETE FROM skill_packages WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    fn project_json_rows<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        project_id: Uuid,
    ) -> AdapterResult<Vec<T>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare(&format!(
            "SELECT value_json FROM {table} WHERE project_id = ?1 ORDER BY id"
        ))?;
        let rows = statement.query_map([project_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    fn simple_json_rows<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
    ) -> AdapterResult<Vec<T>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare(&format!("SELECT value_json FROM {table} ORDER BY id"))?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
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
