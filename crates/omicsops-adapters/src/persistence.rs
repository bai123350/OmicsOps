use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use omicsops_agent::{
    AgentEvent,
    harness_v3::{AgentRunEventV3, AgentSnapshotV3, validate_event_chain},
};
use omicsops_core::{
    audit::RunEventV2,
    domain::{AnalysisPlan, ConnectionProfile, RunEvent},
    plan_v2::{ApprovedPlan, EnvironmentLock, StepAttempt, migrate_v1_plan},
    workspace::{
        AgentTurn, Artifact, Conversation, Message, ModelProfile, NotebookEntry, Project,
        SkillPackage, SyncEntry, TurnStatus,
    },
};
use omicsops_protocol::{
    AgentEventV4, ContextArchiveV4, ContextCheckpointV4, validate_event_chain_v4,
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
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

    pub fn run_ids_for_project(&self, project_id: Uuid) -> AdapterResult<Vec<Uuid>> {
        let project_id = project_id.to_string();
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare("SELECT value_json FROM app_objects")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut run_ids = HashSet::new();
        for row in rows {
            let value = serde_json::from_str::<serde_json::Value>(&row?)?;
            if value.get("project_id").and_then(|field| field.as_str()) == Some(&project_id)
                && let Some(run_id) = value
                    .get("run_id")
                    .and_then(|field| field.as_str())
                    .and_then(|field| Uuid::parse_str(field).ok())
            {
                run_ids.insert(run_id);
            }
        }
        Ok(run_ids.into_iter().collect())
    }

    pub fn has_active_agent_turns(&self, project_id: Uuid) -> AdapterResult<bool> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement =
            connection.prepare("SELECT value_json FROM agent_turns WHERE project_id = ?1")?;
        let rows = statement.query_map([project_id.to_string()], |row| row.get::<_, String>(0))?;
        for row in rows {
            let turn = serde_json::from_str::<AgentTurn>(&row?)?;
            if matches!(
                turn.status,
                TurnStatus::Queued | TurnStatus::Streaming | TurnStatus::WaitingForApproval
            ) {
                return Ok(true);
            }
        }
        Ok(false)
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

        let app_objects = {
            let mut statement =
                transaction.prepare("SELECT kind, id, value_json FROM app_objects")?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let project_artifacts = {
            let mut statement =
                transaction.prepare("SELECT value_json FROM artifacts_v3 WHERE project_id = ?1")?;
            let rows = statement.query_map([&project_id], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut run_ids = HashSet::new();
        let mut plan_ids = HashSet::new();
        let mut approved_plan_ids = HashSet::new();
        let mut app_objects_to_delete = HashSet::new();
        for (kind, id, json) in &app_objects {
            let value = serde_json::from_str::<serde_json::Value>(json)?;
            let belongs_to_project = value.get("project_id").and_then(|field| field.as_str())
                == Some(&project_id)
                || (kind == "project" && id == &project_id);
            if !belongs_to_project {
                continue;
            }
            app_objects_to_delete.insert((kind.clone(), id.clone()));
            collect_uuid_field(&value, "run_id", &mut run_ids);
            collect_uuid_field(&value, "plan_id", &mut plan_ids);
            collect_uuid_field(&value, "approved_plan_id", &mut approved_plan_ids);
        }
        for json in project_artifacts {
            let value = serde_json::from_str::<serde_json::Value>(&json)?;
            collect_uuid_field(&value, "run_id", &mut run_ids);
        }

        for approved_plan_id in &approved_plan_ids {
            let plan_json = transaction
                .query_row(
                    "SELECT approved_plan_json FROM approved_plans WHERE id = ?1",
                    [approved_plan_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            if let Some(plan_json) = plan_json {
                let value = serde_json::from_str::<serde_json::Value>(&plan_json)?;
                collect_uuid_field(&value, "plan_id", &mut plan_ids);
            }
        }

        for (kind, id, json) in &app_objects {
            let value = serde_json::from_str::<serde_json::Value>(json)?;
            let linked_run = value
                .get("run_id")
                .and_then(|field| field.as_str())
                .and_then(|field| Uuid::parse_str(field).ok())
                .is_some_and(|run_id| run_ids.contains(&run_id));
            let linked_plan = kind.starts_with("analysis_plan")
                && Uuid::parse_str(id)
                    .ok()
                    .is_some_and(|plan_id| plan_ids.contains(&plan_id));
            if linked_run || linked_plan {
                app_objects_to_delete.insert((kind.clone(), id.clone()));
            }
        }

        for run_id in &run_ids {
            let run_id = run_id.to_string();
            transaction.execute("DELETE FROM run_events WHERE run_id = ?1", [&run_id])?;
            transaction.execute("DELETE FROM audit_events_v2 WHERE run_id = ?1", [&run_id])?;
            transaction.execute("DELETE FROM step_attempts_v2 WHERE run_id = ?1", [&run_id])?;
            transaction.execute(
                "DELETE FROM environment_locks_v2 WHERE run_id = ?1",
                [&run_id],
            )?;
        }
        for approved_plan_id in &approved_plan_ids {
            transaction.execute(
                "DELETE FROM approved_plans WHERE id = ?1",
                [approved_plan_id.to_string()],
            )?;
        }
        for (kind, id) in app_objects_to_delete {
            transaction.execute(
                "DELETE FROM app_objects WHERE kind = ?1 AND id = ?2",
                params![kind, id],
            )?;
        }

        transaction.execute(
            "DELETE FROM agent_events WHERE turn_id IN (SELECT id FROM agent_turns WHERE project_id = ?1)",
            [&project_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_run_events_v3 WHERE project_id = ?1",
            [&project_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_run_snapshots_v3 WHERE project_id = ?1",
            [&project_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_context_archives_v4 WHERE run_id IN (SELECT run_id FROM agent_runs_v4 WHERE project_id = ?1)",
            [&project_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_events_v4 WHERE project_id = ?1",
            [&project_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_runs_v4 WHERE project_id = ?1",
            [&project_id],
        )?;
        transaction.execute(
            "DELETE FROM tool_calls WHERE turn_id IN (SELECT id FROM agent_turns WHERE project_id = ?1)",
            [&project_id],
        )?;
        for table in [
            "agent_turns",
            "messages",
            "approvals_v3",
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
            "DELETE FROM agent_events WHERE turn_id IN (SELECT id FROM agent_turns WHERE conversation_id = ?1)",
            [&conversation_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_run_events_v3 WHERE project_id = ?1 AND conversation_id = ?2",
            params![project_id, conversation_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_run_snapshots_v3 WHERE project_id = ?1 AND conversation_id = ?2",
            params![project_id, conversation_id],
        )?;
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
            "DELETE FROM tool_calls WHERE turn_id IN (SELECT id FROM agent_turns WHERE conversation_id = ?1)",
            [&conversation_id],
        )?;
        transaction.execute(
            "DELETE FROM agent_turns WHERE conversation_id = ?1",
            [&conversation_id],
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

    pub fn append_agent_run_event_v3(&self, event: &AgentRunEventV3) -> AdapterResult<()> {
        event
            .verify_hash()
            .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;
        let mut connection = self.connection.lock().expect("database lock");
        let transaction = connection.transaction()?;
        let run_id = event.run_id.to_string();
        let previous = transaction
            .query_row(
                "SELECT sequence, event_hash, project_id, conversation_id
                 FROM agent_run_events_v3
                 WHERE run_id = ?1
                 ORDER BY sequence DESC
                 LIMIT 1",
                [&run_id],
                |row| {
                    Ok((
                        row.get::<_, u64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .ok();
        match previous {
            Some((sequence, event_hash, project_id, conversation_id)) => {
                if event.sequence != sequence + 1
                    || event.previous_hash != event_hash
                    || event.project_id.to_string() != project_id
                    || event.conversation_id.to_string() != conversation_id
                {
                    return Err(crate::AdapterError::InvalidInput(
                        "v3 event does not extend the stored run hash chain".into(),
                    ));
                }
            }
            None => {
                if event.sequence != 1
                    || event.previous_hash
                        != "0000000000000000000000000000000000000000000000000000000000000000"
                {
                    return Err(crate::AdapterError::InvalidInput(
                        "v3 run must begin at the genesis event".into(),
                    ));
                }
            }
        }
        transaction.execute(
            "INSERT INTO agent_run_events_v3
                (run_id, project_id, conversation_id, sequence, previous_hash, event_hash, value_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run_id,
                event.project_id.to_string(),
                event.conversation_id.to_string(),
                event.sequence,
                event.previous_hash,
                event.event_hash,
                serde_json::to_string(event)?
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn agent_run_events_v3(&self, run_id: Uuid) -> AdapterResult<Vec<AgentRunEventV3>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection.prepare(
            "SELECT value_json FROM agent_run_events_v3
             WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([run_id.to_string()], |row| row.get::<_, String>(0))?;
        let events = rows
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<AdapterResult<Vec<AgentRunEventV3>>>()?;
        if !events.is_empty() {
            validate_event_chain(&events)
                .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;
        }
        Ok(events)
    }

    pub fn agent_run_events_for_context_v3(
        &self,
        project_id: Uuid,
        conversation_id: Option<Uuid>,
    ) -> AdapterResult<Vec<AgentRunEventV3>> {
        let connection = self.connection.lock().expect("database lock");
        let mut events = if let Some(conversation_id) = conversation_id {
            let mut statement = connection.prepare(
                "SELECT value_json FROM agent_run_events_v3
                 WHERE project_id = ?1 AND conversation_id = ?2
                 ORDER BY run_id, sequence",
            )?;
            let rows = statement.query_map(
                params![project_id.to_string(), conversation_id.to_string()],
                |row| row.get::<_, String>(0),
            )?;
            rows.map(|row| Ok(serde_json::from_str(&row?)?))
                .collect::<AdapterResult<Vec<AgentRunEventV3>>>()?
        } else {
            let mut statement = connection.prepare(
                "SELECT value_json FROM agent_run_events_v3
                 WHERE project_id = ?1
                 ORDER BY run_id, sequence",
            )?;
            let rows =
                statement.query_map([project_id.to_string()], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(serde_json::from_str(&row?)?))
                .collect::<AdapterResult<Vec<AgentRunEventV3>>>()?
        };
        for run in events.chunk_by(|left, right| left.run_id == right.run_id) {
            validate_event_chain(run)
                .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;
        }
        events.sort_by_key(|event| (event.occurred_at, event.sequence));
        Ok(events)
    }

    pub fn save_agent_snapshot_v3(&self, snapshot: &AgentSnapshotV3) -> AdapterResult<()> {
        if snapshot.state.run_id != snapshot.run_id
            || snapshot.state.last_sequence != snapshot.last_sequence
            || snapshot.state.last_event_hash != snapshot.last_event_hash
        {
            return Err(crate::AdapterError::InvalidInput(
                "v3 snapshot state does not match its cache boundary".into(),
            ));
        }
        let connection = self.connection.lock().expect("database lock");
        let boundary_matches = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM agent_run_events_v3
                WHERE run_id = ?1 AND project_id = ?2 AND conversation_id = ?3
                  AND sequence = ?4 AND event_hash = ?5
            )",
            params![
                snapshot.run_id.to_string(),
                snapshot.project_id.to_string(),
                snapshot.conversation_id.to_string(),
                snapshot.last_sequence,
                snapshot.last_event_hash,
            ],
            |row| row.get::<_, bool>(0),
        )?;
        if !boundary_matches {
            return Err(crate::AdapterError::InvalidInput(
                "v3 snapshot is not bound to a verified stored event".into(),
            ));
        }
        connection.execute(
            "INSERT INTO agent_run_snapshots_v3
                (run_id, project_id, conversation_id, last_sequence, last_event_hash, value_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(run_id) DO UPDATE SET
                project_id = excluded.project_id,
                conversation_id = excluded.conversation_id,
                last_sequence = excluded.last_sequence,
                last_event_hash = excluded.last_event_hash,
                value_json = excluded.value_json
             WHERE excluded.last_sequence >= agent_run_snapshots_v3.last_sequence",
            params![
                snapshot.run_id.to_string(),
                snapshot.project_id.to_string(),
                snapshot.conversation_id.to_string(),
                snapshot.last_sequence,
                snapshot.last_event_hash,
                serde_json::to_string(snapshot)?,
            ],
        )?;
        Ok(())
    }

    pub fn agent_snapshot_v3(&self, run_id: Uuid) -> AdapterResult<Option<AgentSnapshotV3>> {
        let connection = self.connection.lock().expect("database lock");
        let mut statement = connection
            .prepare("SELECT value_json FROM agent_run_snapshots_v3 WHERE run_id = ?1")?;
        let mut rows = statement.query([run_id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&row.get::<_, String>(0)?)?))
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

    pub fn append_agent_event_v4(&self, event: &AgentEventV4) -> AdapterResult<()> {
        event
            .verify()
            .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;
        let existing = self.agent_events_v4(event.run_id)?;
        let mut candidate = existing;
        candidate.push(event.clone());
        validate_event_chain_v4(&candidate)
            .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;
        self.connection.lock().expect("repository lock").execute(
            "INSERT INTO agent_events_v4 (run_id, project_id, conversation_id, sequence, previous_hash, event_hash, value_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![event.run_id.to_string(), event.project_id.to_string(), event.conversation_id.to_string(), event.sequence, event.previous_hash, event.event_hash, serde_json::to_string(event)?],
        )?;
        Ok(())
    }

    pub fn agent_events_v4(&self, run_id: Uuid) -> AdapterResult<Vec<AgentEventV4>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement = connection
            .prepare("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence")?;
        let events = statement
            .query_map([run_id.to_string()], |row| row.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str::<AgentEventV4>(&row?)?))
            .collect::<AdapterResult<Vec<_>>>()?;
        validate_event_chain_v4(&events)
            .map_err(|error| crate::AdapterError::InvalidInput(error.to_string()))?;
        Ok(events)
    }

    pub fn agent_events_for_context_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> AdapterResult<Vec<AgentEventV4>> {
        let connection = self.connection.lock().expect("repository lock");
        let mut statement = connection.prepare("SELECT value_json FROM agent_events_v4 WHERE project_id=?1 AND conversation_id=?2 ORDER BY run_id, sequence")?;
        statement
            .query_map(
                params![project_id.to_string(), conversation_id.to_string()],
                |row| row.get::<_, String>(0),
            )?
            .map(|row| Ok(serde_json::from_str::<AgentEventV4>(&row?)?))
            .collect()
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

fn collect_uuid_field(value: &serde_json::Value, field: &str, destination: &mut HashSet<Uuid>) {
    if let Some(id) = value
        .get(field)
        .and_then(|field| field.as_str())
        .and_then(|field| Uuid::parse_str(field).ok())
    {
        destination.insert(id);
    }
}
