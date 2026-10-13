//! Typed immutable host-validated manifests. No source text or filesystem access.
use crate::{Store, StoreError};
use chrono::{DateTime, Utc};
use omicsops_core::redaction::redact_secrets;
use omicsops_dto::*;
use omicsops_science::{AnalysisRecordV4, AnalysisStatusV4, ArtifactRecordV4};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, SqliteConnection, Transaction, sqlite::SqliteRow};
use std::collections::BTreeSet;
use uuid::Uuid;

const OPERATION: &str = "create_file_baseline";
const MAX_CAPTURE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PROJECT_BYTES: i64 = 256 * 1024 * 1024;

/// Fixed internal schema; never deserialize frontend requests into this type.
/// `branch_conversation_id` is the original association, retained after unlink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselineManifestV1 {
    pub schema_version: u8,
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch_conversation_id: Option<Uuid>,
    pub title: String,
    pub entries: Vec<FileBaselineManifestEntryV1>,
    pub captured_text_bytes: u64,
    pub created_at: String,
    pub local_root_binding_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBaselineManifestEntryV1 {
    pub entry: FileBaselineEntryV1,
    pub blob_sha256: Option<String>,
}

/// Host supplies already validated metadata and the exact canonical request digest.
#[derive(Debug, Clone)]
pub struct SaveFileBaselineRecord {
    pub request_id: Uuid,
    pub request_sha256: String,
    pub baseline: FileBaselineV1,
    pub internal_manifest_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFileBaseline {
    pub baseline: FileBaselineV1,
    pub local_root_binding_sha256: String,
    pub blob_sha256_by_entry: Vec<Option<String>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineReceipt {
    project_id: Uuid,
    baseline_id: Uuid,
}

impl Store {
    pub async fn workspace_find_file_baseline_request(
        &self,
        request_id: Uuid,
        request_sha256: &str,
    ) -> Result<Option<FileBaselineV1>, StoreError> {
        let mut tx = self.pool.begin().await?;
        let receipt = replay(&mut tx, request_id, request_sha256).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    pub async fn workspace_save_file_baseline(
        &self,
        record: SaveFileBaselineRecord,
    ) -> Result<FileBaselineV1, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(result) = replay(&mut tx, record.request_id, &record.request_sha256).await? {
            return Ok(result);
        }
        let manifest = parse_manifest(
            &record.internal_manifest_json,
            &record.baseline.manifest_sha256,
        )?;
        let original = public_baseline(&manifest, &record.baseline.manifest_sha256);
        if original != record.baseline {
            return Err(invalid("invalid_selection"));
        }
        ensure_project(&mut tx, original.project_id).await?;
        ensure_branch(
            &mut tx,
            original.project_id,
            original.branch_conversation_id,
        )
        .await?;
        let (count, bytes): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),COALESCE(SUM(captured_text_bytes),0) FROM exploration_baselines_v4 WHERE project_id=?"
        ).bind(original.project_id.to_string()).fetch_one(&mut *tx).await?;
        if count >= 64 || bytes > MAX_PROJECT_BYTES - original.captured_text_bytes as i64 {
            return Err(invalid("limit_exceeded"));
        }
        let created_at = utc_timestamp(&original.created_at)?;
        sqlx::query("INSERT INTO exploration_baselines_v4(id,project_id,branch_conversation_id,request_id,request_sha256,manifest_sha256,manifest_json,captured_text_bytes,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(original.id.to_string()).bind(original.project_id.to_string()).bind(original.branch_conversation_id.map(|id| id.to_string()))
            .bind(record.request_id.to_string()).bind(&record.request_sha256).bind(&original.manifest_sha256)
            .bind(&record.internal_manifest_json).bind(original.captured_text_bytes as i64).bind(created_at).execute(&mut *tx).await?;
        let receipt = BaselineReceipt {
            project_id: original.project_id,
            baseline_id: original.id,
        };
        // Preserve the supplied canonical digest; do not hash its JSON string again.
        sqlx::query("INSERT INTO workspace_request_ledger(request_id,operation,request_sha256,result_json,created_at) VALUES(?,?,?,?,?)")
            .bind(record.request_id.to_string()).bind(OPERATION).bind(&record.request_sha256).bind(serde_json::to_string(&receipt)?).bind(Utc::now().timestamp_millis()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(original)
    }

    pub async fn workspace_list_file_baselines(
        &self,
        request: &ListFileBaselinesRequestV1,
    ) -> Result<FileBaselinePageV1, StoreError> {
        if !(1..=64).contains(&request.limit) {
            return Err(invalid("invalid_selection"));
        }
        let mut tx = self.pool.begin().await?;
        ensure_project(&mut tx, request.project_id).await?;
        ensure_branch(&mut tx, request.project_id, request.branch_conversation_id).await?;
        let rows = sqlx::query("SELECT * FROM exploration_baselines_v4 WHERE project_id=? AND (? IS NULL OR branch_conversation_id=?) ORDER BY created_at DESC,id LIMIT ? OFFSET ?")
            .bind(request.project_id.to_string()).bind(request.branch_conversation_id.map(|id|id.to_string())).bind(request.branch_conversation_id.map(|id|id.to_string()))
            .bind(i64::from(request.limit)+1).bind(i64::from(request.offset)).fetch_all(&mut *tx).await?;
        let next_offset = if rows.len() > request.limit as usize {
            request.offset.checked_add(request.limit)
        } else {
            None
        };
        let items = rows
            .iter()
            .take(request.limit as usize)
            .map(|row| stored_from_row(row).map(|s| s.baseline))
            .collect::<Result<_, _>>()?;
        tx.commit().await?;
        Ok(FileBaselinePageV1 { items, next_offset })
    }

    pub async fn workspace_get_file_baseline(
        &self,
        project_id: Uuid,
        baseline_id: Uuid,
    ) -> Result<Option<StoredFileBaseline>, StoreError> {
        let mut tx = self.pool.begin().await?;
        ensure_project(&mut tx, project_id).await?;
        let result = load(&mut tx, project_id, baseline_id).await?;
        tx.commit().await?;
        Ok(result)
    }
}

async fn replay(
    connection: &mut SqliteConnection,
    request_id: Uuid,
    digest: &str,
) -> Result<Option<FileBaselineV1>, StoreError> {
    if !valid_hash(digest) {
        return Err(invalid("invalid_selection"));
    }
    let row = sqlx::query("SELECT operation,request_sha256,result_json FROM workspace_request_ledger WHERE request_id=?")
        .bind(request_id.to_string()).fetch_optional(&mut *connection).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get::<&str, _>("operation")? != OPERATION
        || row.try_get::<&str, _>("request_sha256")? != digest
    {
        return Err(invalid("request_conflict"));
    }
    let receipt: BaselineReceipt =
        serde_json::from_str(row.try_get("result_json")?).map_err(|_| invalid("storage_failed"))?;
    let stored = load(connection, receipt.project_id, receipt.baseline_id)
        .await?
        .ok_or_else(|| invalid("storage_failed"))?;
    Ok(Some(stored.baseline))
}

async fn load(
    connection: &mut SqliteConnection,
    project_id: Uuid,
    id: Uuid,
) -> Result<Option<StoredFileBaseline>, StoreError> {
    let row = sqlx::query("SELECT * FROM exploration_baselines_v4 WHERE id=?")
        .bind(id.to_string())
        .fetch_optional(connection)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get::<String, _>("project_id")? != project_id.to_string() {
        return Err(invalid("ownership_mismatch"));
    }
    Ok(Some(stored_from_row(&row)?))
}

async fn ensure_project(
    connection: &mut SqliteConnection,
    project: Uuid,
) -> Result<(), StoreError> {
    let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?)")
        .bind(project.to_string())
        .fetch_one(connection)
        .await?;
    if exists == 0 {
        return Err(invalid("not_found"));
    }
    Ok(())
}

async fn ensure_branch(
    connection: &mut SqliteConnection,
    project: Uuid,
    branch: Option<Uuid>,
) -> Result<(), StoreError> {
    if let Some(branch) = branch {
        let exists: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM conversation_branches_v4 b JOIN conversation_records c ON c.frame_id=b.branch_conversation_id WHERE b.branch_conversation_id=? AND b.project_id=? AND c.project_id=?)")
            .bind(branch.to_string()).bind(project.to_string()).bind(project.to_string()).fetch_one(connection).await?;
        if exists == 0 {
            return Err(invalid("ownership_mismatch"));
        }
    }
    Ok(())
}

fn invalid(code: &str) -> StoreError {
    StoreError::InvalidInput(format!("file_baseline_{code}"))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn safe_public(value: &str) -> bool {
    !value.chars().any(char::is_control) && redact_secrets(value, &[] as &[&str]) == value
}
fn utc_timestamp(value: &str) -> Result<i64, StoreError> {
    let timestamp =
        DateTime::parse_from_rfc3339(value).map_err(|_| invalid("invalid_selection"))?;
    if timestamp.offset().local_minus_utc() != 0 {
        return Err(invalid("invalid_selection"));
    }
    Ok(timestamp.timestamp_millis())
}

fn parse_manifest(json: &str, digest: &str) -> Result<FileBaselineManifestV1, StoreError> {
    if json.len() > 128 * 1024
        || !valid_hash(digest)
        || hex::encode(Sha256::digest(json.as_bytes())) != digest
    {
        return Err(invalid("manifest_changed"));
    }
    let manifest: FileBaselineManifestV1 =
        serde_json::from_str(json).map_err(|_| invalid("invalid_selection"))?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &FileBaselineManifestV1) -> Result<(), StoreError> {
    if manifest.schema_version != 1
        || !valid_hash(&manifest.local_root_binding_sha256)
        || manifest.title.trim() != manifest.title
        || manifest.title.is_empty()
        || manifest.title.chars().count() > 80
        || manifest.title.len() > 320
        || !safe_public(&manifest.title)
        || !(1..=32).contains(&manifest.entries.len())
        || manifest.captured_text_bytes > MAX_CAPTURE_BYTES
    {
        return Err(invalid("invalid_selection"));
    }
    utc_timestamp(&manifest.created_at)?;
    let mut captured = 0u64;
    let mut paths = BTreeSet::new();
    let mut previous = None;
    for item in &manifest.entries {
        let entry = &item.entry;
        if entry.backend_id.is_empty()
            || entry.backend_id.len() > 128
            || !safe_public(&entry.backend_id)
        {
            return Err(invalid("invalid_selection"));
        }
        utc_timestamp(&entry.captured_at)?;
        if let Some(path) = &entry.relative_path {
            // Syntactic invariant only; host owns filesystem/reparse validation.
            if path.is_empty()
                || path.len() > 1024
                || !safe_public(path)
                || path.contains(['\\', ':'])
                || path
                    .split('/')
                    .any(|s| s.is_empty() || s == "." || s == "..")
                || !paths.insert((&entry.backend_id, path))
            {
                return Err(invalid("invalid_selection"));
            }
            let key = (&entry.backend_id, path);
            if previous.is_some_and(|old| old >= key) {
                return Err(invalid("invalid_selection"));
            }
            previous = Some(key);
        }
        if entry.hash.as_ref().is_some_and(|h| !valid_hash(&h.sha256)) {
            return Err(invalid("invalid_selection"));
        }
        if entry.content_mode == WorkspaceContentModeV1::TextSnapshot {
            let Some(path) = &entry.relative_path else {
                return Err(invalid("invalid_selection"));
            };
            let extension = path
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            let Some(size) = entry.size_bytes else {
                return Err(invalid("invalid_selection"));
            };
            let Some(hash) = &entry.hash else {
                return Err(invalid("invalid_selection"));
            };
            if !["py", "r", "rmd", "md", "txt", "json", "csv", "tsv"].contains(&extension.as_str())
                || size > 512 * 1024
                || hash.scope != FileReferenceHashScopeV1::FullFile
                || item.blob_sha256.as_ref() != Some(&hash.sha256)
                || entry.omission.is_some()
            {
                return Err(invalid("invalid_selection"));
            }
            captured = captured
                .checked_add(size)
                .ok_or_else(|| invalid("invalid_selection"))?;
        } else if item.blob_sha256.is_some() || entry.omission.is_none() {
            return Err(invalid("invalid_selection"));
        }
    }
    if captured != manifest.captured_text_bytes {
        return Err(invalid("invalid_selection"));
    }
    Ok(())
}

fn public_baseline(manifest: &FileBaselineManifestV1, hash: &str) -> FileBaselineV1 {
    FileBaselineV1 {
        schema_version: manifest.schema_version,
        id: manifest.id,
        project_id: manifest.project_id,
        branch_conversation_id: manifest.branch_conversation_id,
        title: manifest.title.clone(),
        entries: manifest.entries.iter().map(|e| e.entry.clone()).collect(),
        manifest_sha256: hash.into(),
        captured_text_bytes: manifest.captured_text_bytes,
        created_at: manifest.created_at.clone(),
    }
}

fn stored_from_row(row: &SqliteRow) -> Result<StoredFileBaseline, StoreError> {
    Uuid::parse_str(row.try_get("request_id")?).map_err(|_| invalid("storage_failed"))?;
    let digest: &str = row.try_get("manifest_sha256")?;
    let manifest = parse_manifest(row.try_get("manifest_json")?, digest)?;
    let branch: Option<String> = row.try_get("branch_conversation_id")?;
    let branch = branch
        .map(|id| Uuid::parse_str(&id).map_err(|_| invalid("storage_failed")))
        .transpose()?;
    if manifest.id.to_string() != row.try_get::<String, _>("id")?
        || manifest.project_id.to_string() != row.try_get::<String, _>("project_id")?
        || row.try_get::<i64, _>("captured_text_bytes")? != manifest.captured_text_bytes as i64
        || row.try_get::<i64, _>("created_at")? != utc_timestamp(&manifest.created_at)?
        || branch.is_some() && branch != manifest.branch_conversation_id
        || !valid_hash(row.try_get("request_sha256")?)
    {
        return Err(invalid("storage_failed"));
    }
    let mut baseline = public_baseline(&manifest, digest);
    baseline.branch_conversation_id = branch;
    Ok(StoredFileBaseline {
        baseline,
        local_root_binding_sha256: manifest.local_root_binding_sha256,
        blob_sha256_by_entry: manifest
            .entries
            .into_iter()
            .map(|e| e.blob_sha256)
            .collect(),
    })
}

pub(crate) async fn validate_file_baselines(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    let rows = sqlx::query("SELECT * FROM exploration_baselines_v4")
        .fetch_all(&mut **tx)
        .await?;
    for row in rows {
        let stored = stored_from_row(&row)
            .map_err(|_| StoreError::Migration("invalid file baseline metadata".into()))?;
        ensure_branch(
            tx,
            stored.baseline.project_id,
            stored.baseline.branch_conversation_id,
        )
        .await
        .map_err(|_| StoreError::Migration("file baseline branch ownership mismatch".into()))?;
    }
    let exceeded:i64=sqlx::query_scalar("SELECT COUNT(*) FROM (SELECT project_id FROM exploration_baselines_v4 GROUP BY project_id HAVING COUNT(*)>64 OR SUM(captured_text_bytes)>268435456)").fetch_one(&mut **tx).await?;
    if exceeded != 0 {
        return Err(StoreError::Migration("file baseline quota exceeded".into()));
    }
    Ok(())
}

impl Store {
    /// Read only recorded metadata with explicit V4 producer-backend provenance.
    /// At most three bounded candidates; malformed/oversized or ambiguous matches
    /// are unknown. No runtime identity is returned, and this never stats SSH.
    pub async fn workspace_file_recorded_reference(
        &self,
        project_id: Uuid,
        backend_id: &str,
        relative_path: &str,
    ) -> Result<(Option<u64>, Option<String>), StoreError> {
        let mut tx = self.pool.begin().await?;
        let bound:i64=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_omicsops p JOIN connections c ON c.id=p.connection_id WHERE p.project_id=? AND 'ssh:'||c.id=? AND p.remote_root IS NOT NULL AND length(trim(p.remote_root))>0)").bind(project_id.to_string()).bind(backend_id).fetch_one(&mut *tx).await?;
        if bound == 0 {
            return Ok((None, None));
        }
        let rows=sqlx::query("SELECT CASE WHEN length(a.id)=36 THEN a.id ELSE NULL END AS id,a.project_id,a.valid AS artifact_valid,CASE WHEN n.status='succeeded' THEN 'succeeded' ELSE NULL END AS analysis_status,CASE WHEN length(a.producer_analysis_id)=36 THEN a.producer_analysis_id ELSE NULL END AS producer_analysis_id,CASE WHEN length(n.id)=36 THEN n.id ELSE NULL END AS analysis_id,n.project_id AS analysis_project, CASE WHEN length(CAST(a.value_json AS BLOB))<=65536 THEN CASE WHEN json_valid(a.value_json) THEN a.value_json ELSE NULL END ELSE NULL END AS artifact_json, CASE WHEN length(CAST(n.value_json AS BLOB))<=65536 THEN CASE WHEN json_valid(n.value_json) THEN n.value_json ELSE NULL END ELSE NULL END AS analysis_json FROM scientific_artifacts_v4 a LEFT JOIN scientific_analyses_v4 n ON n.id=a.producer_analysis_id AND n.project_id=a.project_id WHERE a.project_id=? AND (CASE WHEN length(CAST(a.value_json AS BLOB))<=65536 THEN CASE WHEN json_valid(a.value_json) THEN json_extract(a.value_json,'$.relative_path') ELSE NULL END ELSE NULL END=? OR CASE WHEN length(CAST(a.value_json AS BLOB))<=65536 THEN json_valid(a.value_json) ELSE 0 END=0) ORDER BY a.id LIMIT 3")
            .bind(project_id.to_string()).bind(relative_path).fetch_all(&mut *tx).await?;
        if rows.len() >= 3 {
            return Ok((None, None));
        }
        let mut result = None;
        for row in rows {
            for column in ["id", "producer_analysis_id", "analysis_id"] {
                let Some(id): Option<&str> = row.try_get(column)? else {
                    return Ok((None, None));
                };
                if !Uuid::parse_str(id)
                    .ok()
                    .is_some_and(|uuid| uuid.to_string() == id)
                {
                    return Ok((None, None));
                }
            }
            let Some(artifact): Option<&str> = row.try_get("artifact_json")? else {
                return Ok((None, None));
            };
            let Some(analysis): Option<&str> = row.try_get("analysis_json")? else {
                return Ok((None, None));
            };
            let artifact: ArtifactRecordV4 = match serde_json::from_str(artifact) {
                Ok(value) => value,
                Err(_) => return Ok((None, None)),
            };
            let analysis: AnalysisRecordV4 = match serde_json::from_str(analysis) {
                Ok(value) => value,
                Err(_) => return Ok((None, None)),
            };
            if artifact.schema_version != 4
                || analysis.schema_version != 4
                || !artifact.valid
                || analysis.status != AnalysisStatusV4::Succeeded
                || row.try_get::<i64, _>("artifact_valid")? != 1
                || row.try_get::<Option<&str>, _>("analysis_status")? != Some("succeeded")
            {
                return Ok((None, None));
            }
            if artifact.id.to_string() != row.try_get::<&str, _>("id")?
                || artifact.project_id != project_id
                || artifact.producer_analysis_id.to_string()
                    != row.try_get::<&str, _>("producer_analysis_id")?
                || analysis.id.to_string() != row.try_get::<&str, _>("analysis_id")?
                || artifact.producer_analysis_id != analysis.id
                || analysis.project_id != project_id
                || row.try_get::<Option<&str>, _>("analysis_project")?
                    != Some(project_id.to_string().as_str())
            {
                return Ok((None, None));
            }
            if artifact.relative_path != relative_path || analysis.runtime.backend_id != backend_id
            {
                continue;
            }
            let value = (
                Some(artifact.size_bytes),
                if valid_hash(&artifact.sha256) {
                    Some(artifact.sha256)
                } else {
                    None
                },
            );
            if result.as_ref().is_some_and(|old| old != &value) {
                return Ok((None, None));
            }
            result = Some(value);
        }
        Ok(result.unwrap_or((None, None)))
    }
}
