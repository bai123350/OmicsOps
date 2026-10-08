//! Host command for durable manual context projection.

use crate::commands::AppState;
use omicsops_dto::{CompactContextRequestV4, ContextCompactionReceiptV4};
use omicsops_protocol::ContextCompactionStatusV4;
use omicsops_store::StoreError;
use tauri::{AppHandle, State};

const RETAINED: &str = "Context compaction failed; the original context was retained";
const BUSY: &str = "Context compaction is unavailable while the run is owned by an active driver";

#[tauri::command]
pub async fn agent_v4_compact_context(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CompactContextRequestV4,
) -> Result<ContextCompactionReceiptV4, String> {
    compact_context_with_lease_v4(&state, request.clone(), || {
        crate::run_ownership::try_run_lease(&app, request.run_id)
    })
    .await
}

/// The lease supplier only abstracts locating the OS lock directory, allowing
/// tests to exercise this exact host flow with a real lock in a temporary root.
/// The returned file remains owned here until Store's commit has completed.
pub(crate) async fn compact_context_with_lease_v4(
    state: &AppState,
    request: CompactContextRequestV4,
    acquire: impl FnOnce() -> Result<Option<std::fs::File>, String>,
) -> Result<ContextCompactionReceiptV4, String> {
    if let Some(receipt) = state
        .repository
        .context_compaction_receipt_v4(&request)
        .await
        .map_err(safe_compaction_error)?
    {
        return match receipt.status {
            ContextCompactionStatusV4::Completed | ContextCompactionStatusV4::NotNeeded => {
                Ok(receipt)
            }
            _ => Err(RETAINED.into()),
        };
    }
    let _lease = acquire()
        .map_err(|_| RETAINED.to_owned())?
        .ok_or_else(|| BUSY.to_owned())?;
    if state
        .active_runs
        .lock()
        .map_err(|_| RETAINED.to_owned())?
        .contains_key(&request.run_id)
    {
        return Err(BUSY.into());
    }
    let run = state
        .repository
        .agent_run_v4(request.run_id)
        .await
        .map_err(safe_compaction_error)?;
    if allows_terminal_noop(run.as_ref()) {
        return state
            .repository
            .compact_context_v4(&request)
            .await
            .map_err(safe_compaction_error);
    }
    let snapshot = state
        .repository
        .context_compaction_snapshot_v4(&request)
        .await
        .map_err(safe_compaction_error)?;
    let prepared = crate::agent_v4::prepare_manual_compaction_v4(state, &snapshot)
        .await
        .map_err(|_| RETAINED.to_owned())?;
    #[cfg(test)]
    crate::agent_v4::manual_compaction_host_tests::before_commit(request.run_id).await;
    if state
        .active_runs
        .lock()
        .map_err(|_| RETAINED.to_owned())?
        .contains_key(&request.run_id)
    {
        return Err(BUSY.into());
    }
    state
        .repository
        .commit_context_compaction_v4(&request, &prepared)
        .await
        .map_err(safe_compaction_error)
}

fn allows_terminal_noop(run: Option<&serde_json::Value>) -> bool {
    run.and_then(|value| value.get("status"))
        .and_then(serde_json::Value::as_str)
        == Some("completed")
}

// Whitelist fixed messages only: Store/provider text can contain paths, source
// values or secrets even when it includes seemingly safe words such as scope.
fn safe_compaction_error(error: StoreError) -> String {
    match error {
        StoreError::GuidancePending => {
            "Context compaction is unavailable while guidance is pending".into()
        }
        _ => RETAINED.into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn manual_compaction_host_errors_never_pass_through_store_details() {
        for message in [
            "requires SECRET_SENTINEL",
            "cannot be SECRET_SENTINEL",
            "does not SECRET_SENTINEL",
            "scope SECRET_SENTINEL",
        ] {
            let public = super::safe_compaction_error(omicsops_store::StoreError::InvalidInput(
                message.into(),
            ));
            assert!(
                !public.contains("SECRET_SENTINEL"),
                "raw Store error crossed the boundary"
            );
            assert_eq!(public, super::RETAINED);
        }
    }
}
