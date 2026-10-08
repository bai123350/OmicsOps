//! Read-only, metadata-only diagnostics for persisted V4 event chains.

use std::collections::{BTreeMap, BTreeSet};

use omicsops_core::redaction::redact_secrets;
use omicsops_dto::{
    RunDiagnosticEventV4, RunDiagnosticStatusV4, RunDiagnosticUsageV4,
    RunDiagnosticsFilterSummaryV4, RunDiagnosticsRequestV4, RunDiagnosticsSnapshotV4,
    RunDiagnosticsSourceHeadV4,
};
use omicsops_protocol::{validate_event_chain_v4, AgentEventKindV4, AgentEventV4, UsageTotalsV4};
use omicsops_store::{Store, StoreError};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{commands::AppState, composer_references::public_text};

const MAX_EVENTS: usize = 10_000;
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PROJECTION_BYTES: usize = 8 * 1024 * 1024;
const MAX_LABEL_BYTES: usize = 256;

fn validate_request(request: &RunDiagnosticsRequestV4) -> Result<(), String> {
    if request.filter.tool_id.as_ref().is_some_and(|id| id.len() > 256)
        || request.redaction_keywords.len() > 32
        || request.redaction_keywords.iter().any(|word| word.is_empty() || word.len() > 1024)
        || request.redaction_keywords.iter().map(String::len).sum::<usize>() > 16 * 1024
    {
        return Err("diagnostics_invalid_request".into());
    }
    Ok(())
}

fn tool_label(value: &str, request: &RunDiagnosticsRequestV4) -> String {
    let mut label = redact_secrets(&public_text(value), &request.redaction_keywords);
    let mut end = label.len().min(MAX_LABEL_BYTES);
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    label.truncate(end);
    label
}

// The UUID has a fixed 16-byte width, so concatenation cannot alias another pair.
fn call_ref(run_id: Uuid, call_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(run_id.as_bytes());
    hash.update(call_id.as_bytes());
    hex::encode(hash.finalize())
}

fn direct_tool(event: &AgentEventKindV4) -> Option<(&str, &str)> {
    use AgentEventKindV4::*;
    match event {
        ToolRequested { call } => Some((&call.call_id, &call.tool_id)),
        ToolApprovalRequested { request } => Some((&request.call.call_id, &request.call.tool_id)),
        ToolDispatchStarted { call_id, tool_id, .. }
        | ToolDispatchUncertain { call_id, tool_id } => Some((call_id, tool_id)),
        ToolFinished { outcome } | ToolOutcomeReused { outcome, .. } => Some((&outcome.call_id, &outcome.tool_id)),
        _ => None,
    }
}

fn associated_call(event: &AgentEventKindV4) -> Option<&str> {
    use AgentEventKindV4::*;
    direct_tool(event).map(|(call, _)| call).or_else(|| match event {
        ToolDispatchResolved { call_id, .. }
        | DelegationGraphStarted { call_id, .. }
        | DelegationNodeFinished { call_id, .. }
        | DelegationGraphFinished { call_id, .. } => Some(call_id),
        _ => None,
    })
}

fn status(event: &AgentEventKindV4) -> Option<RunDiagnosticStatusV4> {
    use AgentEventKindV4::*;
    use RunDiagnosticStatusV4::*;
    match event {
        ToolFinished { outcome } | ToolOutcomeReused { outcome, .. } => Some(if outcome.succeeded { Succeeded } else { Failed }),
        RunCompleted => Some(Succeeded),
        RunFailed { .. } | ModelRetrying { .. } => Some(Failed),
        ToolDispatchUncertain { .. } => Some(Uncertain),
        RunCancelled => Some(Cancelled),
        _ => None,
    }
}

// Exhaustive whitelist: adding a protocol kind requires an explicit metadata decision.
fn event_kind(event: &AgentEventKindV4) -> &'static str {
    use AgentEventKindV4::*;
    match event {
        RuntimeRecoveryAvailable { .. } => "runtime_recovery_available",
        GuidanceConsumed { .. } => "guidance_consumed",
        RunCreated { .. } => "run_created",
        ModelText { .. } => "model_text",
        ModelRetrying { .. } => "model_retrying",
        ModelRequestStarted { .. } => "model_request_started",
        ModelUsageObserved { .. } => "model_usage_observed",
        ModelReplayRecorded { .. } => "model_replay_recorded",
        ToolRequested { .. } => "tool_requested",
        ToolApprovalRequested { .. } => "tool_approval_requested",
        ToolApprovalDecided { .. } => "tool_approval_decided",
        RequestRouted { .. } => "request_routed",
        TaskShapeSelected { .. } => "task_shape_selected",
        PhaseChanged { .. } => "phase_changed",
        CycleStarted { .. } => "cycle_started",
        CycleFinished { .. } => "cycle_finished",
        TaskListUpdated { .. } => "task_list_updated",
        ToolBatchStarted { .. } => "tool_batch_started",
        ToolBatchFinished { .. } => "tool_batch_finished",
        BrowserConnectionRequired { .. } => "browser_connection_required",
        BrowserHumanInterventionRequired { .. } => "browser_human_intervention_required",
        BrowserTabCleanupRequired { .. } => "browser_tab_cleanup_required",
        ToolDispatchStarted { .. } => "tool_dispatch_started",
        ToolFinished { .. } => "tool_finished",
        ToolOutcomeReused { .. } => "tool_outcome_reused",
        ToolDispatchUncertain { .. } => "tool_dispatch_uncertain",
        ToolDispatchResolved { .. } => "tool_dispatch_resolved",
        PlanProposed { .. } => "plan_proposed",
        PlanApproved { .. } => "plan_approved",
        PlanRevisionRequested { .. } => "plan_revision_requested",
        RunSpecFrozen { .. } => "run_spec_frozen",
        ModeChanged { .. } => "mode_changed",
        InputRequested { .. } => "input_requested",
        UserInputAnswered { .. } => "user_input_answered",
        ContextArchived { .. } => "context_archived",
        ContextCheckpointed { .. } => "context_checkpointed",
        ContextCompactionStarted { .. } => "context_compaction_started",
        ContextCompactionNotNeeded { .. } => "context_compaction_not_needed",
        ContextCompactionCompleted { .. } => "context_compaction_completed",
        ContextCompactionAttention { .. } => "context_compaction_attention",
        ScientificStateChanged { .. } => "scientific_state_changed",
        CompletionProposed => "completion_proposed",
        CompletionProposalSubmitted { .. } => "completion_proposal_submitted",
        DeterministicVerificationFinished { .. } => "deterministic_verification_finished",
        ReviewerFinished { .. } => "reviewer_finished",
        ReviewerCorrectionRequested { .. } => "reviewer_correction_requested",
        DelegationGraphStarted { .. } => "delegation_graph_started",
        DelegationNodeFinished { .. } => "delegation_node_finished",
        DelegationGraphFinished { .. } => "delegation_graph_finished",
        RunCompleted => "run_completed",
        RunFailed { .. } => "run_failed",
        RunNeedsAttention { .. } => "run_needs_attention",
        RunCancelled => "run_cancelled",
    }
}

/// Validate all supplied chains before applying any display filter. The host loader
/// additionally establishes persisted ownership and bounds the original JSON bytes.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn project_run_diagnostics(
    events: &[AgentEventV4],
    request: &RunDiagnosticsRequestV4,
) -> Result<RunDiagnosticsSnapshotV4, String> {
    validate_request(request)?;
    if events.len() > MAX_EVENTS {
        return Err("diagnostics_source_limit".into());
    }
    let mut source_bytes = 0usize;
    let mut runs: BTreeMap<Uuid, Vec<AgentEventV4>> = BTreeMap::new();
    for event in events {
        if event.project_id != request.project_id || event.conversation_id != request.conversation_id || event.schema_version != 4 {
            return Err("diagnostics_source_invalid".into());
        }
        source_bytes += serde_json::to_vec(event).map_err(|_| "diagnostics_source_invalid")?.len();
        if source_bytes > MAX_SOURCE_BYTES {
            return Err("diagnostics_source_limit".into());
        }
        runs.entry(event.run_id).or_default().push(event.clone());
    }
    for chain in runs.values_mut() {
        chain.sort_by_key(|event| event.sequence);
        validate_event_chain_v4(chain).map_err(|_| "diagnostics_source_invalid")?;
    }
    project_validated_run_diagnostics(events, request)
}

/// Private trust boundary: request limits and the ENTIRE owned event set must
/// already be validated. The loader obtains that guarantee from the bounded
/// Store read, which checks ORIGINAL JSON hashes before serde adds defaults.
/// Rehashing typed legacy events here would invalidate legitimate source hashes.
fn project_validated_run_diagnostics(
    events: &[AgentEventV4],
    request: &RunDiagnosticsRequestV4,
) -> Result<RunDiagnosticsSnapshotV4, String> {
    validate_request(request)?;
    if events.len() > MAX_EVENTS {
        return Err("diagnostics_source_limit".into());
    }
    if events.iter().any(|event| event.project_id != request.project_id || event.conversation_id != request.conversation_id || event.schema_version != 4) {
        return Err("diagnostics_source_invalid".into());
    }
    let mut runs: BTreeMap<Uuid, Vec<&AgentEventV4>> = BTreeMap::new();
    for event in events {
        runs.entry(event.run_id).or_default().push(event);
    }
    let mut heads = Vec::new();
    for (run_id, chain) in &mut runs {
        chain.sort_by_key(|event| event.sequence);
        let head = chain.last().ok_or("diagnostics_source_invalid")?;
        heads.push(RunDiagnosticsSourceHeadV4 { run_id: *run_id, event_count: chain.len() as u64, last_sequence: head.sequence, event_hash: head.event_hash.clone() });
    }
    let mut ordered = runs.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(run_id, chain)| (chain[0].occurred_at, **run_id));
    let mut rows = Vec::new();
    let mut usage = Vec::new();
    let mut labels = BTreeMap::new();
    for (run_id, chain) in ordered {
        if request.filter.run_id.is_some_and(|selected| selected != *run_id) {
            continue;
        }
        let mut tools: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        let mut starts: BTreeMap<&str, Vec<&AgentEventV4>> = BTreeMap::new();
        let mut finishes: BTreeMap<&str, usize> = BTreeMap::new();
        let mut attempts = BTreeSet::new();
        let mut observed = BTreeSet::new();
        let mut observations = Vec::new();
        for event in chain {
            if let Some((call, tool)) = direct_tool(&event.event) {
                tools.entry(call).or_default().insert(tool);
            }
            match &event.event {
                AgentEventKindV4::ToolDispatchStarted { call_id, .. } => { starts.entry(call_id).or_default().push(event); }
                AgentEventKindV4::ToolFinished { outcome } => { *finishes.entry(&outcome.call_id).or_default() += 1; }
                AgentEventKindV4::ModelRequestStarted { request } => { attempts.insert((request.logical_request_id, request.attempt_id)); }
                AgentEventKindV4::ModelUsageObserved { observation } => {
                    observed.insert((observation.logical_request_id, observation.attempt_id));
                    observations.push(observation.clone());
                }
                _ => {}
            }
        }
        usage.push(RunDiagnosticUsageV4 { run_id: *run_id, totals: UsageTotalsV4::from_observations(observations), missing_observation_attempts: attempts.difference(&observed).count() as u32 });
        for event in chain {
            let call = associated_call(&event.event);
            let tool = direct_tool(&event.event).map(|(_, tool)| tool).or_else(|| {
                let candidates = tools.get(call?)?;
                (candidates.len() == 1).then(|| *candidates.first().expect("one tool"))
            });
            let status = status(&event.event);
            if request.filter.tool_id.as_deref().is_some_and(|selected| Some(selected) != tool)
                || (request.filter.failures_only && !matches!(status, Some(RunDiagnosticStatusV4::Failed | RunDiagnosticStatusV4::Uncertain)))
            {
                continue;
            }
            let duration_ms = if let AgentEventKindV4::ToolFinished { outcome } = &event.event {
                starts.get(outcome.call_id.as_str()).and_then(|starts| {
                    if starts.len() != 1 || finishes.get(outcome.call_id.as_str()) != Some(&1) || tools.get(outcome.call_id.as_str()).is_none_or(|tools| tools.len() != 1) {
                        return None;
                    }
                    let start = starts[0];
                    if start.sequence >= event.sequence || start.occurred_at > event.occurred_at {
                        return None;
                    }
                    u64::try_from((event.occurred_at - start.occurred_at).num_milliseconds()).ok()
                })
            } else { None };
            rows.push(RunDiagnosticEventV4 {
                run_id: *run_id, sequence: event.sequence, event_hash: event.event_hash.clone(), previous_hash: event.previous_hash.clone(), occurred_at: event.occurred_at,
                event_kind: event_kind(&event.event).into(), status, tool_label: tool.map(|tool| labels.entry(tool).or_insert_with(|| tool_label(tool, request)).clone()), call_ref: call.map(|call| call_ref(*run_id, call)), duration_ms,
            });
        }
    }
    let snapshot = RunDiagnosticsSnapshotV4 {
        schema_version: 1, project_id: request.project_id, conversation_id: request.conversation_id,
        filter_summary: RunDiagnosticsFilterSummaryV4 { run_id: request.filter.run_id, tool_label: request.filter.tool_id.as_deref().map(|tool| tool_label(tool, request)), failures_only: request.filter.failures_only },
        source_heads: heads, source_chain_validated: true, total_event_count: events.len() as u64, selected_event_count: rows.len() as u64,
        filtered: request.filter.run_id.is_some() || request.filter.tool_id.is_some() || request.filter.failures_only, events: rows, usage,
    };
    if serde_json::to_vec(&snapshot).map_err(|_| "diagnostics_projection_failed")?.len() > MAX_PROJECTION_BYTES {
        return Err("diagnostics_projection_limit".into());
    }
    Ok(snapshot)
}

pub(crate) async fn load_run_diagnostics(repository: &Store, request: &RunDiagnosticsRequestV4) -> Result<RunDiagnosticsSnapshotV4, String> {
    validate_request(request)?;
    let events = repository.agent_events_for_context_v4_bounded(request.project_id, request.conversation_id, MAX_EVENTS, MAX_SOURCE_BYTES).await.map_err(|error| match error {
        StoreError::InvalidInput(message) if message == "diagnostics source limit exceeded" => "diagnostics_source_limit",
        StoreError::InvalidInput(message) if message == "diagnostics scope invalid" => "diagnostics_scope_invalid",
        StoreError::InvalidInput(_) => "diagnostics_source_invalid",
        _ => "diagnostics_read_failed",
    })?;
    project_validated_run_diagnostics(&events, request)
}

#[tauri::command]
pub async fn get_run_diagnostics_v4(state: State<'_, AppState>, request: RunDiagnosticsRequestV4) -> Result<RunDiagnosticsSnapshotV4, String> {
    load_run_diagnostics(&state.repository, &request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Duration, Utc};
    use omicsops_dto::{RunDiagnosticStatusV4 as Status, RunDiagnosticsFilterV4, RunDiagnosticsRequestV4};
    use omicsops_protocol::{AgentEventKindV4 as Kind, AgentEventV4, ModelRequestStartedV4, ModelUsageObservationV4, RunModeV4, ToolEffectV4, ToolOutcomeV4};
    use serde_json::json;
    use uuid::Uuid;

    fn request() -> RunDiagnosticsRequestV4 {
        RunDiagnosticsRequestV4 {
            project_id: Uuid::from_u128(1),
            conversation_id: Uuid::from_u128(2),
            filter: RunDiagnosticsFilterV4 { run_id: None, tool_id: None, failures_only: false },
            redaction_keywords: vec![],
        }
    }

    fn chain(run: u128, entries: Vec<(i64, Kind)>) -> Vec<AgentEventV4> {
        let req = request();
        let origin: DateTime<Utc> = "2026-10-08T00:00:00Z".parse().unwrap();
        let mut events = Vec::new();
        for (millis, kind) in entries {
            let time = origin + Duration::milliseconds(millis);
            let event = match events.last() {
                Some(previous) => AgentEventV4::next(previous, time, kind),
                None => AgentEventV4::first(Uuid::from_u128(run), req.project_id, req.conversation_id, time, kind),
            };
            events.push(event);
        }
        events
    }

    fn start(call: &str, tool: &str) -> Kind {
        Kind::ToolDispatchStarted { call_id: call.into(), tool_id: tool.into(), effect: ToolEffectV4::ReadOnly, idempotency_key: "PRIVATE_SENTINEL".into() }
    }

    fn outcome(call: &str, tool: &str, succeeded: bool) -> ToolOutcomeV4 {
        ToolOutcomeV4 { call_id: call.into(), tool_id: tool.into(), succeeded, model_content: "PRIVATE_SENTINEL".into(), data: json!({"private": "PRIVATE_SENTINEL"}), provenance: vec!["PRIVATE_SENTINEL".into()] }
    }

    fn finish(call: &str, tool: &str, succeeded: bool) -> Kind {
        Kind::ToolFinished { outcome: outcome(call, tool, succeeded) }
    }

    fn attempt(id: u128) -> Kind {
        Kind::ModelRequestStarted { request: serde_json::from_value::<ModelRequestStartedV4>(json!({
            "logical_request_id": Uuid::from_u128(10), "attempt_id": Uuid::from_u128(id),
            "model_profile_id": Uuid::from_u128(20), "context_limit_source": {"kind": "unknown"}
        })).unwrap() }
    }

    fn usage(id: u128, tokens: u64, aggregation: &str) -> Kind {
        Kind::ModelUsageObserved { observation: serde_json::from_value::<ModelUsageObservationV4>(json!({
            "logical_request_id": Uuid::from_u128(10), "attempt_id": Uuid::from_u128(id),
            "model_profile_id": Uuid::from_u128(20), "sample_index": 0,
            "state": "partial", "aggregation": aggregation,
            "input_tokens": tokens, "context_limit_source": {"kind": "unknown"}
        })).unwrap() }
    }

    #[test]
    fn run_diagnostics_projection_scopes_calls_durations_and_failure_filter() {
        let mut events = chain(30, vec![
            (0, start("same", "shell")), (100, finish("same", "shell", true)),
            (101, Kind::ToolOutcomeReused { idempotency_key: "PRIVATE_SENTINEL".into(), outcome: outcome("same", "shell", false) }),
            (102, Kind::ToolDispatchUncertain { call_id: "uncertain".into(), tool_id: "shell".into() }),
            (103, Kind::RunFailed { message: "PRIVATE_SENTINEL".into() }),
        ]);
        events.extend(chain(31, vec![(0, start("same", "shell")), (400, finish("same", "shell", false)), (401, Kind::RunCancelled)]));
        let snapshot = project_run_diagnostics(&events, &request()).unwrap();
        assert_eq!(snapshot.events[1].duration_ms, Some(100));
        assert_eq!(snapshot.events[6].duration_ms, Some(400));
        assert_ne!(snapshot.events[1].call_ref, snapshot.events[6].call_ref);
        assert!(snapshot.events[1].call_ref.as_ref().unwrap().bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(snapshot.events[1].call_ref.as_ref().unwrap().len(), 64);
        assert_eq!(snapshot.events[2].duration_ms, None);
        assert_eq!(snapshot.events[3].status, Some(Status::Uncertain));
        assert_eq!(snapshot.events[7].status, Some(Status::Cancelled));
        let mut req = request();
        req.filter.failures_only = true;
        let failed = project_run_diagnostics(&events, &req).unwrap();
        assert_eq!(failed.events.iter().map(|e| e.event_kind.as_str()).collect::<Vec<_>>(), vec!["tool_outcome_reused", "tool_dispatch_uncertain", "run_failed", "tool_finished"]);
        req.filter.tool_id = Some("shell".into());
        let filtered = project_run_diagnostics(&events, &req).unwrap();
        assert_eq!(filtered.selected_event_count, 3);
        assert_eq!(filtered.total_event_count, 8);
        assert_eq!(filtered.source_heads.len(), 2);
        assert!(!filtered.events.iter().any(|e| e.event_kind == "run_failed"));
        req.filter.tool_id = Some("shel".into());
        assert!(project_run_diagnostics(&events, &req).unwrap().events.is_empty());
    }

    #[test]
    fn run_diagnostics_projection_unknown_duration_and_deterministic_order() {
        let mut events = chain(33, vec![(0, start("ambiguous", "shell")), (1, start("ambiguous", "shell")), (10, finish("ambiguous", "shell", true))]);
        events.extend(chain(32, vec![(0, start("reverse", "shell")), (-1, finish("reverse", "shell", true)), (20, finish("missing", "shell", true))]));
        events.extend(chain(34, vec![(-10, Kind::RunCreated { mode: RunModeV4::Plan })]));
        events.reverse();
        let snapshot = project_run_diagnostics(&events, &request()).unwrap();
        assert_eq!(snapshot.events.iter().map(|e| (e.run_id.as_u128(), e.sequence)).collect::<Vec<_>>(), vec![(34, 1), (32, 1), (32, 2), (32, 3), (33, 1), (33, 2), (33, 3)]);
        assert!(snapshot.events.iter().all(|e| e.duration_ms.is_none()));
        assert_eq!(snapshot.source_heads.iter().map(|h| h.run_id.as_u128()).collect::<Vec<_>>(), vec![32, 33, 34]);
        let mut req = request();
        req.filter.run_id = Some(Uuid::from_u128(34));
        let filtered = project_run_diagnostics(&events, &req).unwrap();
        assert_eq!(filtered.source_heads, snapshot.source_heads);
        assert_eq!(filtered.selected_event_count, 1);
        assert!(filtered.filtered);
        let singleton = project_run_diagnostics(&events[1..2], &req);
        assert!(singleton.is_err()); // Partial chain must not become a validated snapshot.
    }

    #[test]
    fn run_diagnostics_projection_usage_is_per_run_qualified_and_filter_independent() {
        let mut events = chain(30, vec![(0, attempt(100)), (1, usage(100, 9, "cumulative")), (2, usage(100, 9, "cumulative")), (3, attempt(101)), (4, attempt(101)), (5, finish("x", "shell", false))]);
        events.extend(chain(31, vec![(0, attempt(100)), (1, usage(100, 77, "unknown")), (2, attempt(102))]));
        events.extend(chain(32, vec![(0, attempt(100))]));
        let snapshot = project_run_diagnostics(&events, &request()).unwrap();
        assert_eq!(snapshot.usage[0].totals.input_tokens.known, Some(9));
        assert_eq!(snapshot.usage[0].totals.input_tokens.incomplete_attempts, 1);
        assert_eq!(snapshot.usage[0].totals.observed_attempts, 1);
        assert_eq!(snapshot.usage[0].missing_observation_attempts, 1);
        assert_eq!(snapshot.usage[1].totals.unknown_attempts, 1);
        assert_eq!(snapshot.usage[1].missing_observation_attempts, 1);
        assert_eq!(snapshot.usage[2].totals.input_tokens.known, None);
        assert_eq!(snapshot.usage[2].missing_observation_attempts, 1);
        let mut req = request();
        req.filter.tool_id = Some("shell".into());
        req.filter.failures_only = true;
        assert_eq!(project_run_diagnostics(&events, &req).unwrap().usage, snapshot.usage);
        req.filter.run_id = Some(Uuid::from_u128(31));
        assert_eq!(project_run_diagnostics(&events, &req).unwrap().usage, vec![snapshot.usage[1].clone()]);
    }

    #[test]
    fn run_diagnostics_projection_excludes_private_payloads_and_redacts_before_truncation() {
        let label = format!("{} password=PRIVATE_SENTINEL token=PRIVATE_SENTINEL api_key=PRIVATE_SENTINEL Authorization: Bearer PRIVATE_SENTINEL\n-----BEGIN PRIVATE KEY-----\nPRIVATE_SENTINEL\n-----END PRIVATE KEY-----\nCUSTOM_LITERAL", "界".repeat(70));
        let events = chain(30, vec![
            (0, Kind::ModelText { text: "PRIVATE_SENTINEL".into() }),
            (1, Kind::ToolRequested { call: omicsops_protocol::ToolCallV4 { call_id: "PRIVATE_SENTINEL".into(), tool_id: label.clone(), arguments: json!({"private": "PRIVATE_SENTINEL"}) } }),
            (2, finish("PRIVATE_SENTINEL", &label, false)),
            (3, Kind::UserInputAnswered { question_id: "PRIVATE_SENTINEL".into(), answer: "PRIVATE_SENTINEL".into() }),
            (4, Kind::GuidanceConsumed { message_id: Uuid::from_u128(9), markdown: "PRIVATE_SENTINEL".into() }),
            (5, Kind::RunFailed { message: "PRIVATE_SENTINEL provider/SSH error".into() }),
        ]);
        let mut req = request();
        req.redaction_keywords = vec!["CUSTOM_LITERAL".into()];
        let snapshot = project_run_diagnostics(&events, &req).unwrap();
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("PRIVATE_SENTINEL"));
        assert!(!serialized.contains("CUSTOM_LITERAL"));
        assert!(!serialized.contains("model_content"));
        assert!(!serialized.contains("arguments"));
        assert!(!serialized.contains("redaction_keywords"));
        assert!(snapshot.events[1].tool_label.as_ref().unwrap().len() <= 256);
        assert_eq!(snapshot.events[0].event_kind, "model_text");
        assert_eq!(snapshot.events[0].event_hash, events[0].event_hash);
        for label in ["password=PRIVATE_SENTINEL", "token=PRIVATE_SENTINEL", "api_key=PRIVATE_SENTINEL", "Authorization: Bearer PRIVATE_SENTINEL", "-----BEGIN PRIVATE KEY-----\nPRIVATE_SENTINEL", "CUSTOM_LITERAL"] {
            let mut query = req.clone();
            query.filter.tool_id = Some(label.into());
            let safe = project_run_diagnostics(&chain(40, vec![(0, start("PRIVATE_SENTINEL", label))]), &query).unwrap();
            let text = serde_json::to_string(&safe).unwrap();
            assert!(!text.contains("PRIVATE_SENTINEL"));
            assert!(!text.contains("CUSTOM_LITERAL"));
        }
    }

    #[test]
    fn run_diagnostics_projection_private_replay_plan_progress_and_errors_are_metadata_only() {
        let private = "PRIVATE_SENTINEL";
        let replay = json!({"kind": "model_replay_recorded", "replay": {
            "logical_request_id": Uuid::from_u128(1), "attempt_id": Uuid::from_u128(2),
            "binding": {"model_profile_id": Uuid::from_u128(3), "configuration_hash": private},
            "continuation": {"items": [
                {"kind": "assistant_text", "text": private},
                {"kind": "responses_reasoning", "id": private, "encrypted_content": private},
                {"kind": "tool_call", "call": {"call_id": private, "tool_id": private, "arguments": {"secret": private}}},
                {"kind": "tool_result", "call_id": private, "output": private}
            ]}
        }});
        let kinds = vec![
            replay,
            json!({"kind": "plan_proposed", "plan_hash": private, "plan": {"schema_version": 4, "objective": private, "steps": [private], "completion_criteria": [private], "requested_capabilities": [private]}}),
            json!({"kind": "task_list_updated", "revision": 1, "change_summary": private, "tasks": []}),
            json!({"kind": "task_shape_selected", "task_shape": "fast", "source": "model", "reason": private}),
            json!({"kind": "input_requested", "question_id": private, "question": private}),
            json!({"kind": "context_checkpointed", "checkpoint": {"schema_version": 4, "through_sequence": 1, "completion_criteria": [private], "unresolved_errors": [private], "recent_steps": [private], "scientific_state": {"secret": private}}}),
            json!({"kind": "model_retrying", "attempt": 1, "class": "transport", "message": private}),
            json!({"kind": "tool_dispatch_resolved", "call_id": private, "resolution": "side_effect_observed", "evidence": private}),
        ];
        let events = chain(30, kinds.into_iter().enumerate().map(|(i, value)| (i as i64, serde_json::from_value(value).unwrap())).collect());
        let snapshot = project_run_diagnostics(&events, &request()).unwrap();
        assert_eq!(snapshot.events.len(), 8);
        let serialized = serde_json::to_value(&snapshot).unwrap();
        assert!(!serialized.to_string().contains(private));
        let expected_keys = ["run_id", "sequence", "event_hash", "previous_hash", "occurred_at", "event_kind", "status", "tool_label", "call_ref", "duration_ms"].into_iter().collect::<BTreeSet<_>>();
        for row in serialized["events"].as_array().unwrap() {
            assert_eq!(row.as_object().unwrap().keys().map(String::as_str).collect::<BTreeSet<_>>(), expected_keys);
        }
        let mut req = request();
        req.filter.failures_only = true;
        let failed = project_run_diagnostics(&events, &req).unwrap();
        assert_eq!(failed.selected_event_count, 1);
        assert_eq!(failed.events[0].event_kind, "model_retrying");
    }

    #[test]
    fn run_diagnostics_projection_validates_hidden_chains_and_scope_before_filtering() {
        let visible = chain(30, vec![(0, Kind::RunCreated { mode: RunModeV4::Plan })]);
        let hidden = chain(31, vec![(0, Kind::ModelText { text: "PRIVATE_SENTINEL".into() })]);
        let mut req = request();
        req.filter.run_id = Some(Uuid::from_u128(30));
        for corruption in 0..4 {
            let mut events = visible.clone();
            let mut bad = hidden[0].clone();
            match corruption {
                0 => bad.event_hash = "PRIVATE_SENTINEL".into(),
                1 => bad.previous_hash = "PRIVATE_SENTINEL".into(),
                2 => bad.project_id = Uuid::from_u128(99),
                _ => bad.sequence = 2,
            }
            events.push(bad);
            assert_eq!(project_run_diagnostics(&events, &req).unwrap_err(), "diagnostics_source_invalid");
        }
        let selected = project_run_diagnostics(&visible, &req).unwrap();
        assert!(selected.filtered);
        assert_eq!(selected.total_event_count, selected.selected_event_count);
        let empty = project_run_diagnostics(&[], &request()).unwrap();
        assert!(empty.source_chain_validated);
        assert!(!empty.filtered);
        assert!(empty.events.is_empty() && empty.source_heads.is_empty() && empty.usage.is_empty());
    }

    #[test]
    fn run_diagnostics_projection_request_limits_use_utf8_bytes_and_safe_errors() {
        let mut req = request();
        req.filter.tool_id = Some("界".repeat(85) + "a");
        assert!(project_run_diagnostics(&[], &req).is_ok());
        req.filter.tool_id.as_mut().unwrap().push('b');
        assert_eq!(project_run_diagnostics(&[], &req).unwrap_err(), "diagnostics_invalid_request");
        for keywords in [vec![String::new()], vec!["x".into(); 33], vec!["界".repeat(342)], vec!["x".repeat(1024); 17]] {
            let mut req = request();
            req.redaction_keywords = keywords;
            assert_eq!(project_run_diagnostics(&[], &req).unwrap_err(), "diagnostics_invalid_request");
        }
        for keywords in [vec!["x".into(); 32], vec!["x".repeat(1024); 16], vec!["界".repeat(341) + "a"]] {
            let mut req = request();
            req.redaction_keywords = keywords;
            assert!(project_run_diagnostics(&[], &req).is_ok());
        }
    }

    #[test]
    fn run_diagnostics_projection_source_and_serialized_output_budgets_are_enforced() {
        let mut events = chain(30, (0..10_000).map(|i| (i, Kind::ModelText { text: String::new() })).collect());
        assert_eq!(project_run_diagnostics(&events, &request()).unwrap().selected_event_count, 10_000);
        let next = AgentEventV4::next(events.last().unwrap(), events.last().unwrap().occurred_at, Kind::ModelText { text: String::new() });
        events.push(next);
        let mut req = request();
        req.filter.run_id = Some(Uuid::from_u128(99));
        assert_eq!(project_run_diagnostics(&events, &req).unwrap_err(), "diagnostics_source_limit");
        let oversized = chain(30, vec![(0, Kind::ModelText { text: "x".repeat(16 * 1024 * 1024) })]);
        assert_eq!(project_run_diagnostics(&oversized, &req).unwrap_err(), "diagnostics_source_limit");
        // JSON expands each control byte to six bytes. The raw chain fits 16 MiB,
        // but even a 256-byte label can exceed the 8 MiB public serialization cap.
        let events = chain(30, (0..4_500).map(|i| (i, start("same", &"\u{1}".repeat(256)))).collect());
        assert!(events.iter().map(|e| serde_json::to_vec(e).unwrap().len()).sum::<usize>() < 16 * 1024 * 1024);
        assert_eq!(project_run_diagnostics(&events, &request()).unwrap_err(), "diagnostics_projection_limit");
    }

    #[test]
    fn run_diagnostics_projection_ambiguous_call_identity_cannot_attribute_resolutions() {
        let events = chain(30, vec![(0, start("same", "shell")), (1, finish("same", "read", false)), (2, Kind::ToolDispatchResolved { call_id: "same".into(), resolution: omicsops_protocol::UncertainResolutionV4::SideEffectObserved, evidence: "PRIVATE_SENTINEL".into() })]);
        let snapshot = project_run_diagnostics(&events, &request()).unwrap();
        assert_eq!(snapshot.events[1].duration_ms, None);
        assert_eq!(snapshot.events[2].tool_label, None);
        let mut req = request();
        req.filter.tool_id = Some("shell".into());
        assert_eq!(project_run_diagnostics(&events, &req).unwrap().selected_event_count, 1);
        let duplicate = chain(30, vec![(0, start("same", "shell")), (1, finish("same", "shell", true)), (2, finish("same", "shell", true))]);
        assert!(project_run_diagnostics(&duplicate, &request()).unwrap().events.iter().all(|row| row.duration_ms.is_none()));
        let late_start = chain(30, vec![(1, finish("same", "shell", true)), (0, start("same", "shell"))]);
        assert!(project_run_diagnostics(&late_start, &request()).unwrap().events.iter().all(|row| row.duration_ms.is_none()));
    }

    async fn stored_fixture() -> (Store, RunDiagnosticsRequestV4) {
        use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
        let store = Store::open_in_memory().await.unwrap();
        let req = request();
        store.save_project(&Project::new(req.project_id, "diagnostics", "C:\\diagnostics", ProjectTemplate::Blank, Utc::now())).await.unwrap();
        store.save_conversation(&Conversation::new(req.conversation_id, req.project_id, "diagnostics", Utc::now())).await.unwrap();
        (store, req)
    }

    #[tokio::test]
    async fn run_diagnostics_projection_loader_is_read_only_and_rejects_hidden_corruption() {
        let (store, mut req) = stored_fixture().await;
        assert!(load_run_diagnostics(&store, &req).await.unwrap().events.is_empty());
        let events = chain(30, vec![(0, start("same", "shell")), (1, finish("same", "shell", true))]);
        for event in &events { store.append_agent_event_v4(event).await.unwrap(); }
        let hidden = chain(31, vec![(0, Kind::ModelText { text: "PRIVATE_SENTINEL".into() })]);
        store.append_agent_event_v4(&hidden[0]).await.unwrap();
        let before: Vec<(String, String)> = sqlx::query_as("SELECT run_id,value_json FROM agent_runs_v4 ORDER BY run_id").fetch_all(store.pool()).await.unwrap();
        req.filter.run_id = Some(Uuid::from_u128(30));
        let snapshot = load_run_diagnostics(&store, &req).await.unwrap();
        assert_eq!(snapshot.selected_event_count, 2);
        assert_eq!(snapshot.source_heads.len(), 2);
        sqlx::query("UPDATE agent_events_v4 SET event_hash='invalid' WHERE run_id=?1").bind(hidden[0].run_id.to_string()).execute(store.pool()).await.unwrap();
        assert_eq!(load_run_diagnostics(&store, &req).await.unwrap_err(), "diagnostics_source_invalid");
        req.project_id = Uuid::from_u128(99);
        assert_eq!(load_run_diagnostics(&store, &req).await.unwrap_err(), "diagnostics_scope_invalid");
        let after: Vec<(String, String)> = sqlx::query_as("SELECT run_id,value_json FROM agent_runs_v4 ORDER BY run_id").fetch_all(store.pool()).await.unwrap();
        assert_eq!(before, after);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events_v4").fetch_one(store.pool()).await.unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn run_diagnostics_projection_loader_checks_raw_utf8_budget_before_filtering() {
        let (store, mut req) = stored_fixture().await;
        let events = chain(30, vec![(0, Kind::ModelText { text: "界".repeat(16 * 1024 * 1024 / 3) })]);
        store.append_agent_event_v4(&events[0]).await.unwrap();
        req.filter.run_id = Some(Uuid::from_u128(99));
        assert_eq!(load_run_diagnostics(&store, &req).await.unwrap_err(), "diagnostics_source_limit");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events_v4").fetch_one(store.pool()).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn run_diagnostics_projection_loader_preserves_verified_legacy_event_hash() {
        let (store, req) = stored_fixture().await;
        let event = chain(30, vec![(0, serde_json::from_value(json!({"kind": "input_requested", "question_id": "q", "question": "PRIVATE_SENTINEL"})).unwrap())]).remove(0);
        store.append_agent_event_v4(&event).await.unwrap();
        // Older persisted events omitted the additive defaulted reason field.
        // Their original JSON hash remains valid and must remain the locator.
        let mut value = serde_json::to_value(&event).unwrap();
        value["event"].as_object_mut().unwrap().remove("reason");
        value.as_object_mut().unwrap().remove("event_hash");
        let original_hash = hex::encode(Sha256::digest(serde_json::to_vec(&value).unwrap()));
        value["event_hash"] = json!(original_hash);
        sqlx::query("UPDATE agent_events_v4 SET value_json=?1,event_hash=?2 WHERE run_id=?3")
            .bind(value.to_string()).bind(&original_hash).bind(event.run_id.to_string()).execute(store.pool()).await.unwrap();
        assert!(store.agent_events_for_context_v4_bounded(req.project_id, req.conversation_id, 10_000, 16 * 1024 * 1024).await.is_ok());
        let snapshot = load_run_diagnostics(&store, &req).await.unwrap();
        assert_eq!(snapshot.events[0].event_hash, original_hash);
        assert!(snapshot.source_chain_validated);
    }

    #[tokio::test]
    async fn run_diagnostics_projection_loader_enforces_entire_conversation_event_limit() {
        let (store, mut req) = stored_fixture().await;
        let events = chain(30, (0..10_001).map(|i| (i, Kind::ModelText { text: String::new() })).collect());
        store.append_agent_event_v4(&events[0]).await.unwrap();
        let mut tx = store.pool().begin().await.unwrap();
        for event in &events[1..10_000] {
            sqlx::query("INSERT INTO agent_events_v4(run_id,project_id,conversation_id,sequence,previous_hash,event_hash,value_json) VALUES(?1,?2,?3,?4,?5,?6,?7)")
                .bind(event.run_id.to_string()).bind(event.project_id.to_string()).bind(event.conversation_id.to_string())
                .bind(event.sequence as i64).bind(&event.previous_hash).bind(&event.event_hash).bind(serde_json::to_string(event).unwrap()).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        req.filter.run_id = Some(Uuid::from_u128(99));
        let snapshot = load_run_diagnostics(&store, &req).await.unwrap();
        assert_eq!(snapshot.total_event_count, 10_000);
        assert_eq!(snapshot.selected_event_count, 0);
        store.append_agent_event_v4(&events[10_000]).await.unwrap();
        assert_eq!(load_run_diagnostics(&store, &req).await.unwrap_err(), "diagnostics_source_limit");
    }
}
