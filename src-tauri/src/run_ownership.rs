//! Cross-process ownership of a local Agent driver. A released lease says
//! nothing about remote computations already dispatched by that driver.
use std::{fs::File, path::Path};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

pub(crate) fn try_run_lease(app: &AppHandle, run_id: Uuid) -> Result<Option<File>, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| "Run ownership directory is unavailable")?;
    try_lease_in(&directory, run_id)
}

/// Reconciliation also takes the lease briefly. A busy lock therefore must
/// never be treated as proof that an execution was successfully dispatched.
pub(crate) async fn acquire_driver_lease(app: &AppHandle, run_id: Uuid) -> Result<File, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| "Run ownership directory is unavailable")?;
    acquire_driver_in(&directory, run_id).await
}

async fn acquire_driver_in(directory: &Path, run_id: Uuid) -> Result<File, String> {
    for _ in 0..20 {
        if let Some(lease) = try_lease_in(directory, run_id)? {
            return Ok(lease);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err("Run is owned by another action or window; check its state before retrying".into())
}

fn try_lease_in(data_dir: &Path, run_id: Uuid) -> Result<Option<File>, String> {
    let directory = data_dir.join("agent-run-leases");
    std::fs::create_dir_all(&directory).map_err(|_| "Run ownership directory is unavailable")?;
    // Keep the inode: unlinking a lock file would allow two concurrent owners.
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(format!("{run_id}.lock")))
        .map_err(|_| "Run ownership file is unavailable")?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(_)) => Err("Run ownership lock is unavailable".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_lease_excludes_another_owner_and_releases_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let run = Uuid::new_v4();
        let first = try_lease_in(directory.path(), run).unwrap().unwrap();
        assert!(try_lease_in(directory.path(), run).unwrap().is_none());
        let other_run = try_lease_in(directory.path(), Uuid::new_v4())
            .unwrap()
            .unwrap();
        drop(first);
        assert!(try_lease_in(directory.path(), run).unwrap().is_some());
        drop(other_run);
    }

    #[tokio::test]
    async fn short_reconciliation_lease_does_not_skip_dispatch() {
        let directory = tempfile::tempdir().unwrap();
        let run = Uuid::new_v4();
        let reconciliation = try_lease_in(directory.path(), run).unwrap().unwrap();
        let release = async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            drop(reconciliation);
        };
        let (driver, ()) = tokio::join!(acquire_driver_in(directory.path(), run), release);
        let driver = driver.unwrap();
        assert!(try_lease_in(directory.path(), run).unwrap().is_none());
        drop(driver);
    }

    #[tokio::test]
    async fn queued_driver_rechecks_completion_instead_of_dispatching_twice() {
        use omicsops_protocol::{AgentEventKindV4, AgentEventV4, RunModeV4};
        use std::sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        let directory = tempfile::tempdir().unwrap();
        let run = Uuid::new_v4();
        let first = AgentEventV4::first(
            run,
            Uuid::new_v4(),
            Uuid::new_v4(),
            chrono::Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        let expected_head = first.event_hash.clone();
        let stored = Mutex::new(("running", vec![first]));
        let dispatches = AtomicUsize::new(1);
        let owner = try_lease_in(directory.path(), run).unwrap().unwrap();
        let finish_owner = async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            {
                let mut state = stored.lock().unwrap();
                let event = AgentEventV4::next(
                    state.1.last().unwrap(),
                    chrono::Utc::now(),
                    AgentEventKindV4::RunCompleted,
                );
                state.1.push(event);
                state.0 = "completed";
            }
            drop(owner);
        };
        let queued = async {
            let _lease = acquire_driver_in(directory.path(), run).await.unwrap();
            let state = stored.lock().unwrap();
            if crate::agent_v4::execution_lease_snapshot_is_current(
                state.0,
                &state.1,
                Some(&expected_head),
            )
            .unwrap()
            {
                dispatches.fetch_add(1, Ordering::SeqCst);
            }
            for status in ["completed", "failed", "needs_attention", "cancelled"] {
                assert!(
                    !crate::agent_v4::execution_lease_snapshot_is_current(
                        status,
                        &state.1[..1],
                        Some(&expected_head)
                    )
                    .unwrap()
                );
            }
            assert!(
                crate::agent_v4::execution_lease_snapshot_is_current(
                    "waiting_for_input",
                    &state.1[..1],
                    Some("stale head")
                )
                .is_err()
            );
        };
        tokio::join!(finish_owner, queued);
        assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    }
}
