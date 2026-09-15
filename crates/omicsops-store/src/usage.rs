use sqlx::{QueryBuilder, Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{Store, StoreError};

const RELEVANT_EVENT_SQL: &str = "(json_valid(value_json)=0 OR json_extract(value_json,'$.event.kind') IN ('model_request_started','model_usage_observed','tool_dispatch_started','tool_finished','tool_dispatch_uncertain'))";
const MAX_EVENTS_PER_RUN: i64 = 20_000;
const MAX_EVENTS_PER_PAGE: i64 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSnapshotBoundary {
    pub rowid: i64,
    pub event_hash: String,
    pub event_count: i64,
    pub snapshot_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEventRow {
    pub rowid: i64,
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub sequence: u64,
    pub occurred_at_ms: i64,
    pub value_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEventRun {
    pub run_id: Uuid,
    pub events: Vec<UsageEventRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEventPage {
    pub runs: Vec<UsageEventRun>,
    pub last_run_id: Option<Uuid>,
    pub has_more: bool,
    pub omitted_runs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageConversationEventSet {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub label: String,
    pub latest_activity_ms: i64,
    pub runs: Vec<UsageEventRun>,
    pub incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageConversationEventPage {
    pub items: Vec<UsageConversationEventSet>,
    pub last_activity_ms: Option<i64>,
    pub last_conversation_id: Option<Uuid>,
    pub has_more: bool,
}

impl Store {
    pub async fn usage_snapshot_boundary(&self) -> Result<UsageSnapshotBoundary, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT rowid,event_hash,occurred_at FROM agent_events_v4 ORDER BY rowid DESC LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let (rowid, event_hash, snapshot_at_ms) = match row {
            Some(row) => (row.get(0), row.get(1), row.get(2)),
            None => (0, String::new(), chrono::Utc::now().timestamp_millis()),
        };
        let event_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_events_v4 WHERE rowid<=?1")
                .bind(rowid)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(UsageSnapshotBoundary {
            rowid,
            event_hash,
            event_count,
            snapshot_at_ms,
        })
    }

    pub async fn usage_event_page(
        &self,
        project_id: Option<Uuid>,
        from_ms: Option<i64>,
        until_ms: Option<i64>,
        after_run_id: Option<Uuid>,
        boundary: &UsageSnapshotBoundary,
        limit: u32,
    ) -> Result<UsageEventPage, StoreError> {
        let limit = limit.clamp(1, 50);
        let mut tx = self.pool.begin().await?;
        validate_boundary(&mut tx, boundary).await?;
        let candidate_sql = format!(
            "SELECT DISTINCT run_id FROM agent_events_v4
             WHERE rowid<=?1 AND (?2 IS NULL OR project_id=?2)
               AND (?3 IS NULL OR occurred_at>=?3) AND (?4 IS NULL OR occurred_at<?4)
               AND (?5 IS NULL OR run_id>?5) AND {RELEVANT_EVENT_SQL}
             ORDER BY run_id ASC LIMIT ?6"
        );
        let project = project_id.map(|value| value.to_string());
        let after = after_run_id.map(|value| value.to_string());
        let candidate_ids = sqlx::query_scalar::<_, String>(&candidate_sql)
            .bind(boundary.rowid)
            .bind(project)
            .bind(from_ms)
            .bind(until_ms)
            .bind(after)
            .bind(i64::from(limit) + 1)
            .fetch_all(&mut *tx)
            .await?;
        let mut has_more = candidate_ids.len() > limit as usize;
        let mut runs = Vec::new();
        let mut omitted_runs = 0u32;
        let mut event_total = 0i64;
        let mut last_run_id = after_run_id;
        for run_id in candidate_ids.into_iter().take(limit as usize) {
            let count_sql = format!(
                "SELECT COUNT(*) FROM agent_events_v4 WHERE rowid<=?1 AND run_id=?2 AND {RELEVANT_EVENT_SQL}"
            );
            let count = sqlx::query_scalar::<_, i64>(&count_sql)
                .bind(boundary.rowid)
                .bind(&run_id)
                .fetch_one(&mut *tx)
                .await?;
            let parsed_run_id = Uuid::parse_str(&run_id)
                .map_err(|_| StoreError::InvalidInput("invalid usage run id".into()))?;
            if count > MAX_EVENTS_PER_RUN {
                omitted_runs = omitted_runs.saturating_add(1);
                last_run_id = Some(parsed_run_id);
                continue;
            }
            if event_total + count > MAX_EVENTS_PER_PAGE {
                has_more = true;
                break;
            }
            let events_sql = format!(
                "SELECT rowid,run_id,project_id,conversation_id,sequence,occurred_at,value_json
                 FROM agent_events_v4 WHERE rowid<=?1 AND run_id=?2 AND {RELEVANT_EVENT_SQL}
                 ORDER BY sequence ASC"
            );
            let rows = sqlx::query(&events_sql)
                .bind(boundary.rowid)
                .bind(&run_id)
                .fetch_all(&mut *tx)
                .await?;
            let events = rows
                .into_iter()
                .map(parse_event_row)
                .collect::<Result<Vec<_>, _>>()?;
            event_total += count;
            last_run_id = Some(parsed_run_id);
            runs.push(UsageEventRun {
                run_id: parsed_run_id,
                events,
            });
        }
        tx.commit().await?;
        Ok(UsageEventPage {
            runs,
            last_run_id,
            has_more,
            omitted_runs,
        })
    }

    pub async fn usage_conversation_event_page(
        &self,
        project_id: Option<Uuid>,
        from_ms: Option<i64>,
        until_ms: Option<i64>,
        after: Option<(i64, Uuid)>,
        boundary: &UsageSnapshotBoundary,
        limit: u32,
    ) -> Result<UsageConversationEventPage, StoreError> {
        let limit = limit.clamp(1, 20);
        let mut tx = self.pool.begin().await?;
        validate_boundary(&mut tx, boundary).await?;
        let project = project_id.map(|value| value.to_string());
        let after_activity = after.map(|value| value.0);
        let after_id = after.map(|value| value.1.to_string());
        let sql = format!(
            "WITH candidates AS (
               SELECT project_id,conversation_id,MAX(occurred_at) AS latest_activity
               FROM agent_events_v4 WHERE rowid<=?1 AND (?2 IS NULL OR project_id=?2)
                 AND (?3 IS NULL OR occurred_at>=?3) AND (?4 IS NULL OR occurred_at<?4)
                 AND {RELEVANT_EVENT_SQL}
               GROUP BY project_id,conversation_id
             )
             SELECT candidates.project_id,candidates.conversation_id,
                    COALESCE(NULLIF(conversation_records.title,''),candidates.conversation_id),latest_activity
             FROM candidates LEFT JOIN conversation_records ON conversation_records.frame_id=candidates.conversation_id
             WHERE (?5 IS NULL OR latest_activity<?5 OR (latest_activity=?5 AND candidates.conversation_id>?6))
             ORDER BY latest_activity DESC,candidates.conversation_id ASC LIMIT ?7"
        );
        let candidates = sqlx::query(&sql)
            .bind(boundary.rowid)
            .bind(project)
            .bind(from_ms)
            .bind(until_ms)
            .bind(after_activity)
            .bind(after_id)
            .bind(i64::from(limit) + 1)
            .fetch_all(&mut *tx)
            .await?;
        let has_more = candidates.len() > limit as usize;
        let mut items = Vec::new();
        for candidate in candidates.into_iter().take(limit as usize) {
            let project_text: String = candidate.get(0);
            let conversation_text: String = candidate.get(1);
            let label: String = candidate.get(2);
            let latest_activity_ms: i64 = candidate.get(3);
            let counts_sql = format!(
                "SELECT run_id,COUNT(*)
                 FROM agent_events_v4 WHERE rowid<=?1 AND project_id=?2 AND conversation_id=?3
                   AND {RELEVANT_EVENT_SQL}
                 GROUP BY run_id ORDER BY run_id ASC LIMIT ?4"
            );
            let run_counts = sqlx::query(&counts_sql)
                .bind(boundary.rowid)
                .bind(&project_text)
                .bind(&conversation_text)
                .bind(MAX_EVENTS_PER_PAGE + 1)
                .fetch_all(&mut *tx)
                .await?;
            let mut incomplete = run_counts.len() > MAX_EVENTS_PER_PAGE as usize;
            let mut selected_run_ids = Vec::new();
            let mut total_events = 0_i64;
            for row in run_counts {
                let run_id_text: String = row.get(0);
                let event_count: i64 = row.get(1);
                if event_count > MAX_EVENTS_PER_RUN {
                    incomplete = true;
                    continue;
                }
                if total_events.saturating_add(event_count) > MAX_EVENTS_PER_PAGE {
                    incomplete = true;
                    break;
                }
                total_events += event_count;
                selected_run_ids.push(run_id_text);
            }
            let mut by_run = std::collections::BTreeMap::<Uuid, Vec<UsageEventRow>>::new();
            for chunk in selected_run_ids.chunks(400) {
                let mut events = QueryBuilder::<Sqlite>::new(
                    "SELECT rowid,run_id,project_id,conversation_id,sequence,occurred_at,value_json \
                     FROM agent_events_v4 WHERE rowid<=",
                );
                events
                    .push_bind(boundary.rowid)
                    .push(" AND project_id=")
                    .push_bind(&project_text)
                    .push(" AND conversation_id=")
                    .push_bind(&conversation_text)
                    .push(" AND run_id IN (");
                let mut separated = events.separated(",");
                for run_id in chunk {
                    separated.push_bind(run_id);
                }
                separated
                    .push_unseparated(") AND ")
                    .push_unseparated(RELEVANT_EVENT_SQL)
                    .push_unseparated(" ORDER BY run_id ASC,sequence ASC");
                for row in events.build().fetch_all(&mut *tx).await? {
                    let event = parse_event_row(row)?;
                    by_run.entry(event.run_id).or_default().push(event);
                }
            }
            let runs = by_run
                .into_iter()
                .map(|(run_id, events)| UsageEventRun { run_id, events })
                .collect();
            items.push(UsageConversationEventSet {
                project_id: Uuid::parse_str(&project_text)
                    .map_err(|_| StoreError::InvalidInput("invalid usage project id".into()))?,
                conversation_id: Uuid::parse_str(&conversation_text).map_err(|_| {
                    StoreError::InvalidInput("invalid usage conversation id".into())
                })?,
                label,
                latest_activity_ms,
                runs,
                incomplete,
            });
        }
        let (last_activity_ms, last_conversation_id) = items.last().map_or((None, None), |item| {
            (Some(item.latest_activity_ms), Some(item.conversation_id))
        });
        tx.commit().await?;
        Ok(UsageConversationEventPage {
            items,
            last_activity_ms,
            last_conversation_id,
            has_more,
        })
    }
}

async fn validate_boundary(
    tx: &mut Transaction<'_, Sqlite>,
    boundary: &UsageSnapshotBoundary,
) -> Result<(), StoreError> {
    let count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_events_v4 WHERE rowid<=?1")
            .bind(boundary.rowid)
            .fetch_one(&mut **tx)
            .await?;
    let hash = if boundary.rowid == 0 {
        Some(String::new())
    } else {
        sqlx::query_scalar::<_, String>("SELECT event_hash FROM agent_events_v4 WHERE rowid=?1")
            .bind(boundary.rowid)
            .fetch_optional(&mut **tx)
            .await?
    };
    if count != boundary.event_count || hash.as_deref() != Some(boundary.event_hash.as_str()) {
        return Err(StoreError::InvalidInput(
            "usage history changed; refresh required".into(),
        ));
    }
    Ok(())
}

fn parse_event_row(row: sqlx::sqlite::SqliteRow) -> Result<UsageEventRow, StoreError> {
    let run_id = Uuid::parse_str(row.get::<String, _>(1).as_str())
        .map_err(|_| StoreError::InvalidInput("invalid usage run id".into()))?;
    let project_id = Uuid::parse_str(row.get::<String, _>(2).as_str())
        .map_err(|_| StoreError::InvalidInput("invalid usage project id".into()))?;
    let conversation_id = Uuid::parse_str(row.get::<String, _>(3).as_str())
        .map_err(|_| StoreError::InvalidInput("invalid usage conversation id".into()))?;
    let sequence = u64::try_from(row.get::<i64, _>(4))
        .map_err(|_| StoreError::InvalidInput("invalid usage sequence".into()))?;
    Ok(UsageEventRow {
        rowid: row.get(0),
        run_id,
        project_id,
        conversation_id,
        sequence,
        occurred_at_ms: row.get(5),
        value_json: row.get(6),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seed_scope(store: &Store, project_id: Uuid, conversation_id: Uuid, run_id: Uuid) {
        sqlx::query("INSERT INTO projects(id,name,workspace_dir,created_at,updated_at) VALUES (?1,'P','E:/P',0,0)")
            .bind(project_id.to_string()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO frames(id,root_frame_id,agent_name,status,project_id,created_at,updated_at) VALUES (?1,?1,'Agent','active',?2,0,0)")
            .bind(conversation_id.to_string()).bind(project_id.to_string()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO conversation_records(frame_id,project_id,title,status,created_at,updated_at) VALUES (?1,?2,'Session','active',0,0)")
            .bind(conversation_id.to_string()).bind(project_id.to_string()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,'running','{}')")
            .bind(run_id.to_string()).bind(project_id.to_string()).bind(conversation_id.to_string()).execute(store.pool()).await.unwrap();
    }

    async fn add_event(
        store: &Store,
        project_id: Uuid,
        conversation_id: Uuid,
        run_id: Uuid,
        sequence: i64,
        at: i64,
    ) {
        sqlx::query("INSERT INTO agent_events_v4(run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json,occurred_at) VALUES (?1,?2,?3,?4,'',?5,?6,?7)")
            .bind(run_id.to_string()).bind(project_id.to_string()).bind(conversation_id.to_string()).bind(sequence)
            .bind(format!("hash-{run_id}-{sequence}"))
            .bind("{\"event\":{\"kind\":\"model_usage_observed\"}}")
            .bind(at).execute(store.pool()).await.unwrap();
    }

    #[tokio::test]
    async fn isolates_projects_and_keeps_complete_runs_at_the_page_boundary() {
        let store = Store::open_in_memory().await.unwrap();
        let project_a = Uuid::from_u128(1);
        let project_b = Uuid::from_u128(2);
        let conversation_a = Uuid::from_u128(11);
        let conversation_b = Uuid::from_u128(12);
        let run_a = Uuid::from_u128(21);
        let run_b = Uuid::from_u128(22);
        seed_scope(&store, project_a, conversation_a, run_a).await;
        seed_scope(&store, project_b, conversation_b, run_b).await;
        add_event(&store, project_a, conversation_a, run_a, 1, 100).await;
        add_event(&store, project_a, conversation_a, run_a, 2, 101).await;
        add_event(&store, project_b, conversation_b, run_b, 1, 102).await;
        let boundary = store.usage_snapshot_boundary().await.unwrap();

        let page = store
            .usage_event_page(Some(project_a), None, None, None, &boundary, 1)
            .await
            .unwrap();
        assert_eq!(page.runs.len(), 1);
        assert_eq!(page.runs[0].run_id, run_a);
        assert_eq!(page.runs[0].events.len(), 2);
        assert!(!page.has_more);
    }

    #[tokio::test]
    async fn ignores_events_appended_after_the_snapshot() {
        let store = Store::open_in_memory().await.unwrap();
        let project_id = Uuid::from_u128(31);
        let conversation_id = Uuid::from_u128(32);
        let run_id = Uuid::from_u128(33);
        seed_scope(&store, project_id, conversation_id, run_id).await;
        add_event(&store, project_id, conversation_id, run_id, 1, 100).await;
        let boundary = store.usage_snapshot_boundary().await.unwrap();
        add_event(&store, project_id, conversation_id, run_id, 2, 200).await;

        let page = store
            .usage_event_page(None, None, None, None, &boundary, 50)
            .await
            .unwrap();
        assert_eq!(page.runs[0].events.len(), 1);
    }

    #[tokio::test]
    async fn rejects_a_cursor_snapshot_when_a_non_watermark_event_was_deleted() {
        let store = Store::open_in_memory().await.unwrap();
        let project_id = Uuid::from_u128(41);
        let conversation_id = Uuid::from_u128(42);
        let run_id = Uuid::from_u128(43);
        seed_scope(&store, project_id, conversation_id, run_id).await;
        add_event(&store, project_id, conversation_id, run_id, 1, 100).await;
        add_event(&store, project_id, conversation_id, run_id, 2, 200).await;
        let boundary = store.usage_snapshot_boundary().await.unwrap();
        sqlx::query("DELETE FROM agent_events_v4 WHERE run_id=?1 AND sequence=1")
            .bind(run_id.to_string())
            .execute(store.pool())
            .await
            .unwrap();

        let error = store
            .usage_event_page(None, None, None, None, &boundary, 50)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("history changed"));
    }

    #[tokio::test]
    async fn omits_an_oversized_conversation_run_before_loading_event_values() {
        let store = Store::open_in_memory().await.unwrap();
        let project_id = Uuid::from_u128(51);
        let conversation_id = Uuid::from_u128(52);
        let oversized_run = Uuid::from_u128(53);
        let included_run = Uuid::from_u128(54);
        seed_scope(&store, project_id, conversation_id, oversized_run).await;
        sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,'running','{}')")
            .bind(included_run.to_string()).bind(project_id.to_string()).bind(conversation_id.to_string())
            .execute(store.pool()).await.unwrap();
        sqlx::query(
            "WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<=20000)
             INSERT INTO agent_events_v4(run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json,occurred_at)
             SELECT ?1,?2,?3,n,'','oversized-'||n,'not-json',100+n FROM seq",
        )
        .bind(oversized_run.to_string())
        .bind(project_id.to_string())
        .bind(conversation_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
        add_event(&store, project_id, conversation_id, included_run, 1, 30000).await;
        let boundary = store.usage_snapshot_boundary().await.unwrap();

        let page = store
            .usage_conversation_event_page(Some(project_id), None, None, None, &boundary, 20)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.items[0].incomplete);
        assert_eq!(page.items[0].runs.len(), 1);
        assert_eq!(page.items[0].runs[0].run_id, included_run);
        assert_eq!(page.items[0].runs[0].events.len(), 1);
    }
}
