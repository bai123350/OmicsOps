use chrono::Utc;
use omicsops_protocol::AgentEventV4;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use super::{Store, StoreError};

const NOTIFICATION_RECEIPT_KIND: &str = "notification_receipt_v1";

#[derive(Serialize)]
struct ClaimedNotificationReceipt<'a> {
    event_hash: &'a str,
    state: &'static str,
    claimed_at: String,
}

impl Store {
    /// Read one exact persisted Agent event candidate without materializing
    /// the rest of a potentially long run transcript.
    pub async fn agent_event_v4_by_hash(
        &self,
        run_id: Uuid,
        event_hash: &str,
    ) -> Result<Option<AgentEventV4>, StoreError> {
        let row = sqlx::query(
            "SELECT value_json FROM agent_events_v4 WHERE run_id=?1 AND event_hash=?2 LIMIT 1",
        )
        .bind(run_id.to_string())
        .bind(event_hash)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let event: AgentEventV4 = serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?;
            event
                .verify()
                .map_err(|error| StoreError::InvalidInput(error.to_string()))?;
            if event.run_id != run_id || event.event_hash != event_hash {
                return Err(StoreError::InvalidInput(
                    "persisted notification event identity mismatch".into(),
                ));
            }
            Ok(event)
        })
        .transpose()
    }

    /// Atomically claim one immutable Agent event for OS notification.
    ///
    /// The receipt is inserted before calling the operating system. A later
    /// delivery error therefore remains at-most-once and cannot cause an
    /// already persisted Agent event to be replayed as a notification.
    pub async fn claim_notification_receipt(&self, event_hash: &str) -> Result<bool, StoreError> {
        if event_hash.len() != 64
            || !event_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(StoreError::InvalidInput(
                "notification event hash must be lowercase SHA-256".into(),
            ));
        }
        let value = serde_json::to_string(&ClaimedNotificationReceipt {
            event_hash,
            state: "claimed",
            claimed_at: Utc::now().to_rfc3339(),
        })?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let inserted =
            sqlx::query("INSERT OR IGNORE INTO app_objects(kind,id,value_json) VALUES (?1,?2,?3)")
                .bind(NOTIFICATION_RECEIPT_KIND)
                .bind(event_hash)
                .bind(value)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                == 1;
        tx.commit().await?;
        Ok(inserted)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Store;

    #[tokio::test]
    async fn notification_receipt_claim_is_atomic_and_at_most_once() {
        let store = Store::open_in_memory().await.unwrap();
        let first_store = store.clone();
        let second_store = store.clone();
        let event_hash = "a".repeat(64);
        let first_hash = event_hash.clone();
        let second_hash = event_hash.clone();

        let (first, second) = tokio::join!(
            first_store.claim_notification_receipt(&first_hash),
            second_store.claim_notification_receipt(&second_hash),
        );

        assert_eq!(
            usize::from(first.unwrap()) + usize::from(second.unwrap()),
            1
        );
        assert!(!store.claim_notification_receipt(&event_hash).await.unwrap());
    }

    #[tokio::test]
    async fn exact_event_lookup_does_not_accept_a_hash_from_another_run() {
        use chrono::Utc;
        use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
        use omicsops_protocol::{AgentEventKindV4, AgentEventV4};
        use uuid::Uuid;

        let store = Store::open_in_memory().await.unwrap();
        let now = Utc::now();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let root = tempfile::tempdir().unwrap();
        store
            .save_project(&Project::new(
                project_id,
                "notifications",
                root.path().to_string_lossy(),
                ProjectTemplate::Blank,
                now,
            ))
            .await
            .unwrap();
        store
            .save_conversation(&Conversation::new(
                conversation_id,
                project_id,
                "notifications",
                now,
            ))
            .await
            .unwrap();
        store
            .save_agent_run_v4(
                run_id,
                project_id,
                conversation_id,
                "running",
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        let event = AgentEventV4::first(
            run_id,
            project_id,
            conversation_id,
            now,
            AgentEventKindV4::RunFailed {
                message: "private diagnostic".into(),
            },
        );
        store.append_agent_event_v4(&event).await.unwrap();

        assert_eq!(
            store
                .agent_event_v4_by_hash(run_id, &event.event_hash)
                .await
                .unwrap(),
            Some(event.clone())
        );
        assert!(
            store
                .agent_event_v4_by_hash(Uuid::new_v4(), &event.event_hash)
                .await
                .unwrap()
                .is_none()
        );
    }
}
