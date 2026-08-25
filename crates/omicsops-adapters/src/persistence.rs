use std::sync::{Arc, Mutex};

use omicsops_core::{
    domain::ConnectionProfile,
    workspace::{
        Artifact, Conversation, Message, MessageRole, ModelProfile, NotebookEntry, Project,
        SkillPackage, SyncEntry,
    },
};
use omicsops_protocol::{
    AgentEventV4, ContextArchiveV4, ContextCheckpointV4, deserialize_event_chain_v4,
};
use omicsops_science::ScientificStateV4;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{AdapterError, AdapterResult};

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
            CREATE TABLE IF NOT EXISTS agent_run_events_v3 (
                run_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                previous_hash TEXT NOT NULL,
                event_hash TEXT NOT NULL UNIQUE,
                value_json TEXT NOT NULL,
                PRIMARY KEY(run_id, sequence)
            );
            CREATE TABLE IF NOT EXISTS agent_run_snapshots_v3 (
                run_id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                last_sequence INTEGER NOT NULL,
                last_event_hash TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS agent_runs_v4 (
                run_id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                status TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS agent_events_v4 (
                run_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                conversation_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                previous_hash TEXT NOT NULL,
                event_hash TEXT NOT NULL UNIQUE,
                value_json TEXT NOT NULL,
                PRIMARY KEY(run_id, sequence)
            );
            CREATE TABLE IF NOT EXISTS agent_context_archives_v4 (
                archive_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                through_sequence INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                sha256 TEXT NOT NULL,
                transcript_json TEXT NOT NULL,
                checkpoint_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS scientific_states_v4 (
                project_id TEXT PRIMARY KEY,
                revision INTEGER NOT NULL,
                state_sha256 TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS scientific_datasets_v4 (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                active INTEGER NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS scientific_analyses_v4 (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                status TEXT NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS scientific_artifacts_v4 (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                producer_analysis_id TEXT NOT NULL,
                valid INTEGER NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS scientific_evidence_v4 (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                valid INTEGER NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS scientific_provenance_v4 (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                run_id TEXT NOT NULL,
                analysis_id TEXT NOT NULL,
                complete INTEGER NOT NULL,
                value_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS conversations_project_updated ON conversations(project_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS agent_run_events_v3_project_conversation
                ON agent_run_events_v3(project_id, conversation_id, run_id, sequence);
            CREATE INDEX IF NOT EXISTS agent_run_snapshots_v3_project_conversation
                ON agent_run_snapshots_v3(project_id, conversation_id, run_id);
            CREATE INDEX IF NOT EXISTS agent_events_v4_project_conversation
                ON agent_events_v4(project_id, conversation_id, run_id, sequence);
            CREATE INDEX IF NOT EXISTS agent_context_archives_v4_run
                ON agent_context_archives_v4(run_id, through_sequence);
            PRAGMA user_version = 3;
            ",
        )?;
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

    pub fn agent_run_ids_v4_for_project(&self, project_id: Uuid) -> AdapterResult<Vec<Uuid>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare("SELECT run_id FROM agent_runs_v4 WHERE project_id = ?1")?;
        let rows = statement.query_map([project_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            Uuid::parse_str(&row?).map_err(|error| {
                AdapterError::InvalidInput(format!("invalid V4 run id in database: {error}"))
            })
        })
        .collect()
    }

    pub fn delete_project(&self, project_id: Uuid) -> AdapterResult<bool> {
        let mut connection = self.connection.lock().expect("database lock");
        let transaction = connection.transaction()?;
        let project_id = project_id.to_string();
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            [&project_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            transaction.rollback()?;
            return Ok(false);
        }

        transaction.execute(
            "DELETE FROM agent_context_archives_v4 WHERE run_id IN (SELECT run_id FROM agent_runs_v4 WHERE project_id = ?1)",
            [&project_id],
        )?;
        for table in [
            "scientific_provenance_v4",
            "scientific_evidence_v4",
            "scientific_artifacts_v4",
            "scientific_analyses_v4",
            "scientific_datasets_v4",
            "scientific_states_v4",
        ] {
            transaction.execute(
                &format!("DELETE FROM {table} WHERE project_id = ?1"),
                [&project_id],
            )?;
        }
        transaction.execute(
            "DELETE FROM agent_events_v4 WHERE project_id = ?1",
            [&project_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_runs_v4 WHERE project_id = ?1",
            [&project_id],
        )?;
        for table in [
            "messages",
            "artifacts_v3",
            "notebook_entries",
            "sync_entries",
            "conversations",
        ] {
            transaction.execute(
                &format!("DELETE FROM {table} WHERE project_id = ?1"),
                [&project_id],
            )?;
        }
        transaction.execute("DELETE FROM projects WHERE id = ?1", [&project_id])?;
        transaction.commit()?;
        Ok(true)
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

    pub fn delete_conversation(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> AdapterResult<bool> {
        let mut connection = self.connection.lock().expect("database lock");
        let transaction = connection.transaction()?;
        let project_id = project_id.to_string();
        let conversation_id = conversation_id.to_string();
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1 AND project_id = ?2)",
            params![conversation_id, project_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            transaction.rollback()?;
            return Ok(false);
        }
        transaction.execute(
            "DELETE FROM agent_context_archives_v4 WHERE run_id IN (SELECT run_id FROM agent_runs_v4 WHERE project_id = ?1 AND conversation_id = ?2)",
            params![project_id, conversation_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_events_v4 WHERE project_id = ?1 AND conversation_id = ?2",
            params![project_id, conversation_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_runs_v4 WHERE project_id = ?1 AND conversation_id = ?2",
            params![project_id, conversation_id],
        )?;
        transaction.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            [&conversation_id],
        )?;
        transaction.execute(
            "DELETE FROM conversations WHERE id = ?1 AND project_id = ?2",
            params![conversation_id, project_id],
        )?;
        transaction.commit()?;
        Ok(true)
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

    pub fn save_agent_run_v4(
        &self,
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        status: &str,
        value: &serde_json::Value,
    ) -> AdapterResult<()> {
        self.connection.lock().expect("repository lock").execute(
            "INSERT INTO agent_runs_v4 (run_id, project_id, conversation_id, status, value_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(run_id) DO UPDATE SET status=excluded.status, value_json=excluded.value_json",
            params![run_id.to_string(), project_id.to_string(), conversation_id.to_string(), status, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    pub fn agent_run_v4(&self, run_id: Uuid) -> AdapterResult<Option<serde_json::Value>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement =
            connection.prepare("SELECT value_json FROM agent_runs_v4 WHERE run_id=?1")?;
        let mut rows = statement.query([run_id.to_string()])?;
        rows.next()?
            .map(|row| {
                let value: String = row.get(0)?;
                serde_json::from_str(&value).map_err(Into::into)
            })
            .transpose()
    }

    pub fn agent_runs_for_context_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> AdapterResult<Vec<serde_json::Value>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement = connection.prepare(
            "SELECT value_json FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2 ORDER BY rowid",
        )?;
        statement
            .query_map(
                params![project_id.to_string(), conversation_id.to_string()],
                |row| row.get::<_, String>(0),
            )?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect()
    }

    /// Append a V4 event and, for a successful completion, persist its public
    /// assistant answer in the same SQLite transaction.
    ///
    /// The returned message is intended for the UI event emitter.  Keeping the
    /// message creation in the repository transaction means a completed run
    /// can never be observed without its answer (or vice versa).
    pub fn append_agent_event_v4_with_conversation(
        &self,
        event: &AgentEventV4,
    ) -> AdapterResult<Option<Message>> {
        event
            .verify()
            .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;
        let mut connection = self.connection.lock().expect("repository lock");
        let transaction = connection.transaction()?;
        let mut statement = transaction
            .prepare("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence")?;
        let serialized = statement
            .query_map([event.run_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let existing = deserialize_event_chain_v4(&serialized)
            .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;

        // Replaying an already durable event is safe.  If an old process
        // wrote RunCompleted without its message, repair the missing message
        // here using the same transaction.
        if let Some(stored) = existing
            .iter()
            .find(|stored| stored.sequence == event.sequence)
        {
            if stored.event_hash != event.event_hash || stored != event {
                return Err(crate::AdapterError::InvalidInput(format!(
                    "event sequence {} is already occupied by a different event",
                    event.sequence
                )));
            }
            if matches!(
                event.event,
                omicsops_protocol::AgentEventKindV4::RunCompleted
            ) {
                let message = persist_completion_message(&transaction, &existing, event)?;
                transaction.commit()?;
                return Ok(message);
            }
            transaction.commit()?;
            return Ok(None);
        }

        let chain_continues = existing.last().map_or_else(
            || event.sequence == 1 && event.previous_hash.is_empty(),
            |previous| {
                event.sequence == previous.sequence + 1
                    && event.previous_hash == previous.event_hash
            },
        );
        if !chain_continues {
            return Err(crate::AdapterError::InvalidInput(
                "broken V4 event chain".into(),
            ));
        }
        let mut candidate = existing;
        candidate.push(event.clone());
        transaction.execute(
            "INSERT INTO agent_events_v4 (run_id, project_id, conversation_id, sequence, previous_hash, event_hash, value_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![event.run_id.to_string(), event.project_id.to_string(), event.conversation_id.to_string(), event.sequence, event.previous_hash, event.event_hash, serde_json::to_string(event)?],
        )?;
        let message = if matches!(
            event.event,
            omicsops_protocol::AgentEventKindV4::RunCompleted
        ) {
            persist_completion_message(&transaction, &candidate, event)?
        } else {
            None
        };
        transaction.commit()?;
        Ok(message)
    }

    /// Compatibility wrapper for callers that only need the event store.
    pub fn append_agent_event_v4(&self, event: &AgentEventV4) -> AdapterResult<()> {
        self.append_agent_event_v4_with_conversation(event)
            .map(|_| ())
    }

    pub fn agent_events_v4(&self, run_id: Uuid) -> AdapterResult<Vec<AgentEventV4>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement = connection
            .prepare("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence")?;
        let serialized = statement
            .query_map([run_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        deserialize_event_chain_v4(&serialized)
            .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))
    }

    pub fn agent_events_for_context_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> AdapterResult<Vec<AgentEventV4>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement = connection.prepare("SELECT run_id, value_json FROM agent_events_v4 WHERE project_id=?1 AND conversation_id=?2 ORDER BY run_id, sequence")?;
        let rows = statement
            .query_map(
                params![project_id.to_string(), conversation_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut events = Vec::new();
        let mut offset = 0;
        while offset < rows.len() {
            let run_id = &rows[offset].0;
            let end = rows[offset..]
                .iter()
                .position(|(candidate, _)| candidate != run_id)
                .map(|relative| offset + relative)
                .unwrap_or(rows.len());
            let serialized = rows[offset..end]
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>();
            events.extend(
                deserialize_event_chain_v4(&serialized)
                    .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?,
            );
            offset = end;
        }
        Ok(events)
    }

    pub fn archive_agent_context_v4(
        &self,
        run_id: Uuid,
        transcript: &str,
        checkpoint: &ContextCheckpointV4,
    ) -> AdapterResult<ContextArchiveV4> {
        let archive = ContextArchiveV4 {
            archive_id: Uuid::new_v4(),
            through_sequence: checkpoint.through_sequence,
            size_bytes: transcript.len() as u64,
            sha256: hex::encode(Sha256::digest(transcript.as_bytes())),
        };
        self.connection.lock().expect("repository lock").execute(
            "INSERT INTO agent_context_archives_v4 (archive_id, run_id, through_sequence, size_bytes, sha256, transcript_json, checkpoint_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![archive.archive_id.to_string(), run_id.to_string(), archive.through_sequence, archive.size_bytes, archive.sha256, transcript, serde_json::to_string(checkpoint)?],
        )?;
        Ok(archive)
    }

    pub fn agent_context_archive_v4(
        &self,
        archive_id: Uuid,
    ) -> AdapterResult<Option<(String, ContextCheckpointV4)>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement = connection.prepare(
            "SELECT transcript_json, checkpoint_json FROM agent_context_archives_v4 WHERE archive_id=?1",
        )?;
        let mut rows = statement.query([archive_id.to_string()])?;
        rows.next()?
            .map(|row| {
                let transcript: String = row.get(0)?;
                let checkpoint: String = row.get(1)?;
                Ok((transcript, serde_json::from_str(&checkpoint)?))
            })
            .transpose()
    }

    pub fn save_scientific_state_v4(&self, state: &ScientificStateV4) -> AdapterResult<()> {
        let mut connection = self.connection.lock().expect("repository lock");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO scientific_states_v4 (project_id, revision, state_sha256, value_json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(project_id) DO UPDATE SET revision=excluded.revision, state_sha256=excluded.state_sha256, value_json=excluded.value_json",
            params![state.project_id.to_string(), state.revision, state.digest(), serde_json::to_string(state)?],
        )?;
        for dataset in state.datasets.values() {
            transaction.execute(
                "INSERT INTO scientific_datasets_v4 (id, project_id, active, value_json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET active=excluded.active, value_json=excluded.value_json",
                params![dataset.id.to_string(), state.project_id.to_string(), dataset.active, serde_json::to_string(dataset)?],
            )?;
        }
        for analysis in state.analyses.values() {
            transaction.execute(
                "INSERT INTO scientific_analyses_v4 (id, project_id, status, value_json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET status=excluded.status, value_json=excluded.value_json",
                params![analysis.id.to_string(), state.project_id.to_string(), format!("{:?}", analysis.status).to_ascii_lowercase(), serde_json::to_string(analysis)?],
            )?;
        }
        for artifact in state.artifacts.values() {
            transaction.execute(
                "INSERT INTO scientific_artifacts_v4 (id, project_id, producer_analysis_id, valid, value_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET valid=excluded.valid, value_json=excluded.value_json",
                params![artifact.id.to_string(), state.project_id.to_string(), artifact.producer_analysis_id.to_string(), artifact.valid, serde_json::to_string(artifact)?],
            )?;
        }
        for evidence in state.evidence.values() {
            transaction.execute(
                "INSERT INTO scientific_evidence_v4 (id, project_id, valid, value_json) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(id) DO UPDATE SET valid=excluded.valid, value_json=excluded.value_json",
                params![evidence.id.to_string(), state.project_id.to_string(), evidence.valid, serde_json::to_string(evidence)?],
            )?;
        }
        for manifest in state.provenance.values() {
            transaction.execute(
                "INSERT INTO scientific_provenance_v4 (id, project_id, run_id, analysis_id, complete, value_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET complete=excluded.complete, value_json=excluded.value_json",
                params![manifest.id.to_string(), state.project_id.to_string(), manifest.run_id.to_string(), manifest.analysis_id.to_string(), manifest.complete, serde_json::to_string(manifest)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn scientific_state_v4(
        &self,
        project_id: Uuid,
    ) -> AdapterResult<Option<ScientificStateV4>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement = connection
            .prepare("SELECT value_json FROM scientific_states_v4 WHERE project_id=?1")?;
        let mut rows = statement.query([project_id.to_string()])?;
        rows.next()?
            .map(|row| {
                let value: String = row.get(0)?;
                serde_json::from_str(&value).map_err(Into::into)
            })
            .transpose()
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

fn completion_answer(events: &[AgentEventV4]) -> AdapterResult<String> {
    let proposal = events
        .iter()
        .rev()
        .find_map(|event| match &event.event {
            omicsops_protocol::AgentEventKindV4::CompletionProposalSubmitted { proposal } => {
                Some(proposal)
            }
            _ => None,
        })
        .ok_or_else(|| {
            crate::AdapterError::InvalidInput(
                "RunCompleted has no persisted CompletionProposalSubmitted event".into(),
            )
        })?;

    // Read answer_markdown dynamically so repositories can replay pre-answer
    // protocol records while the protocol crate evolves.  Legacy records use
    // summary as their public answer.
    let value = serde_json::to_value(proposal)?;
    let answer = value
        .get("answer_markdown")
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            value
                .get("summary")
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.trim().is_empty())
        })
        .map(str::trim)
        .unwrap_or_default();
    if answer.is_empty() {
        return Err(crate::AdapterError::InvalidInput(
            "RunCompleted cannot be persisted without a non-empty assistant answer".into(),
        ));
    }
    Ok(answer.to_owned())
}

fn persist_completion_message(
    transaction: &rusqlite::Transaction<'_>,
    events: &[AgentEventV4],
    completed: &AgentEventV4,
) -> AdapterResult<Option<Message>> {
    let markdown = completion_answer(events)?;
    let message_id = completed.run_id.to_string();

    let existing = transaction
        .query_row(
            "SELECT value_json FROM messages WHERE id=?1",
            [&message_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(value) = existing {
        let message: Message = serde_json::from_str(&value)?;
        if message.project_id != completed.project_id
            || message.conversation_id != completed.conversation_id
            || message.role != MessageRole::Assistant
            || message.markdown != markdown
        {
            return Err(crate::AdapterError::InvalidInput(
                "run completion message id is already used by a different message".into(),
            ));
        }
        // The event replay is idempotent; only a newly inserted (or repaired)
        // message should produce a second UI conversation event.
        return Ok(None);
    }

    let sequence = transaction.query_row(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE conversation_id=?1",
        [completed.conversation_id.to_string()],
        |row| row.get::<_, i64>(0),
    )?;
    let sequence = u64::try_from(sequence).map_err(|_| {
        crate::AdapterError::InvalidInput("conversation message sequence overflow".into())
    })?;
    let message = Message::markdown(
        completed.run_id,
        completed.project_id,
        completed.conversation_id,
        sequence,
        MessageRole::Assistant,
        markdown,
        completed.occurred_at,
    );
    transaction.execute(
        "INSERT INTO messages (id, project_id, conversation_id, sequence, value_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            message.id.to_string(),
            message.project_id.to_string(),
            message.conversation_id.to_string(),
            message.sequence,
            serde_json::to_string(&message)?
        ],
    )?;
    Ok(Some(message))
}
