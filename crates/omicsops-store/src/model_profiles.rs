use omicsops_core::workspace::ModelProfile;
use omicsops_protocol::{ReviewerBackendChoiceV4, ReviewerSettingsV4};
use sqlx::Row;
use uuid::Uuid;

use crate::{Store, StoreError, timestamp};

const REVIEWER_SETTINGS_KEY: &str = "reviewer_settings_v4";

impl Store {
    pub async fn delete_model_profile_with<F>(
        &self,
        profile_id: Uuid,
        active_run_ids: &[Uuid],
        delete_credential: F,
    ) -> Result<bool, StoreError>
    where
        F: FnOnce(&str) -> Result<(), String>,
    {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT value_json FROM model_profiles WHERE id=?1")
            .bind(profile_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(false);
        };
        let profile: ModelProfile = serde_json::from_str(row.try_get::<String, _>(0)?.as_str())?;

        let run_rows = sqlx::query("SELECT run_id,status,value_json FROM agent_runs_v4")
            .fetch_all(&mut *tx)
            .await?;
        for row in run_rows {
            let run_id = row.try_get::<String, _>(0)?;
            let status = row.try_get::<String, _>(1)?;
            let in_memory_active = active_run_ids
                .iter()
                .any(|candidate| candidate.to_string() == run_id);
            if !in_memory_active && is_terminal_run_status(&status) {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_str(row.try_get::<String, _>(2)?.as_str())?;
            if frozen_value_references_profile(&value, profile_id) {
                return Err(StoreError::InvalidInput(format!(
                    "model profile is frozen into active agent run {run_id}"
                )));
            }
        }

        let queue_rows = sqlx::query(
            "SELECT request_id,frozen_json FROM composer_queue_v4
             WHERE status IN ('pending','dispatching','running','uncertain')",
        )
        .fetch_all(&mut *tx)
        .await?;
        for row in queue_rows {
            let value: serde_json::Value =
                serde_json::from_str(row.try_get::<String, _>(1)?.as_str())?;
            if frozen_value_references_profile(&value, profile_id) {
                return Err(StoreError::InvalidInput(format!(
                    "model profile is frozen into active composer queue item {}",
                    row.try_get::<String, _>(0)?
                )));
            }
        }

        let side_chat_rows = sqlx::query(
            "SELECT request_id,value_json FROM side_chat_turns_v4
             WHERE status IN ('queued','running')",
        )
        .fetch_all(&mut *tx)
        .await?;
        for row in side_chat_rows {
            let value: serde_json::Value =
                serde_json::from_str(row.try_get::<String, _>(1)?.as_str())?;
            if json_pointer_matches(&value, "/model_profile_id", profile_id) {
                return Err(StoreError::InvalidInput(format!(
                    "model profile is frozen into active side chat {}",
                    row.try_get::<String, _>(0)?
                )));
            }
        }

        let review_rows =
            sqlx::query("SELECT id,value_json FROM session_reviews WHERE status='running'")
                .fetch_all(&mut *tx)
                .await?;
        for row in review_rows {
            let value: serde_json::Value =
                serde_json::from_str(row.try_get::<String, _>(1)?.as_str())?;
            if json_pointer_matches(&value, "/reviewer_profile_id", profile_id) {
                return Err(StoreError::InvalidInput(format!(
                    "model profile is frozen into running session review {}",
                    row.try_get::<String, _>(0)?
                )));
            }
        }

        let credential_reference = profile.credential_reference.clone();

        let result: Result<(), StoreError> = async {
            let rows = sqlx::query("SELECT id,value_json FROM model_profiles WHERE id<>?1")
                .bind(profile_id.to_string())
                .fetch_all(&mut *tx)
                .await?;
            for row in rows {
                let mut candidate: ModelProfile =
                    serde_json::from_str(row.try_get::<String, _>(1)?.as_str())?;
                if candidate.delegated_model_profile_id == Some(profile_id) {
                    candidate.delegated_model_profile_id = None;
                    sqlx::query("UPDATE model_profiles SET value_json=?1 WHERE id=?2")
                        .bind(serde_json::to_string(&candidate)?)
                        .bind(row.try_get::<String, _>(0)?)
                        .execute(&mut *tx)
                        .await?;
                }
            }

            if let Some(value) = sqlx::query_scalar::<_, String>(
                "SELECT value_json FROM settings WHERE scope='global' AND key=?1",
            )
            .bind(REVIEWER_SETTINGS_KEY)
            .fetch_optional(&mut *tx)
            .await?
            {
                let mut settings: ReviewerSettingsV4 = serde_json::from_str(&value)?;
                let default_was_removed = settings.default_http_profile_id == Some(profile_id);
                if default_was_removed {
                    settings.default_http_profile_id = None;
                }
                if matches!(
                    settings.backend,
                    ReviewerBackendChoiceV4::HttpProfile { profile_id: id } if id == profile_id
                ) || (default_was_removed
                    && matches!(settings.backend, ReviewerBackendChoiceV4::DefaultHttp))
                {
                    settings.backend = ReviewerBackendChoiceV4::FollowSession;
                }
                sqlx::query(
                    "UPDATE settings SET value_json=?1,updated_at=?2
                     WHERE scope='global' AND key=?3",
                )
                .bind(serde_json::to_string(&settings)?)
                .bind(timestamp(chrono::Utc::now()))
                .bind(REVIEWER_SETTINGS_KEY)
                .execute(&mut *tx)
                .await?;
            }

            sqlx::query("DELETE FROM model_profiles WHERE id=?1")
                .bind(profile_id.to_string())
                .execute(&mut *tx)
                .await?;
            Ok(())
        }
        .await;

        if let Err(error) = result {
            let _ = tx.rollback().await;
            return Err(error);
        }
        let credential_deleted = if let Some(reference) = credential_reference.as_deref() {
            if let Err(error) = delete_credential(reference) {
                let _ = tx.rollback().await;
                return Err(StoreError::Credential(error));
            }
            true
        } else {
            false
        };
        if let Err(error) = tx.commit().await {
            if credential_deleted {
                return Err(StoreError::CredentialDeletedDatabaseFailed(
                    error.to_string(),
                ));
            }
            return Err(StoreError::Database(error));
        }
        Ok(true)
    }
}

fn is_terminal_run_status(status: &str) -> bool {
    matches!(
        status,
        "completed" | "cancelled" | "failed" | "needs_attention"
    )
}

fn frozen_value_references_profile(value: &serde_json::Value, profile_id: Uuid) -> bool {
    [
        "/model_profile_id",
        "/delegated_model/profile_id",
        "/reviewer_model/profile_id",
        "/spec/model_profile_id",
        "/spec/delegated_model/profile_id",
        "/spec/reviewer_model/profile_id",
    ]
    .into_iter()
    .any(|pointer| json_pointer_matches(value, pointer, profile_id))
}

fn json_pointer_matches(value: &serde_json::Value, pointer: &str, profile_id: Uuid) -> bool {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| value == profile_id.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use omicsops_core::workspace::{
        Conversation, ModelProfile, ModelProviderKind, Project, ProjectTemplate,
    };
    use omicsops_protocol::{ReviewerBackendChoiceV4, ReviewerSettingsV4};
    use sqlx::Row;
    use uuid::Uuid;

    use crate::Store;

    fn profile(id: Uuid, delegated_model_profile_id: Option<Uuid>) -> ModelProfile {
        ModelProfile {
            id,
            label: format!("profile-{id}"),
            provider: ModelProviderKind::OpenAiCompatible,
            base_url: "https://example.invalid/v1".into(),
            model: "test-model".into(),
            credential_reference: Some(format!("model/{id}")),
            supports_tools: true,
            supports_vision: false,
            context_window_tokens: None,
            catalog_capabilities: None,
            reasoning_effort: None,
            fast_mode: None,
            delegated_model_profile_id,
        }
    }

    async fn context(store: &Store, model_profile_id: Uuid) -> (Uuid, Uuid) {
        let now = Utc::now();
        let project = Project::new(
            Uuid::new_v4(),
            "model deletion fixture",
            "C:\\omicsops-test",
            ProjectTemplate::Blank,
            now,
        );
        store.save_project(&project).await.unwrap();
        let mut conversation = Conversation::new(Uuid::new_v4(), project.id, "fixture", now);
        conversation.model_profile_id = Some(model_profile_id);
        store.save_conversation(&conversation).await.unwrap();
        (project.id, conversation.id)
    }

    #[tokio::test]
    async fn deletion_clears_mutable_selectors_and_preserves_terminal_frozen_history() {
        let store = Store::open_in_memory().await.unwrap();
        let removed_id = Uuid::new_v4();
        let parent_id = Uuid::new_v4();
        store
            .save_model_profile(&profile(removed_id, None))
            .await
            .unwrap();
        store
            .save_model_profile(&profile(parent_id, Some(removed_id)))
            .await
            .unwrap();
        let (project_id, conversation_id) = context(&store, removed_id).await;
        sqlx::query("INSERT INTO codex_turn_configs(frame_id,model_profile_id) VALUES (?1,?2)")
            .bind(conversation_id.to_string())
            .bind(removed_id.to_string())
            .execute(&store.pool)
            .await
            .unwrap();
        store
            .save_reviewer_settings(&ReviewerSettingsV4 {
                backend: ReviewerBackendChoiceV4::HttpProfile {
                    profile_id: removed_id,
                },
                default_http_profile_id: Some(removed_id),
            })
            .await
            .unwrap();
        let run_id = Uuid::new_v4();
        let frozen = serde_json::json!({
            "model_profile_id": removed_id,
            "delegated_model": { "profile_id": removed_id },
            "reviewer_model": { "profile_id": removed_id },
            "audit": "must remain unchanged"
        });
        store
            .save_agent_run_v4(run_id, project_id, conversation_id, "completed", &frozen)
            .await
            .unwrap();

        let mut deleted_accounts = Vec::new();
        let deleted = store
            .delete_model_profile_with(removed_id, &[], |account| {
                deleted_accounts.push(account.to_owned());
                Ok(())
            })
            .await
            .unwrap();

        assert!(deleted);
        assert_eq!(deleted_accounts, vec![format!("model/{removed_id}")]);
        assert!(store.get_model_profile(removed_id).await.unwrap().is_none());
        assert_eq!(
            store
                .get_model_profile(parent_id)
                .await
                .unwrap()
                .unwrap()
                .delegated_model_profile_id,
            None
        );
        assert_eq!(
            store.get_reviewer_settings().await.unwrap(),
            ReviewerSettingsV4::default()
        );
        assert_eq!(store.agent_run_v4(run_id).await.unwrap().unwrap(), frozen);
        let row =
            sqlx::query("SELECT model_profile_id FROM conversation_records WHERE frame_id=?1")
                .bind(conversation_id.to_string())
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(row.try_get::<Option<String>, _>(0).unwrap(), None);
        let codex_profile: Option<String> =
            sqlx::query_scalar("SELECT model_profile_id FROM codex_turn_configs WHERE frame_id=?1")
                .bind(conversation_id.to_string())
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(codex_profile, None);
    }

    #[tokio::test]
    async fn deletion_rejects_a_profile_frozen_into_an_active_run_before_vault_cleanup() {
        let store = Store::open_in_memory().await.unwrap();
        let removed_id = Uuid::new_v4();
        store
            .save_model_profile(&profile(removed_id, None))
            .await
            .unwrap();
        let (project_id, conversation_id) = context(&store, removed_id).await;
        let run_id = Uuid::new_v4();
        store
            .save_agent_run_v4(
                run_id,
                project_id,
                conversation_id,
                "running",
                &serde_json::json!({
                    "model_profile_id": Uuid::new_v4(),
                    "delegated_model": { "profile_id": removed_id },
                }),
            )
            .await
            .unwrap();

        let mut vault_calls = 0;
        let error = store
            .delete_model_profile_with(removed_id, &[], |_| {
                vault_calls += 1;
                Ok(())
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("active agent run"));
        assert_eq!(vault_calls, 0);
        assert!(store.get_model_profile(removed_id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn deletion_rejects_all_active_queue_statuses_and_frozen_profile_roles() {
        for (index, status) in ["pending", "dispatching", "running", "uncertain"]
            .into_iter()
            .enumerate()
        {
            let store = Store::open_in_memory().await.unwrap();
            let removed_id = Uuid::new_v4();
            store
                .save_model_profile(&profile(removed_id, None))
                .await
                .unwrap();
            let (project_id, conversation_id) = context(&store, removed_id).await;
            let frozen = match index % 3 {
                0 => serde_json::json!({ "model_profile_id": removed_id }),
                1 => serde_json::json!({
                    "model_profile_id": Uuid::new_v4(),
                    "delegated_model": { "profile_id": removed_id }
                }),
                _ => serde_json::json!({
                    "model_profile_id": Uuid::new_v4(),
                    "reviewer_model": { "profile_id": removed_id }
                }),
            };
            sqlx::query(
                "INSERT INTO composer_queue_v4
                 (request_id,project_id,conversation_id,message_id,run_id,position,revision,
                  mode,status,message_markdown,request_hash,frozen_json,references_json,
                  attachments_json,material_json,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,1,1,'agent',?6,'fixture',?7,?8,'[]','[]','{}',0,0)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind(Uuid::new_v4().to_string())
            .bind(Uuid::new_v4().to_string())
            .bind(status)
            .bind("a".repeat(64))
            .bind(frozen.to_string())
            .execute(&store.pool)
            .await
            .unwrap();

            let mut vault_calls = 0;
            let error = store
                .delete_model_profile_with(removed_id, &[], |_| {
                    vault_calls += 1;
                    Ok(())
                })
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("active composer queue"),
                "{status}: {error}"
            );
            assert_eq!(vault_calls, 0, "{status}");
        }
    }

    #[tokio::test]
    async fn deletion_rejects_queued_and_running_side_chats() {
        for status in ["queued", "running"] {
            let store = Store::open_in_memory().await.unwrap();
            let removed_id = Uuid::new_v4();
            store
                .save_model_profile(&profile(removed_id, None))
                .await
                .unwrap();
            let (project_id, conversation_id) = context(&store, removed_id).await;
            sqlx::query(
                "INSERT INTO side_chat_turns_v4
                 (request_id,project_id,conversation_id,request_hash,status,value_json,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,0,0)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(project_id.to_string())
            .bind(conversation_id.to_string())
            .bind("b".repeat(64))
            .bind(status)
            .bind(serde_json::json!({ "model_profile_id": removed_id }).to_string())
            .execute(&store.pool)
            .await
            .unwrap();

            let mut vault_calls = 0;
            let error = store
                .delete_model_profile_with(removed_id, &[], |_| {
                    vault_calls += 1;
                    Ok(())
                })
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("active side chat"),
                "{status}: {error}"
            );
            assert_eq!(vault_calls, 0, "{status}");
        }
    }

    #[tokio::test]
    async fn deletion_rejects_a_running_session_review() {
        let store = Store::open_in_memory().await.unwrap();
        let removed_id = Uuid::new_v4();
        store
            .save_model_profile(&profile(removed_id, None))
            .await
            .unwrap();
        let (_project_id, conversation_id) = context(&store, removed_id).await;
        sqlx::query(
            "INSERT INTO session_reviews(id,frame_id,reviewer,status,value_json,created_at,updated_at)
             VALUES (?1,?2,'reviewer','running',?3,0,0)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(conversation_id.to_string())
        .bind(serde_json::json!({ "reviewer_profile_id": removed_id }).to_string())
        .execute(&store.pool)
        .await
        .unwrap();

        let mut vault_calls = 0;
        let error = store
            .delete_model_profile_with(removed_id, &[], |_| {
                vault_calls += 1;
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("running session review"));
        assert_eq!(vault_calls, 0);
    }

    #[tokio::test]
    async fn deletion_does_not_remove_the_credential_when_database_cleanup_cannot_be_staged() {
        let store = Store::open_in_memory().await.unwrap();
        let removed_id = Uuid::new_v4();
        store
            .save_model_profile(&profile(removed_id, None))
            .await
            .unwrap();
        sqlx::query("INSERT INTO model_profiles(id,value_json) VALUES (?1,'not-json')")
            .bind(Uuid::new_v4().to_string())
            .execute(&store.pool)
            .await
            .unwrap();

        let mut vault_calls = 0;
        let error = store
            .delete_model_profile_with(removed_id, &[], |_| {
                vault_calls += 1;
                Ok(())
            })
            .await
            .unwrap_err();

        assert!(matches!(error, crate::StoreError::Json(_)));
        assert_eq!(vault_calls, 0);
        assert!(store.get_model_profile(removed_id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn credential_failure_rolls_back_the_staged_database_changes() {
        let store = Store::open_in_memory().await.unwrap();
        let removed_id = Uuid::new_v4();
        let parent_id = Uuid::new_v4();
        store
            .save_model_profile(&profile(removed_id, None))
            .await
            .unwrap();
        store
            .save_model_profile(&profile(parent_id, Some(removed_id)))
            .await
            .unwrap();

        let error = store
            .delete_model_profile_with(removed_id, &[], |_| Err("vault unavailable".into()))
            .await
            .unwrap_err();

        assert!(matches!(error, crate::StoreError::Credential(_)));
        assert!(store.get_model_profile(removed_id).await.unwrap().is_some());
        assert_eq!(
            store
                .get_model_profile(parent_id)
                .await
                .unwrap()
                .unwrap()
                .delegated_model_profile_id,
            Some(removed_id)
        );
    }

    #[tokio::test]
    async fn deleting_a_missing_profile_is_idempotent_without_touching_the_vault() {
        let store = Store::open_in_memory().await.unwrap();
        let mut vault_calls = 0;

        let deleted = store
            .delete_model_profile_with(Uuid::new_v4(), &[], |_| {
                vault_calls += 1;
                Ok(())
            })
            .await
            .unwrap();

        assert!(!deleted);
        assert_eq!(vault_calls, 0);
    }
}
