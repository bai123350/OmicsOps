//! Host command for durable manual context projection.

use omicsops_dto::{CompactContextRequestV4, ContextCompactionReceiptV4};
use omicsops_store::StoreError;
use tauri::{AppHandle, State};

use crate::commands::AppState;

/// Persist a bounded archive/checkpoint receipt for one frozen, paused run.
///
/// The Store performs scope, lifecycle, frozen-spec, idempotency, and event
/// chain validation. This adapter deliberately maps database/provider details
/// to a safe user-facing error; the original transcript remains durable on
/// every failure path. The short lease makes the command fail closed while a
/// local or cross-window execution driver still owns the paused run.
#[tauri::command]
pub async fn agent_v4_compact_context(
    app: AppHandle,
    state: State<'_, AppState>,
    request: CompactContextRequestV4,
) -> Result<ContextCompactionReceiptV4, String> {
    let Some(_lease) = crate::run_ownership::try_run_lease(&app, request.run_id)? else {
        return Err(
            "Context compaction is unavailable while the run is owned by an active driver".into(),
        );
    };
    let run = state
        .repository
        .agent_run_v4(request.run_id)
        .await
        .map_err(safe_compaction_error)?;
    if !allows_terminal_noop(run.as_ref()) {
        // The Store has a deterministic paused-run foundation, but this host
        // boundary does not yet have the model/provider admission proof needed
        // to advertise it. Keep the only exposed outcome truthful: completed
        // runs can be acknowledged as NotNeeded without changing their chain.
        return Err(
            "Context compaction is currently available only for completed runs; the original context was retained".into(),
        );
    }
    state
        .repository
        .compact_context_v4(&request)
        .await
        .map_err(safe_compaction_error)
}

fn allows_terminal_noop(run: Option<&serde_json::Value>) -> bool {
    run.and_then(|value| value.get("status"))
        .and_then(serde_json::Value::as_str)
        == Some("completed")
}

fn safe_compaction_error(error: StoreError) -> String {
    match error {
        StoreError::InvalidInput(message)
            if message.contains("cannot be")
                || message.contains("requires")
                || message.contains("does not")
                || message.contains("scope") =>
        {
            message
        }
        StoreError::GuidancePending => {
            "Context compaction is unavailable while guidance is pending".into()
        }
        _ => "Context compaction failed; the original context was retained".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::allows_terminal_noop;
    use serde_json::json;

    #[test]
    fn host_boundary_only_exposes_completed_noop_until_model_projection_is_proven() {
        assert!(allows_terminal_noop(Some(&json!({"status": "completed"}))));
        for status in [
            "waiting_for_input",
            "running",
            "failed",
            "cancelled",
            "needs_attention",
            "idle",
        ] {
            assert!(!allows_terminal_noop(Some(&json!({"status": status}))));
        }
        assert!(!allows_terminal_noop(None));
        assert!(!allows_terminal_noop(Some(&json!({"status": 0}))));
    }
}
