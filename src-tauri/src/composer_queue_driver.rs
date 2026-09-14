//! One durable dispatcher per project/conversation queue.
//!
//! The database lease fences queue rows, while the small OS file lease fences
//! two desktop windows from driving the same conversation at once.  The file
//! is intentionally kept in place after a process exits; only the OS lock is
//! authoritative, so a restart can safely acquire it and reconcile the
//! durable row before claiming another item.

use std::{fs::File, path::Path};

use chrono::Utc;
use omicsops_dto::{ComposerQueueFailureCodeV4, ComposerQueueItemV4};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::commands::AppState;

const LEASE_DIRECTORY: &str = "composer-queue-leases";
const RUN_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Start (or attempt to start) the scoped queue driver.  A concurrent kick is
/// harmless: the OS lock makes only one task proceed, and the other task
/// returns without changing the durable queue.
pub(crate) fn kick_composer_queue(app: &AppHandle, project_id: Uuid, conversation_id: Uuid) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let data_dir = match app.path().app_data_dir() {
            Ok(path) => path,
            Err(error) => {
                eprintln!("composer queue data directory is unavailable: {error}");
                return;
            }
        };
        let Some(_scope_lease) = (match try_scope_lease(&data_dir, project_id, conversation_id) {
            Ok(lease) => lease,
            Err(error) => {
                eprintln!("composer queue scope lease failed: {error}");
                return;
            }
        }) else {
            return;
        };
        let state = app.state::<AppState>();
        if let Err(error) = drive(&app, state.inner(), project_id, conversation_id).await {
            eprintln!("composer queue driver stopped: {error}");
        }
    });
}

async fn drive(
    app: &AppHandle,
    state: &AppState,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<(), String> {
    state
        .repository
        .reconcile_composer_queue(project_id, conversation_id, Utc::now())
        .await
        .map_err(|error| error.to_string())?;

    loop {
        // An accepted replacement must progress without a visible window's
        // polling. Observe only its already committed Stop; OS ownership
        // protects a live driver and recovery never replays provider work.
        let next = state
            .repository
            .list_composer_queue(project_id, conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|item| item.status == omicsops_dto::ComposerQueueStatusV4::Pending);
        if let Some(target) = next.and_then(|item| item.replacement_target_run_id) {
            wait_for_committed_stop(|| async {
                crate::agent_v4::observe_durable_stop(
                    app,
                    state,
                    project_id,
                    conversation_id,
                    target,
                )
                .await?
                .map(|receipt| receipt.status)
                .ok_or_else(|| "Replacement Stop receipt is unavailable".to_owned())
            })
            .await?;
        }
        let Some(lease) = state
            .repository
            .claim_next_composer_queue(project_id, conversation_id, Utc::now())
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(());
        };

        // Bind ownership to the row actually claimed, not an earlier queue
        // listing: another window may insert a replacement between reads.
        // A terminal event may precede the old driver's final cleanup.
        let _predecessor_lease = if let Some(target) = lease.item.replacement_target_run_id {
            match crate::run_ownership::try_run_lease(app, target) {
                Ok(Some(owner)) => Some(owner),
                blocked => {
                    state
                        .repository
                        .release_composer_queue_preparation(&lease, Utc::now(), None)
                        .await
                        .map_err(|error| error.to_string())?;
                    if let Err(error) = blocked {
                        return Err(error);
                    }
                    tokio::time::sleep(RUN_POLL_INTERVAL).await;
                    continue;
                }
            }
        } else {
            None
        };
        match crate::agent_v4::dispatch_composer_queue(app.clone(), state, lease.clone()).await {
            Ok(item) => {
                // Approval, input-wait, and needs-attention states deliberately
                // yield the queue. A stop advances FIFO only after its
                // cancellation has a durable terminal event; an uncertain or
                // incomplete settlement remains fenced for reconciliation.
                if !wait_for_run_settlement(state, &item).await? {
                    return Ok(());
                }
            }
            Err(error) => {
                let code = classify_preparation_failure(&error);
                match state
                    .repository
                    .release_composer_queue_preparation(&lease, Utc::now(), Some(code))
                    .await
                {
                    Ok(_) => {
                        // A deterministic pre-commit failure is terminal for
                        // this queued item.  Continue FIFO with the next row;
                        // its material/configuration is independently checked.
                    }
                    Err(release_error) => {
                        // If the release fence says side effects crossed the
                        // boundary, do not retry or mark the row pending. A
                        // later reconciliation can inspect the durable run.
                        return Err(format!(
                            "queue preparation failed ({error}); release was not confirmed ({release_error})"
                        ));
                    }
                }
            }
        }
    }
}

fn classify_preparation_failure(error: &str) -> ComposerQueueFailureCodeV4 {
    let error = error.to_ascii_lowercase();
    if error.contains("material")
        || error.contains("reference")
        || error.contains("attachment")
        || error.contains("hash")
        || error.contains("file")
    {
        ComposerQueueFailureCodeV4::MaterialChanged
    } else if error.contains("profile")
        || error.contains("configuration")
        || error.contains("compute")
        || error.contains("frozen")
    {
        ComposerQueueFailureCodeV4::ConfigurationChanged
    } else {
        ComposerQueueFailureCodeV4::DispatchFailed
    }
}

async fn wait_for_run_settlement(
    state: &AppState,
    item: &ComposerQueueItemV4,
) -> Result<bool, String> {
    loop {
        let Some(value) = state
            .repository
            .agent_run_v4(item.run_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(false);
        };
        let status = value.get("status").and_then(serde_json::Value::as_str);
        let events = state
            .repository
            .agent_events_v4(item.run_id)
            .await
            .map_err(|error| error.to_string())?;
        let terminal_event_status = events.iter().find_map(|event| match event.event {
            omicsops_protocol::AgentEventKindV4::RunCompleted => Some("completed"),
            omicsops_protocol::AgentEventKindV4::RunFailed { .. } => Some("failed"),
            omicsops_protocol::AgentEventKindV4::RunCancelled => Some("cancelled"),
            omicsops_protocol::AgentEventKindV4::RunNeedsAttention { .. } => {
                Some("needs_attention")
            }
            _ => None,
        });
        // Wisp's Stop action is scoped to the active turn: it leaves pending
        // queue rows intact and its FIFO driver proceeds once the cancelled
        // turn has a durable terminal event.  Do not treat the stop receipt
        // itself as a queue pause; an observed receipt remains stored for
        // restart reconciliation.
        if let Some(advance) = settlement_decision(status, terminal_event_status) {
            return Ok(advance);
        }
        tokio::time::sleep(RUN_POLL_INTERVAL).await;
    }
}

fn settlement_decision(status: Option<&str>, terminal_event_status: Option<&str>) -> Option<bool> {
    match status {
        Some(status @ ("completed" | "failed" | "cancelled")) => match terminal_event_status {
            Some(event_status) if event_status == status => Some(true),
            Some(_) => Some(false),
            None => None,
        },
        Some("needs_attention") => Some(false),
        Some("waiting_for_input") | Some("waiting_for_approval") | Some("awaiting_approval") => {
            Some(false)
        }
        Some("planning") | Some("running") => None,
        Some(_) | None => Some(false),
    }
}

/// Try to acquire the per-conversation OS lock.  This is kept as a pure path
/// seam for deterministic tests and follows the same inode-preserving rule as
/// the existing per-run ownership lock.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn try_scope_lease(
    data_dir: &Path,
    project_id: Uuid,
    conversation_id: Uuid,
) -> Result<Option<File>, String> {
    let directory = data_dir.join(LEASE_DIRECTORY);
    std::fs::create_dir_all(&directory)
        .map_err(|_| "Composer queue lease directory is unavailable".to_owned())?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(format!("{project_id}-{conversation_id}.lock")))
        .map_err(|_| "Composer queue lease file is unavailable".to_owned())?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(_)) => {
            Err("Composer queue lease is unavailable".to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replacement_waits_then_wakes_without_a_ui_kick() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let settled = AtomicBool::new(false);
        let reads = AtomicUsize::new(0);
        let waiting = wait_for_committed_stop(|| async {
            reads.fetch_add(1, Ordering::SeqCst);
            Ok(if settled.load(Ordering::SeqCst) {
                omicsops_dto::StopRunStatusV4::Observed
            } else {
                omicsops_dto::StopRunStatusV4::Requested
            })
        });
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut waiting)
                .await
                .is_err()
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        settled.store(true, Ordering::SeqCst);
        tokio::time::timeout(std::time::Duration::from_secs(1), &mut waiting)
            .await
            .unwrap()
            .unwrap();
        assert!(reads.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn scope_lease_is_exclusive_and_reusable_after_drop() {
        let directory = tempfile::tempdir().unwrap();
        let project_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let first = try_scope_lease(directory.path(), project_id, conversation_id)
            .unwrap()
            .unwrap();
        assert!(
            try_scope_lease(directory.path(), project_id, conversation_id)
                .unwrap()
                .is_none()
        );
        drop(first);
        assert!(
            try_scope_lease(directory.path(), project_id, conversation_id)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn different_conversations_do_not_share_the_scope_lease() {
        let directory = tempfile::tempdir().unwrap();
        let project_id = Uuid::new_v4();
        let first = try_scope_lease(directory.path(), project_id, Uuid::new_v4())
            .unwrap()
            .unwrap();
        assert!(
            try_scope_lease(directory.path(), project_id, Uuid::new_v4())
                .unwrap()
                .is_some()
        );
        drop(first);
    }

    #[test]
    fn cancelled_run_advances_only_after_its_terminal_event_is_durable() {
        assert_eq!(settlement_decision(Some("cancelled"), None), None);
        assert_eq!(
            settlement_decision(Some("cancelled"), Some("cancelled")),
            Some(true)
        );
        assert_eq!(
            settlement_decision(Some("failed"), Some("needs_attention")),
            Some(false)
        );
        assert_eq!(
            settlement_decision(Some("completed"), Some("failed")),
            Some(false)
        );
        assert_eq!(
            settlement_decision(Some("needs_attention"), Some("needs_attention")),
            Some(false)
        );
        assert_eq!(settlement_decision(Some("running"), None), None);
    }
}

/// Retain the driver's wait until the existing Stop is observed; no visible
/// page, new request identity or provider replay is needed to wake it.
async fn wait_for_committed_stop<F, Fut>(mut observe: F) -> Result<(), String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<omicsops_dto::StopRunStatusV4, String>>,
{
    while observe().await? == omicsops_dto::StopRunStatusV4::Requested {
        tokio::time::sleep(RUN_POLL_INTERVAL).await;
    }
    Ok(())
}
