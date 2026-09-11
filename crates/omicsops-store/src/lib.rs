//! Async SQLite persistence for OmicsOps.
//!
//! `Store` is the sole owner of the SQLite connection pool. Desktop commands
//! and persistence-aware orchestration use its native async API directly.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use omicsops_core::{
    domain::ConnectionProfile,
    workspace::{
        Artifact, Conversation, Message, MessageRole, ModelProfile, NotebookEntry, Project,
        SkillPackage, SyncEntry,
    },
};
use omicsops_dto::{PlanRevisionStatusV4, ProposedPlanRevisionV4, SessionAgentModeV4};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, BrowserApprovalBindingV4, BrowserApprovalScopeV4,
    BrowserAuthorizationV4, BrowserSessionKindV4, ComputeSelectionV4, ContextArchiveV4,
    ContextCheckpointV4, ExecutionPlanV4, PlanApprovalScopeV4, RunModeV4, RunSpecV4,
    ToolApprovalDecisionV4, ToolApprovalRequestV4, ToolEffectV4, deserialize_event_chain_v4,
};
use omicsops_science::ScientificStateV4;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePoolOptions},
};
use thiserror::Error;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 4;
mod guidance;
mod runtime_jobs;
const INIT_SQL: &str = include_str!("../migrations/init.sql");
const SETTINGS_GLOBAL_SCOPE: &str = "global";
const CONVERSATION_AGENT_MODE_SETTING_PREFIX: &str = "conversation_agent_mode:";
const PROPOSED_PLAN_TRIGGER_SQL: &str = r#"
CREATE TRIGGER trg_proposed_plans_immutable_content
BEFORE UPDATE OF status,id,project_id,frame_id,revision,plan_hash,plan_json,markdown,run_id,created_at
ON proposed_plans
WHEN NOT (
    NEW.id IS OLD.id
    AND NEW.project_id IS OLD.project_id
    AND NEW.frame_id IS OLD.frame_id
    AND NEW.revision IS OLD.revision
    AND NEW.plan_hash IS OLD.plan_hash
    AND NEW.plan_json IS OLD.plan_json
    AND NEW.markdown IS OLD.markdown
    AND NEW.run_id IS OLD.run_id
    AND NEW.created_at IS OLD.created_at
    AND (
        NEW.status IS OLD.status
        OR (OLD.status = 'generating' AND NEW.status IN ('revising','cancelled','superseded'))
        OR (OLD.status = 'revising' AND NEW.status IN ('generating','cancelled','superseded'))
        OR (OLD.status = 'pending' AND NEW.status IN ('approved','revising','cancelled','superseded'))
        OR (OLD.status = 'approved' AND NEW.status = 'superseded')
    )
)
BEGIN
    SELECT RAISE(ABORT, 'proposed plan revision content is immutable');
END
"#;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("accepted guidance is pending")]
    GuidancePending,
    #[error("database failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("migration failed: {0}")]
    Migration(String),
}

/// Optional deterministic migration fault injection used by tests and by
/// recovery diagnostics.  A value of `Some(0)` fails before the first legacy
/// row is copied; the transaction then rolls back all DDL and data writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MigrationOptions {
    pub fail_after_rows: Option<usize>,
}

/// Optional deterministic failure injection for the plan approval transaction.
/// It is intentionally public so Store integration tests can prove that no
/// partial approval, mode change, or event is visible after a rollback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApprovalOptionsV4 {
    pub fail_after_step: Option<u8>,
}

/// Optional deterministic failure injection for plan-revision request tests.
/// The fault is raised after lifecycle metadata is changed but before the
/// request event is inserted, proving that both writes share one transaction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanRevisionRequestOptionsV4 {
    pub fail_after_step: Option<u8>,
}

/// Fields that are known only after the planning model returns. They are
/// persisted by the same transaction that materializes the generating row so
/// no later whole-run snapshot can roll them back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanRevisionFinalizeOptionsV4 {
    pub approval_hash: Option<String>,
    pub compute_selection: Option<ComputeSelectionV4>,
}

/// The durable result of requesting changes to a plan revision. The event is
/// returned for post-commit Tauri broadcasting; it is inserted by the same
/// Store transaction as the feedback/status transition.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanRevisionRequestResultV4 {
    pub revision: ProposedPlanRevisionV4,
    pub event: AgentEventV4,
}

/// The durable result of an atomic plan approval. The returned events are
/// emitted by the Tauri adapter only after the transaction commits.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanApprovalResultV4 {
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub revision: u64,
    pub plan_hash: String,
    pub approval_hash: Option<String>,
    pub spec_hash: Option<String>,
    pub mode: SessionAgentModeV4,
    pub events: Vec<AgentEventV4>,
}

/// The durable result of cancelling a pending/revising plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanCancellationResultV4 {
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub revision: u64,
    pub mode: SessionAgentModeV4,
    pub events: Vec<AgentEventV4>,
}

/// A coherent, read-only view of one conversation's durable Agent state.
///
/// `latest_run_json` deliberately remains untyped at the Store boundary. The
/// desktop command owns the Agent V4 run record contract and turns this value
/// into the shared `RunSummaryV4` DTO after validating its context.
#[derive(Debug, Clone, PartialEq)]
pub struct ConversationAgentStateSnapshotV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: SessionAgentModeV4,
    pub locked: bool,
    pub latest_plan_revision: Option<ProposedPlanRevisionV4>,
    pub latest_run_json: Option<Value>,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Open (or create) a file-backed Store and run the idempotent schema
    /// upgrade. Existing pre-v4 files are copied to
    /// `<db>.pre-store-v4.<timestamp>-<id>.bak` before their transaction
    /// begins.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_options(path, MigrationOptions::default()).await
    }

    /// Open an in-memory Store. A single pool connection is intentional:
    /// SQLite in-memory databases are connection-local.
    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::new()
            .filename(":memory:")
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        initialize(&pool, 0, MigrationOptions::default()).await?;
        Ok(Self { pool })
    }

    /// Test/recovery entry point that injects a rollback after a deterministic
    /// number of copied v3 rows.
    pub async fn open_with_migration_failure(
        path: impl AsRef<Path>,
        fail_after_rows: usize,
    ) -> Result<Self, StoreError> {
        Self::open_with_options(
            path,
            MigrationOptions {
                fail_after_rows: Some(fail_after_rows),
            },
        )
        .await
    }

    pub async fn open_with_options(
        path: impl AsRef<Path>,
        options: MigrationOptions,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let existed = path.exists();
        let version = if existed {
            read_schema_version(&path).await?
        } else {
            0
        };
        if version > SCHEMA_VERSION {
            return Err(StoreError::Migration(format!(
                "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
            )));
        }
        if existed && version < SCHEMA_VERSION {
            create_backup(&path, version)?;
        }
        let connect_options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(connect_options)
            .await?;
        if let Err(error) = initialize(&pool, version, options).await {
            pool.close().await;
            return Err(error);
        }
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn schema_version(&self) -> Result<u32, StoreError> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&self.pool)
            .await?;
        u32::try_from(version)
            .map_err(|_| StoreError::InvalidInput("invalid SQLite schema version".into()))
    }

    pub async fn foreign_keys_enabled(&self) -> Result<bool, StoreError> {
        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&self.pool)
            .await?;
        Ok(enabled == 1)
    }

    pub async fn has_table(&self, table: &str) -> Result<bool, StoreError> {
        let exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        )
        .bind(table)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists == 1)
    }

    pub async fn save_connection(&self, profile: &ConnectionProfile) -> Result<(), StoreError> {
        let json = serde_json::to_string(profile)?;
        sqlx::query(
            "INSERT INTO connections (id,label,profile_json) VALUES (?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET label=excluded.label, profile_json=excluded.profile_json",
        )
        .bind(profile.id.to_string())
        .bind(&profile.label)
        .bind(json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_connections(&self) -> Result<Vec<ConnectionProfile>, StoreError> {
        let rows = sqlx::query("SELECT profile_json FROM connections ORDER BY label,id")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .collect()
    }

    pub async fn save_project(&self, project: &Project) -> Result<(), StoreError> {
        validate_project_input(project)?;
        let mut tx = self.pool.begin().await?;
        insert_project(&mut tx, project).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let rows = sqlx::query(
            "SELECT p.id,p.name,p.description,p.workspace_dir,p.created_at,p.updated_at,
                    e.local_root,e.remote_root,e.connection_id,e.template,e.status,e.ollama_only
             FROM projects p LEFT JOIN project_omicsops e ON e.project_id=p.id
             ORDER BY p.updated_at DESC,p.id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(project_from_row).collect()
    }

    pub async fn get_project(&self, id: Uuid) -> Result<Option<Project>, StoreError> {
        let row = sqlx::query(
            "SELECT p.id,p.name,p.description,p.workspace_dir,p.created_at,p.updated_at,
                    e.local_root,e.remote_root,e.connection_id,e.template,e.status,e.ollama_only
             FROM projects p LEFT JOIN project_omicsops e ON e.project_id=p.id WHERE p.id=?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(project_from_row).transpose()
    }

    pub async fn agent_run_ids_v4_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        let rows =
            sqlx::query("SELECT run_id FROM agent_runs_v4 WHERE project_id=?1 ORDER BY rowid")
                .bind(project_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|row| parse_uuid(row.try_get::<String, _>(0)?, "V4 run id"))
            .collect()
    }

    pub async fn delete_project(&self, project_id: Uuid) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await?;
        let project_id = project_id.to_string();
        let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)")
            .bind(&project_id)
            .fetch_one(&mut *tx)
            .await?;
        if exists == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            "DELETE FROM agent_context_archives_v4 WHERE run_id IN
             (SELECT run_id FROM agent_runs_v4 WHERE project_id=?1)",
        )
        .bind(&project_id)
        .execute(&mut *tx)
        .await?;
        for table in [
            "scientific_provenance_v4",
            "scientific_evidence_v4",
            "scientific_artifacts_v4",
            "scientific_analyses_v4",
            "scientific_datasets_v4",
            "scientific_states_v4",
            "agent_events_v4",
            "agent_runs_v4",
            "runs",
            "remote_staging",
            "sync_entries",
            "notebook_entries",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE project_id=?1"))
                .bind(&project_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM projects WHERE id=?1")
            .bind(&project_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn save_conversation(&self, conversation: &Conversation) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        insert_conversation(&mut tx, conversation).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Read all durable Agent state for a conversation from one SQLite
    /// snapshot. The transaction is intentionally read-only: ownership,
    /// mode, lock, latest revision, and latest run are never observed across
    /// separate pool connections or separate transactions.
    pub async fn conversation_agent_state_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<ConversationAgentStateSnapshotV4, StoreError> {
        let mut tx = self.pool.begin().await?;
        let project_id_string = project_id.to_string();
        let conversation_id_string = conversation_id.to_string();

        let owns_conversation: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM conversation_records
                WHERE frame_id=?1 AND project_id=?2
            )",
        )
        .bind(&conversation_id_string)
        .bind(&project_id_string)
        .fetch_one(&mut *tx)
        .await?;
        if owns_conversation == 0 {
            return Err(StoreError::InvalidInput(format!(
                "conversation {conversation_id} does not belong to project {project_id}"
            )));
        }

        let mode_value = sqlx::query_scalar::<_, String>(
            "SELECT value_json FROM settings WHERE scope=?1 AND key=?2",
        )
        .bind(SETTINGS_GLOBAL_SCOPE)
        .bind(conversation_agent_mode_setting_key(conversation_id))
        .fetch_optional(&mut *tx)
        .await?;
        let mode = mode_value
            .as_deref()
            .map(parse_conversation_agent_mode)
            .transpose()?
            .unwrap_or_default();

        let locked: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM proposed_plans
                WHERE project_id=?1 AND frame_id=?2
                  AND status IN ('generating','revising','pending')
            )",
        )
        .bind(&project_id_string)
        .bind(&conversation_id_string)
        .fetch_one(&mut *tx)
        .await?;

        let latest_plan_revision = sqlx::query(
            "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
             FROM proposed_plans
             WHERE project_id=?1 AND frame_id=?2
             ORDER BY revision DESC,id DESC LIMIT 1",
        )
        .bind(&project_id_string)
        .bind(&conversation_id_string)
        .fetch_optional(&mut *tx)
        .await?
        .map(proposed_plan_revision_from_row)
        .transpose()?;

        if locked != 0 {
            if mode != SessionAgentModeV4::Plan {
                return Err(StoreError::InvalidInput(
                    "conversation has an active plan lock but is not in Plan mode".into(),
                ));
            }
            if !latest_plan_revision
                .as_ref()
                .is_some_and(|revision| revision.status.is_active())
            {
                return Err(StoreError::InvalidInput(
                    "conversation active plan lock has no active latest revision".into(),
                ));
            }
        }

        // Run timestamps are optional on legacy rows and may remain zero;
        // rowid preserves the insertion-order meaning used by the existing
        // conversation run listing API.
        let latest_run = sqlx::query(
            "SELECT run_id,status,value_json
             FROM agent_runs_v4
             WHERE project_id=?1 AND conversation_id=?2
             ORDER BY rowid DESC LIMIT 1",
        )
        .bind(&project_id_string)
        .bind(&conversation_id_string)
        .fetch_optional(&mut *tx)
        .await?;
        let latest_run_json = if let Some(row) = latest_run {
            let run_id = row.try_get::<String, _>(0)?;
            let status = row.try_get::<String, _>(1)?;
            let serialized = row.try_get::<String, _>(2)?;
            let value: Value = serde_json::from_str(&serialized).map_err(|error| {
                StoreError::InvalidInput(format!(
                    "latest Agent V4 run {run_id} has invalid JSON: {error}"
                ))
            })?;
            validate_latest_agent_run_json(
                &value,
                &run_id,
                &project_id_string,
                &conversation_id_string,
                &status,
            )?;
            Some(value)
        } else {
            None
        };

        tx.commit().await?;
        Ok(ConversationAgentStateSnapshotV4 {
            project_id,
            conversation_id,
            mode,
            locked: locked != 0,
            latest_plan_revision,
            latest_run_json,
        })
    }

    /// Read the durable Agent/Plan mode for a conversation.
    ///
    /// The project id is part of the lookup contract rather than merely a
    /// caller hint: a conversation must belong to that project before its
    /// setting is read. Missing settings are the legacy behavior and default
    /// to Agent without creating a row.
    pub async fn get_conversation_agent_mode(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<SessionAgentModeV4, StoreError> {
        self.ensure_conversation_owner(project_id, conversation_id)
            .await?;
        let key = conversation_agent_mode_setting_key(conversation_id);
        let row = sqlx::query("SELECT value_json FROM settings WHERE scope=?1 AND key=?2")
            .bind(SETTINGS_GLOBAL_SCOPE)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(SessionAgentModeV4::default());
        };
        let value = row.try_get::<String, _>(0)?;
        parse_conversation_agent_mode(&value)
    }

    /// Persist the Agent/Plan mode for a conversation.
    ///
    /// Ownership validation and the setting upsert share one transaction so a
    /// cross-project request cannot leave behind a setting row.
    pub async fn set_conversation_agent_mode(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        mode: SessionAgentModeV4,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        if mode == SessionAgentModeV4::Agent {
            let locked: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM proposed_plans
                    WHERE project_id=?1 AND frame_id=?2
                      AND status IN ('generating','revising','pending')
                )",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if locked != 0 {
                return Err(StoreError::InvalidInput(
                    "conversation is locked by an active plan; approve, request changes, or cancel it first".into(),
                ));
            }
        }
        let key = conversation_agent_mode_setting_key(conversation_id);
        sqlx::query(
            "INSERT INTO settings (scope,key,value_json,updated_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(scope,key) DO UPDATE SET value_json=excluded.value_json,
             updated_at=excluded.updated_at",
        )
        .bind(SETTINGS_GLOBAL_SCOPE)
        .bind(key)
        .bind(serde_json::to_string(&mode)?)
        .bind(timestamp(Utc::now()))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn agent_iteration_settings(&self) -> Result<Option<Value>, StoreError> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value_json FROM settings WHERE scope='global' AND key='agent_iterations_v1'",
        )
        .fetch_optional(&self.pool)
        .await?;
        value
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    pub async fn save_agent_iteration_settings(&self, value: &Value) -> Result<(), StoreError> {
        serde_json::from_value::<omicsops_dto::AgentIterationSettingsV4>(value.clone()).map_err(
            |error| StoreError::InvalidInput(format!("invalid session settings: {error}")),
        )?;
        sqlx::query(
            "INSERT INTO settings(scope,key,value_json,updated_at)
             VALUES('global','agent_iterations_v1',?1,?2)
             ON CONFLICT(scope,key) DO UPDATE SET
               value_json=excluded.value_json,updated_at=excluded.updated_at",
        )
        .bind(serde_json::to_string(value)?)
        .bind(timestamp(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn browser_settings(&self) -> Result<Option<Value>, StoreError> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value_json FROM settings WHERE scope='global' AND key='browser_v1'",
        )
        .fetch_optional(&self.pool)
        .await?;
        value
            .map(|value| serde_json::from_str(&value).map_err(StoreError::from))
            .transpose()
    }

    pub async fn save_browser_settings(&self, value: &Value) -> Result<(), StoreError> {
        if !value.is_object() {
            return Err(StoreError::InvalidInput(
                "browser settings must be a JSON object".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO settings(scope,key,value_json,updated_at)
             VALUES('global','browser_v1',?1,?2)
             ON CONFLICT(scope,key) DO UPDATE SET
               value_json=excluded.value_json,updated_at=excluded.updated_at",
        )
        .bind(serde_json::to_string(value)?)
        .bind(timestamp(Utc::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_browser_authorization_v4(
        &self,
        authorization: &BrowserAuthorizationV4,
    ) -> Result<(), StoreError> {
        authorization
            .validate()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        if let (Some(project_id), Some(conversation_id)) =
            (authorization.project_id, authorization.conversation_id)
        {
            self.ensure_conversation_owner(project_id, conversation_id)
                .await?;
        } else if let Some(project_id) = authorization.project_id {
            let exists: i64 =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)")
                    .bind(project_id.to_string())
                    .fetch_one(&self.pool)
                    .await?;
            if exists == 0 {
                return Err(StoreError::InvalidInput(
                    "browser authorization project does not exist".into(),
                ));
            }
        }
        let scope = match authorization.scope {
            BrowserApprovalScopeV4::Once => "once",
            BrowserApprovalScopeV4::Conversation => "conversation",
            BrowserApprovalScopeV4::Project => "project",
            BrowserApprovalScopeV4::Global => "global",
        };
        let session = match authorization.binding.session {
            BrowserSessionKindV4::Shared => "shared",
            BrowserSessionKindV4::Workspace => "workspace",
        };
        sqlx::query(
            "INSERT INTO browser_authorizations_v4
             (id,scope,capability,target_host,session,protocol_version,project_id,conversation_id,value_json,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(&authorization.id)
        .bind(scope)
        .bind(&authorization.binding.capability)
        .bind(&authorization.binding.target_host)
        .bind(session)
        .bind(i64::from(authorization.binding.protocol_version))
        .bind(authorization.project_id.map(|value| value.to_string()))
        .bind(authorization.conversation_id.map(|value| value.to_string()))
        .bind(serde_json::to_string(authorization)?)
        .bind(authorization.created_at_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_browser_authorizations_v4(
        &self,
    ) -> Result<Vec<BrowserAuthorizationV4>, StoreError> {
        let rows = sqlx::query(
            "SELECT value_json FROM browser_authorizations_v4 ORDER BY created_at DESC,id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let value = row.try_get::<String, _>(0)?;
                let authorization: BrowserAuthorizationV4 = serde_json::from_str(&value)?;
                authorization
                    .validate()
                    .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
                Ok(authorization)
            })
            .collect()
    }

    pub async fn revoke_browser_authorization_v4(&self, id: &str) -> Result<bool, StoreError> {
        if id.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "browser authorization id is empty".into(),
            ));
        }
        Ok(
            sqlx::query("DELETE FROM browser_authorizations_v4 WHERE id=?1")
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                != 0,
        )
    }

    /// Check the binding immediately before browser dispatch. A once grant is
    /// consumed in the same transaction, so duplicate calls cannot replay it.
    pub async fn consume_browser_authorization_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        binding: &BrowserApprovalBindingV4,
    ) -> Result<bool, StoreError> {
        self.ensure_conversation_owner(project_id, conversation_id)
            .await?;
        let session = match binding.session {
            BrowserSessionKindV4::Shared => "shared",
            BrowserSessionKindV4::Workspace => "workspace",
        };
        // Serialize matching and consumption so two runs cannot both observe
        // the same one-shot grant before either deletes it.
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let project_id_text = project_id.to_string();
        let conversation_id_text = conversation_id.to_string();
        let rows = sqlx::query(
            "SELECT id,scope,project_id,conversation_id
             FROM browser_authorizations_v4
             WHERE capability=?1 AND target_host=?2 AND session=?3 AND protocol_version=?4
             ORDER BY CASE scope WHEN 'once' THEN 0 WHEN 'conversation' THEN 1 WHEN 'project' THEN 2 ELSE 3 END,
                      created_at DESC,id",
        )
        .bind(&binding.capability)
        .bind(&binding.target_host)
        .bind(session)
        .bind(i64::from(binding.protocol_version))
        .fetch_all(&mut *tx)
        .await?;
        for row in rows {
            let id = row.try_get::<String, _>(0)?;
            let scope = row.try_get::<String, _>(1)?;
            let row_project = row.try_get::<Option<String>, _>(2)?;
            let row_conversation = row.try_get::<Option<String>, _>(3)?;
            let matches = match scope.as_str() {
                "once" | "conversation" => {
                    row_project.as_deref() == Some(project_id_text.as_str())
                        && row_conversation.as_deref() == Some(conversation_id_text.as_str())
                }
                "project" => {
                    row_project.as_deref() == Some(project_id_text.as_str())
                        && row_conversation.is_none()
                }
                "global" => row_project.is_none() && row_conversation.is_none(),
                _ => false,
            };
            if !matches {
                continue;
            }
            if scope == "once" {
                let deleted = sqlx::query("DELETE FROM browser_authorizations_v4 WHERE id=?1")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                if deleted != 1 {
                    continue;
                }
            }
            tx.commit().await?;
            return Ok(true);
        }
        tx.commit().await?;
        Ok(false)
    }

    async fn ensure_conversation_owner(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<(), StoreError> {
        let owns: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM conversation_records
                WHERE frame_id=?1 AND project_id=?2
            )",
        )
        .bind(conversation_id.to_string())
        .bind(project_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        if owns == 0 {
            return Err(StoreError::InvalidInput(format!(
                "conversation {conversation_id} does not belong to project {project_id}"
            )));
        }
        Ok(())
    }

    pub async fn conversations_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Conversation>, StoreError> {
        let rows = sqlx::query(
            "SELECT frame_id,project_id,title,status,model_profile_id,created_at,updated_at
             FROM conversation_records WHERE project_id=?1 ORDER BY updated_at DESC,frame_id",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(conversation_from_row).collect()
    }

    pub async fn delete_conversation(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await?;
        let conversation_setting_key = conversation_agent_mode_setting_key(conversation_id);
        let project_id = project_id.to_string();
        let conversation_id = conversation_id.to_string();
        let exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM conversation_records WHERE frame_id=?1 AND project_id=?2)",
        )
        .bind(&conversation_id)
        .bind(&project_id)
        .fetch_one(&mut *tx)
        .await?;
        if exists == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        // Conversation deletion is a destructive ordinary mutation. Check
        // the lifecycle lock in this same transaction so a plan cannot win
        // between the ownership check and the deletes below.
        let locked: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM proposed_plans
                WHERE project_id=?1 AND frame_id=?2
                  AND status IN ('generating','revising','pending')
            )",
        )
        .bind(&project_id)
        .bind(&conversation_id)
        .fetch_one(&mut *tx)
        .await?;
        if locked != 0 {
            return Err(StoreError::InvalidInput(
                "conversation is locked by an active plan; approve, request changes, or cancel it first".into(),
            ));
        }
        sqlx::query(
            "DELETE FROM agent_context_archives_v4 WHERE run_id IN
             (SELECT run_id FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2)",
        )
        .bind(&project_id)
        .bind(&conversation_id)
        .execute(&mut *tx)
        .await?;
        for table in ["agent_events_v4", "agent_runs_v4"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE project_id=?1 AND conversation_id=?2"
            ))
            .bind(&project_id)
            .bind(&conversation_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("DELETE FROM frames WHERE id=?1")
            .bind(&conversation_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM settings WHERE scope=?1 AND key=?2")
            .bind(SETTINGS_GLOBAL_SCOPE)
            .bind(conversation_setting_key)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn save_message(&self, message: &Message) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_frame(
            &mut tx,
            message.project_id,
            message.conversation_id,
            message.created_at,
        )
        .await?;
        ensure_conversation_unlocked_executor(
            &mut *tx,
            message.project_id,
            message.conversation_id,
        )
        .await?;
        insert_message(&mut tx, message).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Atomically persist a submitted user message and, when it is the first
    /// user message in the conversation, update the conversation title.
    ///
    /// The lock predicate is evaluated inside the same `BEGIN IMMEDIATE`
    /// transaction as both writes. This prevents a plan revision from winning
    /// between a title preflight and the message insert, which would otherwise
    /// leave a title-only mutation behind after the message is rejected.
    pub async fn save_message_with_first_title(
        &self,
        message: &Message,
        title: &str,
    ) -> Result<Option<Conversation>, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_frame(
            &mut tx,
            message.project_id,
            message.conversation_id,
            message.created_at,
        )
        .await?;
        ensure_conversation_unlocked_executor(
            &mut *tx,
            message.project_id,
            message.conversation_id,
        )
        .await?;

        let has_user_message: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM messages
                WHERE frame_id=?1 AND role='user'
            )",
        )
        .bind(message.conversation_id.to_string())
        .fetch_one(&mut *tx)
        .await?;

        let updated_conversation = if has_user_message == 0 {
            let row = sqlx::query(
                "SELECT frame_id,project_id,title,status,model_profile_id,created_at,updated_at
                 FROM conversation_records
                 WHERE frame_id=?1 AND project_id=?2",
            )
            .bind(message.conversation_id.to_string())
            .bind(message.project_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("conversation not found".into()))?;
            let mut conversation = conversation_from_row(row)?;
            conversation.title = title.to_owned();
            conversation.updated_at = message.created_at;
            sqlx::query(
                "UPDATE conversation_records
                 SET title=?1,updated_at=?2
                 WHERE frame_id=?3 AND project_id=?4",
            )
            .bind(&conversation.title)
            .bind(timestamp(conversation.updated_at))
            .bind(message.conversation_id.to_string())
            .bind(message.project_id.to_string())
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE frames SET updated_at=?1 WHERE id=?2 AND project_id=?3")
                .bind(timestamp(conversation.updated_at))
                .bind(message.conversation_id.to_string())
                .bind(message.project_id.to_string())
                .execute(&mut *tx)
                .await?;
            Some(conversation)
        } else {
            None
        };

        insert_message(&mut tx, message).await?;
        tx.commit().await?;
        Ok(updated_conversation)
    }

    pub async fn messages_for_conversation(
        &self,
        conversation_id: Uuid,
    ) -> Result<Vec<Message>, StoreError> {
        let rows = sqlx::query(
            "SELECT m.id,m.project_id,m.conversation_id,m.seq,m.role,m.content,m.ts,
                    COALESCE(m.project_id,c.project_id) AS resolved_project_id
             FROM messages m LEFT JOIN conversation_records c ON c.frame_id=m.frame_id
             WHERE m.frame_id=?1 ORDER BY m.seq,m.id",
        )
        .bind(conversation_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(message_from_row).collect()
    }

    pub async fn save_notebook_entry(&self, entry: &NotebookEntry) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO notebook_entries (id,project_id,conversation_id,turn_id,kind,title,
             markdown,confidence,evidence_json,artifact_ids_json,created_at,updated_at,value_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,
             conversation_id=excluded.conversation_id,turn_id=excluded.turn_id,kind=excluded.kind,
             title=excluded.title,markdown=excluded.markdown,confidence=excluded.confidence,
             evidence_json=excluded.evidence_json,artifact_ids_json=excluded.artifact_ids_json,
             created_at=excluded.created_at,updated_at=excluded.updated_at,value_json=excluded.value_json",
        )
        .bind(entry.id.to_string())
        .bind(entry.project_id.to_string())
        .bind(entry.conversation_id.map(|id| id.to_string()))
        .bind(entry.turn_id.map(|id| id.to_string()))
        .bind(enum_string(&entry.kind)?)
        .bind(&entry.title)
        .bind(&entry.markdown)
        .bind(entry.confidence.map(f64::from))
        .bind(serde_json::to_string(&entry.evidence_ids)?)
        .bind(serde_json::to_string(&entry.artifact_ids)?)
        .bind(timestamp(entry.created_at))
        .bind(timestamp(entry.updated_at))
        .bind(serde_json::to_string(entry)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn notebook_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<NotebookEntry>, StoreError> {
        self.project_json_rows("notebook_entries", project_id).await
    }

    /// Kept as a source-compatible name while writing only the normalized v4
    /// `artifacts` table. The retired `artifacts_v3` table is never modified.
    pub async fn save_artifact_v3(&self, artifact: &Artifact) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO artifacts (id,project_id,root_frame_id,filename,content_type,storage_path,
             created_at,logical_key,source_run_id,remote_path,size_bytes,sha256,verified)
             VALUES (?1,?2,NULL,?3,?4,?5,?6,NULL,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,filename=excluded.filename,
             content_type=excluded.content_type,storage_path=excluded.storage_path,
             created_at=excluded.created_at,source_run_id=excluded.source_run_id,
             remote_path=excluded.remote_path,size_bytes=excluded.size_bytes,sha256=excluded.sha256,
             verified=excluded.verified",
        )
        .bind(artifact.id.to_string())
        .bind(artifact.project_id.to_string())
        .bind(&artifact.relative_path)
        .bind(&artifact.media_type)
        .bind(&artifact.relative_path)
        .bind(timestamp(artifact.created_at))
        .bind(artifact.run_id.map(|id| id.to_string()))
        .bind(&artifact.remote_path)
        .bind(i64::try_from(artifact.size_bytes).map_err(|_| StoreError::InvalidInput("artifact size exceeds SQLite integer range".into()))?)
        .bind(&artifact.sha256)
        .bind(if artifact.verified { 1_i64 } else { 0_i64 })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn artifacts_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<Artifact>, StoreError> {
        let rows = sqlx::query(
            "SELECT id,project_id,source_run_id,filename,remote_path,content_type,size_bytes,sha256,
                    verified,created_at FROM artifacts WHERE project_id=?1 ORDER BY id",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(artifact_from_row).collect()
    }

    pub async fn save_sync_entry(&self, entry: &SyncEntry) -> Result<(), StoreError> {
        if entry.relative_path.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "sync entry relative_path cannot be blank".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO sync_entries (id,project_id,relative_path,value_json) VALUES (?1,?2,?3,?4)
             ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,
                 relative_path=excluded.relative_path,value_json=excluded.value_json",
        )
        .bind(entry.id.to_string())
        .bind(entry.project_id.to_string())
        .bind(&entry.relative_path)
        .bind(serde_json::to_string(entry)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn sync_entries_for_project(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<SyncEntry>, StoreError> {
        self.project_json_rows("sync_entries", project_id).await
    }

    pub async fn save_model_profile(&self, profile: &ModelProfile) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO model_profiles (id,value_json) VALUES (?1,?2)
             ON CONFLICT(id) DO UPDATE SET value_json=excluded.value_json",
        )
        .bind(profile.id.to_string())
        .bind(serde_json::to_string(profile)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_model_profiles(&self) -> Result<Vec<ModelProfile>, StoreError> {
        self.simple_json_rows("model_profiles").await
    }

    pub async fn get_model_profile(&self, id: Uuid) -> Result<Option<ModelProfile>, StoreError> {
        self.get_simple_json("model_profiles", &id.to_string())
            .await
    }

    pub async fn save_skill_package(&self, skill: &SkillPackage) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO skill_packages (id,value_json) VALUES (?1,?2)
             ON CONFLICT(id) DO UPDATE SET value_json=excluded.value_json",
        )
        .bind(skill.id.to_string())
        .bind(serde_json::to_string(skill)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically insert a content-addressed skill package, or return the
    /// package that won a concurrent insert of the same source SHA.  The
    /// immediate transaction serializes writers across the desktop process;
    /// callers never need a read-then-insert race window.
    pub async fn upsert_skill_package_by_sha(
        &self,
        skill: &SkillPackage,
        enabled_by_default: bool,
    ) -> Result<SkillPackage, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut packages = skill_packages_in_tx(&mut tx).await?;
        if let Some(index) = packages
            .iter()
            .position(|candidate| candidate.sha256 == skill.sha256)
        {
            let mut existing = packages[index].clone();
            let mut changed = false;
            if skill.category.is_some() && existing.category != skill.category {
                existing.category = skill.category.clone();
                changed = true;
            }
            if enabled_by_default {
                for candidate in &mut packages {
                    if candidate.name == existing.name && candidate.enabled {
                        candidate.enabled = false;
                        save_skill_package_in_tx(&mut tx, candidate).await?;
                    }
                }
                existing.enabled = true;
                changed = true;
            }
            if changed {
                save_skill_package_in_tx(&mut tx, &existing).await?;
            }
            tx.commit().await?;
            return Ok(existing);
        }

        let mut inserted = skill.clone();
        inserted.enabled = enabled_by_default;
        if enabled_by_default {
            for candidate in &mut packages {
                if candidate.name == inserted.name && candidate.enabled {
                    candidate.enabled = false;
                    save_skill_package_in_tx(&mut tx, candidate).await?;
                }
            }
        }
        save_skill_package_in_tx(&mut tx, &inserted).await?;
        tx.commit().await?;
        Ok(inserted)
    }

    /// Atomically switch the enabled version for one skill name.  Skill
    /// packages are intentionally stored as JSON, so the transaction loads
    /// and rewrites the small control-plane set while holding SQLite's write
    /// lock; this keeps same-name concurrent toggles single-writer safe.
    pub async fn set_skill_enabled_atomic(
        &self,
        skill_id: Uuid,
        enabled: bool,
    ) -> Result<SkillPackage, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut packages = skill_packages_in_tx(&mut tx).await?;
        let index = packages
            .iter()
            .position(|skill| skill.id == skill_id)
            .ok_or_else(|| StoreError::InvalidInput("skill package was not found".into()))?;
        let target_name = packages[index].name.clone();
        if enabled {
            for candidate in &mut packages {
                if candidate.name == target_name {
                    candidate.enabled = false;
                    save_skill_package_in_tx(&mut tx, candidate).await?;
                }
            }
        }
        packages[index].enabled = enabled;
        let result = packages[index].clone();
        save_skill_package_in_tx(&mut tx, &result).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn list_skill_packages(&self) -> Result<Vec<SkillPackage>, StoreError> {
        self.simple_json_rows("skill_packages").await
    }

    pub async fn delete_skill_package(&self, id: Uuid) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM skill_packages WHERE id=?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn put_json<T: Serialize>(
        &self,
        kind: &str,
        id: &str,
        value: &T,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO app_objects (kind,id,value_json) VALUES (?1,?2,?3)
             ON CONFLICT(kind,id) DO UPDATE SET value_json=excluded.value_json",
        )
        .bind(kind)
        .bind(id)
        .bind(serde_json::to_string(value)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<Option<T>, StoreError> {
        let row = sqlx::query("SELECT value_json FROM app_objects WHERE kind=?1 AND id=?2")
            .bind(kind)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .transpose()
    }

    pub async fn list_json<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>, StoreError> {
        let rows = sqlx::query("SELECT value_json FROM app_objects WHERE kind=?1 ORDER BY id")
            .bind(kind)
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .collect()
    }

    pub async fn save_agent_run_v4(
        &self,
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        status: &str,
        value: &Value,
    ) -> Result<(), StoreError> {
        // This is the ordinary public persistence seam. Lifecycle methods
        // write their run row inside their own transition transaction; this
        // seam must never be able to overwrite a generating/revising/pending
        // plan (including with a stale whole-row snapshot).
        let mut tx = self.pool.begin().await?;
        let existing_owner =
            sqlx::query("SELECT project_id,conversation_id FROM agent_runs_v4 WHERE run_id=?1")
                .bind(run_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(existing_owner) = existing_owner {
            let owner_matches = existing_owner.try_get::<String, _>(0)? == project_id.to_string()
                && existing_owner.try_get::<String, _>(1)? == conversation_id.to_string();
            if !owner_matches {
                return Err(StoreError::InvalidInput(
                    "agent run owner/context mismatch: it already belongs to a different project or conversation".into(),
                ));
            }
        }
        ensure_conversation_unlocked_executor(&mut *tx, project_id, conversation_id).await?;
        let saved = sqlx::query(
            "INSERT INTO agent_runs_v4 (run_id,project_id,conversation_id,status,value_json)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(run_id) DO UPDATE SET status=excluded.status,value_json=excluded.value_json
             WHERE agent_runs_v4.project_id=excluded.project_id
               AND agent_runs_v4.conversation_id=excluded.conversation_id",
        )
        .bind(run_id.to_string())
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .bind(status)
        .bind(serde_json::to_string(value)?)
        .execute(&mut *tx)
        .await?;
        if saved.rows_affected() != 1 {
            return Err(StoreError::InvalidInput(
                "agent run owner/context mismatch: it already belongs to a different project or conversation".into(),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    /// Persist a newly started ordinary/direct run only when the conversation
    /// is currently unlocked. The ownership and lock predicate are checked in
    /// the same transaction as the write, so a Tauri preflight cannot race a
    /// generating plan revision.
    pub async fn save_agent_run_v4_if_unlocked(
        &self,
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        status: &str,
        value: &Value,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_conversation_unlocked_executor(&mut tx, project_id, conversation_id).await?;
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2 AND status IN ('running','waiting_for_input','waiting_for_approval','awaiting_approval')")
            .bind(project_id.to_string()).bind(conversation_id.to_string()).fetch_one(&mut *tx).await?;
        if active > 0 {
            return Err(StoreError::InvalidInput(
                "conversation already has an active run; guide or resume it instead".into(),
            ));
        }
        sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,?4,?5)")
            .bind(run_id.to_string()).bind(project_id.to_string()).bind(conversation_id.to_string()).bind(status).bind(serde_json::to_string(value)?)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn agent_run_v4(&self, run_id: Uuid) -> Result<Option<Value>, StoreError> {
        let row = sqlx::query("SELECT value_json FROM agent_runs_v4 WHERE run_id=?1")
            .bind(run_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .transpose()
    }

    /// Atomically create a planning run, reserve its generating revision, and
    /// acquire the conversation lock before any model call is made. This is
    /// the write-path seam used by the Tauri planning command; its lock check
    /// is not merely a preflight performed by the adapter.
    pub async fn start_plan_run_v4(
        &self,
        run_id: Uuid,
        project_id: Uuid,
        conversation_id: Uuid,
        status: &str,
        value: &Value,
        objective: &str,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        let objective = objective.trim();
        if objective.is_empty() {
            return Err(StoreError::InvalidInput(
                "plan generation objective cannot be empty".into(),
            ));
        }
        if !value.is_object() {
            return Err(StoreError::InvalidInput(
                "planning run value must be a JSON object".into(),
            ));
        }
        let timestamp = timestamp(now);
        let stored_now = from_timestamp(timestamp, "plan revision timestamp")?;
        let mut connection = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<ProposedPlanRevisionV4, StoreError> = async {
            let owns_conversation: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM conversation_records
                    WHERE frame_id=?1 AND project_id=?2
                )",
            )
            .bind(conversation_id.to_string())
            .bind(project_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if owns_conversation == 0 {
                return Err(StoreError::InvalidInput(format!(
                    "conversation {conversation_id} does not belong to project {project_id}"
                )));
            }
            let active: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM proposed_plans
                    WHERE project_id=?1 AND frame_id=?2
                      AND status IN ('generating','revising','pending')
                )",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if active != 0 {
                return Err(StoreError::InvalidInput(
                    "conversation already has an active plan generation or proposal".into(),
                ));
            }
            let active_run: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM agent_runs_v4 WHERE conversation_id=?1
                 AND status IN ('running','waiting_for_input','waiting_for_approval','awaiting_approval')",
            )
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if active_run != 0 {
                return Err(StoreError::InvalidInput(
                    "conversation already has an active run".into(),
                ));
            }
            let existing_run: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM agent_runs_v4 WHERE run_id=?1)",
            )
            .bind(run_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if existing_run != 0 {
                return Err(StoreError::InvalidInput(
                    "planning run id already exists and cannot be reused".into(),
                ));
            }
            sqlx::query(
                "INSERT INTO agent_runs_v4 (run_id,project_id,conversation_id,status,value_json)
                 VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(run_id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(status)
            .bind(serde_json::to_string(value)?)
            .execute(&mut *connection)
            .await?;
            let next_revision: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(revision),0)+1 FROM proposed_plans WHERE frame_id=?1",
            )
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            let revision = u64::try_from(next_revision)
                .map_err(|_| StoreError::InvalidInput("invalid next plan revision".into()))?;
            let seed_plan = generation_seed_plan(objective, revision);
            let plan_hash = seed_plan
                .canonical_hash()
                .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO proposed_plans
                 (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,'generating',?6,'',NULL,?7,?8,?8)",
            )
            .bind(id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(next_revision)
            .bind(&plan_hash)
            .bind(serde_json::to_string(&seed_plan)?)
            .bind(run_id.to_string())
            .bind(timestamp)
            .execute(&mut *connection)
            .await?;
            sqlx::query(
                "INSERT INTO settings (scope,key,value_json,updated_at)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(scope,key) DO UPDATE SET value_json=excluded.value_json,
                 updated_at=excluded.updated_at",
            )
            .bind(SETTINGS_GLOBAL_SCOPE)
            .bind(conversation_agent_mode_setting_key(conversation_id))
            .bind(serde_json::to_string(&SessionAgentModeV4::Plan)?)
            .bind(timestamp)
            .execute(&mut *connection)
            .await?;
            let mut stored_value = value.clone();
            let object = stored_value.as_object_mut().ok_or_else(|| {
                StoreError::InvalidInput("planning run value must be a JSON object".into())
            })?;
            object.insert("status".into(), Value::String("planning".into()));
            object.insert("plan_revision".into(), Value::from(revision));
            sqlx::query(
                "UPDATE agent_runs_v4 SET status='planning',value_json=?,updated_at=? WHERE run_id=?3",
            )
            .bind(serde_json::to_string(&stored_value)?)
            .bind(timestamp)
            .bind(run_id.to_string())
            .execute(&mut *connection)
            .await?;
            Ok(ProposedPlanRevisionV4 {
                id,
                project_id,
                conversation_id,
                run_id,
                revision,
                plan: seed_plan,
                markdown: String::new(),
                plan_hash,
                status: PlanRevisionStatusV4::Generating,
                feedback: None,
                created_at: stored_now,
                updated_at: stored_now,
            })
        }
        .await;
        match result {
            Ok(value) => {
                connection.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = connection.rollback().await;
                Err(error)
            }
        }
    }

    /// Atomically acquire the conversation's plan-generation lock and reserve
    /// the first immutable revision number. `BEGIN IMMEDIATE` serializes two
    /// concurrent planners before either can observe the no-active-lock
    /// predicate; a uniqueness violation is not used as synchronization.
    pub async fn acquire_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        objective: &str,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        self.acquire_plan_revision_generation_v4(
            project_id,
            conversation_id,
            run_id,
            objective,
            now,
            false,
        )
        .await
    }

    /// Atomically acquire the next revision after a user requested changes.
    /// Only a matching latest `revising` revision may be resumed; pending,
    /// approved, superseded, or cancelled plans cannot be revived.
    pub async fn acquire_plan_revision_resume_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        objective: &str,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        self.acquire_plan_revision_generation_v4(
            project_id,
            conversation_id,
            run_id,
            objective,
            now,
            true,
        )
        .await
    }

    /// Pause a model-driven plan generation for one exact tool-approval
    /// request. The immutable revision stays in place and the conversation
    /// lock remains held; only the lifecycle status and run snapshot move to
    /// their approval-waiting states. This is intentionally separate from
    /// `terminate_plan_generation_v4`, whose revising transition means that a
    /// user asked for plan changes and therefore starts a new revision on
    /// resume.
    pub async fn pause_plan_generation_for_approval_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        feedback: Option<&str>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        let mut connection = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<ProposedPlanRevisionV4, StoreError> = async {
            ensure_conversation_owner_executor(&mut *connection, project_id, conversation_id)
                .await?;
            ensure_run_owner_executor(&mut *connection, project_id, conversation_id, run_id).await?;
            let run_status: String =
                sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
                    .bind(run_id.to_string())
                    .fetch_one(&mut *connection)
                    .await?;
            if run_status != "planning" {
                return Err(StoreError::InvalidInput(format!(
                    "plan generation run is not planning (status={run_status})"
                )));
            }
            let row = sqlx::query(
                "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
                 FROM proposed_plans WHERE project_id=?1 AND frame_id=?2 AND run_id=?3 AND revision=?4",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(run_id.to_string())
            .bind(i64::try_from(revision).map_err(|_| {
                StoreError::InvalidInput("plan revision exceeds SQLite integer range".into())
            })?)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("plan generation revision was not found".into()))?;
            let current = proposed_plan_revision_from_row(row)?;
            if current.status != PlanRevisionStatusV4::Generating {
                return Err(StoreError::InvalidInput(
                    "plan generation is no longer generating; approval pause was already applied".into(),
                ));
            }
            let scope = PlanApprovalScopeV4 {
                project_id,
                conversation_id,
                run_id,
                revision_id: current.id,
                revision: current.revision,
            };
            let events = load_agent_events_in_tx(&mut *connection, run_id).await?;
            // A decision can legitimately win the narrow request -> pause
            // race. Validate the exact current request (and any decision),
            // then still persist the waiting state so a subsequent resume
            // observes one coherent lifecycle instead of leaving the
            // conversation locked in planning/generating.
            plan_approval_request_state(&events, scope)?;
            let now = Utc::now();
            let stored_now = from_timestamp(timestamp(now), "plan revision timestamp")?;
            let updated = sqlx::query(
                "UPDATE proposed_plans SET status='revising',feedback=?1,updated_at=?2
                 WHERE id=?3 AND status='generating'",
            )
            .bind(feedback)
            .bind(timestamp(stored_now))
            .bind(current.id.to_string())
            .execute(&mut *connection)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "plan generation changed before approval pause could be recorded".into(),
                ));
            }
            set_agent_run_status_in_tx(&mut *connection, run_id, "waiting_for_approval").await?;
            Ok(ProposedPlanRevisionV4 {
                status: PlanRevisionStatusV4::Revising,
                feedback: feedback.map(ToOwned::to_owned),
                updated_at: stored_now,
                ..current
            })
        }
        .await;
        match result {
            Ok(value) => {
                connection.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = connection.rollback().await;
                Err(error)
            }
        }
    }

    /// Resume a Plan generation after its exact pending approval was decided.
    /// Unlike a user-requested revision resume, this transitions the same
    /// revision row back to `generating`, preserving the revision UUID and
    /// number that are part of the approval scope hash.
    pub async fn resume_plan_generation_after_approval_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision_id: Uuid,
        revision: u64,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        let timestamp = timestamp(now);
        let stored_now = from_timestamp(timestamp, "plan revision timestamp")?;
        let mut connection = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<ProposedPlanRevisionV4, StoreError> = async {
            ensure_conversation_owner_executor(&mut *connection, project_id, conversation_id)
                .await?;
            ensure_run_owner_executor(&mut *connection, project_id, conversation_id, run_id).await?;
            let run_status: String =
                sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
                    .bind(run_id.to_string())
                    .fetch_one(&mut *connection)
                    .await?;
            if run_status != "waiting_for_approval" {
                return Err(StoreError::InvalidInput(format!(
                    "plan generation run is not waiting for approval (status={run_status})"
                )));
            }
            let row = sqlx::query(
                "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
                 FROM proposed_plans WHERE id=?1 AND project_id=?2 AND frame_id=?3 AND run_id=?4 AND revision=?5",
            )
            .bind(revision_id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(run_id.to_string())
            .bind(i64::try_from(revision).map_err(|_| {
                StoreError::InvalidInput("plan revision exceeds SQLite integer range".into())
            })?)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("approval revision was not found".into()))?;
            let current = proposed_plan_revision_from_row(row)?;
            if current.status != PlanRevisionStatusV4::Revising {
                return Err(StoreError::InvalidInput(
                    "approval revision is no longer revising".into(),
                ));
            }
            let latest_id: String = sqlx::query_scalar(
                "SELECT id FROM proposed_plans
                 WHERE project_id=?1 AND frame_id=?2
                 ORDER BY revision DESC,id DESC LIMIT 1",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if latest_id != revision_id.to_string() {
                return Err(StoreError::InvalidInput(
                    "approval revision is no longer the latest revision".into(),
                ));
            }
            let scope = PlanApprovalScopeV4 {
                project_id,
                conversation_id,
                run_id,
                revision_id: current.id,
                revision: current.revision,
            };
            let events = load_agent_events_in_tx(&mut *connection, run_id).await?;
            let decision = plan_approval_request_state(&events, scope)?;
            if decision.is_none() {
                return Err(StoreError::InvalidInput(
                    "Plan approval must be decided before the revision can resume".into(),
                ));
            }
            let updated = sqlx::query(
                "UPDATE proposed_plans SET status='generating',updated_at=?1
                 WHERE id=?2 AND status='revising'",
            )
            .bind(timestamp)
            .bind(revision_id.to_string())
            .execute(&mut *connection)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "approval revision changed before it could resume".into(),
                ));
            }
            set_agent_run_status_in_tx(&mut *connection, run_id, "planning").await?;
            Ok(ProposedPlanRevisionV4 {
                status: PlanRevisionStatusV4::Generating,
                updated_at: stored_now,
                ..current
            })
        }
        .await;
        match result {
            Ok(value) => {
                connection.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = connection.rollback().await;
                Err(error)
            }
        }
    }

    async fn acquire_plan_revision_generation_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        objective: &str,
        now: DateTime<Utc>,
        resume: bool,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        let objective = objective.trim();
        if objective.is_empty() {
            return Err(StoreError::InvalidInput(
                "plan generation objective cannot be empty".into(),
            ));
        }
        let timestamp = timestamp(now);
        let stored_now = from_timestamp(timestamp, "plan revision timestamp")?;
        let mut connection = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<ProposedPlanRevisionV4, StoreError> = async {
            let owns_conversation: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM conversation_records
                    WHERE frame_id=?1 AND project_id=?2
                )",
            )
            .bind(conversation_id.to_string())
            .bind(project_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if owns_conversation == 0 {
                return Err(StoreError::InvalidInput(format!(
                    "conversation {conversation_id} does not belong to project {project_id}"
                )));
            }
            let owns_run: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM agent_runs_v4
                    WHERE run_id=?1 AND project_id=?2 AND conversation_id=?3
                )",
            )
            .bind(run_id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if owns_run == 0 {
                return Err(StoreError::InvalidInput(format!(
                    "run {run_id} does not belong to conversation {conversation_id} in project {project_id}"
                )));
            }
            let run_status: String =
                sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
                    .bind(run_id.to_string())
                    .fetch_one(&mut *connection)
                    .await?;
            if matches!(
                run_status.as_str(),
                "completed" | "cancelled" | "failed" | "needs_attention"
            ) {
                return Err(StoreError::InvalidInput(
                    "terminal run cannot acquire a plan revision".into(),
                ));
            }

            let latest = sqlx::query(
                "SELECT revision,run_id,status FROM proposed_plans
                 WHERE project_id=?1 AND frame_id=?2
                 ORDER BY revision DESC,id DESC LIMIT 1",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_optional(&mut *connection)
            .await?;
            if resume {
                let Some(latest) = latest else {
                    return Err(StoreError::InvalidInput(
                        "only a latest revising plan can be resumed".into(),
                    ));
                };
                let latest_run = latest.try_get::<Option<String>, _>(1)?;
                let latest_status = latest.try_get::<String, _>(2)?;
                if latest_run.as_deref() != Some(run_id.to_string().as_str())
                    || latest_status != "revising"
                {
                    return Err(StoreError::InvalidInput(
                        "only the latest revising plan can be resumed".into(),
                    ));
                }
            } else {
                let active: i64 = sqlx::query_scalar(
                    "SELECT EXISTS(
                        SELECT 1 FROM proposed_plans
                        WHERE project_id=?1 AND frame_id=?2
                          AND status IN ('generating','revising','pending')
                    )",
                )
                .bind(project_id.to_string())
                .bind(conversation_id.to_string())
                .fetch_one(&mut *connection)
                .await?;
                if active != 0 {
                    return Err(StoreError::InvalidInput(
                        "conversation already has an active plan generation or proposal".into(),
                    ));
                }
            }

            let next_revision: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(revision),0)+1 FROM proposed_plans WHERE frame_id=?1",
            )
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            let revision = u64::try_from(next_revision)
                .map_err(|_| StoreError::InvalidInput("invalid next plan revision".into()))?;
            let seed_plan = generation_seed_plan(objective, revision);
            let plan_hash = seed_plan
                .canonical_hash()
                .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
            let plan_json = serde_json::to_string(&seed_plan)?;
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO proposed_plans
                 (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,'generating',?6,'',NULL,?7,?8,?8)",
            )
            .bind(id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(next_revision)
            .bind(&plan_hash)
            .bind(plan_json)
            .bind(run_id.to_string())
            .bind(timestamp)
            .execute(&mut *connection)
            .await?;
            sqlx::query(
                "INSERT INTO settings (scope,key,value_json,updated_at)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(scope,key) DO UPDATE SET value_json=excluded.value_json,
                 updated_at=excluded.updated_at",
            )
            .bind(SETTINGS_GLOBAL_SCOPE)
            .bind(conversation_agent_mode_setting_key(conversation_id))
            .bind(serde_json::to_string(&SessionAgentModeV4::Plan)?)
            .bind(timestamp)
            .execute(&mut *connection)
            .await?;

            let value_json: String = sqlx::query_scalar(
                "SELECT value_json FROM agent_runs_v4 WHERE run_id=?1",
            )
            .bind(run_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            let mut value: Value = serde_json::from_str(&value_json)?;
            let object = value.as_object_mut().ok_or_else(|| {
                StoreError::InvalidInput(
                    "plan resume requires agent_runs_v4.value_json to be a JSON object".into(),
                )
            })?;
            object.insert("status".into(), Value::String("planning".into()));
            object.insert("plan_revision".into(), Value::from(revision));
            sqlx::query(
                "UPDATE agent_runs_v4 SET status='planning',value_json=?,updated_at=? WHERE run_id=?3",
            )
            .bind(serde_json::to_string(&value)?)
            .bind(timestamp)
            .bind(run_id.to_string())
            .execute(&mut *connection)
            .await?;
            Ok(ProposedPlanRevisionV4 {
                id,
                project_id,
                conversation_id,
                run_id,
                revision,
                plan: seed_plan,
                markdown: String::new(),
                plan_hash,
                status: PlanRevisionStatusV4::Generating,
                feedback: None,
                created_at: stored_now,
                updated_at: stored_now,
            })
        }
        .await;
        match result {
            Ok(value) => {
                connection.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = connection.rollback().await;
                Err(error)
            }
        }
    }

    /// Finalize the reserved generating row in place. The database trigger
    /// permits exactly this generating -> pending content materialization;
    /// every later lifecycle update leaves identity/content/hash immutable.
    #[allow(clippy::too_many_arguments)]
    pub async fn finalize_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: String,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        self.finalize_plan_revision_v4_with_options(
            project_id,
            conversation_id,
            run_id,
            revision,
            plan,
            markdown,
            plan_hash,
            now,
            PlanRevisionFinalizeOptionsV4::default(),
        )
        .await
    }

    /// Finalize a generating revision and atomically persist the run metadata
    /// derived from that same model result. Any failure attempts a terminal
    /// cancellation through the Store before returning the original error;
    /// cleanup failures are retained in a combined diagnostic.
    #[allow(clippy::too_many_arguments)]
    pub async fn finalize_plan_revision_v4_with_options(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: String,
        now: DateTime<Utc>,
        options: PlanRevisionFinalizeOptionsV4,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        let result = Self::finalize_plan_revision_v4_inner(
            self.pool.clone(),
            project_id,
            conversation_id,
            run_id,
            revision,
            plan,
            markdown,
            plan_hash,
            now,
            options,
        )
        .await;
        let error = match result {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let cleanup_feedback = format!("plan finalization failed: {error}");
        match self
            .terminate_plan_generation_v4(
                project_id,
                conversation_id,
                run_id,
                revision,
                PlanRevisionStatusV4::Cancelled,
                Some(&cleanup_feedback),
            )
            .await
        {
            Ok(_) => Err(error),
            Err(cleanup_error) => Err(StoreError::InvalidInput(format!(
                "{error}; cleanup failed: {cleanup_error}"
            ))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn finalize_plan_revision_v4_inner(
        pool: SqlitePool,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: String,
        now: DateTime<Utc>,
        options: PlanRevisionFinalizeOptionsV4,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        validate_plan_revision_input(
            project_id,
            conversation_id,
            run_id,
            revision,
            &plan,
            &plan_hash,
            PlanRevisionStatusV4::Pending,
        )?;
        let actual_hash = plan
            .canonical_hash()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        if actual_hash != plan_hash {
            return Err(StoreError::InvalidInput(
                "proposed plan hash does not match the structured plan".into(),
            ));
        }
        let timestamp = timestamp(now);
        let stored_now = from_timestamp(timestamp, "plan revision timestamp")?;
        let mut connection = pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result = finalize_plan_revision_v4_in_connection(
            &mut *connection,
            project_id,
            conversation_id,
            run_id,
            revision,
            plan,
            markdown,
            plan_hash,
            timestamp,
            stored_now,
            options,
        )
        .await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    /// Mark a still-generating revision as revising (for an input pause) or
    /// cancelled (for a failed/cancelled generation), releasing the lock when
    /// the terminal cancellation path is used.
    pub async fn terminate_plan_generation_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        status: PlanRevisionStatusV4,
        feedback: Option<&str>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        if !matches!(
            status,
            PlanRevisionStatusV4::Revising | PlanRevisionStatusV4::Cancelled
        ) {
            return Err(StoreError::InvalidInput(
                "plan generation can only terminate as revising or cancelled".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
        let row = sqlx::query(
            "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
             FROM proposed_plans WHERE project_id=?1 AND frame_id=?2 AND run_id=?3 AND revision=?4",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .bind(run_id.to_string())
        .bind(i64::try_from(revision).map_err(|_| {
            StoreError::InvalidInput("plan revision exceeds SQLite integer range".into())
        })?)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::InvalidInput("plan generation revision was not found".into()))?;
        let current = proposed_plan_revision_from_row(row)?;
        if current.status != PlanRevisionStatusV4::Generating {
            return Err(StoreError::InvalidInput(
                "plan generation is no longer active".into(),
            ));
        }
        let now = Utc::now();
        let stored_now = from_timestamp(timestamp(now), "plan revision timestamp")?;
        let terminated = sqlx::query(
            "UPDATE proposed_plans SET status=?1,feedback=?2,updated_at=?3 WHERE id=?4 AND status='generating'",
        )
        .bind(enum_string(&status)?)
        .bind(feedback)
        .bind(timestamp(stored_now))
        .bind(current.id.to_string())
        .execute(&mut *tx)
        .await?;
        if terminated.rows_affected() != 1 {
            return Err(StoreError::InvalidInput(
                "plan generation changed before it could be terminated".into(),
            ));
        }
        set_agent_run_status_in_tx(
            &mut *tx,
            run_id,
            if status == PlanRevisionStatusV4::Cancelled {
                "cancelled"
            } else {
                "waiting_for_input"
            },
        )
        .await?;
        if status == PlanRevisionStatusV4::Cancelled {
            let existing = load_agent_events_in_tx(&mut *tx, run_id).await?;
            if let Some(terminal) = existing.iter().find(|event| is_terminal_event(event)) {
                if !matches!(&terminal.event, AgentEventKindV4::RunCancelled) {
                    return Err(StoreError::InvalidInput(
                        "plan generation run already has a different terminal event".into(),
                    ));
                }
            } else {
                let event = if let Some(previous) = existing.last() {
                    AgentEventV4::next(previous, now, AgentEventKindV4::RunCancelled)
                } else {
                    AgentEventV4::first(
                        run_id,
                        project_id,
                        conversation_id,
                        now,
                        AgentEventKindV4::RunCancelled,
                    )
                };
                insert_agent_event_in_tx(&mut *tx, &event).await?;
            }
        }
        tx.commit().await?;
        Ok(ProposedPlanRevisionV4 {
            status,
            feedback: feedback.map(ToOwned::to_owned),
            updated_at: stored_now,
            ..current
        })
    }

    /// Insert one immutable proposed-plan revision.
    ///
    /// The revision, plan JSON, Markdown, and content hash are never updated
    /// in place. Lifecycle metadata is changed only by the explicit
    /// request/approve/cancel transitions below.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_proposed_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: String,
        status: PlanRevisionStatusV4,
        feedback: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        validate_plan_revision_input(
            project_id,
            conversation_id,
            run_id,
            revision,
            &plan,
            &plan_hash,
            status,
        )?;
        let actual_hash = plan
            .canonical_hash()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        if actual_hash != plan_hash {
            return Err(StoreError::InvalidInput(
                "proposed plan hash does not match the structured plan".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
        let run_status: String =
            sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
                .bind(run_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if matches!(
            run_status.as_str(),
            "completed" | "cancelled" | "failed" | "needs_attention"
        ) {
            return Err(StoreError::InvalidInput(
                "cannot create a plan revision for a terminal run".into(),
            ));
        }
        let active = sqlx::query(
            "SELECT run_id,status FROM proposed_plans
             WHERE project_id=?1 AND frame_id=?2
               AND status IN ('generating','revising','pending')
             ORDER BY revision DESC,id DESC LIMIT 1",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(active) = active {
            let active_run = active.try_get::<Option<String>, _>(0)?;
            let active_status = active.try_get::<String, _>(1)?;
            let allowed_materialization = status == PlanRevisionStatusV4::Pending
                && active_run.as_deref() == Some(run_id.to_string().as_str())
                && active_status == "revising";
            if !allowed_materialization {
                return Err(StoreError::InvalidInput(
                    "conversation already has an active plan revision".into(),
                ));
            }
        }
        let plan_json = serde_json::to_string(&plan)?;
        let status_string = enum_string(&status)?;
        let timestamp = timestamp(now);
        let stored_now = from_timestamp(timestamp, "plan revision timestamp")?;
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO proposed_plans
             (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .bind(i64::try_from(revision).map_err(|_| {
            StoreError::InvalidInput("plan revision exceeds SQLite integer range".into())
        })?)
        .bind(&plan_hash)
        .bind(status_string)
        .bind(plan_json)
        .bind(&markdown)
        .bind(feedback.as_deref())
        .bind(run_id.to_string())
        .bind(timestamp)
        .bind(timestamp)
        .execute(&mut *tx)
        .await?;
        if status.is_active() {
            upsert_conversation_mode_in_tx(
                &mut *tx,
                conversation_id,
                SessionAgentModeV4::Plan,
                stored_now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(ProposedPlanRevisionV4 {
            id,
            project_id,
            conversation_id,
            run_id,
            revision,
            plan,
            markdown,
            plan_hash,
            status,
            feedback,
            created_at: stored_now,
            updated_at: stored_now,
        })
    }

    /// Materialize revision one for a pre-revision V4 run that already has a
    /// plan/hash in its legacy run record. This compatibility seam is
    /// intentionally separate from `create_next_*`, which requires a latest
    /// `revising` revision and is therefore not valid for this migration path.
    pub async fn create_legacy_approval_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: String,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        if self
            .latest_proposed_plan_revision_v4(project_id, conversation_id)
            .await?
            .is_some()
        {
            return Err(StoreError::InvalidInput(
                "legacy approval revision can only be created when no proposal exists".into(),
            ));
        }
        self.create_proposed_plan_revision_v4(
            project_id,
            conversation_id,
            run_id,
            1,
            plan,
            markdown,
            plan_hash,
            PlanRevisionStatusV4::Pending,
            None,
            now,
        )
        .await
    }

    /// Insert the next revision number for a conversation. The current
    /// revision is left intact; a caller may use this after a revision request
    /// has put the conversation into `revising`.
    pub async fn create_next_proposed_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: String,
        status: PlanRevisionStatusV4,
        feedback: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        validate_plan_revision_input(
            project_id,
            conversation_id,
            run_id,
            1,
            &plan,
            &plan_hash,
            status,
        )?;
        let actual_hash = plan
            .canonical_hash()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        if actual_hash != plan_hash {
            return Err(StoreError::InvalidInput(
                "proposed plan hash does not match the structured plan".into(),
            ));
        }
        let timestamp = timestamp(now);
        let stored_now = from_timestamp(timestamp, "plan revision timestamp")?;
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result: Result<ProposedPlanRevisionV4, StoreError> = async {
            let owns_conversation: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM conversation_records
                    WHERE frame_id=?1 AND project_id=?2
                )",
            )
            .bind(conversation_id.to_string())
            .bind(project_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if owns_conversation == 0 {
                return Err(StoreError::InvalidInput(
                    "proposed plan context is not owned by the project".into(),
                ));
            }
            let owns_run: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM agent_runs_v4
                    WHERE run_id=?1 AND project_id=?2 AND conversation_id=?3
                )",
            )
            .bind(run_id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if owns_run == 0 {
                return Err(StoreError::InvalidInput(
                    "proposed plan run does not belong to the requested context".into(),
                ));
            }
            let run_status: String = sqlx::query_scalar(
                "SELECT status FROM agent_runs_v4 WHERE run_id=?1",
            )
            .bind(run_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if matches!(
                run_status.as_str(),
                "completed" | "cancelled" | "failed" | "needs_attention"
            ) {
                return Err(StoreError::InvalidInput(
                    "cannot create a plan revision for a terminal run".into(),
                ));
            }
            if status != PlanRevisionStatusV4::Pending {
                return Err(StoreError::InvalidInput(
                    "low-level next plan revisions must be pending materializations".into(),
                ));
            }
            let latest = sqlx::query(
                "SELECT run_id,status FROM proposed_plans
                 WHERE project_id=?1 AND frame_id=?2
                 ORDER BY revision DESC,id DESC LIMIT 1",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_optional(&mut *connection)
            .await?;
            let latest = latest.ok_or_else(|| {
                StoreError::InvalidInput(
                    "next plan revision requires the latest matching revising plan".into(),
                )
            })?;
            let latest_run = latest.try_get::<Option<String>, _>(0)?;
            let latest_status = latest.try_get::<String, _>(1)?;
            if latest_run.as_deref() != Some(run_id.to_string().as_str())
                || latest_status != "revising"
            {
                return Err(StoreError::InvalidInput(
                    "next plan revision requires the latest matching revising plan".into(),
                ));
            }
            let next_revision: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(revision),0)+1 FROM proposed_plans WHERE frame_id=?1",
            )
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            let revision = u64::try_from(next_revision)
                .map_err(|_| StoreError::InvalidInput("invalid next plan revision".into()))?;
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO proposed_plans
                 (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
            )
            .bind(id.to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(next_revision)
            .bind(&plan_hash)
            .bind(enum_string(&status)?)
            .bind(serde_json::to_string(&plan)?)
            .bind(&markdown)
            .bind(feedback.as_deref())
            .bind(run_id.to_string())
            .bind(timestamp)
            .execute(&mut *connection)
            .await?;
            if status.is_active() {
                sqlx::query(
                    "INSERT INTO settings (scope,key,value_json,updated_at)
                     VALUES (?1,?2,?3,?4)
                     ON CONFLICT(scope,key) DO UPDATE SET value_json=excluded.value_json,
                     updated_at=excluded.updated_at",
                )
                .bind(SETTINGS_GLOBAL_SCOPE)
                .bind(conversation_agent_mode_setting_key(conversation_id))
                .bind(serde_json::to_string(&SessionAgentModeV4::Plan)?)
                .bind(timestamp)
                .execute(&mut *connection)
                .await?;
            }
            Ok(ProposedPlanRevisionV4 {
                id,
                project_id,
                conversation_id,
                run_id,
                revision,
                plan,
                markdown,
                plan_hash,
                status,
                feedback,
                created_at: stored_now,
                updated_at: stored_now,
            })
        }
        .await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    /// Compatibility spelling for Store consumers that do not include the
    /// protocol version in their repository method names.
    #[allow(clippy::too_many_arguments)]
    pub async fn save_proposed_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: String,
        status: PlanRevisionStatusV4,
        feedback: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        self.create_proposed_plan_revision_v4(
            project_id,
            conversation_id,
            run_id,
            revision,
            plan,
            markdown,
            plan_hash,
            status,
            feedback,
            now,
        )
        .await
    }

    pub async fn proposed_plan_revision_v4(
        &self,
        revision_id: Uuid,
    ) -> Result<Option<ProposedPlanRevisionV4>, StoreError> {
        let row = sqlx::query(
            "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
             FROM proposed_plans WHERE id=?1",
        )
        .bind(revision_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(proposed_plan_revision_from_row).transpose()
    }

    pub async fn proposed_plan_revisions_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Vec<ProposedPlanRevisionV4>, StoreError> {
        self.ensure_conversation_owner(project_id, conversation_id)
            .await?;
        let rows = sqlx::query(
            "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
             FROM proposed_plans WHERE project_id=?1 AND frame_id=?2 ORDER BY revision,id",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(proposed_plan_revision_from_row)
            .collect()
    }

    pub async fn list_proposed_plan_revisions_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Vec<ProposedPlanRevisionV4>, StoreError> {
        self.proposed_plan_revisions_v4(project_id, conversation_id)
            .await
    }

    pub async fn latest_proposed_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Option<ProposedPlanRevisionV4>, StoreError> {
        self.ensure_conversation_owner(project_id, conversation_id)
            .await?;
        let row = sqlx::query(
            "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
             FROM proposed_plans WHERE project_id=?1 AND frame_id=?2 ORDER BY revision DESC,id DESC LIMIT 1",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(proposed_plan_revision_from_row).transpose()
    }

    pub async fn latest_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Option<ProposedPlanRevisionV4>, StoreError> {
        self.latest_proposed_plan_revision_v4(project_id, conversation_id)
            .await
    }

    pub async fn latest_pending_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Option<ProposedPlanRevisionV4>, StoreError> {
        self.ensure_conversation_owner(project_id, conversation_id)
            .await?;
        let row = sqlx::query(
            "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
             FROM proposed_plans
             WHERE project_id=?1 AND frame_id=?2 AND status='pending'
             ORDER BY revision DESC,id DESC LIMIT 1",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(proposed_plan_revision_from_row).transpose()
    }

    /// Lifecycle transitions must go through the transaction APIs. This
    /// method deliberately rejects all direct updates so plan content and
    /// hashes cannot be rewritten by a generic persistence caller.
    pub async fn update_proposed_plan_revision_v4(
        &self,
        _revision_id: Uuid,
        _markdown: String,
        _status: PlanRevisionStatusV4,
        _feedback: Option<String>,
    ) -> Result<(), StoreError> {
        Err(StoreError::InvalidInput(
            "proposed plan revisions are immutable; use a lifecycle transition".into(),
        ))
    }

    /// Record feedback against the latest revision and move that revision to
    /// `revising`. The plan JSON/hash/revision remain unchanged. The next
    /// planner result is inserted as a new immutable revision.
    pub async fn request_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        plan_hash: &str,
        feedback: &str,
    ) -> Result<ProposedPlanRevisionV4, StoreError> {
        self.request_plan_revision_v4_with_event(
            project_id,
            conversation_id,
            run_id,
            plan_hash,
            feedback,
        )
        .await
        .map(|result| result.revision)
    }

    /// Request changes and return the event committed alongside the
    /// lifecycle transition. Tauri broadcasts this event only after this
    /// method returns successfully.
    pub async fn request_plan_revision_v4_with_event(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        plan_hash: &str,
        feedback: &str,
    ) -> Result<PlanRevisionRequestResultV4, StoreError> {
        self.request_plan_revision_v4_with_options(
            project_id,
            conversation_id,
            run_id,
            plan_hash,
            feedback,
            PlanRevisionRequestOptionsV4::default(),
        )
        .await
    }

    pub async fn request_plan_revision_v4_with_options(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        plan_hash: &str,
        feedback: &str,
        options: PlanRevisionRequestOptionsV4,
    ) -> Result<PlanRevisionRequestResultV4, StoreError> {
        let feedback = feedback.trim();
        if feedback.is_empty() {
            return Err(StoreError::InvalidInput(
                "plan revision feedback cannot be empty".into(),
            ));
        }
        let mut connection = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<PlanRevisionRequestResultV4, StoreError> = async {
            ensure_conversation_owner_executor(&mut *connection, project_id, conversation_id)
                .await?;
            ensure_run_owner_executor(&mut *connection, project_id, conversation_id, run_id).await?;
            let run_status: String =
                sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
                    .bind(run_id.to_string())
                    .fetch_one(&mut *connection)
                    .await?;
            if matches!(
                run_status.as_str(),
                "completed" | "cancelled" | "failed" | "needs_attention"
            ) {
                return Err(StoreError::InvalidInput(
                    "terminal run cannot request a plan revision".into(),
                ));
            }
            let row = sqlx::query(
                "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
                 FROM proposed_plans WHERE project_id=?1 AND frame_id=?2
                 ORDER BY revision DESC,id DESC LIMIT 1",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("no proposed plan revision exists".into()))?;
            let current = proposed_plan_revision_from_row(row)?;
            if current.run_id != run_id {
                return Err(StoreError::InvalidInput(
                    "plan revision does not belong to the requested run".into(),
                ));
            }
            if current.plan_hash != plan_hash {
                return Err(StoreError::InvalidInput(
                    "plan revision hash does not match the latest revision".into(),
                ));
            }
            if current.status != PlanRevisionStatusV4::Pending {
                return Err(StoreError::InvalidInput(
                    "plan revision is no longer pending; only one concurrent request may transition it".into(),
                ));
            }
            let now = Utc::now();
            let stored_now = from_timestamp(timestamp(now), "plan revision timestamp")?;
            let updated = sqlx::query(
                "UPDATE proposed_plans SET status='revising',feedback=?1,updated_at=?2
                 WHERE id=?3 AND status='pending'",
            )
            .bind(feedback)
            .bind(timestamp(stored_now))
            .bind(current.id.to_string())
            .execute(&mut *connection)
            .await?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::InvalidInput(
                    "plan revision changed before feedback could be recorded".into(),
                ));
            }
            set_agent_run_status_in_tx(&mut *connection, run_id, "planning").await?;
            maybe_fail_plan_request(options, 1)?;
            let existing = load_agent_events_in_tx(&mut *connection, run_id).await?;
            let event = if let Some(previous) = existing.last() {
                AgentEventV4::next(
                    previous,
                    now,
                    AgentEventKindV4::PlanRevisionRequested {
                        plan_hash: plan_hash.to_owned(),
                        feedback: feedback.to_owned(),
                    },
                )
            } else {
                AgentEventV4::first(
                    run_id,
                    project_id,
                    conversation_id,
                    now,
                    AgentEventKindV4::PlanRevisionRequested {
                        plan_hash: plan_hash.to_owned(),
                        feedback: feedback.to_owned(),
                    },
                )
            };
            insert_agent_event_in_tx(&mut *connection, &event).await?;
            Ok(PlanRevisionRequestResultV4 {
                revision: ProposedPlanRevisionV4 {
                    status: PlanRevisionStatusV4::Revising,
                    feedback: Some(feedback.to_owned()),
                    updated_at: stored_now,
                    ..current
                },
                event,
            })
        }
        .await;
        match result {
            Ok(value) => {
                connection.commit().await?;
                Ok(value)
            }
            Err(error) => {
                let _ = connection.rollback().await;
                Err(error)
            }
        }
    }

    /// Return whether this exact conversation has an active plan revision.
    /// The project ownership check prevents a project from observing another
    /// project's lock by guessing a conversation id.
    pub async fn conversation_plan_lock_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<bool, StoreError> {
        self.ensure_conversation_owner(project_id, conversation_id)
            .await?;
        let locked: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM proposed_plans
                WHERE project_id=?1 AND frame_id=?2
                  AND status IN ('generating','revising','pending')
            )",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(locked != 0)
    }

    pub async fn is_conversation_locked_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<bool, StoreError> {
        self.conversation_plan_lock_v4(project_id, conversation_id)
            .await
    }

    pub async fn conversation_is_locked_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<bool, StoreError> {
        self.conversation_plan_lock_v4(project_id, conversation_id)
            .await
    }

    /// Guard ordinary direct/planning sends. Explicit plan actions call their
    /// own transition methods and are therefore allowed while this guard is
    /// active.
    pub async fn ensure_conversation_unlocked_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<(), StoreError> {
        if self
            .conversation_plan_lock_v4(project_id, conversation_id)
            .await?
        {
            return Err(StoreError::InvalidInput(
                "conversation is locked by an active plan; approve, request changes, or cancel it first".into(),
            ));
        }
        Ok(())
    }

    pub async fn ensure_plan_action_allowed_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<(), StoreError> {
        self.ensure_conversation_owner(project_id, conversation_id)
            .await
    }

    /// Atomically approve the latest pending revision, freeze the execution
    /// specification, append approval/mode events, switch the conversation to
    /// Agent mode, and persist the running record. The caller must not spawn
    /// execution until this method returns successfully.
    #[allow(clippy::too_many_arguments)]
    pub async fn approve_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        plan_hash: &str,
        spec: &RunSpecV4,
        run_value: &Value,
    ) -> Result<PlanApprovalResultV4, StoreError> {
        self.approve_plan_revision_v4_with_options(
            project_id,
            conversation_id,
            run_id,
            revision,
            plan_hash,
            spec,
            run_value,
            ApprovalOptionsV4::default(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn approve_plan_revision_v4_with_options(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        revision: u64,
        plan_hash: &str,
        spec: &RunSpecV4,
        run_value: &Value,
        options: ApprovalOptionsV4,
    ) -> Result<PlanApprovalResultV4, StoreError> {
        validate_approval_inputs(project_id, conversation_id, run_id, spec, run_value)?;
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result = approve_plan_revision_v4_in_connection(
            &mut *connection,
            project_id,
            conversation_id,
            run_id,
            revision,
            plan_hash,
            spec,
            run_value,
            options,
        )
        .await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    /// Compatibility approval for a pre-revision run. Revision one is
    /// materialized and approved in the same `BEGIN IMMEDIATE` transaction so
    /// an invalid plan/spec/hash or a concurrent state change cannot leave a
    /// pending proposal behind.
    #[allow(clippy::too_many_arguments)]
    pub async fn approve_legacy_plan_revision_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: &str,
        spec: &RunSpecV4,
        run_value: &Value,
    ) -> Result<PlanApprovalResultV4, StoreError> {
        self.approve_legacy_plan_revision_v4_with_options(
            project_id,
            conversation_id,
            run_id,
            plan,
            markdown,
            plan_hash,
            spec,
            run_value,
            ApprovalOptionsV4::default(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn approve_legacy_plan_revision_v4_with_options(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        plan: ExecutionPlanV4,
        markdown: String,
        plan_hash: &str,
        spec: &RunSpecV4,
        run_value: &Value,
        options: ApprovalOptionsV4,
    ) -> Result<PlanApprovalResultV4, StoreError> {
        validate_approval_inputs(project_id, conversation_id, run_id, spec, run_value)?;
        let actual_hash = plan
            .canonical_hash()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        if actual_hash != plan_hash || spec.plan != plan || spec.approved_plan_hash != plan_hash {
            return Err(StoreError::InvalidInput(
                "legacy plan hash does not match the frozen plan".into(),
            ));
        }
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        let result: Result<PlanApprovalResultV4, StoreError> = async {
            ensure_conversation_owner_executor(&mut *connection, project_id, conversation_id)
                .await?;
            ensure_run_owner_executor(&mut *connection, project_id, conversation_id, run_id)
                .await?;
            ensure_run_awaiting_approval(&mut *connection, run_id).await?;
            let proposal_exists: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM proposed_plans
                    WHERE project_id=?1 AND frame_id=?2
                )",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
            if proposal_exists != 0 {
                return Err(StoreError::InvalidInput(
                    "legacy approval requires no existing plan revision".into(),
                ));
            }
            let stored_value = load_agent_run_value_in_tx(&mut *connection, run_id).await?;
            let stored_plan = stored_value.get("plan").cloned().ok_or_else(|| {
                StoreError::InvalidInput("legacy run is missing its persisted plan".into())
            })?;
            let stored_plan: ExecutionPlanV4 =
                serde_json::from_value(stored_plan).map_err(|error| {
                    StoreError::InvalidInput(format!(
                        "legacy run has invalid persisted plan: {error}"
                    ))
                })?;
            let stored_hash = stored_value
                .get("plan_hash")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    StoreError::InvalidInput("legacy run is missing its plan hash".into())
                })?;
            if stored_plan != plan || stored_hash != plan_hash {
                return Err(StoreError::InvalidInput(
                    "legacy plan does not match the persisted run plan".into(),
                ));
            }
            insert_proposed_plan_revision_in_connection(
                &mut *connection,
                project_id,
                conversation_id,
                run_id,
                1,
                plan,
                markdown,
                plan_hash,
                PlanRevisionStatusV4::Pending,
                None,
                Utc::now(),
            )
            .await?;
            approve_plan_revision_v4_in_connection(
                &mut *connection,
                project_id,
                conversation_id,
                run_id,
                1,
                plan_hash,
                spec,
                run_value,
                options,
            )
            .await
        }
        .await;
        match result {
            Ok(value) => {
                sqlx::query("COMMIT").execute(&mut *connection).await?;
                Ok(value)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }

    /// Cancel an active planning revision. The conversation remains in Plan
    /// mode and becomes available for ordinary sends after commit.
    pub async fn cancel_plan_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<PlanCancellationResultV4, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
        ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
        let rows = sqlx::query(
            "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
             FROM proposed_plans WHERE project_id=?1 AND frame_id=?2
               AND status IN ('generating','revising','pending')
             ORDER BY revision DESC,id DESC",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let current = rows
            .into_iter()
            .next()
            .map(proposed_plan_revision_from_row)
            .transpose()?;
        let now = Utc::now();
        if current.is_none() {
            let latest = sqlx::query(
                "SELECT revision,status FROM proposed_plans
                 WHERE project_id=?1 AND frame_id=?2 AND run_id=?3
                 ORDER BY revision DESC,id DESC LIMIT 1",
            )
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(run_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            let Some(latest) = latest else {
                return Err(StoreError::InvalidInput(
                    "no active plan revision to cancel".into(),
                ));
            };
            let latest_revision = u64::try_from(latest.try_get::<i64, _>(0)?)
                .map_err(|_| StoreError::InvalidInput("invalid plan revision".into()))?;
            let latest_status = latest.try_get::<String, _>(1)?;
            let run_status: String =
                sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
                    .bind(run_id.to_string())
                    .fetch_one(&mut *tx)
                    .await?;
            let existing = load_agent_events_in_tx(&mut *tx, run_id).await?;
            if run_status == "cancelled"
                && latest_status == "cancelled"
                && existing
                    .iter()
                    .any(|event| matches!(event.event, AgentEventKindV4::RunCancelled))
            {
                upsert_conversation_mode_in_tx(
                    &mut *tx,
                    conversation_id,
                    SessionAgentModeV4::Plan,
                    now,
                )
                .await?;
                tx.commit().await?;
                return Ok(PlanCancellationResultV4 {
                    run_id,
                    project_id,
                    conversation_id,
                    revision: latest_revision,
                    mode: SessionAgentModeV4::Plan,
                    events: vec![],
                });
            }
            return Err(StoreError::InvalidInput(
                "no active plan revision to cancel".into(),
            ));
        }
        let current = current.expect("checked above");
        if current.run_id != run_id {
            return Err(StoreError::InvalidInput(
                "only the newest active plan revision can be cancelled".into(),
            ));
        }
        let cancelled = sqlx::query(
            "UPDATE proposed_plans SET status='cancelled',updated_at=?1
             WHERE project_id=?2 AND frame_id=?3 AND run_id=?4
               AND status IN ('generating','revising','pending')",
        )
        .bind(timestamp(now))
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .bind(run_id.to_string())
        .execute(&mut *tx)
        .await?;
        if cancelled.rows_affected() == 0 {
            return Err(StoreError::InvalidInput(
                "plan changed before cancellation could be committed".into(),
            ));
        }
        // Cancellation of the newest revision also closes any older active
        // rows left by a recovered/imported database. Keeping those rows
        // active would retain the conversation lock after the run is closed.
        sqlx::query(
            "UPDATE proposed_plans SET status='superseded',updated_at=?1
             WHERE project_id=?2 AND frame_id=?3 AND id<>?4
               AND status IN ('generating','revising','pending')",
        )
        .bind(timestamp(now))
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .bind(current.id.to_string())
        .execute(&mut *tx)
        .await?;
        let mut persisted_value = load_agent_run_value_in_tx(&mut *tx, run_id).await?;
        let object = persisted_value.as_object_mut().ok_or_else(|| {
            StoreError::InvalidInput(
                "plan cancellation requires agent_runs_v4.value_json to be a JSON object".into(),
            )
        })?;
        object.insert("status".into(), Value::String("cancelled".into()));
        object.insert("session_mode".into(), Value::String("plan".into()));
        sqlx::query(
            "UPDATE agent_runs_v4 SET status='cancelled',value_json=?,updated_at=? WHERE run_id=?3",
        )
        .bind(serde_json::to_string(&persisted_value)?)
        .bind(timestamp(now))
        .bind(run_id.to_string())
        .execute(&mut *tx)
        .await?;
        let existing = load_agent_events_in_tx(&mut *tx, run_id).await?;
        let event = if existing.iter().any(is_terminal_event) {
            if existing
                .iter()
                .any(|event| matches!(event.event, AgentEventKindV4::RunCancelled))
            {
                None
            } else {
                return Err(StoreError::InvalidInput(
                    "V4 run event chain is already terminal".into(),
                ));
            }
        } else if let Some(previous) = existing.last() {
            Some(AgentEventV4::next(
                previous,
                now,
                AgentEventKindV4::RunCancelled,
            ))
        } else {
            Some(AgentEventV4::first(
                run_id,
                project_id,
                conversation_id,
                now,
                AgentEventKindV4::RunCancelled,
            ))
        };
        if let Some(event) = &event {
            insert_agent_event_in_tx(&mut *tx, event).await?;
        }
        upsert_conversation_mode_in_tx(&mut *tx, conversation_id, SessionAgentModeV4::Plan, now)
            .await?;
        tx.commit().await?;
        Ok(PlanCancellationResultV4 {
            run_id,
            project_id,
            conversation_id,
            revision: current.revision,
            mode: SessionAgentModeV4::Plan,
            events: event.into_iter().collect(),
        })
    }

    pub async fn cancel_proposed_plan_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
    ) -> Result<PlanCancellationResultV4, StoreError> {
        self.cancel_plan_v4(project_id, conversation_id, run_id)
            .await
    }

    pub async fn agent_runs_for_context_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Vec<Value>, StoreError> {
        let rows = sqlx::query(
            "SELECT value_json FROM agent_runs_v4 WHERE project_id=?1 AND conversation_id=?2 ORDER BY rowid",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .collect()
    }

    /// Validate and append one event. A terminal completion and its public
    /// assistant message are committed together, so a restart cannot expose
    /// one without the other.
    pub async fn append_agent_event_v4_with_conversation(
        &self,
        event: &AgentEventV4,
    ) -> Result<Option<Message>, StoreError> {
        if event.schema_version != 4 {
            return Err(StoreError::InvalidInput(
                "only Agent Event schema version 4 is supported".into(),
            ));
        }
        event
            .verify()
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure_event_context_tx(&mut tx, event).await?;
        let serialized =
            sqlx::query("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence")
                .bind(event.run_id.to_string())
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .map(|row| row.try_get::<String, _>(0))
                .collect::<Result<Vec<_>, _>>()?;
        let existing = deserialize_event_chain_v4(&serialized)
            .map_err(|error| StoreError::InvalidInput(error.to_string()))?;

        if let Some(stored) = existing
            .iter()
            .find(|stored| stored.sequence == event.sequence)
        {
            if stored.event_hash != event.event_hash || stored != event {
                return Err(StoreError::InvalidInput(format!(
                    "event sequence {} is already occupied by a different event",
                    event.sequence
                )));
            }
            let message = if is_run_completed(event) {
                persist_completion_message(&mut tx, &existing, event).await?
            } else {
                None
            };
            tx.commit().await?;
            return Ok(message);
        }

        if is_run_completed(event)
            && sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent_guidance_v4 WHERE run_id=?1 AND consumed_at IS NULL",
            )
            .bind(event.run_id.to_string())
            .fetch_one(&mut *tx)
            .await?
                > 0
        {
            return Err(StoreError::GuidancePending);
        }
        if existing.iter().any(is_terminal_event)
            && !post_terminal_browser_cleanup_allowed(&existing, event)
        {
            return Err(StoreError::InvalidInput(
                "V4 run event chain is terminal; no events may be appended".into(),
            ));
        }

        let chain_continues = existing.last().map_or_else(
            || event.sequence == 1 && event.previous_hash.is_empty(),
            |previous| {
                event.sequence == previous.sequence + 1
                    && event.previous_hash == previous.event_hash
            },
        );
        if !chain_continues {
            return Err(StoreError::InvalidInput("broken V4 event chain".into()));
        }
        let mut candidate = existing;
        candidate.push(event.clone());
        let sequence = i64::try_from(event.sequence)
            .map_err(|_| StoreError::InvalidInput("event sequence exceeds SQLite range".into()))?;
        sqlx::query(
            "INSERT INTO agent_events_v4
             (run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json,occurred_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        )
        .bind(event.run_id.to_string())
        .bind(event.project_id.to_string())
        .bind(event.conversation_id.to_string())
        .bind(sequence)
        .bind(&event.previous_hash)
        .bind(&event.event_hash)
        .bind(serde_json::to_string(event)?)
        .bind(timestamp(event.occurred_at))
        .execute(&mut *tx)
        .await?;
        let message = if is_run_completed(event) {
            persist_completion_message(&mut tx, &candidate, event).await?
        } else {
            None
        };
        runtime_jobs::observe_runtime_event(&mut tx, event).await?;
        tx.commit().await?;
        Ok(message)
    }

    pub async fn append_agent_event_v4(&self, event: &AgentEventV4) -> Result<(), StoreError> {
        self.append_agent_event_v4_with_conversation(event)
            .await
            .map(|_| ())
    }

    /// Validate and append exactly one tool-approval decision while holding a
    /// SQLite write lock. The caller supplies the binding that is valid for
    /// the current phase: the historical Execute spec hash, or the current
    /// Plan revision scope hash. This prevents two concurrent decide calls
    /// from both observing an undecided request and appending decisions.
    #[allow(clippy::too_many_arguments)]
    pub async fn decide_tool_approval_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        approval_id: &str,
        call_hash: &str,
        decision: ToolApprovalDecisionV4,
        binding_hash: &str,
        expected_mode: RunModeV4,
        expected_scope_hash: Option<&str>,
    ) -> Result<AgentEventV4, StoreError> {
        self.decide_tool_approval_v4_inner(
            project_id,
            conversation_id,
            run_id,
            approval_id,
            call_hash,
            decision,
            binding_hash,
            expected_mode,
            expected_scope_hash,
            None,
        )
        .await
    }

    /// Atomically persist an approved browser grant with its exact approval
    /// decision. A caller can never observe a committed decision whose
    /// requested durable browser scope was silently lost.
    #[allow(clippy::too_many_arguments)]
    pub async fn decide_tool_approval_v4_with_browser_authorization(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        approval_id: &str,
        call_hash: &str,
        decision: ToolApprovalDecisionV4,
        binding_hash: &str,
        authorization: &BrowserAuthorizationV4,
    ) -> Result<AgentEventV4, StoreError> {
        self.decide_tool_approval_v4_inner(
            project_id,
            conversation_id,
            run_id,
            approval_id,
            call_hash,
            decision,
            binding_hash,
            RunModeV4::Execute,
            None,
            Some(authorization),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn decide_tool_approval_v4_inner(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        approval_id: &str,
        call_hash: &str,
        decision: ToolApprovalDecisionV4,
        binding_hash: &str,
        expected_mode: RunModeV4,
        expected_scope_hash: Option<&str>,
        browser_authorization: Option<&BrowserAuthorizationV4>,
    ) -> Result<AgentEventV4, StoreError> {
        if approval_id.trim().is_empty() || call_hash.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "tool approval decision identifiers cannot be empty".into(),
            ));
        }
        if let Some(authorization) = browser_authorization {
            authorization
                .validate()
                .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
            let context_matches = match authorization.scope {
                BrowserApprovalScopeV4::Once | BrowserApprovalScopeV4::Conversation => {
                    authorization.project_id == Some(project_id)
                        && authorization.conversation_id == Some(conversation_id)
                }
                BrowserApprovalScopeV4::Project => {
                    authorization.project_id == Some(project_id)
                        && authorization.conversation_id.is_none()
                }
                BrowserApprovalScopeV4::Global => {
                    authorization.project_id.is_none() && authorization.conversation_id.is_none()
                }
            };
            if decision != ToolApprovalDecisionV4::Approved
                || expected_mode != RunModeV4::Execute
                || !context_matches
            {
                return Err(StoreError::InvalidInput(
                    "browser authorization does not match the approved Execute context".into(),
                ));
            }
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result: Result<AgentEventV4, StoreError> = async {
            ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
            ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
            let mut current_plan_scope = None;
            if expected_mode == RunModeV4::Plan {
                let status: String =
                    sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
                        .bind(run_id.to_string())
                        .fetch_one(&mut *tx)
                        .await?;
                let latest = sqlx::query(
                    "SELECT id,revision,run_id,status FROM proposed_plans
                     WHERE project_id=?1 AND frame_id=?2
                     ORDER BY revision DESC,id DESC LIMIT 1",
                )
                .bind(project_id.to_string())
                .bind(conversation_id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    StoreError::InvalidInput("Plan approval has no current revision".into())
                })?;
                let latest_id = Uuid::parse_str(&latest.try_get::<String, _>(0)?).map_err(|_| {
                    StoreError::InvalidInput("Plan approval current revision has an invalid id".into())
                })?;
                let latest_revision = u64::try_from(latest.try_get::<i64, _>(1)?).map_err(|_| {
                    StoreError::InvalidInput("Plan approval current revision is invalid".into())
                })?;
                let latest_run = latest.try_get::<String, _>(2)?;
                let latest_status = latest.try_get::<String, _>(3)?;
                let consistent_phase =
                    (status == "planning" && latest_status == "generating")
                        || (status == "waiting_for_approval" && latest_status == "revising");
                if !consistent_phase || latest_run != run_id.to_string() {
                    return Err(StoreError::InvalidInput(
                        "Plan tool approval is not in a consistent planning or approval-waiting phase".into(),
                    ));
                }
                let expected_scope_hash = expected_scope_hash.ok_or_else(|| {
                    StoreError::InvalidInput("Plan tool approval requires a revision scope hash".into())
                })?;
                let current_scope = PlanApprovalScopeV4 {
                    project_id,
                    conversation_id,
                    run_id,
                    revision_id: latest_id,
                    revision: latest_revision,
                };
                if expected_scope_hash != current_scope.hash() {
                    return Err(StoreError::InvalidInput(
                        "Plan approval scope does not match the current revision".into(),
                    ));
                }
                current_plan_scope = Some(current_scope);
            } else if expected_scope_hash.is_some() {
                return Err(StoreError::InvalidInput(
                    "Execute tool approval cannot carry a Plan scope hash".into(),
                ));
            }
            let existing = load_agent_events_in_tx(&mut *tx, run_id).await?;
            let request = existing
                .iter()
                .find_map(|event| match &event.event {
                    AgentEventKindV4::ToolApprovalRequested { request }
                        if request.approval_id == approval_id => Some(request),
                    _ => None,
                })
                .ok_or_else(|| {
                    StoreError::InvalidInput("tool approval request was not found".into())
                })?;
            if request.call_hash != call_hash {
                return Err(StoreError::InvalidInput(
                    "tool approval call hash mismatch".into(),
                ));
            }
            match expected_mode {
                RunModeV4::Plan => {
                    if request.mode != RunModeV4::Plan
                        || request.scope_hash.is_none()
                        || request.effect != omicsops_protocol::ToolEffectV4::ReadOnly
                        || request.call.tool_id != "use_mcp_tool"
                    {
                        return Err(StoreError::InvalidInput(
                            "Plan approval request is not a concrete read-only MCP request".into(),
                        ));
                    }
                    request
                        .validate_with_scope(
                            run_id,
                            expected_scope_hash.expect("validated above"),
                            RunModeV4::Plan,
                        )
                        .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
                    let current_request = current_plan_approval_request(
                        &existing,
                        current_plan_scope.expect("validated Plan scope above"),
                    )?;
                    if current_request.approval_id != request.approval_id {
                        return Err(StoreError::InvalidInput(
                            "Plan approval request is not the current unfinished request".into(),
                        ));
                    }
                }
                RunModeV4::Execute => {
                    if request.mode != RunModeV4::Execute || request.scope_hash.is_some() {
                        return Err(StoreError::InvalidInput(
                            "Execute approval request has a Plan binding".into(),
                        ));
                    }
                    let run_value = load_agent_run_value_in_tx(&mut *tx, run_id).await?;
                    let stored_spec_hash = run_value
                        .get("spec")
                        .and_then(Value::as_object)
                        .and_then(|spec| spec.get("spec_hash"))
                        .and_then(Value::as_str)
                        .filter(|hash| !hash.trim().is_empty())
                        .ok_or_else(|| {
                            StoreError::InvalidInput(
                                "Execute approval requires a persisted frozen spec_hash".into(),
                            )
                        })?;
                    if stored_spec_hash != binding_hash {
                        return Err(StoreError::InvalidInput(
                            "Execute approval binding does not match the frozen spec".into(),
                        ));
                    }
                    request
                        .validate(run_id, binding_hash)
                        .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
                }
            }
            if existing.iter().any(|event| {
                matches!(
                    &event.event,
                    AgentEventKindV4::ToolApprovalDecided { approval_id: decided, .. }
                        if decided == approval_id
                )
            }) {
                return Err(StoreError::InvalidInput(
                    "tool approval request was already decided".into(),
                ));
            }
            if let Some(authorization) = browser_authorization {
                if decision != ToolApprovalDecisionV4::Approved {
                    return Err(StoreError::InvalidInput(
                        "browser authorization requires an approved tool decision".into(),
                    ));
                }
                authorization
                    .validate()
                    .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
                let context_matches = match authorization.scope {
                    BrowserApprovalScopeV4::Once | BrowserApprovalScopeV4::Conversation => {
                        authorization.project_id == Some(project_id)
                            && authorization.conversation_id == Some(conversation_id)
                    }
                    BrowserApprovalScopeV4::Project => {
                        authorization.project_id == Some(project_id)
                            && authorization.conversation_id.is_none()
                    }
                    BrowserApprovalScopeV4::Global => {
                        authorization.project_id.is_none()
                            && authorization.conversation_id.is_none()
                    }
                };
                if !context_matches {
                    return Err(StoreError::InvalidInput(
                        "browser authorization scope does not match the approved run context"
                            .into(),
                    ));
                }
                if request.call.tool_id != authorization.binding.capability
                    || !(request.call.tool_id == "browser_setup"
                        || request.call.tool_id.starts_with("web_"))
                {
                    return Err(StoreError::InvalidInput(
                        "browser authorization capability does not match the approved call".into(),
                    ));
                }
                let scope = match authorization.scope {
                    BrowserApprovalScopeV4::Once => "once",
                    BrowserApprovalScopeV4::Conversation => "conversation",
                    BrowserApprovalScopeV4::Project => "project",
                    BrowserApprovalScopeV4::Global => "global",
                };
                let session = match authorization.binding.session {
                    BrowserSessionKindV4::Shared => "shared",
                    BrowserSessionKindV4::Workspace => "workspace",
                };
                sqlx::query(
                    "INSERT INTO browser_authorizations_v4
                     (id,scope,capability,target_host,session,protocol_version,project_id,conversation_id,value_json,created_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                     ON CONFLICT(id) DO NOTHING",
                )
                .bind(&authorization.id)
                .bind(scope)
                .bind(&authorization.binding.capability)
                .bind(&authorization.binding.target_host)
                .bind(session)
                .bind(i64::from(authorization.binding.protocol_version))
                .bind(authorization.project_id.map(|value| value.to_string()))
                .bind(authorization.conversation_id.map(|value| value.to_string()))
                .bind(serde_json::to_string(authorization)?)
                .bind(authorization.created_at_ms)
                .execute(&mut *tx)
                .await?;
            }
            let previous = existing.last().ok_or_else(|| {
                StoreError::InvalidInput("V4 run has no event chain".into())
            })?;
            let event = AgentEventV4::next(
                previous,
                Utc::now(),
                AgentEventKindV4::ToolApprovalDecided {
                    approval_id: approval_id.to_owned(),
                    call_hash: call_hash.to_owned(),
                    decision,
                },
            );
            insert_agent_event_in_tx(&mut *tx, &event).await?;
            Ok(event)
        }
        .await;
        match result {
            Ok(event) => {
                tx.commit().await?;
                Ok(event)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    pub async fn agent_events_v4(&self, run_id: Uuid) -> Result<Vec<AgentEventV4>, StoreError> {
        let rows =
            sqlx::query("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence")
                .bind(run_id.to_string())
                .fetch_all(&self.pool)
                .await?;
        let serialized = rows
            .into_iter()
            .map(|row| row.try_get::<String, _>(0))
            .collect::<Result<Vec<_>, _>>()?;
        deserialize_event_chain_v4(&serialized)
            .map_err(|error| StoreError::InvalidInput(error.to_string()))
    }

    pub async fn agent_events_for_context_v4(
        &self,
        project_id: Uuid,
        conversation_id: Uuid,
    ) -> Result<Vec<AgentEventV4>, StoreError> {
        let rows = sqlx::query(
            "SELECT run_id,value_json FROM agent_events_v4
             WHERE project_id=?1 AND conversation_id=?2 ORDER BY run_id,sequence",
        )
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut chains: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in rows {
            chains
                .entry(row.try_get::<String, _>(0)?)
                .or_default()
                .push(row.try_get::<String, _>(1)?);
        }
        let mut events = Vec::new();
        for serialized in chains.values() {
            events.extend(
                deserialize_event_chain_v4(serialized)
                    .map_err(|error| StoreError::InvalidInput(error.to_string()))?,
            );
        }
        Ok(events)
    }

    pub async fn archive_agent_context_v4(
        &self,
        run_id: Uuid,
        transcript: &str,
        checkpoint: &ContextCheckpointV4,
    ) -> Result<ContextArchiveV4, StoreError> {
        if checkpoint.schema_version != 4 {
            return Err(StoreError::InvalidInput(
                "only context checkpoint schema version 4 is supported".into(),
            ));
        }
        let run_exists: i64 =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_runs_v4 WHERE run_id=?1)")
                .bind(run_id.to_string())
                .fetch_one(&self.pool)
                .await?;
        if run_exists == 0 {
            return Err(StoreError::InvalidInput(format!(
                "context archive references unknown run {run_id}"
            )));
        }
        let archive = ContextArchiveV4 {
            archive_id: Uuid::new_v4(),
            through_sequence: checkpoint.through_sequence,
            size_bytes: u64::try_from(transcript.len())
                .map_err(|_| StoreError::InvalidInput("transcript size exceeds range".into()))?,
            sha256: hex::encode(Sha256::digest(transcript.as_bytes())),
        };
        sqlx::query(
            "INSERT INTO agent_context_archives_v4
             (archive_id,run_id,through_sequence,size_bytes,sha256,transcript_json,checkpoint_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(archive.archive_id.to_string())
        .bind(run_id.to_string())
        .bind(i64::try_from(archive.through_sequence).map_err(|_| {
            StoreError::InvalidInput("checkpoint sequence exceeds SQLite range".into())
        })?)
        .bind(
            i64::try_from(archive.size_bytes).map_err(|_| {
                StoreError::InvalidInput("archive size exceeds SQLite range".into())
            })?,
        )
        .bind(&archive.sha256)
        .bind(transcript)
        .bind(serde_json::to_string(checkpoint)?)
        .execute(&self.pool)
        .await?;
        Ok(archive)
    }

    pub async fn agent_context_archive_v4(
        &self,
        archive_id: Uuid,
    ) -> Result<Option<(String, ContextCheckpointV4)>, StoreError> {
        let row = sqlx::query(
            "SELECT transcript_json,checkpoint_json FROM agent_context_archives_v4 WHERE archive_id=?1",
        )
        .bind(archive_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok((
                row.try_get::<String, _>(0)?,
                serde_json::from_str(row.try_get::<String, _>(1)?.as_str())?,
            ))
        })
        .transpose()
    }

    pub async fn save_scientific_state_v4(
        &self,
        state: &ScientificStateV4,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO scientific_states_v4 (project_id,revision,state_sha256,value_json)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(project_id) DO UPDATE SET revision=excluded.revision,
             state_sha256=excluded.state_sha256,value_json=excluded.value_json",
        )
        .bind(state.project_id.to_string())
        .bind(i64::try_from(state.revision).map_err(|_| {
            StoreError::InvalidInput("scientific revision exceeds SQLite range".into())
        })?)
        .bind(state.digest())
        .bind(serde_json::to_string(state)?)
        .execute(&mut *tx)
        .await?;
        for dataset in state.datasets.values() {
            sqlx::query(
                "INSERT INTO scientific_datasets_v4 (id,project_id,active,value_json)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(id) DO UPDATE SET active=excluded.active,value_json=excluded.value_json",
            )
            .bind(dataset.id.to_string())
            .bind(state.project_id.to_string())
            .bind(if dataset.active { 1_i64 } else { 0_i64 })
            .bind(serde_json::to_string(dataset)?)
            .execute(&mut *tx)
            .await?;
        }
        for analysis in state.analyses.values() {
            sqlx::query(
                "INSERT INTO scientific_analyses_v4 (id,project_id,status,value_json)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(id) DO UPDATE SET status=excluded.status,value_json=excluded.value_json",
            )
            .bind(analysis.id.to_string())
            .bind(state.project_id.to_string())
            .bind(enum_string(&analysis.status)?)
            .bind(serde_json::to_string(analysis)?)
            .execute(&mut *tx)
            .await?;
        }
        for artifact in state.artifacts.values() {
            sqlx::query(
                "INSERT INTO scientific_artifacts_v4
                 (id,project_id,producer_analysis_id,valid,value_json) VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(id) DO UPDATE SET valid=excluded.valid,value_json=excluded.value_json",
            )
            .bind(artifact.id.to_string())
            .bind(state.project_id.to_string())
            .bind(artifact.producer_analysis_id.to_string())
            .bind(if artifact.valid { 1_i64 } else { 0_i64 })
            .bind(serde_json::to_string(artifact)?)
            .execute(&mut *tx)
            .await?;
        }
        for evidence in state.evidence.values() {
            sqlx::query(
                "INSERT INTO scientific_evidence_v4 (id,project_id,valid,value_json)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(id) DO UPDATE SET valid=excluded.valid,value_json=excluded.value_json",
            )
            .bind(evidence.id.to_string())
            .bind(state.project_id.to_string())
            .bind(if evidence.valid { 1_i64 } else { 0_i64 })
            .bind(serde_json::to_string(evidence)?)
            .execute(&mut *tx)
            .await?;
        }
        for manifest in state.provenance.values() {
            sqlx::query(
                "INSERT INTO scientific_provenance_v4
                 (id,project_id,run_id,analysis_id,complete,value_json) VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(id) DO UPDATE SET complete=excluded.complete,value_json=excluded.value_json",
            )
            .bind(manifest.id.to_string())
            .bind(state.project_id.to_string())
            .bind(manifest.run_id.to_string())
            .bind(manifest.analysis_id.to_string())
            .bind(if manifest.complete { 1_i64 } else { 0_i64 })
            .bind(serde_json::to_string(manifest)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn scientific_state_v4(
        &self,
        project_id: Uuid,
    ) -> Result<Option<ScientificStateV4>, StoreError> {
        let row = sqlx::query("SELECT value_json FROM scientific_states_v4 WHERE project_id=?1")
            .bind(project_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .transpose()
    }

    async fn project_json_rows<T: DeserializeOwned>(
        &self,
        table: &str,
        project_id: Uuid,
    ) -> Result<Vec<T>, StoreError> {
        let query = match table {
            "notebook_entries" | "sync_entries" => {
                format!("SELECT value_json FROM {table} WHERE project_id=?1 ORDER BY id")
            }
            _ => {
                return Err(StoreError::InvalidInput(
                    "unsupported project registry".into(),
                ));
            }
        };
        let rows = sqlx::query(&query)
            .bind(project_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .collect()
    }

    async fn simple_json_rows<T: DeserializeOwned>(
        &self,
        table: &str,
    ) -> Result<Vec<T>, StoreError> {
        let query = match table {
            "model_profiles" | "skill_packages" => {
                format!("SELECT value_json FROM {table} ORDER BY id")
            }
            _ => {
                return Err(StoreError::InvalidInput(
                    "unsupported simple registry".into(),
                ));
            }
        };
        let rows = sqlx::query(&query).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .collect()
    }

    async fn get_simple_json<T: DeserializeOwned>(
        &self,
        table: &str,
        id: &str,
    ) -> Result<Option<T>, StoreError> {
        let query = match table {
            "model_profiles" | "skill_packages" => {
                format!("SELECT value_json FROM {table} WHERE id=?1")
            }
            _ => {
                return Err(StoreError::InvalidInput(
                    "unsupported simple registry".into(),
                ));
            }
        };
        let row = sqlx::query(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
            .transpose()
    }
}

async fn skill_packages_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<Vec<SkillPackage>, StoreError> {
    let rows = sqlx::query("SELECT value_json FROM skill_packages ORDER BY id")
        .fetch_all(&mut **tx)
        .await?;
    rows.into_iter()
        .map(|row| Ok(serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?))
        .collect()
}

async fn save_skill_package_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    skill: &SkillPackage,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO skill_packages (id,value_json) VALUES (?1,?2)
         ON CONFLICT(id) DO UPDATE SET value_json=excluded.value_json",
    )
    .bind(skill.id.to_string())
    .bind(serde_json::to_string(skill)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn read_schema_version(path: &Path) -> Result<u32, StoreError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await?;
    pool.close().await;
    u32::try_from(version)
        .map_err(|_| StoreError::InvalidInput("invalid SQLite schema version".into()))
}

fn create_backup(path: &Path, _version: u32) -> Result<PathBuf, StoreError> {
    create_backup_with_id_factory(path, Utc::now().timestamp_millis().max(0), || {
        Uuid::new_v4().simple().to_string()
    })
}

fn create_backup_with_id_factory<F>(
    path: &Path,
    timestamp: i64,
    mut id_factory: F,
) -> Result<PathBuf, StoreError>
where
    F: FnMut() -> String,
{
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let database_name = path
        .file_name()
        .ok_or_else(|| StoreError::InvalidInput("database path has no file name".into()))?
        .to_string_lossy();
    loop {
        let identifier = id_factory();
        let backup = directory.join(format!(
            "{database_name}.pre-store-v4.{timestamp}-{identifier}.bak"
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(file) => {
                drop(file);
                if let Err(error) = fs::copy(path, &backup) {
                    let _ = fs::remove_file(&backup);
                    return Err(error.into());
                }
                return Ok(backup);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

async fn initialize(
    pool: &SqlitePool,
    version: u32,
    options: MigrationOptions,
) -> Result<(), StoreError> {
    if version > SCHEMA_VERSION {
        return Err(StoreError::Migration(format!(
            "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
        )));
    }
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(pool)
        .await?;
    let mut tx = pool.begin().await?;
    if version < SCHEMA_VERSION {
        rename_legacy_tables(&mut tx).await?;
    }
    // Existing v3/pre-release-v4 extension tables must receive additive
    // columns before INIT_SQL creates indexes that reference those columns.
    // This also backfills the legacy Agent V4 event timestamp while its exact
    // JSON envelope and hash chain are still available for later validation.
    ensure_schema_extensions(&mut tx).await?;
    sqlx::raw_sql(INIT_SQL).execute(&mut *tx).await?;
    repair_proposed_plans_schema(&mut tx).await?;
    // `CREATE TRIGGER IF NOT EXISTS` intentionally does not replace a
    // pre-release v4 trigger. Re-install this security boundary on every
    // open, including fresh, existing, and repeated opens, so old trigger
    // bodies cannot leave plan materialization or immutable content exposed.
    repair_proposed_plan_trigger(&mut tx).await?;
    if version < SCHEMA_VERSION {
        migrate_legacy_rows(&mut tx, options).await?;
    }
    validate_before_commit(&mut tx).await?;
    sqlx::query("PRAGMA user_version = 4")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn repair_proposed_plan_trigger(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    sqlx::query("DROP TRIGGER IF EXISTS trg_proposed_plans_immutable_content")
        .execute(&mut **tx)
        .await?;
    sqlx::query(PROPOSED_PLAN_TRIGGER_SQL)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn repair_proposed_plans_schema(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let columns = sqlx::query("PRAGMA table_info(proposed_plans)")
        .fetch_all(&mut **tx)
        .await?;
    // A few pre-release v4 databases created this table before `run_id` was
    // added.  Return a contextual migration error before any query references
    // that missing column; this keeps the failure deterministic and leaves the
    // pre-release table untouched for recovery.
    if !columns.iter().any(|row| {
        row.try_get::<String, _>(1)
            .is_ok_and(|name| name == "run_id")
    }) {
        return Err(StoreError::Migration(
            "proposed_plans is missing the required run_id column; cannot establish plan/run ownership safely".into(),
        ));
    }
    let run_id_required = columns.iter().any(|row| {
        row.try_get::<String, _>(1)
            .is_ok_and(|name| name == "run_id")
            && row.try_get::<i64, _>(3).is_ok_and(|not_null| not_null != 0)
    });
    let run_id_foreign_key = sqlx::query("PRAGMA foreign_key_list(proposed_plans)")
        .fetch_all(&mut **tx)
        .await?
        .iter()
        .any(|row| {
            row.try_get::<String, _>(2)
                .is_ok_and(|table| table == "agent_runs_v4")
                && row
                    .try_get::<String, _>(3)
                    .is_ok_and(|column| column == "run_id")
                && row
                    .try_get::<String, _>(4)
                    .is_ok_and(|column| column == "run_id")
        });
    if run_id_required && run_id_foreign_key {
        validate_proposed_plan_run_owners(tx).await?;
        return Ok(());
    }

    let null_run_ids: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM proposed_plans WHERE run_id IS NULL")
            .fetch_one(&mut **tx)
            .await?;
    if null_run_ids != 0 {
        return Err(StoreError::Migration(
            "proposed_plans contains NULL run_id values and cannot be upgraded safely".into(),
        ));
    }
    let orphaned_or_mismatched_runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM proposed_plans p
         LEFT JOIN agent_runs_v4 r
           ON r.run_id=p.run_id AND r.project_id=p.project_id AND r.conversation_id=p.frame_id
         WHERE r.run_id IS NULL",
    )
    .fetch_one(&mut **tx)
    .await?;
    if orphaned_or_mismatched_runs != 0 {
        return Err(StoreError::Migration(
            "proposed_plans contains orphaned or mismatched run owners".into(),
        ));
    }

    sqlx::query("DROP TRIGGER IF EXISTS trg_proposed_plans_immutable_content")
        .execute(&mut **tx)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS proposed_plans_v4_repair")
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "CREATE TABLE proposed_plans_v4_repair (
            id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            frame_id TEXT NOT NULL REFERENCES frames(id) ON DELETE CASCADE,
            revision INTEGER NOT NULL CHECK (revision > 0),
            plan_hash TEXT NOT NULL CHECK (length(trim(plan_hash)) > 0),
            status TEXT NOT NULL CHECK (length(trim(status)) > 0),
            plan_json TEXT NOT NULL,
            markdown TEXT NOT NULL DEFAULT '',
            feedback TEXT,
            run_id TEXT NOT NULL REFERENCES agent_runs_v4(run_id) ON DELETE CASCADE,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            UNIQUE (frame_id, revision),
            UNIQUE (frame_id, plan_hash)
        )",
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO proposed_plans_v4_repair
         (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
         SELECT id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at
         FROM proposed_plans",
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query("DROP TABLE proposed_plans")
        .execute(&mut **tx)
        .await?;
    sqlx::query("ALTER TABLE proposed_plans_v4_repair RENAME TO proposed_plans")
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_proposed_plans_context_revision
         ON proposed_plans(project_id, frame_id, revision DESC, id DESC)",
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_proposed_plans_active
         ON proposed_plans(project_id, frame_id, status, revision DESC)",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn validate_proposed_plan_run_owners(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    let null_run_ids: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM proposed_plans WHERE run_id IS NULL")
            .fetch_one(&mut **tx)
            .await?;
    if null_run_ids != 0 {
        return Err(StoreError::Migration(
            "proposed_plans contains NULL run_id values and cannot be upgraded safely".into(),
        ));
    }
    let orphaned_or_mismatched_runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM proposed_plans p
         LEFT JOIN agent_runs_v4 r ON r.run_id=p.run_id
         WHERE r.run_id IS NULL
            OR r.project_id <> p.project_id
            OR r.conversation_id <> p.frame_id",
    )
    .fetch_one(&mut **tx)
    .await?;
    if orphaned_or_mismatched_runs != 0 {
        return Err(StoreError::Migration(
            "proposed_plans contains orphaned or mismatched run owners".into(),
        ));
    }
    Ok(())
}

async fn ensure_schema_extensions(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    // These additive columns make opening a pre-release v4 database safe when
    // it already contains the table but predates the final compatibility
    // projection. Every operation is idempotent and remains transactional.
    let backfill_agent_event_occurred_at = table_exists(tx, "agent_events_v4").await?
        && !table_has_column(tx, "agent_events_v4", "occurred_at").await?;
    for (table, column, definition) in [
        ("messages", "project_id", "TEXT"),
        ("messages", "conversation_id", "TEXT"),
        ("artifacts", "source_run_id", "TEXT"),
        ("artifacts", "remote_path", "TEXT"),
        ("artifacts", "size_bytes", "INTEGER NOT NULL DEFAULT 0"),
        ("artifacts", "sha256", "TEXT NOT NULL DEFAULT ''"),
        ("artifacts", "verified", "INTEGER NOT NULL DEFAULT 0"),
        ("notebook_entries", "conversation_id", "TEXT"),
        ("notebook_entries", "turn_id", "TEXT"),
        ("notebook_entries", "kind", "TEXT"),
        ("notebook_entries", "title", "TEXT"),
        ("notebook_entries", "markdown", "TEXT"),
        ("notebook_entries", "confidence", "REAL"),
        ("notebook_entries", "evidence_json", "TEXT"),
        ("notebook_entries", "artifact_ids_json", "TEXT"),
        ("notebook_entries", "created_at", "INTEGER"),
        ("notebook_entries", "updated_at", "INTEGER"),
        (
            "agent_events_v4",
            "occurred_at",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        ("agent_runs_v4", "created_at", "INTEGER NOT NULL DEFAULT 0"),
        ("agent_runs_v4", "updated_at", "INTEGER NOT NULL DEFAULT 0"),
        (
            "agent_context_archives_v4",
            "created_at",
            "INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "scientific_states_v4",
            "updated_at",
            "INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if table_exists(tx, table).await? && !table_has_column(tx, table, column).await? {
            sqlx::query(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))
            .execute(&mut **tx)
            .await?;
        }
    }
    if backfill_agent_event_occurred_at {
        backfill_legacy_agent_event_occurred_at(tx).await?;
    }
    Ok(())
}

async fn backfill_legacy_agent_event_occurred_at(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT run_id,sequence,value_json
         FROM agent_events_v4 ORDER BY run_id,sequence",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let run_id = row.try_get::<String, _>(0)?;
        let sequence = row.try_get::<i64, _>(1)?;
        let event: AgentEventV4 = serde_json::from_str(row.try_get::<String, _>(2)?.as_str())
            .map_err(|error| {
                StoreError::Migration(format!(
                    "event {run_id}/{sequence} has invalid JSON while backfilling occurred_at: {error}"
                ))
            })?;
        let updated = sqlx::query(
            "UPDATE agent_events_v4 SET occurred_at=?1 WHERE run_id=?2 AND sequence=?3",
        )
        .bind(event.occurred_at.timestamp_millis())
        .bind(&run_id)
        .bind(sequence)
        .execute(&mut **tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Migration(format!(
                "event {run_id}/{sequence} could not be backfilled uniquely"
            )));
        }
    }
    Ok(())
}

async fn table_exists(tx: &mut Transaction<'_, Sqlite>, table: &str) -> Result<bool, StoreError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
    )
    .bind(table)
    .fetch_one(&mut **tx)
    .await?;
    Ok(exists == 1)
}

async fn table_has_column(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    column: &str,
) -> Result<bool, StoreError> {
    // Table names are selected from a fixed internal list by callers. The
    // value is escaped defensively because SQLite PRAGMA does not accept a
    // bind parameter for its table argument.
    let escaped = table.replace('"', "\"\"");
    let rows = sqlx::query(&format!("PRAGMA table_info(\"{escaped}\")"))
        .fetch_all(&mut **tx)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>(1).ok())
        .any(|name| name == column))
}

async fn table_column_is_not_null(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    column: &str,
) -> Result<bool, StoreError> {
    let escaped = table.replace('"', "\"\"");
    let rows = sqlx::query(&format!("PRAGMA table_info(\"{escaped}\")"))
        .fetch_all(&mut **tx)
        .await?;
    Ok(rows.into_iter().any(|row| {
        row.try_get::<String, _>(1).ok().as_deref() == Some(column)
            && row.try_get::<i64, _>(3).ok() == Some(1)
    }))
}

async fn rename_legacy_tables(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    for (current, legacy) in [
        ("projects", "projects_v3_legacy"),
        ("conversations", "conversations_v3_legacy"),
        ("messages", "messages_v3_legacy"),
    ] {
        if table_exists(tx, current).await? && table_has_column(tx, current, "value_json").await? {
            if table_exists(tx, legacy).await? {
                return Err(StoreError::Migration(format!(
                    "legacy table {legacy} already exists while {current} still has v3 columns"
                )));
            }
            sqlx::query(&format!("ALTER TABLE {current} RENAME TO {legacy}"))
                .execute(&mut **tx)
                .await?;
        }
    }
    if table_exists(tx, "sync_entries").await?
        && (!table_has_column(tx, "sync_entries", "relative_path").await?
            || !table_column_is_not_null(tx, "sync_entries", "relative_path").await?)
    {
        if table_exists(tx, "sync_entries_v3_legacy").await? {
            return Err(StoreError::Migration(
                "legacy table sync_entries_v3_legacy already exists while sync_entries still needs migration"
                    .into(),
            ));
        }
        sqlx::query("ALTER TABLE sync_entries RENAME TO sync_entries_v3_legacy")
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

fn migration_checkpoint(
    copied_rows: &mut usize,
    options: MigrationOptions,
) -> Result<(), StoreError> {
    if options
        .fail_after_rows
        .is_some_and(|limit| *copied_rows >= limit)
    {
        return Err(StoreError::Migration(
            "injected failure while copying legacy rows".into(),
        ));
    }
    *copied_rows = copied_rows.saturating_add(1);
    Ok(())
}

async fn migrate_legacy_rows(
    tx: &mut Transaction<'_, Sqlite>,
    options: MigrationOptions,
) -> Result<(), StoreError> {
    let mut copied_rows = 0_usize;
    if table_exists(tx, "projects_v3_legacy").await? {
        let rows =
            sqlx::query("SELECT id,updated_at,value_json FROM projects_v3_legacy ORDER BY id")
                .fetch_all(&mut **tx)
                .await?;
        for row in rows {
            migration_checkpoint(&mut copied_rows, options)?;
            let row_id = row.try_get::<String, _>(0)?;
            let row_updated_at = parse_legacy_timestamp(
                row.try_get::<String, _>(1)?.as_str(),
                "project updated_at",
            )?;
            let project: Project = serde_json::from_str(row.try_get::<String, _>(2)?.as_str())
                .map_err(|error| StoreError::Migration(format!("project {row_id}: {error}")))?;
            if project.id.to_string() != row_id {
                return Err(StoreError::Migration(format!(
                    "project row id {row_id} does not match its JSON identifier {}",
                    project.id
                )));
            }
            if project.updated_at.timestamp_millis() != row_updated_at.timestamp_millis() {
                return Err(StoreError::Migration(format!(
                    "project row {row_id} has inconsistent updated_at"
                )));
            }
            insert_project(tx, &project).await?;
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects")
            .fetch_one(&mut **tx)
            .await?;
        let expected = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects_v3_legacy")
            .fetch_one(&mut **tx)
            .await?;
        if count != expected {
            return Err(StoreError::Migration(
                "project migration count validation failed".into(),
            ));
        }
        let extension_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_omicsops")
            .fetch_one(&mut **tx)
            .await?;
        if extension_count != expected {
            return Err(StoreError::Migration(
                "project extension migration count validation failed".into(),
            ));
        }
        let missing_extensions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects p
             LEFT JOIN project_omicsops e ON e.project_id=p.id
             WHERE e.project_id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if missing_extensions != 0 {
            return Err(StoreError::Migration(
                "project extension identifier set validation failed".into(),
            ));
        }
        let orphan_extensions: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_omicsops e
             LEFT JOIN projects p ON p.id=e.project_id
             WHERE p.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if orphan_extensions != 0 {
            return Err(StoreError::Migration(
                "project extension contains an unknown project id".into(),
            ));
        }
        let unknown_projects: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects p
             LEFT JOIN projects_v3_legacy l ON l.id=p.id
             WHERE l.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        let missing_projects: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects_v3_legacy l
             LEFT JOIN projects p ON p.id=l.id
             WHERE p.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if unknown_projects != 0 || missing_projects != 0 {
            return Err(StoreError::Migration(
                "project identifier sets are not an exact match".into(),
            ));
        }
    }

    if table_exists(tx, "sync_entries_v3_legacy").await? {
        let rows =
            sqlx::query("SELECT id,project_id,value_json FROM sync_entries_v3_legacy ORDER BY id")
                .fetch_all(&mut **tx)
                .await?;
        for row in rows {
            migration_checkpoint(&mut copied_rows, options)?;
            let row_id = row.try_get::<String, _>(0)?;
            let row_project_id = row.try_get::<String, _>(1)?;
            let value_json = row.try_get::<String, _>(2)?;
            let entry: SyncEntry = serde_json::from_str(&value_json)
                .map_err(|error| StoreError::Migration(format!("sync entry {row_id}: {error}")))?;
            if entry.id.to_string() != row_id
                || entry.project_id.to_string() != row_project_id
                || entry.relative_path.trim().is_empty()
            {
                return Err(StoreError::Migration(format!(
                    "sync entry {row_id} has inconsistent id, project, or relative_path"
                )));
            }
            let project_exists: i64 =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)")
                    .bind(&row_project_id)
                    .fetch_one(&mut **tx)
                    .await?;
            if project_exists == 0 {
                return Err(StoreError::Migration(format!(
                    "sync entry {row_id} references missing project {row_project_id}"
                )));
            }
            sqlx::query(
                "INSERT INTO sync_entries (id,project_id,relative_path,value_json)
                 VALUES (?1,?2,?3,?4)",
            )
            .bind(&row_id)
            .bind(&row_project_id)
            .bind(&entry.relative_path)
            .bind(&value_json)
            .execute(&mut **tx)
            .await?;
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_entries")
            .fetch_one(&mut **tx)
            .await?;
        let expected = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_entries_v3_legacy")
            .fetch_one(&mut **tx)
            .await?;
        if count != expected {
            return Err(StoreError::Migration(
                "sync entry migration count validation failed".into(),
            ));
        }
        let unknown_entries: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sync_entries s
             LEFT JOIN sync_entries_v3_legacy l ON l.id=s.id
             WHERE l.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        let missing_entries: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sync_entries_v3_legacy l
             LEFT JOIN sync_entries s ON s.id=l.id
             WHERE s.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if unknown_entries != 0 || missing_entries != 0 {
            return Err(StoreError::Migration(
                "sync entry identifier set validation failed".into(),
            ));
        }
    }

    if table_exists(tx, "conversations_v3_legacy").await? {
        let rows = sqlx::query(
            "SELECT id,project_id,updated_at,value_json FROM conversations_v3_legacy ORDER BY id",
        )
        .fetch_all(&mut **tx)
        .await?;
        for row in rows {
            migration_checkpoint(&mut copied_rows, options)?;
            let row_id = row.try_get::<String, _>(0)?;
            let row_project_id = row.try_get::<String, _>(1)?;
            let row_updated_at = parse_legacy_timestamp(
                row.try_get::<String, _>(2)?.as_str(),
                "conversation updated_at",
            )?;
            let conversation: Conversation =
                serde_json::from_str(row.try_get::<String, _>(3)?.as_str()).map_err(|error| {
                    StoreError::Migration(format!("conversation {row_id}: {error}"))
                })?;
            if conversation.id.to_string() != row_id
                || conversation.project_id.to_string() != row_project_id
            {
                return Err(StoreError::Migration(format!(
                    "conversation row {row_id} has inconsistent identifiers"
                )));
            }
            if conversation.updated_at.timestamp_millis() != row_updated_at.timestamp_millis() {
                return Err(StoreError::Migration(format!(
                    "conversation row {row_id} has inconsistent updated_at"
                )));
            }
            let project_exists: i64 =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)")
                    .bind(&row_project_id)
                    .fetch_one(&mut **tx)
                    .await?;
            if project_exists == 0 {
                return Err(StoreError::Migration(format!(
                    "conversation {row_id} references missing project {row_project_id}"
                )));
            }
            insert_conversation(tx, &conversation).await?;
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM conversation_records")
            .fetch_one(&mut **tx)
            .await?;
        let expected = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM conversations_v3_legacy")
            .fetch_one(&mut **tx)
            .await?;
        if count != expected {
            return Err(StoreError::Migration(
                "conversation migration count validation failed".into(),
            ));
        }
        let unknown_conversations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM conversation_records c
             LEFT JOIN conversations_v3_legacy l ON l.id=c.frame_id
             WHERE l.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if unknown_conversations != 0 {
            return Err(StoreError::Migration(
                "conversation identifier set validation failed".into(),
            ));
        }
        let missing_conversations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM conversations_v3_legacy l
             LEFT JOIN conversation_records c ON c.frame_id=l.id
             WHERE c.frame_id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if missing_conversations != 0 {
            return Err(StoreError::Migration(
                "conversation identifier set validation failed".into(),
            ));
        }
    }

    if table_exists(tx, "messages_v3_legacy").await? {
        let rows = sqlx::query(
            "SELECT id,project_id,conversation_id,sequence,value_json
             FROM messages_v3_legacy ORDER BY conversation_id,sequence,id",
        )
        .fetch_all(&mut **tx)
        .await?;
        let mut previous: Option<(String, i64)> = None;
        for row in rows {
            migration_checkpoint(&mut copied_rows, options)?;
            let row_id = row.try_get::<String, _>(0)?;
            let row_project_id = row.try_get::<String, _>(1)?;
            let row_conversation_id = row.try_get::<String, _>(2)?;
            let sequence = row.try_get::<i64, _>(3)?;
            if sequence < 0 {
                return Err(StoreError::Migration(format!(
                    "message {row_id} has a negative sequence"
                )));
            }
            let message: Message = serde_json::from_str(row.try_get::<String, _>(4)?.as_str())
                .map_err(|error| StoreError::Migration(format!("message {row_id}: {error}")))?;
            if message.id.to_string() != row_id
                || message.project_id.to_string() != row_project_id
                || message.conversation_id.to_string() != row_conversation_id
                || i64::try_from(message.sequence).ok() != Some(sequence)
            {
                return Err(StoreError::Migration(format!(
                    "message row {row_id} has inconsistent identifiers or sequence"
                )));
            }
            if previous
                .as_ref()
                .is_some_and(|(conversation_id, previous_sequence)| {
                    conversation_id == &row_conversation_id && sequence <= *previous_sequence
                })
            {
                return Err(StoreError::Migration(format!(
                    "message sequence is not strictly ordered for conversation {row_conversation_id}"
                )));
            }
            previous = Some((row_conversation_id.clone(), sequence));
            let frame_exists: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM conversation_records WHERE frame_id=?1 AND project_id=?2)",
            )
            .bind(&row_conversation_id)
            .bind(&row_project_id)
            .fetch_one(&mut **tx)
            .await?;
            if frame_exists == 0 {
                return Err(StoreError::Migration(format!(
                    "message {row_id} references missing conversation {row_conversation_id}"
                )));
            }
            insert_message(tx, &message).await?;
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
            .fetch_one(&mut **tx)
            .await?;
        let expected = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages_v3_legacy")
            .fetch_one(&mut **tx)
            .await?;
        if count != expected {
            return Err(StoreError::Migration(
                "message migration count validation failed".into(),
            ));
        }
        let unknown_messages: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages m
             LEFT JOIN messages_v3_legacy l ON l.id=m.id
             WHERE l.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if unknown_messages != 0 {
            return Err(StoreError::Migration(
                "message identifier set validation failed".into(),
            ));
        }
        let missing_messages: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages_v3_legacy l
             LEFT JOIN messages m ON m.id=l.id
             WHERE m.id IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?;
        if missing_messages != 0 {
            return Err(StoreError::Migration(
                "message identifier set validation failed".into(),
            ));
        }
    }
    Ok(())
}

async fn validate_before_commit(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut **tx)
        .await?;
    if integrity.to_ascii_lowercase() != "ok" {
        return Err(StoreError::Migration(format!(
            "SQLite integrity check failed: {integrity}"
        )));
    }
    // Validate archive ownership before the generic FK report so a damaged
    // archive identifies the missing run that caused the failure.
    validate_context_archives(tx).await?;
    validate_sync_entries(tx).await?;
    if !sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut **tx)
        .await?
        .is_empty()
    {
        return Err(StoreError::Migration(
            "SQLite foreign-key validation failed".into(),
        ));
    }
    validate_projects(tx).await?;
    validate_project_extensions(tx).await?;
    validate_conversation_ownership(tx).await?;
    validate_message_ownership(tx).await?;
    validate_agent_runs(tx).await?;
    validate_proposed_plan_run_owners(tx).await?;
    validate_agent_events(tx).await?;
    validate_scientific_states(tx).await?;
    Ok(())
}

async fn validate_projects(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let rows = sqlx::query("SELECT id,name,workspace_dir FROM projects ORDER BY id")
        .fetch_all(&mut **tx)
        .await?;
    for row in rows {
        let id = row.try_get::<String, _>(0)?;
        let name = row.try_get::<String, _>(1)?;
        let workspace_dir = row.try_get::<String, _>(2)?;
        if id.trim().is_empty() || name.trim().is_empty() || workspace_dir.trim().is_empty() {
            return Err(StoreError::Migration(format!(
                "project {id} has a blank identity, name, or path"
            )));
        }
    }
    Ok(())
}

async fn validate_project_extensions(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let missing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM projects p
         LEFT JOIN project_omicsops e ON e.project_id=p.id
         WHERE e.project_id IS NULL",
    )
    .fetch_one(&mut **tx)
    .await?;
    let orphan: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_omicsops e
         LEFT JOIN projects p ON p.id=e.project_id
         WHERE p.id IS NULL",
    )
    .fetch_one(&mut **tx)
    .await?;
    if missing != 0 || orphan != 0 {
        return Err(StoreError::Migration(
            "project/project_omicsops identifier sets are not one-to-one".into(),
        ));
    }
    let rows = sqlx::query("SELECT project_id,local_root,remote_root FROM project_omicsops")
        .fetch_all(&mut **tx)
        .await?;
    for row in rows {
        let project_id = row.try_get::<String, _>(0)?;
        let local_root = row.try_get::<String, _>(1)?;
        let remote_root = row.try_get::<Option<String>, _>(2)?;
        if local_root.trim().is_empty()
            || remote_root
                .as_deref()
                .is_some_and(|root| root.trim().is_empty())
        {
            return Err(StoreError::Migration(format!(
                "project extension {project_id} has a blank path"
            )));
        }
    }
    Ok(())
}

async fn validate_conversation_ownership(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT c.frame_id,c.project_id,f.project_id
         FROM conversation_records c JOIN frames f ON f.id=c.frame_id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let frame_id = row.try_get::<String, _>(0)?;
        let conversation_project = row.try_get::<String, _>(1)?;
        let frame_project = row.try_get::<Option<String>, _>(2)?;
        if frame_project.as_deref() != Some(conversation_project.as_str()) {
            return Err(StoreError::Migration(format!(
                "conversation {frame_id} has inconsistent frame/project ownership"
            )));
        }
    }
    Ok(())
}

async fn validate_message_ownership(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT m.id,m.project_id,m.conversation_id,m.frame_id,m.seq,c.project_id
         FROM messages m LEFT JOIN conversation_records c ON c.frame_id=m.frame_id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let message_id = row.try_get::<String, _>(0)?;
        let project_id = row.try_get::<String, _>(1)?;
        let conversation_id = row.try_get::<String, _>(2)?;
        let frame_id = row.try_get::<String, _>(3)?;
        let sequence = row.try_get::<i64, _>(4)?;
        let conversation_project = row.try_get::<Option<String>, _>(5)?;
        if sequence < 0
            || conversation_id != frame_id
            || conversation_project.as_deref() != Some(project_id.as_str())
        {
            return Err(StoreError::Migration(format!(
                "message {message_id} has inconsistent project/conversation/sequence columns"
            )));
        }
    }
    Ok(())
}

async fn validate_agent_runs(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT run_id,project_id,conversation_id,value_json
         FROM agent_runs_v4 ORDER BY run_id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let run_id = row.try_get::<String, _>(0)?;
        parse_uuid(run_id.as_str(), "V4 run id")
            .map_err(|error| StoreError::Migration(error.to_string()))?;
        parse_uuid(row.try_get::<String, _>(1)?.as_str(), "V4 run project id")
            .map_err(|error| StoreError::Migration(error.to_string()))?;
        parse_uuid(
            row.try_get::<String, _>(2)?.as_str(),
            "V4 run conversation id",
        )
        .map_err(|error| StoreError::Migration(error.to_string()))?;
        let owns_context: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM conversation_records
             WHERE frame_id=?1 AND project_id=?2)",
        )
        .bind(row.try_get::<String, _>(2)?)
        .bind(row.try_get::<String, _>(1)?)
        .fetch_one(&mut **tx)
        .await?;
        if owns_context == 0 {
            return Err(StoreError::Migration(format!(
                "V4 run {run_id} references an unknown project or conversation"
            )));
        }
        serde_json::from_str::<Value>(row.try_get::<String, _>(3)?.as_str()).map_err(|error| {
            StoreError::Migration(format!("V4 run {run_id} has invalid JSON: {error}"))
        })?;
    }
    Ok(())
}

async fn validate_agent_events(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT event.run_id,event.project_id,event.conversation_id,event.sequence,
                event.previous_hash,event.event_hash,
                event.value_json,event.occurred_at,run.project_id,run.conversation_id
         FROM agent_events_v4 event
         LEFT JOIN agent_runs_v4 run ON run.run_id=event.run_id
         ORDER BY event.run_id,event.sequence",
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut chains: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        let run_id = row.try_get::<String, _>(0)?;
        let event = serde_json::from_str::<AgentEventV4>(row.try_get::<String, _>(6)?.as_str())
            .map_err(|error| {
                StoreError::Migration(format!("event {run_id}: invalid JSON: {error}"))
            })?;
        let sql_sequence = row.try_get::<i64, _>(3)?;
        let sql_project_id = row.try_get::<String, _>(1)?;
        let sql_conversation_id = row.try_get::<String, _>(2)?;
        let sql_previous_hash = row.try_get::<String, _>(4)?;
        let sql_event_hash = row.try_get::<String, _>(5)?;
        let sql_occurred_at = row.try_get::<i64, _>(7)?;
        let run_project_id = row.try_get::<Option<String>, _>(8)?;
        let run_conversation_id = row.try_get::<Option<String>, _>(9)?;
        let owns_context: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM conversation_records
             WHERE frame_id=?1 AND project_id=?2)",
        )
        .bind(&sql_conversation_id)
        .bind(&sql_project_id)
        .fetch_one(&mut **tx)
        .await?;
        if event.schema_version != 4
            || event.run_id.to_string() != run_id
            || i64::try_from(event.sequence).ok() != Some(sql_sequence)
            || event.project_id.to_string() != sql_project_id
            || event.conversation_id.to_string() != sql_conversation_id
            || run_project_id.as_deref() != Some(sql_project_id.as_str())
            || run_conversation_id.as_deref() != Some(sql_conversation_id.as_str())
            || owns_context == 0
            || event.previous_hash != sql_previous_hash
            || event.event_hash != sql_event_hash
            || event.occurred_at.timestamp_millis() != sql_occurred_at
        {
            return Err(StoreError::Migration(format!(
                "event {run_id} has inconsistent durable columns"
            )));
        }
        parse_uuid(run_id.as_str(), "V4 event run id")
            .map_err(|error| StoreError::Migration(error.to_string()))?;
        chains
            .entry(run_id)
            .or_default()
            .push(row.try_get::<String, _>(6)?);
    }
    for (run_id, serialized) in chains {
        deserialize_event_chain_v4(&serialized).map_err(|error| {
            StoreError::Migration(format!("event {run_id} hash validation failed: {error}"))
        })?;
    }
    Ok(())
}

async fn validate_context_archives(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT a.archive_id,a.run_id,a.through_sequence,a.size_bytes,a.sha256,a.transcript_json,
                a.checkpoint_json,run.run_id
         FROM agent_context_archives_v4 a
         LEFT JOIN agent_runs_v4 run ON run.run_id=a.run_id
         ORDER BY a.archive_id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let archive_id = row.try_get::<String, _>(0)?;
        let run_id = row.try_get::<String, _>(1)?;
        let through_sequence = row.try_get::<i64, _>(2)?;
        let size_bytes = row.try_get::<i64, _>(3)?;
        let sha256 = row.try_get::<String, _>(4)?;
        let transcript = row.try_get::<String, _>(5)?;
        let owner_run_id = row.try_get::<Option<String>, _>(7)?;
        let checkpoint: ContextCheckpointV4 =
            serde_json::from_str(row.try_get::<String, _>(6)?.as_str()).map_err(|error| {
                StoreError::Migration(format!(
                    "context archive {archive_id} has invalid checkpoint JSON: {error}"
                ))
            })?;
        let expected_size = i64::try_from(transcript.len()).map_err(|_| {
            StoreError::Migration(format!("context archive {archive_id} size exceeds range"))
        })?;
        let expected_hash = hex::encode(Sha256::digest(transcript.as_bytes()));
        if parse_uuid(run_id.as_str(), "context archive run id").is_err()
            || parse_uuid(archive_id.as_str(), "context archive id").is_err()
            || owner_run_id.as_deref() != Some(run_id.as_str())
            || checkpoint.schema_version != 4
            || i64::try_from(checkpoint.through_sequence).ok() != Some(through_sequence)
            || size_bytes != expected_size
            || sha256 != expected_hash
        {
            return Err(StoreError::Migration(format!(
                "context archive {archive_id} has inconsistent durable columns"
            )));
        }
    }
    Ok(())
}

async fn validate_sync_entries(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let rows = sqlx::query(
        "SELECT id,project_id,relative_path,value_json
         FROM sync_entries ORDER BY id",
    )
    .fetch_all(&mut **tx)
    .await?;
    for row in rows {
        let row_id = row.try_get::<String, _>(0)?;
        let row_project_id = row.try_get::<String, _>(1)?;
        let relative_path = row.try_get::<String, _>(2)?;
        let value_json = row.try_get::<String, _>(3)?;
        let entry: SyncEntry = serde_json::from_str(&value_json).map_err(|error| {
            StoreError::Migration(format!("sync entry {row_id}: invalid JSON: {error}"))
        })?;
        let project_exists: i64 =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)")
                .bind(&row_project_id)
                .fetch_one(&mut **tx)
                .await?;
        if entry.id.to_string() != row_id
            || entry.project_id.to_string() != row_project_id
            || entry.relative_path != relative_path
            || relative_path.trim().is_empty()
            || project_exists == 0
        {
            return Err(StoreError::Migration(format!(
                "sync entry {row_id} has inconsistent durable columns or owner"
            )));
        }
    }
    Ok(())
}

async fn validate_scientific_states(tx: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    let rows =
        sqlx::query("SELECT project_id,revision,state_sha256,value_json FROM scientific_states_v4")
            .fetch_all(&mut **tx)
            .await?;
    for row in rows {
        let project_id = row.try_get::<String, _>(0)?;
        let revision = row.try_get::<i64, _>(1)?;
        let stored_digest = row.try_get::<String, _>(2)?;
        let state: ScientificStateV4 = serde_json::from_str(row.try_get::<String, _>(3)?.as_str())
            .map_err(|error| {
                StoreError::Migration(format!(
                    "scientific state {project_id}: invalid JSON: {error}"
                ))
            })?;
        if state.project_id.to_string() != project_id
            || i64::try_from(state.revision).ok() != Some(revision)
            || state.digest() != stored_digest
        {
            return Err(StoreError::Migration(format!(
                "scientific state {project_id} has inconsistent durable columns"
            )));
        }
    }
    Ok(())
}

async fn insert_project(
    tx: &mut Transaction<'_, Sqlite>,
    project: &Project,
) -> Result<(), StoreError> {
    validate_project_input(project)?;
    sqlx::query(
        "INSERT INTO projects (id,name,description,workspace_dir,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,
         workspace_dir=excluded.workspace_dir,created_at=excluded.created_at,updated_at=excluded.updated_at",
    )
    .bind(project.id.to_string())
    .bind(&project.name)
    .bind(&project.description)
    .bind(&project.local_root)
    .bind(timestamp(project.created_at))
    .bind(timestamp(project.updated_at))
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO project_omicsops
         (project_id,local_root,remote_root,connection_id,template,status,ollama_only,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
         ON CONFLICT(project_id) DO UPDATE SET local_root=excluded.local_root,
         remote_root=excluded.remote_root,connection_id=excluded.connection_id,
         template=excluded.template,status=excluded.status,ollama_only=excluded.ollama_only,
         updated_at=excluded.updated_at",
    )
    .bind(project.id.to_string())
    .bind(&project.local_root)
    .bind(&project.remote_root)
    .bind(project.connection_id.map(|id| id.to_string()))
    .bind(enum_string(&project.template)?)
    .bind(enum_string(&project.status)?)
    .bind(if project.ollama_only { 1_i64 } else { 0_i64 })
    .bind(timestamp(project.updated_at))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_conversation(
    tx: &mut Transaction<'_, Sqlite>,
    conversation: &Conversation,
) -> Result<(), StoreError> {
    let project_exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)")
            .bind(conversation.project_id.to_string())
            .fetch_one(&mut **tx)
            .await?;
    if project_exists == 0 {
        return Err(StoreError::InvalidInput(format!(
            "conversation {} references unknown project {}",
            conversation.id, conversation.project_id
        )));
    }
    let project_id = conversation.project_id.to_string();
    let existing_project = sqlx::query_scalar::<_, String>(
        "SELECT project_id FROM conversation_records WHERE frame_id=?1",
    )
    .bind(conversation.id.to_string())
    .fetch_optional(&mut **tx)
    .await?;
    if existing_project
        .as_deref()
        .is_some_and(|stored| stored != project_id.as_str())
    {
        return Err(StoreError::InvalidInput(format!(
            "conversation {} already belongs to another project",
            conversation.id
        )));
    }
    sqlx::query(
        "INSERT INTO frames
         (id,parent_frame_id,root_frame_id,agent_name,status,project_id,created_at,updated_at)
         VALUES (?1,NULL,?1,'omicsops',?2,?3,?4,?5)
         ON CONFLICT(id) DO UPDATE SET root_frame_id=excluded.root_frame_id,
         status=excluded.status,project_id=excluded.project_id,updated_at=excluded.updated_at",
    )
    .bind(conversation.id.to_string())
    .bind(enum_string(&conversation.status)?)
    .bind(&project_id)
    .bind(timestamp(conversation.created_at))
    .bind(timestamp(conversation.updated_at))
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO conversation_records
         (frame_id,project_id,title,status,model_profile_id,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(frame_id) DO UPDATE SET project_id=excluded.project_id,title=excluded.title,
         status=excluded.status,model_profile_id=excluded.model_profile_id,
         created_at=excluded.created_at,updated_at=excluded.updated_at",
    )
    .bind(conversation.id.to_string())
    .bind(conversation.project_id.to_string())
    .bind(&conversation.title)
    .bind(enum_string(&conversation.status)?)
    .bind(conversation.model_profile_id.map(|id| id.to_string()))
    .bind(timestamp(conversation.created_at))
    .bind(timestamp(conversation.updated_at))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn ensure_frame(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: Uuid,
    conversation_id: Uuid,
    at: DateTime<Utc>,
) -> Result<(), StoreError> {
    let project_id_text = project_id.to_string();
    let conversation_id_text = conversation_id.to_string();
    let conversation_exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_records WHERE frame_id=?1 AND project_id=?2)",
    )
    .bind(&conversation_id_text)
    .bind(&project_id_text)
    .fetch_one(&mut **tx)
    .await?;
    if conversation_exists == 0 {
        return Err(StoreError::InvalidInput(format!(
            "message references unknown project or conversation: {project_id}/{conversation_id}"
        )));
    }
    let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM frames WHERE id=?1)")
        .bind(&conversation_id_text)
        .fetch_one(&mut **tx)
        .await?;
    if exists == 0 {
        sqlx::query(
            "INSERT INTO frames
             (id,parent_frame_id,root_frame_id,agent_name,status,project_id,created_at,updated_at)
             VALUES (?1,NULL,?1,'omicsops','idle',?2,?3,?3)",
        )
        .bind(&conversation_id_text)
        .bind(&project_id_text)
        .bind(timestamp(at))
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO conversation_records
             (frame_id,project_id,title,status,created_at,updated_at)
             VALUES (?1,?2,'','idle',?3,?3)",
        )
        .bind(&conversation_id_text)
        .bind(&project_id_text)
        .bind(timestamp(at))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn validate_project_input(project: &Project) -> Result<(), StoreError> {
    if project.name.trim().is_empty() {
        return Err(StoreError::InvalidInput(
            "project name cannot be blank".into(),
        ));
    }
    if project.local_root.trim().is_empty() {
        return Err(StoreError::InvalidInput(
            "project local root cannot be blank".into(),
        ));
    }
    if project
        .remote_root
        .as_deref()
        .is_some_and(|root| root.trim().is_empty())
    {
        return Err(StoreError::InvalidInput(
            "project remote root cannot be blank".into(),
        ));
    }
    Ok(())
}

async fn insert_message(
    tx: &mut Transaction<'_, Sqlite>,
    message: &Message,
) -> Result<(), StoreError> {
    let sequence = i64::try_from(message.sequence)
        .map_err(|_| StoreError::InvalidInput("message sequence exceeds SQLite range".into()))?;
    sqlx::query(
        "INSERT INTO messages
         (id,frame_id,project_id,conversation_id,seq,role,content,ts)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
         ON CONFLICT(id) DO UPDATE SET frame_id=excluded.frame_id,project_id=excluded.project_id,
         conversation_id=excluded.conversation_id,seq=excluded.seq,role=excluded.role,
         content=excluded.content,ts=excluded.ts",
    )
    .bind(message.id.to_string())
    .bind(message.conversation_id.to_string())
    .bind(message.project_id.to_string())
    .bind(message.conversation_id.to_string())
    .bind(sequence)
    .bind(enum_string(&message.role)?)
    .bind(&message.markdown)
    .bind(timestamp(message.created_at))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn ensure_event_context(
    tx: &mut SqliteConnection,
    event: &AgentEventV4,
) -> Result<(), StoreError> {
    let project_id = event.project_id.to_string();
    let conversation_id = event.conversation_id.to_string();
    let owns_conversation: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_records WHERE frame_id=?1 AND project_id=?2)",
    )
    .bind(&conversation_id)
    .bind(&project_id)
    .fetch_one(&mut *tx)
    .await?;
    if owns_conversation == 0 {
        return Err(StoreError::InvalidInput(format!(
            "event {} references unknown project or conversation: {project_id}/{conversation_id}",
            event.run_id
        )));
    }

    let run_id = event.run_id.to_string();
    let existing =
        sqlx::query("SELECT project_id,conversation_id FROM agent_runs_v4 WHERE run_id=?1")
            .bind(&run_id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(row) = existing {
        let stored_project = row.try_get::<String, _>(0)?;
        let stored_conversation = row.try_get::<String, _>(1)?;
        if stored_project != project_id || stored_conversation != conversation_id {
            return Err(StoreError::InvalidInput(format!(
                "event {} does not match its persisted run context",
                event.run_id
            )));
        }
    } else {
        sqlx::query(
            "INSERT INTO agent_runs_v4
             (run_id,project_id,conversation_id,status,value_json)
             VALUES (?1,?2,?3,'event_only','{}')",
        )
        .bind(&run_id)
        .bind(&project_id)
        .bind(&conversation_id)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

async fn ensure_event_context_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event: &AgentEventV4,
) -> Result<(), StoreError> {
    let project_id = event.project_id.to_string();
    let conversation_id = event.conversation_id.to_string();
    let owns_conversation: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM conversation_records WHERE frame_id=?1 AND project_id=?2)",
    )
    .bind(&conversation_id)
    .bind(&project_id)
    .fetch_one(&mut **tx)
    .await?;
    if owns_conversation == 0 {
        return Err(StoreError::InvalidInput(format!(
            "event {} references unknown project or conversation: {project_id}/{conversation_id}",
            event.run_id
        )));
    }

    let run_id = event.run_id.to_string();
    let existing =
        sqlx::query("SELECT project_id,conversation_id FROM agent_runs_v4 WHERE run_id=?1")
            .bind(&run_id)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(row) = existing {
        let stored_project = row.try_get::<String, _>(0)?;
        let stored_conversation = row.try_get::<String, _>(1)?;
        if stored_project != project_id || stored_conversation != conversation_id {
            return Err(StoreError::InvalidInput(format!(
                "event {} does not match its persisted run context",
                event.run_id
            )));
        }
    } else {
        sqlx::query(
            "INSERT INTO agent_runs_v4
             (run_id,project_id,conversation_id,status,value_json)
             VALUES (?1,?2,?3,'event_only','{}')",
        )
        .bind(&run_id)
        .bind(&project_id)
        .bind(&conversation_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn project_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Project, StoreError> {
    let id = parse_uuid(row.try_get::<String, _>(0)?, "project id")?;
    let workspace_dir = row.try_get::<String, _>(3)?;
    let local_root = row
        .try_get::<Option<String>, _>(6)?
        .unwrap_or(workspace_dir);
    Ok(Project {
        id,
        name: row.try_get(1)?,
        description: row.try_get(2)?,
        local_root,
        remote_root: row.try_get(7)?,
        connection_id: row
            .try_get::<Option<String>, _>(8)?
            .map(|value| parse_uuid(value.as_str(), "project connection id"))
            .transpose()?,
        template: parse_json_enum(
            row.try_get::<Option<String>, _>(9)?
                .as_deref()
                .unwrap_or("blank"),
            "project template",
        )?,
        status: parse_json_enum(
            row.try_get::<Option<String>, _>(10)?
                .as_deref()
                .unwrap_or("ready"),
            "project status",
        )?,
        ollama_only: row.try_get::<Option<i64>, _>(11)?.unwrap_or(0) != 0,
        created_at: from_timestamp(row.try_get(4)?, "project created_at")?,
        updated_at: from_timestamp(row.try_get(5)?, "project updated_at")?,
    })
}

fn conversation_agent_mode_setting_key(conversation_id: Uuid) -> String {
    format!("{CONVERSATION_AGENT_MODE_SETTING_PREFIX}{conversation_id}")
}

fn validate_latest_agent_run_json(
    value: &Value,
    run_id: &str,
    project_id: &str,
    conversation_id: &str,
    status: &str,
) -> Result<(), StoreError> {
    let Some(object) = value.as_object() else {
        return Err(StoreError::InvalidInput(format!(
            "latest Agent V4 run {run_id} must be a JSON object"
        )));
    };
    for (key, expected) in [
        ("run_id", run_id),
        ("project_id", project_id),
        ("conversation_id", conversation_id),
        ("status", status),
    ] {
        if let Some(actual) = object.get(key) {
            let Some(actual) = actual.as_str() else {
                return Err(StoreError::InvalidInput(format!(
                    "latest Agent V4 run {run_id} has a non-string {key}"
                )));
            };
            if actual != expected {
                return Err(StoreError::InvalidInput(format!(
                    "latest Agent V4 run {run_id} has inconsistent {key}"
                )));
            }
        }
    }
    Ok(())
}

fn parse_conversation_agent_mode(value: &str) -> Result<SessionAgentModeV4, StoreError> {
    serde_json::from_str(value)
        .or_else(|_| serde_json::from_value(Value::String(value.to_owned())))
        .map_err(|error| {
            StoreError::InvalidInput(format!("invalid conversation agent mode setting: {error}"))
        })
}

#[allow(clippy::too_many_arguments)]
async fn finalize_plan_revision_v4_in_connection(
    connection: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    revision: u64,
    plan: ExecutionPlanV4,
    markdown: String,
    plan_hash: String,
    timestamp: i64,
    stored_now: chrono::DateTime<Utc>,
    options: PlanRevisionFinalizeOptionsV4,
) -> Result<ProposedPlanRevisionV4, StoreError> {
    let owns_conversation: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM conversation_records
            WHERE frame_id=?1 AND project_id=?2
        )",
    )
    .bind(conversation_id.to_string())
    .bind(project_id.to_string())
    .fetch_one(&mut *connection)
    .await?;
    if owns_conversation == 0 {
        return Err(StoreError::InvalidInput(
            "plan generation context is not owned by the project".into(),
        ));
    }
    let owns_run: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM agent_runs_v4
            WHERE run_id=?1 AND project_id=?2 AND conversation_id=?3
        )",
    )
    .bind(run_id.to_string())
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_one(&mut *connection)
    .await?;
    if owns_run == 0 {
        return Err(StoreError::InvalidInput(
            "plan generation run does not belong to the requested context".into(),
        ));
    }
    let run_status: String = sqlx::query_scalar("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
        .bind(run_id.to_string())
        .fetch_one(&mut *connection)
        .await?;
    if matches!(
        run_status.as_str(),
        "completed" | "cancelled" | "failed" | "needs_attention"
    ) {
        return Err(StoreError::InvalidInput(
            "plan generation run is already terminal".into(),
        ));
    }
    let serialized =
        sqlx::query("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence")
            .bind(run_id.to_string())
            .fetch_all(&mut *connection)
            .await?
            .into_iter()
            .map(|row| row.try_get::<String, _>(0))
            .collect::<Result<Vec<_>, _>>()?;
    let existing_events = deserialize_event_chain_v4(&serialized)
        .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
    if existing_events.iter().any(is_terminal_event) {
        return Err(StoreError::InvalidInput(
            "plan generation run already has a terminal event".into(),
        ));
    }
    let row = sqlx::query(
        "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
         FROM proposed_plans WHERE project_id=?1 AND frame_id=?2 AND run_id=?3 AND revision=?4",
    )
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .bind(run_id.to_string())
    .bind(i64::try_from(revision).map_err(|_| {
        StoreError::InvalidInput("plan revision exceeds SQLite integer range".into())
    })?)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| StoreError::InvalidInput("generating plan revision was not found".into()))?;
    let current = proposed_plan_revision_from_row(row)?;
    if current.status != PlanRevisionStatusV4::Generating {
        return Err(StoreError::InvalidInput(
            "plan revision is no longer generating and cannot be finalized".into(),
        ));
    }
    // Materialization is the sole operation that intentionally replaces plan
    // content. The trigger is repaired before each open and recreated before
    // this transaction commits, so a pre-release trigger cannot broaden this
    // Store-owned authority.
    sqlx::query("DROP TRIGGER IF EXISTS trg_proposed_plans_immutable_content")
        .execute(&mut *connection)
        .await?;
    let finalized = sqlx::query(
        "UPDATE proposed_plans
         SET plan_hash=?1,plan_json=?2,markdown=?3,status='pending',updated_at=?4
         WHERE id=?5 AND status='generating'",
    )
    .bind(&plan_hash)
    .bind(serde_json::to_string(&plan)?)
    .bind(&markdown)
    .bind(timestamp)
    .bind(current.id.to_string())
    .execute(&mut *connection)
    .await?;
    if finalized.rows_affected() != 1 {
        return Err(StoreError::InvalidInput(
            "plan generation changed before it could be finalized".into(),
        ));
    }
    sqlx::query(PROPOSED_PLAN_TRIGGER_SQL)
        .execute(&mut *connection)
        .await?;

    let value_json: String =
        sqlx::query_scalar("SELECT value_json FROM agent_runs_v4 WHERE run_id=?1")
            .bind(run_id.to_string())
            .fetch_one(&mut *connection)
            .await?;
    let mut value: Value = serde_json::from_str(&value_json)?;
    let object = value.as_object_mut().ok_or_else(|| {
        StoreError::InvalidInput(
            "plan finalization requires agent_runs_v4.value_json to be a JSON object".into(),
        )
    })?;
    object.insert("status".into(), Value::String("awaiting_approval".into()));
    object.insert("plan".into(), serde_json::to_value(&plan)?);
    object.insert("plan_hash".into(), Value::String(plan_hash.clone()));
    object.insert("plan_revision".into(), Value::from(revision));
    if let Some(approval_hash) = &options.approval_hash {
        object.insert("approval_hash".into(), Value::String(approval_hash.clone()));
    }
    if let Some(compute_selection) = &options.compute_selection {
        object.insert(
            "compute_selection".into(),
            serde_json::to_value(compute_selection)?,
        );
    }
    sqlx::query(
        "UPDATE agent_runs_v4 SET status='awaiting_approval',value_json=?,updated_at=? WHERE run_id=?3",
    )
    .bind(serde_json::to_string(&value)?)
    .bind(timestamp)
    .bind(run_id.to_string())
    .execute(&mut *connection)
    .await?;
    Ok(ProposedPlanRevisionV4 {
        id: current.id,
        project_id,
        conversation_id,
        run_id,
        revision,
        plan,
        markdown,
        plan_hash,
        status: PlanRevisionStatusV4::Pending,
        feedback: current.feedback,
        created_at: current.created_at,
        updated_at: stored_now,
    })
}

async fn ensure_conversation_owner_executor(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    let owns: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM conversation_records
            WHERE frame_id=?1 AND project_id=?2
        )",
    )
    .bind(conversation_id.to_string())
    .bind(project_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if owns == 0 {
        return Err(StoreError::InvalidInput(format!(
            "conversation {conversation_id} does not belong to project {project_id}"
        )));
    }
    Ok(())
}

async fn ensure_conversation_unlocked_executor(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), StoreError> {
    ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
    let locked: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM proposed_plans
            WHERE project_id=?1 AND frame_id=?2
              AND status IN ('generating','revising','pending')
        )",
    )
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if locked != 0 {
        return Err(StoreError::InvalidInput(
            "conversation is locked by an active plan; approve, request changes, or cancel it first".into(),
        ));
    }
    Ok(())
}

fn validate_plan_revision_input(
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    revision: u64,
    plan: &ExecutionPlanV4,
    plan_hash: &str,
    status: PlanRevisionStatusV4,
) -> Result<(), StoreError> {
    let _ = (project_id, conversation_id, run_id);
    if revision == 0 {
        return Err(StoreError::InvalidInput(
            "plan revision must be greater than zero".into(),
        ));
    }
    if plan_hash.trim().is_empty() {
        return Err(StoreError::InvalidInput(
            "proposed plan hash cannot be empty".into(),
        ));
    }
    if !status.is_active()
        && !matches!(
            status,
            PlanRevisionStatusV4::Approved
                | PlanRevisionStatusV4::Superseded
                | PlanRevisionStatusV4::Cancelled
        )
    {
        return Err(StoreError::InvalidInput(
            "unknown plan revision status".into(),
        ));
    }
    plan.validate()
        .map_err(|error| StoreError::InvalidInput(error.to_string()))
}

async fn ensure_run_owner_executor(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
) -> Result<(), StoreError> {
    let owns: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM agent_runs_v4
            WHERE run_id=?1 AND project_id=?2 AND conversation_id=?3
        )",
    )
    .bind(run_id.to_string())
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if owns == 0 {
        return Err(StoreError::InvalidInput(format!(
            "run {run_id} does not belong to conversation {conversation_id} in project {project_id}"
        )));
    }
    Ok(())
}

fn validate_approval_inputs(
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    spec: &RunSpecV4,
    run_value: &Value,
) -> Result<(), StoreError> {
    if spec.run_id != run_id
        || spec.project_id != project_id
        || spec.conversation_id != conversation_id
    {
        return Err(StoreError::InvalidInput(
            "frozen specification does not belong to the requested run context".into(),
        ));
    }
    spec.validate_integrity()
        .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
    if !run_value.is_object() {
        return Err(StoreError::InvalidInput(
            "approved run value must be a JSON object".into(),
        ));
    }
    Ok(())
}

async fn ensure_run_awaiting_approval(
    tx: &mut SqliteConnection,
    run_id: Uuid,
) -> Result<(), StoreError> {
    let status =
        sqlx::query_scalar::<_, String>("SELECT status FROM agent_runs_v4 WHERE run_id=?1")
            .bind(run_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("run was not found".into()))?;
    if status != "awaiting_approval" {
        if matches!(
            status.as_str(),
            "completed" | "cancelled" | "failed" | "needs_attention"
        ) {
            return Err(StoreError::InvalidInput(
                "terminal run cannot be approved; run is not awaiting approval".into(),
            ));
        }
        return Err(StoreError::InvalidInput(format!(
            "run must be awaiting approval, but is {status}"
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_proposed_plan_revision_in_connection(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    revision: u64,
    plan: ExecutionPlanV4,
    markdown: String,
    plan_hash: &str,
    status: PlanRevisionStatusV4,
    feedback: Option<String>,
    now: DateTime<Utc>,
) -> Result<ProposedPlanRevisionV4, StoreError> {
    validate_plan_revision_input(
        project_id,
        conversation_id,
        run_id,
        revision,
        &plan,
        plan_hash,
        status,
    )?;
    let actual_hash = plan
        .canonical_hash()
        .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
    if actual_hash != plan_hash {
        return Err(StoreError::InvalidInput(
            "proposed plan hash does not match the structured plan".into(),
        ));
    }
    let timestamp = timestamp(now);
    let stored_now = from_timestamp(timestamp, "plan revision timestamp")?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO proposed_plans
         (id,project_id,frame_id,revision,plan_hash,status,plan_json,markdown,feedback,run_id,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    )
    .bind(id.to_string())
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .bind(i64::try_from(revision).map_err(|_| {
        StoreError::InvalidInput("plan revision exceeds SQLite integer range".into())
    })?)
    .bind(plan_hash)
    .bind(enum_string(&status)?)
    .bind(serde_json::to_string(&plan)?)
    .bind(&markdown)
    .bind(feedback.as_deref())
    .bind(run_id.to_string())
    .bind(timestamp)
    .bind(timestamp)
    .execute(&mut *tx)
    .await?;
    if status.is_active() {
        upsert_conversation_mode_in_tx(
            &mut *tx,
            conversation_id,
            SessionAgentModeV4::Plan,
            stored_now,
        )
        .await?;
    }
    Ok(ProposedPlanRevisionV4 {
        id,
        project_id,
        conversation_id,
        run_id,
        revision,
        plan,
        markdown,
        plan_hash: plan_hash.to_owned(),
        status,
        feedback,
        created_at: stored_now,
        updated_at: stored_now,
    })
}

#[allow(clippy::too_many_arguments)]
async fn approve_plan_revision_v4_in_connection(
    tx: &mut SqliteConnection,
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    revision: u64,
    plan_hash: &str,
    spec: &RunSpecV4,
    run_value: &Value,
    options: ApprovalOptionsV4,
) -> Result<PlanApprovalResultV4, StoreError> {
    ensure_conversation_owner_executor(&mut *tx, project_id, conversation_id).await?;
    ensure_run_owner_executor(&mut *tx, project_id, conversation_id, run_id).await?;
    ensure_run_awaiting_approval(&mut *tx, run_id).await?;
    let row = sqlx::query(
        "SELECT id,project_id,frame_id,run_id,revision,plan_json,markdown,plan_hash,status,feedback,created_at,updated_at
         FROM proposed_plans WHERE project_id=?1 AND frame_id=?2
         ORDER BY revision DESC,id DESC LIMIT 1",
    )
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| StoreError::InvalidInput("no proposed plan revision exists".into()))?;
    let current = proposed_plan_revision_from_row(row)?;
    if current.revision != revision {
        return Err(StoreError::InvalidInput(
            "only the latest plan revision can be approved".into(),
        ));
    }
    if current.status != PlanRevisionStatusV4::Pending {
        return Err(StoreError::InvalidInput(
            "only a pending plan revision can be approved".into(),
        ));
    }
    if current.run_id != run_id || current.plan_hash != plan_hash {
        return Err(StoreError::InvalidInput(
            "plan revision ownership or hash does not match the approval".into(),
        ));
    }
    if spec.approved_plan_hash != plan_hash {
        return Err(StoreError::InvalidInput(
            "frozen specification plan hash does not match the requested revision".into(),
        ));
    }
    if current.plan != spec.plan {
        return Err(StoreError::InvalidInput(
            "frozen specification plan does not match the pending revision".into(),
        ));
    }

    let mut persisted_value = run_value.clone();
    let object = persisted_value.as_object_mut().ok_or_else(|| {
        StoreError::InvalidInput("approved run value must be a JSON object".into())
    })?;
    object.insert("status".into(), Value::String("running".into()));
    object.insert("spec".into(), serde_json::to_value(spec)?);
    object.insert("plan_revision".into(), Value::from(revision));
    object.insert("session_mode".into(), Value::String("agent".into()));
    let now = Utc::now();
    let updated = sqlx::query(
        "UPDATE proposed_plans SET status='approved',updated_at=?1 WHERE id=?2 AND status='pending'",
    )
    .bind(timestamp(now))
    .bind(current.id.to_string())
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(StoreError::InvalidInput(
            "plan revision changed before approval could be committed".into(),
        ));
    }
    // A newer revision supersedes all older active proposals. Their immutable
    // plan content remains available for audit/replay, while the approved
    // latest revision is the sole source of the lock state.
    sqlx::query(
        "UPDATE proposed_plans SET status='superseded',updated_at=?1
         WHERE project_id=?2 AND frame_id=?3 AND revision<?4
           AND status IN ('generating','revising','pending')",
    )
    .bind(timestamp(now))
    .bind(project_id.to_string())
    .bind(conversation_id.to_string())
    .bind(i64::try_from(revision).map_err(|_| {
        StoreError::InvalidInput("plan revision exceeds SQLite integer range".into())
    })?)
    .execute(&mut *tx)
    .await?;
    maybe_fail_approval(options, 1)?;
    let updated_run = sqlx::query(
        "UPDATE agent_runs_v4 SET status='running',value_json=?,updated_at=? WHERE run_id=?3",
    )
    .bind(serde_json::to_string(&persisted_value)?)
    .bind(timestamp(now))
    .bind(run_id.to_string())
    .execute(&mut *tx)
    .await?;
    if updated_run.rows_affected() != 1 {
        return Err(StoreError::InvalidInput(
            "run changed before approval could be committed".into(),
        ));
    }
    maybe_fail_approval(options, 2)?;

    let mut events = load_agent_events_in_tx(&mut *tx, run_id).await?;
    let mut appended = Vec::new();
    let approval = if let Some(previous) = events.last() {
        AgentEventV4::next(
            previous,
            now,
            AgentEventKindV4::PlanApproved {
                plan_hash: plan_hash.to_owned(),
            },
        )
    } else {
        AgentEventV4::first(
            run_id,
            project_id,
            conversation_id,
            now,
            AgentEventKindV4::PlanApproved {
                plan_hash: plan_hash.to_owned(),
            },
        )
    };
    insert_agent_event_in_tx(&mut *tx, &approval).await?;
    events.push(approval.clone());
    appended.push(approval);
    let frozen = AgentEventV4::next(
        events.last().expect("approval event was appended"),
        now,
        AgentEventKindV4::RunSpecFrozen {
            approval_hash: spec.approval_hash.clone().unwrap_or_default(),
            spec_hash: spec.spec_hash.clone().unwrap_or_default(),
        },
    );
    insert_agent_event_in_tx(&mut *tx, &frozen).await?;
    events.push(frozen.clone());
    appended.push(frozen);
    let mode = AgentEventV4::next(
        events.last().expect("frozen event was appended"),
        now,
        AgentEventKindV4::ModeChanged {
            mode: RunModeV4::Execute,
        },
    );
    insert_agent_event_in_tx(&mut *tx, &mode).await?;
    appended.push(mode);
    maybe_fail_approval(options, 3)?;
    upsert_conversation_mode_in_tx(&mut *tx, conversation_id, SessionAgentModeV4::Agent, now)
        .await?;
    maybe_fail_approval(options, 4)?;
    Ok(PlanApprovalResultV4 {
        run_id,
        project_id,
        conversation_id,
        revision,
        plan_hash: plan_hash.to_owned(),
        approval_hash: spec.approval_hash.clone(),
        spec_hash: spec.spec_hash.clone(),
        mode: SessionAgentModeV4::Agent,
        events: appended,
    })
}

fn proposed_plan_revision_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ProposedPlanRevisionV4, StoreError> {
    let id = parse_uuid(row.try_get::<String, _>(0)?, "proposed plan revision id")?;
    let project_id = parse_uuid(row.try_get::<String, _>(1)?, "proposed plan project id")?;
    let conversation_id = parse_uuid(
        row.try_get::<String, _>(2)?,
        "proposed plan conversation id",
    )?;
    let run_id = parse_uuid(
        row.try_get::<Option<String>, _>(3)?.ok_or_else(|| {
            StoreError::InvalidInput(format!("proposed plan revision {id} has no run id"))
        })?,
        "proposed plan run id",
    )?;
    let revision = u64::try_from(row.try_get::<i64, _>(4)?).map_err(|_| {
        StoreError::InvalidInput(format!(
            "proposed plan revision {id} is outside the supported range"
        ))
    })?;
    let plan_json = row.try_get::<String, _>(5)?;
    let plan = serde_json::from_str::<ExecutionPlanV4>(&plan_json).map_err(|error| {
        StoreError::InvalidInput(format!(
            "proposed plan revision {id} has invalid plan JSON: {error}"
        ))
    })?;
    let plan_hash = row.try_get::<String, _>(7)?;
    let status = parse_json_enum(
        row.try_get::<String, _>(8)?.as_str(),
        "plan revision status",
    )?;
    Ok(ProposedPlanRevisionV4 {
        id,
        project_id,
        conversation_id,
        run_id,
        revision,
        plan,
        markdown: row.try_get(6)?,
        plan_hash,
        status,
        feedback: row.try_get(9)?,
        created_at: from_timestamp(row.try_get(10)?, "plan revision created_at")?,
        updated_at: from_timestamp(row.try_get(11)?, "plan revision updated_at")?,
    })
}

async fn load_agent_run_value_in_tx(
    tx: &mut SqliteConnection,
    run_id: Uuid,
) -> Result<Value, StoreError> {
    let value: String = sqlx::query_scalar("SELECT value_json FROM agent_runs_v4 WHERE run_id=?1")
        .bind(run_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
    let value: Value = serde_json::from_str(&value)?;
    if !value.is_object() {
        return Err(StoreError::InvalidInput(format!(
            "agent run {run_id} value_json must be a JSON object"
        )));
    }
    Ok(value)
}

async fn set_agent_run_status_in_tx(
    tx: &mut SqliteConnection,
    run_id: Uuid,
    status: &str,
) -> Result<(), StoreError> {
    let mut value = load_agent_run_value_in_tx(&mut *tx, run_id).await?;
    let object = value.as_object_mut().ok_or_else(|| {
        StoreError::InvalidInput(format!(
            "agent run {run_id} value_json must be a JSON object"
        ))
    })?;
    object.insert("status".into(), Value::String(status.into()));
    sqlx::query("UPDATE agent_runs_v4 SET status=?1,value_json=?2,updated_at=?3 WHERE run_id=?4")
        .bind(status)
        .bind(serde_json::to_string(&value)?)
        .bind(timestamp(Utc::now()))
        .bind(run_id.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(())
}

async fn upsert_conversation_mode_in_tx(
    tx: &mut SqliteConnection,
    conversation_id: Uuid,
    mode: SessionAgentModeV4,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO settings (scope,key,value_json,updated_at)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT(scope,key) DO UPDATE SET value_json=excluded.value_json,
         updated_at=excluded.updated_at",
    )
    .bind(SETTINGS_GLOBAL_SCOPE)
    .bind(conversation_agent_mode_setting_key(conversation_id))
    .bind(serde_json::to_string(&mode)?)
    .bind(timestamp(now))
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn load_agent_events_in_tx(
    tx: &mut SqliteConnection,
    run_id: Uuid,
) -> Result<Vec<AgentEventV4>, StoreError> {
    let rows =
        sqlx::query("SELECT value_json FROM agent_events_v4 WHERE run_id=?1 ORDER BY sequence")
            .bind(run_id.to_string())
            .fetch_all(&mut *tx)
            .await?;
    let serialized = rows
        .into_iter()
        .map(|row| row.try_get::<String, _>(0))
        .collect::<Result<Vec<_>, _>>()?;
    deserialize_event_chain_v4(&serialized)
        .map_err(|error| StoreError::InvalidInput(error.to_string()))
}

/// Find and validate the one unfinished, current-revision Plan MCP approval
/// request. A request whose dispatch has already started is not a resumable
/// approval: it must be handled by uncertain-side-effect recovery instead.
fn current_plan_approval_request<'a>(
    events: &'a [AgentEventV4],
    scope: PlanApprovalScopeV4,
) -> Result<&'a ToolApprovalRequestV4, StoreError> {
    let scope_hash = scope.hash();
    let mut pending: Option<&ToolApprovalRequestV4> = None;
    for event in events {
        let AgentEventKindV4::ToolApprovalRequested { request } = &event.event else {
            continue;
        };
        if request.mode != RunModeV4::Plan
            || request.scope_hash.as_deref() != Some(scope_hash.as_str())
            || request.effect != ToolEffectV4::ReadOnly
            || request.call.tool_id != "use_mcp_tool"
        {
            continue;
        }
        let requested = events.iter().any(|candidate| {
            matches!(
                &candidate.event,
                AgentEventKindV4::ToolRequested { call } if call == &request.call
            )
        });
        let finished = events.iter().any(|candidate| {
            matches!(
                &candidate.event,
                AgentEventKindV4::ToolFinished { outcome }
                    if outcome.call_id == request.call.call_id
            ) || matches!(
                &candidate.event,
                AgentEventKindV4::ToolOutcomeReused { outcome, .. }
                    if outcome.call_id == request.call.call_id
            )
        });
        let dispatched = events.iter().any(|candidate| {
            matches!(
                &candidate.event,
                AgentEventKindV4::ToolDispatchStarted { call_id, .. }
                    if call_id == &request.call.call_id
            ) || matches!(
                &candidate.event,
                AgentEventKindV4::ToolDispatchUncertain { call_id, .. }
                    if call_id == &request.call.call_id
            ) || matches!(
                &candidate.event,
                AgentEventKindV4::ToolDispatchResolved { call_id, .. }
                    if call_id == &request.call.call_id
            )
        });
        if requested && !finished && !dispatched {
            if pending.is_some() {
                return Err(StoreError::InvalidInput(
                    "multiple pending Plan tool approvals are not supported".into(),
                ));
            }
            request
                .validate_with_scope(scope.run_id, &scope_hash, RunModeV4::Plan)
                .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
            pending = Some(request);
        }
    }
    pending.ok_or_else(|| {
        StoreError::InvalidInput(
            "current revision has no unfinished Plan tool approval request".into(),
        )
    })
}

/// Find the one unfinished, current-revision Plan MCP approval request and
/// validate its optional decision. `None` means the request is still waiting;
/// `Some` is the single approved/denied decision that may unlock resume.
fn plan_approval_request_state(
    events: &[AgentEventV4],
    scope: PlanApprovalScopeV4,
) -> Result<Option<ToolApprovalDecisionV4>, StoreError> {
    let request = current_plan_approval_request(events, scope)?;
    let mut decision = None;
    for event in events {
        if let AgentEventKindV4::ToolApprovalDecided {
            approval_id,
            call_hash,
            decision: value,
        } = &event.event
        {
            if approval_id == &request.approval_id {
                if call_hash != &request.call_hash || decision.is_some() {
                    return Err(StoreError::InvalidInput(
                        "tool approval decision is duplicated or tampered".into(),
                    ));
                }
                decision = Some(*value);
            }
        }
    }
    Ok(decision)
}

fn is_terminal_event(event: &AgentEventV4) -> bool {
    matches!(
        event.event,
        AgentEventKindV4::RunCompleted
            | AgentEventKindV4::RunFailed { .. }
            | AgentEventKindV4::RunNeedsAttention { .. }
            | AgentEventKindV4::RunCancelled
    )
}

fn post_terminal_browser_cleanup_allowed(existing: &[AgentEventV4], event: &AgentEventV4) -> bool {
    matches!(
        event.event,
        AgentEventKindV4::BrowserTabCleanupRequired { .. }
    ) && !existing.iter().any(|candidate| {
        matches!(
            candidate.event,
            AgentEventKindV4::BrowserTabCleanupRequired { .. }
        )
    })
}

async fn insert_agent_event_in_tx(
    tx: &mut SqliteConnection,
    event: &AgentEventV4,
) -> Result<(), StoreError> {
    event
        .verify()
        .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
    ensure_event_context(&mut *tx, event).await?;
    let existing = load_agent_events_in_tx(&mut *tx, event.run_id).await?;
    if existing.iter().any(is_terminal_event)
        && !post_terminal_browser_cleanup_allowed(&existing, event)
    {
        return Err(StoreError::InvalidInput(
            "V4 run event chain is terminal; no events may be appended".into(),
        ));
    }
    let sequence = i64::try_from(event.sequence)
        .map_err(|_| StoreError::InvalidInput("event sequence exceeds SQLite range".into()))?;
    sqlx::query(
        "INSERT INTO agent_events_v4
         (run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json,occurred_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
    )
    .bind(event.run_id.to_string())
    .bind(event.project_id.to_string())
    .bind(event.conversation_id.to_string())
    .bind(sequence)
    .bind(&event.previous_hash)
    .bind(&event.event_hash)
    .bind(serde_json::to_string(event)?)
    .bind(timestamp(event.occurred_at))
        .execute(&mut *tx)
    .await?;
    runtime_jobs::observe_runtime_event(tx, event).await?;
    Ok(())
}

fn maybe_fail_approval(options: ApprovalOptionsV4, step: u8) -> Result<(), StoreError> {
    if options.fail_after_step == Some(step) {
        return Err(StoreError::Migration(format!(
            "injected plan approval failure after step {step}"
        )));
    }
    Ok(())
}

fn maybe_fail_plan_request(
    options: PlanRevisionRequestOptionsV4,
    step: u8,
) -> Result<(), StoreError> {
    if options.fail_after_step == Some(step) {
        return Err(StoreError::Migration(format!(
            "injected plan revision request failure after step {step}"
        )));
    }
    Ok(())
}

fn generation_seed_plan(objective: &str, revision: u64) -> ExecutionPlanV4 {
    ExecutionPlanV4 {
        schema_version: 4,
        objective: objective.to_owned(),
        steps: vec![format!("Generate immutable plan revision {revision}")],
        completion_criteria: vec!["A schema-valid plan is returned".into()],
        requested_capabilities: Default::default(),
    }
}

fn conversation_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Conversation, StoreError> {
    Ok(Conversation {
        id: parse_uuid(row.try_get::<String, _>(0)?, "conversation id")?,
        project_id: parse_uuid(row.try_get::<String, _>(1)?, "conversation project id")?,
        title: row.try_get(2)?,
        status: parse_json_enum(row.try_get::<String, _>(3)?.as_str(), "conversation status")?,
        model_profile_id: row
            .try_get::<Option<String>, _>(4)?
            .map(|value| parse_uuid(value.as_str(), "conversation model profile id"))
            .transpose()?,
        created_at: from_timestamp(row.try_get(5)?, "conversation created_at")?,
        updated_at: from_timestamp(row.try_get(6)?, "conversation updated_at")?,
    })
}

fn message_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Message, StoreError> {
    let project_id = row
        .try_get::<Option<String>, _>(7)?
        .ok_or_else(|| StoreError::InvalidInput("message has no project id".into()))?;
    Ok(Message {
        id: parse_uuid(row.try_get::<String, _>(0)?, "message id")?,
        project_id: parse_uuid(project_id.as_str(), "message project id")?,
        conversation_id: parse_uuid(
            row.try_get::<Option<String>, _>(2)?
                .as_deref()
                .unwrap_or_default(),
            "message conversation id",
        )?,
        sequence: u64::try_from(row.try_get::<i64, _>(3)?)
            .map_err(|_| StoreError::InvalidInput("message sequence is negative".into()))?,
        role: parse_json_enum(row.try_get::<String, _>(4)?.as_str(), "message role")?,
        markdown: row.try_get::<Option<String>, _>(5)?.unwrap_or_default(),
        created_at: from_timestamp(row.try_get(6)?, "message timestamp")?,
    })
}

fn artifact_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Artifact, StoreError> {
    Ok(Artifact {
        id: parse_uuid(row.try_get::<String, _>(0)?, "artifact id")?,
        project_id: parse_uuid(row.try_get::<String, _>(1)?, "artifact project id")?,
        run_id: row
            .try_get::<Option<String>, _>(2)?
            .map(|value| parse_uuid(value.as_str(), "artifact run id"))
            .transpose()?,
        relative_path: row.try_get(3)?,
        remote_path: row.try_get(4)?,
        media_type: row.try_get(5)?,
        size_bytes: u64::try_from(row.try_get::<i64, _>(6)?)
            .map_err(|_| StoreError::InvalidInput("artifact size is negative".into()))?,
        sha256: row.try_get(7)?,
        verified: row.try_get::<i64, _>(8)? != 0,
        created_at: from_timestamp(row.try_get(9)?, "artifact created_at")?,
    })
}

fn timestamp(value: DateTime<Utc>) -> i64 {
    value.timestamp_millis()
}

fn from_timestamp(value: i64, label: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::<Utc>::from_timestamp_millis(value).ok_or_else(|| {
        StoreError::InvalidInput(format!("{label} is outside the supported timestamp range"))
    })
}

fn parse_legacy_timestamp(value: &str, label: &str) -> Result<DateTime<Utc>, StoreError> {
    if let Ok(millis) = value.parse::<i64>() {
        return from_timestamp(millis, label);
    }
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| StoreError::Migration(format!("{label} is invalid: {error}")))
}

fn parse_uuid(value: impl AsRef<str>, label: &str) -> Result<Uuid, StoreError> {
    Uuid::parse_str(value.as_ref())
        .map_err(|error| StoreError::InvalidInput(format!("invalid {label}: {error}")))
}

fn enum_string<T: Serialize>(value: &T) -> Result<String, StoreError> {
    let value = serde_json::to_value(value)?;
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| StoreError::InvalidInput("enum did not serialize as a string".into()))
}

fn parse_json_enum<T: DeserializeOwned>(value: &str, label: &str) -> Result<T, StoreError> {
    serde_json::from_value(Value::String(value.to_owned()))
        .map_err(|error| StoreError::InvalidInput(format!("invalid {label}: {error}")))
}

fn is_run_completed(event: &AgentEventV4) -> bool {
    matches!(
        event.event,
        omicsops_protocol::AgentEventKindV4::RunCompleted
    )
}

fn completion_answer(events: &[AgentEventV4]) -> Result<String, StoreError> {
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
            StoreError::InvalidInput(
                "RunCompleted has no persisted CompletionProposalSubmitted event".into(),
            )
        })?;
    let value = serde_json::to_value(proposal)?;
    let answer = value
        .get("answer_markdown")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            value
                .get("summary")
                .and_then(Value::as_str)
                .filter(|text| !text.trim().is_empty())
        })
        .map(str::trim)
        .unwrap_or_default();
    if answer.is_empty() {
        return Err(StoreError::InvalidInput(
            "RunCompleted cannot be persisted without a non-empty assistant answer".into(),
        ));
    }
    Ok(answer.to_owned())
}

async fn persist_completion_message(
    tx: &mut Transaction<'_, Sqlite>,
    events: &[AgentEventV4],
    completed: &AgentEventV4,
) -> Result<Option<Message>, StoreError> {
    let markdown = completion_answer(events)?;
    let message_id = completed.run_id.to_string();
    let existing = sqlx::query(
        "SELECT id,project_id,conversation_id,seq,role,content,ts FROM messages WHERE id=?1",
    )
    .bind(&message_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(row) = existing {
        let message = Message {
            id: parse_uuid(row.try_get::<String, _>(0)?.as_str(), "message id")?,
            project_id: parse_uuid(row.try_get::<String, _>(1)?.as_str(), "message project id")?,
            conversation_id: parse_uuid(
                row.try_get::<String, _>(2)?.as_str(),
                "message conversation id",
            )?,
            sequence: u64::try_from(row.try_get::<i64, _>(3)?).map_err(|_| {
                StoreError::InvalidInput("stored message sequence is negative".into())
            })?,
            role: parse_json_enum(row.try_get::<String, _>(4)?.as_str(), "message role")?,
            markdown: row.try_get::<Option<String>, _>(5)?.unwrap_or_default(),
            created_at: from_timestamp(row.try_get(6)?, "message timestamp")?,
        };
        if message.project_id != completed.project_id
            || message.conversation_id != completed.conversation_id
            || message.role != MessageRole::Assistant
            || message.markdown != markdown
        {
            return Err(StoreError::InvalidInput(
                "run completion message id is already used by a different message".into(),
            ));
        }
        return Ok(None);
    }
    ensure_frame(
        tx,
        completed.project_id,
        completed.conversation_id,
        completed.occurred_at,
    )
    .await?;
    let sequence: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(seq),0)+1 FROM messages WHERE frame_id=?1")
            .bind(completed.conversation_id.to_string())
            .fetch_one(&mut **tx)
            .await?;
    let sequence = u64::try_from(sequence)
        .map_err(|_| StoreError::InvalidInput("conversation message sequence overflow".into()))?;
    let message = Message::markdown(
        completed.run_id,
        completed.project_id,
        completed.conversation_id,
        sequence,
        MessageRole::Assistant,
        markdown,
        completed.occurred_at,
    );
    insert_message(tx, &message).await?;
    Ok(Some(message))
}

#[cfg(test)]
mod lifecycle_tests;
