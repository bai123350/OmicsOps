use omicsops_core::workspace::SkillPackage;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use super::{Store, StoreError};

impl Store {
    /// Return every durable run that can still require live host material.
    /// Unknown states and needs-attention rows are intentionally blocking.
    pub async fn skill_removal_blocking_run_ids(&self) -> Result<Vec<Uuid>, StoreError> {
        let rows = sqlx::query(
            "SELECT DISTINCT r.run_id
             FROM agent_runs_v4 r
             LEFT JOIN runtime_jobs_v4 j ON j.run_id=r.run_id
             LEFT JOIN runtime_job_results_v4 result ON result.job_id=j.job_id
             WHERE r.status NOT IN ('completed','failed','cancelled')
                OR (j.job_id IS NOT NULL AND (
                    COALESCE(json_extract(j.value_json,'$.state'),'unknown') NOT IN ('succeeded','failed')
                    OR result.job_id IS NULL
                ))
             ORDER BY r.run_id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let value = row.try_get::<String, _>(0)?;
                Uuid::parse_str(&value).map_err(|_| {
                    StoreError::InvalidInput("persisted run has an invalid identifier".into())
                })
            })
            .collect()
    }

    /// Remove one exact catalog row while atomically preserving its ownership
    /// tombstone and, when present, the recoverable file-operation journal.
    pub async fn commit_skill_library_removal<R: Serialize, O: Serialize>(
        &self,
        skill_id: Uuid,
        expected_package_sha256: &str,
        receipt_kind: &str,
        receipt: &R,
        operation: Option<(&str, &str, &O)>,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT value_json FROM skill_packages WHERE id=?1")
            .bind(skill_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| StoreError::InvalidInput("skill package was not found".into()))?;
        let skill: SkillPackage = serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?;
        if skill.id != skill_id || skill.sha256 != expected_package_sha256 {
            return Err(StoreError::InvalidInput(
                "skill package changed before removal".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO app_objects(kind,id,value_json) VALUES (?1,?2,?3)
             ON CONFLICT(kind,id) DO UPDATE SET value_json=excluded.value_json",
        )
        .bind(receipt_kind)
        .bind(skill_id.to_string())
        .bind(serde_json::to_string(receipt)?)
        .execute(&mut *tx)
        .await?;
        if let Some((kind, id, value)) = operation {
            sqlx::query(
                "INSERT INTO app_objects(kind,id,value_json) VALUES (?1,?2,?3)
                 ON CONFLICT(kind,id) DO UPDATE SET value_json=excluded.value_json",
            )
            .bind(kind)
            .bind(id)
            .bind(serde_json::to_string(value)?)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("DELETE FROM skill_packages WHERE id=?1")
            .bind(skill_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn seed_run(store: &Store, status: &str) -> Uuid {
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        sqlx::query("INSERT INTO projects(id,name,workspace_dir,created_at,updated_at) VALUES (?1,'P','E:/P',0,0)")
            .bind(project_id.to_string()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO frames(id,root_frame_id,agent_name,status,project_id,created_at,updated_at) VALUES (?1,?1,'Agent','active',?2,0,0)")
            .bind(conversation_id.to_string()).bind(project_id.to_string()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO conversation_records(frame_id,project_id,title,status,created_at,updated_at) VALUES (?1,?2,'Session','active',0,0)")
            .bind(conversation_id.to_string()).bind(project_id.to_string()).execute(store.pool()).await.unwrap();
        sqlx::query("INSERT INTO agent_runs_v4(run_id,project_id,conversation_id,status,value_json) VALUES (?1,?2,?3,?4,'{}')")
            .bind(run_id.to_string()).bind(project_id.to_string()).bind(conversation_id.to_string()).bind(status).execute(store.pool()).await.unwrap();
        run_id
    }

    #[tokio::test]
    async fn blocks_nonterminal_unknown_and_unreconciled_runtime_runs() {
        let store = Store::open_in_memory().await.unwrap();
        let running = seed_run(&store, "running").await;
        let needs_attention = seed_run(&store, "needs_attention").await;
        let unknown = seed_run(&store, "future_state").await;
        let completed = seed_run(&store, "completed").await;
        let runtime = seed_run(&store, "completed").await;
        sqlx::query("INSERT INTO runtime_jobs_v4(job_id,run_id,call_id,value_json) VALUES ('job',?1,'call','{\"state\":\"reserved\"}')")
            .bind(runtime.to_string()).execute(store.pool()).await.unwrap();

        let blocked = store.skill_removal_blocking_run_ids().await.unwrap();
        assert!(blocked.contains(&running));
        assert!(blocked.contains(&needs_attention));
        assert!(blocked.contains(&unknown));
        assert!(blocked.contains(&runtime));
        assert!(!blocked.contains(&completed));

        sqlx::query(
            "UPDATE runtime_jobs_v4 SET value_json='{\"state\":\"succeeded\"}' WHERE job_id='job'",
        )
        .execute(store.pool())
        .await
        .unwrap();
        sqlx::query("INSERT INTO runtime_job_results_v4(job_id,result_json) VALUES ('job','{}')")
            .execute(store.pool())
            .await
            .unwrap();
        assert!(
            !store
                .skill_removal_blocking_run_ids()
                .await
                .unwrap()
                .contains(&runtime)
        );
    }

    #[tokio::test]
    async fn removes_only_the_expected_catalog_row_with_tombstones() {
        let store = Store::open_in_memory().await.unwrap();
        let skill = SkillPackage {
            id: Uuid::new_v4(),
            name: "qc".into(),
            version: "1".into(),
            source_path: "E:/skills/qc/hash".into(),
            sha256: "a".repeat(64),
            enabled: false,
            capabilities: vec![],
            category: None,
        };
        store.save_skill_package(&skill).await.unwrap();
        let wrong = store
            .commit_skill_library_removal(
                skill.id,
                &"b".repeat(64),
                "receipt",
                &json!({"phase":"removed"}),
                None::<(&str, &str, &serde_json::Value)>,
            )
            .await;
        assert!(wrong.is_err());
        assert!(
            store
                .list_skill_packages()
                .await
                .unwrap()
                .iter()
                .any(|item| item.id == skill.id)
        );

        store
            .commit_skill_library_removal(
                skill.id,
                &skill.sha256,
                "receipt",
                &json!({"phase":"removed"}),
                Some(("operation", "op", &json!({"phase":"cleaning"}))),
            )
            .await
            .unwrap();
        assert!(store.list_skill_packages().await.unwrap().is_empty());
        assert_eq!(
            store
                .get_json::<serde_json::Value>("receipt", &skill.id.to_string())
                .await
                .unwrap()
                .unwrap()["phase"],
            "removed"
        );
        assert_eq!(
            store
                .get_json::<serde_json::Value>("operation", "op")
                .await
                .unwrap()
                .unwrap()["phase"],
            "cleaning"
        );
    }
}
