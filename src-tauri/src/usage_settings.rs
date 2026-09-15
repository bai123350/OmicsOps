use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, TimeZone, Utc};
use omicsops_core::workspace::ModelProfile;
use omicsops_dto::{
    UsageAggregatePage, UsageConversationPage, UsageConversationRow, UsageDay, UsageFilter,
    UsageGroup, UsageTool,
};
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, ModelRequestStartedV4, ModelUsageObservationV4,
    UsageAggregationV4, UsageObservationStateV4, UsageTotalsV4,
};
use omicsops_store::{Store, UsageEventRun, UsageSnapshotBoundary};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;

const CURSOR_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy)]
struct NormalizedFilter {
    project_id: Option<Uuid>,
    from_ms: Option<i64>,
    until_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageCursor {
    version: u8,
    kind: String,
    snapshot_rowid: i64,
    snapshot_event_hash: String,
    snapshot_event_count: i64,
    snapshot_at_ms: i64,
    filter_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after_activity_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after_conversation_id: Option<Uuid>,
}

#[derive(Default)]
struct Projection {
    totals: UsageTotalsV4,
    projects: BTreeMap<String, UsageTotalsV4>,
    models: BTreeMap<String, (String, UsageTotalsV4)>,
    days: BTreeMap<String, (u32, u32, UsageTotalsV4)>,
    tools: BTreeMap<String, UsageTool>,
    unattributed_events: u32,
    partial: bool,
}

#[derive(Default)]
struct Attempt {
    start: Option<(DateTime<Utc>, ModelRequestStartedV4)>,
    observations: Vec<(DateTime<Utc>, ModelUsageObservationV4)>,
    identity_conflict: bool,
}

#[derive(Clone)]
struct ToolDispatch {
    tool_id: String,
    at: DateTime<Utc>,
    succeeded: Option<bool>,
    uncertain: bool,
}

#[tauri::command]
pub async fn settings_usage_page(
    state: State<'_, AppState>,
    filter: UsageFilter,
    cursor: Option<String>,
) -> Result<UsageAggregatePage, String> {
    settings_usage_page_for_repository(&state.repository, filter, cursor).await
}

#[tauri::command]
pub async fn settings_usage_conversations(
    state: State<'_, AppState>,
    filter: UsageFilter,
    cursor: Option<String>,
) -> Result<UsageConversationPage, String> {
    settings_usage_conversations_for_repository(&state.repository, filter, cursor).await
}

pub async fn settings_usage_page_for_repository(
    repository: &Store,
    filter: UsageFilter,
    cursor: Option<String>,
) -> Result<UsageAggregatePage, String> {
    let normalized = normalize_filter(repository, &filter).await?;
    let filter_hash = filter_hash(normalized);
    let (boundary, after_run_id) = match cursor {
        Some(cursor) => {
            let cursor = decode_cursor(&cursor, "aggregate", &filter_hash)?;
            (boundary_from_cursor(&cursor), cursor.after_run_id)
        }
        None => (
            repository
                .usage_snapshot_boundary()
                .await
                .map_err(safe_store_error)?,
            None,
        ),
    };
    let page = repository
        .usage_event_page(
            normalized.project_id,
            normalized.from_ms,
            normalized.until_ms,
            after_run_id,
            &boundary,
            50,
        )
        .await
        .map_err(safe_store_error)?;
    let projects = repository.list_projects().await.map_err(safe_store_error)?;
    let project_labels = projects
        .into_iter()
        .map(|project| (project.id, project.name))
        .collect();
    let profiles = repository
        .list_model_profiles()
        .await
        .map_err(safe_store_error)?;
    let projection = project_runs(&page.runs, normalized, &project_labels, &profiles);
    let next_cursor = if page.has_more {
        page.last_run_id.map(|after_run_id| {
            encode_cursor(UsageCursor {
                version: CURSOR_VERSION,
                kind: "aggregate".into(),
                snapshot_rowid: boundary.rowid,
                snapshot_event_hash: boundary.event_hash.clone(),
                snapshot_event_count: boundary.event_count,
                snapshot_at_ms: boundary.snapshot_at_ms,
                filter_hash,
                after_run_id: Some(after_run_id),
                after_activity_ms: None,
                after_conversation_id: None,
            })
        })
    } else {
        None
    };
    let incomplete_totals = totals_incomplete(&projection.totals);
    Ok(UsageAggregatePage {
        totals: projection.totals,
        projects: projection
            .projects
            .into_iter()
            .map(|(key, totals)| UsageGroup {
                label: project_labels
                    .get(&Uuid::parse_str(&key).unwrap_or_default())
                    .cloned()
                    .unwrap_or_else(|| key.clone()),
                key,
                totals,
            })
            .collect(),
        models: projection
            .models
            .into_iter()
            .map(|(key, (label, totals))| UsageGroup { key, label, totals })
            .collect(),
        days: projection
            .days
            .into_iter()
            .map(|(date, (attempts, tools, totals))| UsageDay {
                date,
                attempts,
                tools,
                totals,
            })
            .collect(),
        tools: projection.tools.into_values().collect(),
        next_cursor,
        scanned_runs: u32::try_from(page.runs.len())
            .unwrap_or(u32::MAX)
            .saturating_add(page.omitted_runs),
        omitted_runs: page.omitted_runs,
        unattributed_events: projection.unattributed_events,
        snapshot_at: timestamp_rfc3339(boundary.snapshot_at_ms)?,
        completeness: if page.omitted_runs > 0 || projection.partial || incomplete_totals {
            "partial"
        } else {
            "complete"
        }
        .into(),
    })
}

pub async fn settings_usage_conversations_for_repository(
    repository: &Store,
    filter: UsageFilter,
    cursor: Option<String>,
) -> Result<UsageConversationPage, String> {
    let normalized = normalize_filter(repository, &filter).await?;
    let filter_hash = filter_hash(normalized);
    let (boundary, after) = match cursor {
        Some(cursor) => {
            let cursor = decode_cursor(&cursor, "conversations", &filter_hash)?;
            let after = match (cursor.after_activity_ms, cursor.after_conversation_id) {
                (Some(at), Some(id)) => Some((at, id)),
                _ => return Err("usage cursor is invalid".into()),
            };
            (boundary_from_cursor(&cursor), after)
        }
        None => (
            repository
                .usage_snapshot_boundary()
                .await
                .map_err(safe_store_error)?,
            None,
        ),
    };
    let page = repository
        .usage_conversation_event_page(
            normalized.project_id,
            normalized.from_ms,
            normalized.until_ms,
            after,
            &boundary,
            20,
        )
        .await
        .map_err(safe_store_error)?;
    let project_labels = repository
        .list_projects()
        .await
        .map_err(safe_store_error)?
        .into_iter()
        .map(|project| (project.id, project.name))
        .collect();
    let profiles = repository
        .list_model_profiles()
        .await
        .map_err(safe_store_error)?;
    let items = page
        .items
        .into_iter()
        .map(|item| {
            let projection = project_runs(&item.runs, normalized, &project_labels, &profiles);
            Ok(UsageConversationRow {
                project_id: item.project_id,
                conversation_id: item.conversation_id,
                label: item.label,
                latest_activity: timestamp_rfc3339(item.latest_activity_ms)?,
                incomplete: item.incomplete
                    || projection.partial
                    || totals_incomplete(&projection.totals),
                totals: projection.totals,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let next_cursor = if page.has_more {
        match (page.last_activity_ms, page.last_conversation_id) {
            (Some(after_activity_ms), Some(after_conversation_id)) => {
                Some(encode_cursor(UsageCursor {
                    version: CURSOR_VERSION,
                    kind: "conversations".into(),
                    snapshot_rowid: boundary.rowid,
                    snapshot_event_hash: boundary.event_hash.clone(),
                    snapshot_event_count: boundary.event_count,
                    snapshot_at_ms: boundary.snapshot_at_ms,
                    filter_hash,
                    after_run_id: None,
                    after_activity_ms: Some(after_activity_ms),
                    after_conversation_id: Some(after_conversation_id),
                }))
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(UsageConversationPage {
        items,
        next_cursor,
        snapshot_at: timestamp_rfc3339(boundary.snapshot_at_ms)?,
    })
}

fn project_runs(
    runs: &[UsageEventRun],
    filter: NormalizedFilter,
    _project_labels: &BTreeMap<Uuid, String>,
    profiles: &[ModelProfile],
) -> Projection {
    let mut projection = Projection::default();
    for run in runs {
        let mut attempts = BTreeMap::<(Uuid, Uuid), Attempt>::new();
        let mut dispatches = BTreeMap::<String, ToolDispatch>::new();
        for row in &run.events {
            let Ok(event) = serde_json::from_str::<AgentEventV4>(&row.value_json) else {
                projection.unattributed_events = projection.unattributed_events.saturating_add(1);
                projection.partial = true;
                continue;
            };
            if event.run_id != row.run_id
                || event.project_id != row.project_id
                || event.conversation_id != row.conversation_id
                || event.sequence != row.sequence
                || event.occurred_at.timestamp_millis() != row.occurred_at_ms
            {
                projection.unattributed_events = projection.unattributed_events.saturating_add(1);
                projection.partial = true;
                continue;
            }
            match event.event {
                AgentEventKindV4::ModelRequestStarted { request } => {
                    let key = (request.logical_request_id, request.attempt_id);
                    let attempt = attempts.entry(key).or_default();
                    if let Some((_, existing)) = &attempt.start {
                        if existing.model_profile_id != request.model_profile_id
                            || existing.model_configuration_hash != request.model_configuration_hash
                        {
                            attempt.identity_conflict = true;
                        }
                    } else {
                        attempt.start = Some((event.occurred_at, request));
                    }
                }
                AgentEventKindV4::ModelUsageObserved { observation } => {
                    let key = (observation.logical_request_id, observation.attempt_id);
                    let attempt = attempts.entry(key).or_default();
                    let identity = attempt
                        .start
                        .as_ref()
                        .map(|(_, request)| {
                            (
                                request.model_profile_id,
                                request.model_configuration_hash.as_deref(),
                            )
                        })
                        .or_else(|| {
                            attempt.observations.first().map(|(_, value)| {
                                (
                                    value.model_profile_id,
                                    value.model_configuration_hash.as_deref(),
                                )
                            })
                        });
                    if identity.is_some_and(|current| {
                        current
                            != (
                                observation.model_profile_id,
                                observation.model_configuration_hash.as_deref(),
                            )
                    }) {
                        attempt.identity_conflict = true;
                    }
                    attempt.observations.push((event.occurred_at, observation));
                }
                AgentEventKindV4::ToolDispatchStarted {
                    call_id, tool_id, ..
                } => {
                    dispatches.entry(call_id).or_insert(ToolDispatch {
                        tool_id,
                        at: event.occurred_at,
                        succeeded: None,
                        uncertain: false,
                    });
                }
                AgentEventKindV4::ToolFinished { outcome } => {
                    if let Some(dispatch) = dispatches.get_mut(&outcome.call_id) {
                        if dispatch.tool_id == outcome.tool_id {
                            dispatch.succeeded = Some(outcome.succeeded);
                        } else {
                            projection.unattributed_events =
                                projection.unattributed_events.saturating_add(1);
                            projection.partial = true;
                        }
                    }
                }
                AgentEventKindV4::ToolDispatchUncertain { call_id, tool_id } => {
                    if let Some(dispatch) = dispatches.get_mut(&call_id) {
                        if dispatch.tool_id == tool_id {
                            dispatch.uncertain = true;
                        }
                    }
                }
                _ => {}
            }
        }

        for attempt in attempts.into_values() {
            let anchor = attempt
                .start
                .as_ref()
                .map(|(at, _)| *at)
                .or_else(|| attempt.observations.iter().map(|(at, _)| *at).min());
            let Some(anchor) = anchor else {
                projection.partial = true;
                continue;
            };
            if !time_in_filter(anchor.timestamp_millis(), filter) {
                continue;
            }
            let mut observations = attempt
                .observations
                .iter()
                .map(|(_, observation)| observation.clone())
                .collect::<Vec<_>>();
            if observations.is_empty() {
                if let Some((_, request)) = &attempt.start {
                    observations.push(unknown_observation(request));
                }
            }
            let totals = UsageTotalsV4::from_observations(observations.clone());
            if !merge_usage_totals(&mut projection.totals, &totals) {
                projection.partial = true;
            }
            let project_id = runs_project_id(run).unwrap_or_default();
            let project_key = project_id.to_string();
            if !merge_usage_totals(projection.projects.entry(project_key).or_default(), &totals) {
                projection.partial = true;
            }
            let (model_key, model_label) =
                model_identity(&attempt, &observations, attempt.identity_conflict, profiles);
            if attempt.identity_conflict {
                projection.unattributed_events = projection.unattributed_events.saturating_add(1);
                projection.partial = true;
            }
            let entry = projection
                .models
                .entry(model_key)
                .or_insert_with(|| (model_label, UsageTotalsV4::default()));
            if !merge_usage_totals(&mut entry.1, &totals) {
                projection.partial = true;
            }
            let day = anchor.date_naive().to_string();
            let day_entry = projection.days.entry(day).or_default();
            day_entry.0 = day_entry
                .0
                .checked_add(totals.observed_attempts)
                .unwrap_or_else(|| {
                    projection.partial = true;
                    u32::MAX
                });
            if !merge_usage_totals(&mut day_entry.2, &totals) {
                projection.partial = true;
            }
        }

        for dispatch in dispatches.into_values() {
            if !time_in_filter(dispatch.at.timestamp_millis(), filter) {
                continue;
            }
            let tool = projection
                .tools
                .entry(dispatch.tool_id.clone())
                .or_insert(UsageTool {
                    tool_id: dispatch.tool_id,
                    dispatched: 0,
                    succeeded: 0,
                    failed: 0,
                    uncertain: 0,
                });
            tool.dispatched = tool.dispatched.checked_add(1).unwrap_or_else(|| {
                projection.partial = true;
                u64::MAX
            });
            if dispatch.uncertain {
                tool.uncertain = tool.uncertain.checked_add(1).unwrap_or_else(|| {
                    projection.partial = true;
                    u64::MAX
                });
            } else if dispatch.succeeded == Some(true) {
                tool.succeeded = tool.succeeded.checked_add(1).unwrap_or_else(|| {
                    projection.partial = true;
                    u64::MAX
                });
            } else if dispatch.succeeded == Some(false) {
                tool.failed = tool.failed.checked_add(1).unwrap_or_else(|| {
                    projection.partial = true;
                    u64::MAX
                });
            } else {
                projection.partial = true;
            }
            let day = dispatch.at.date_naive().to_string();
            let day_entry = projection.days.entry(day).or_default();
            day_entry.1 = day_entry.1.checked_add(1).unwrap_or_else(|| {
                projection.partial = true;
                u32::MAX
            });
        }
    }
    for tool in projection.tools.values_mut() {
        // BTreeMap iteration already provides deterministic tool ranking ties;
        // the UI sorts by the observed dispatch count.
        tool.tool_id = tool.tool_id.trim().to_owned();
    }
    projection
}

fn runs_project_id(run: &UsageEventRun) -> Option<Uuid> {
    run.events.first().map(|event| event.project_id)
}

fn model_identity(
    attempt: &Attempt,
    observations: &[ModelUsageObservationV4],
    conflict: bool,
    profiles: &[ModelProfile],
) -> (String, String) {
    if conflict {
        return ("unknown".into(), "Unknown identity".into());
    }
    let identity = attempt
        .start
        .as_ref()
        .map(|(_, request)| {
            (
                request.model_profile_id,
                request.model_configuration_hash.clone(),
            )
        })
        .or_else(|| {
            observations.first().map(|value| {
                (
                    value.model_profile_id,
                    value.model_configuration_hash.clone(),
                )
            })
        });
    let Some((profile_id, hash)) = identity else {
        return ("unknown".into(), "Unknown identity".into());
    };
    let hash_label = hash
        .as_deref()
        .map(|value| value.chars().take(12).collect::<String>())
        .unwrap_or_else(|| "unknown".into());
    let key = format!("{profile_id}:{}", hash.as_deref().unwrap_or("unknown"));
    let label = profiles
        .iter()
        .find(|profile| {
            profile.id == profile_id
                && hash.as_deref() == Some(profile.execution_configuration_hash().as_str())
        })
        .map(|profile| format!("{} · {hash_label}", profile.label))
        .unwrap_or_else(|| format!("{profile_id} · {hash_label}"));
    (key, label)
}

fn unknown_observation(request: &ModelRequestStartedV4) -> ModelUsageObservationV4 {
    ModelUsageObservationV4 {
        logical_request_id: request.logical_request_id,
        attempt_id: request.attempt_id,
        sample_index: 0,
        model_profile_id: request.model_profile_id,
        model_configuration_hash: request.model_configuration_hash.clone(),
        state: UsageObservationStateV4::Interrupted,
        aggregation: UsageAggregationV4::Unknown,
        input_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        reported_total_tokens: None,
        context_tokens: None,
        context_limit_tokens: request.context_limit_tokens,
        context_limit_source: request.context_limit_source.clone(),
        serialized_request_bytes: request.serialized_request_bytes,
        image_bound_tokens: request.image_bound_tokens,
    }
}

fn merge_usage_totals(target: &mut UsageTotalsV4, source: &UsageTotalsV4) -> bool {
    let mut exact = true;
    macro_rules! counter {
        ($field:ident) => {{
            target.$field.known = match (target.$field.known, source.$field.known) {
                (Some(left), Some(right)) => left.checked_add(right).or_else(|| {
                    exact = false;
                    None
                }),
                (Some(value), None) | (None, Some(value)) => Some(value),
                (None, None) => None,
            };
            target.$field.incomplete_attempts = target
                .$field
                .incomplete_attempts
                .checked_add(source.$field.incomplete_attempts)
                .unwrap_or_else(|| {
                    exact = false;
                    u32::MAX
                });
        }};
    }
    counter!(input_tokens);
    counter!(output_tokens);
    counter!(reasoning_tokens);
    counter!(cache_read_input_tokens);
    counter!(cache_creation_input_tokens);
    counter!(reported_total_tokens);
    macro_rules! count {
        ($field:ident) => {
            target.$field = target.$field.checked_add(source.$field).unwrap_or_else(|| {
                exact = false;
                u32::MAX
            });
        };
    }
    count!(observed_attempts);
    count!(final_attempts);
    count!(partial_attempts);
    count!(interrupted_attempts);
    count!(unknown_attempts);
    exact
}

fn totals_incomplete(totals: &UsageTotalsV4) -> bool {
    totals.unknown_attempts > 0
        || totals.interrupted_attempts > 0
        || totals.final_attempts < totals.observed_attempts
        || [
            &totals.input_tokens,
            &totals.output_tokens,
            &totals.reasoning_tokens,
            &totals.cache_read_input_tokens,
            &totals.cache_creation_input_tokens,
            &totals.reported_total_tokens,
        ]
        .iter()
        .any(|counter| counter.incomplete_attempts > 0)
}

async fn normalize_filter(
    repository: &Store,
    filter: &UsageFilter,
) -> Result<NormalizedFilter, String> {
    if let Some(project_id) = filter.project_id {
        let exists = repository
            .list_projects()
            .await
            .map_err(safe_store_error)?
            .into_iter()
            .any(|project| project.id == project_id);
        if !exists {
            return Err("usage project is unavailable".into());
        }
    }
    let from_ms = filter.from.as_deref().map(parse_timestamp).transpose()?;
    let until_ms = filter.until.as_deref().map(parse_timestamp).transpose()?;
    if from_ms
        .zip(until_ms)
        .is_some_and(|(from, until)| from >= until)
    {
        return Err("usage date range is invalid".into());
    }
    Ok(NormalizedFilter {
        project_id: filter.project_id,
        from_ms,
        until_ms,
    })
}

fn parse_timestamp(value: &str) -> Result<i64, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp_millis())
        .map_err(|_| "usage date is invalid".into())
}

fn timestamp_rfc3339(value: i64) -> Result<String, String> {
    Utc.timestamp_millis_opt(value)
        .single()
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| "usage timestamp is invalid".into())
}

fn time_in_filter(value: i64, filter: NormalizedFilter) -> bool {
    filter.from_ms.is_none_or(|from| value >= from)
        && filter.until_ms.is_none_or(|until| value < until)
}

fn filter_hash(filter: NormalizedFilter) -> String {
    let value = serde_json::json!({"project_id":filter.project_id,"from_ms":filter.from_ms,"until_ms":filter.until_ms});
    hex::encode(Sha256::digest(
        serde_json::to_vec(&value).expect("usage filter serializes"),
    ))
}

fn encode_cursor(cursor: UsageCursor) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(&cursor).expect("usage cursor serializes"))
}

fn decode_cursor(value: &str, kind: &str, filter_hash: &str) -> Result<UsageCursor, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "usage cursor is invalid")?;
    let cursor: UsageCursor =
        serde_json::from_slice(&bytes).map_err(|_| "usage cursor is invalid")?;
    if cursor.version != CURSOR_VERSION
        || cursor.kind != kind
        || cursor.filter_hash != filter_hash
        || cursor.snapshot_rowid < 0
        || cursor.snapshot_event_count < 0
    {
        return Err("usage cursor does not match this request".into());
    }
    Ok(cursor)
}

fn boundary_from_cursor(cursor: &UsageCursor) -> UsageSnapshotBoundary {
    UsageSnapshotBoundary {
        rowid: cursor.snapshot_rowid,
        event_hash: cursor.snapshot_event_hash.clone(),
        event_count: cursor.snapshot_event_count,
        snapshot_at_ms: cursor.snapshot_at_ms,
    }
}

fn safe_store_error(error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    if message.contains("history changed") {
        "usage history changed; refresh required".into()
    } else {
        "usage history is unavailable".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_protocol::{ContextLimitSourceV4, ToolEffectV4, ToolOutcomeV4};
    use omicsops_store::UsageEventRow;
    use serde_json::json;

    fn request(logical: u128, attempt: u128, profile: Uuid) -> ModelRequestStartedV4 {
        ModelRequestStartedV4 {
            logical_request_id: Uuid::from_u128(logical),
            attempt_id: Uuid::from_u128(attempt),
            model_profile_id: profile,
            model_configuration_hash: Some("a".repeat(64)),
            context_limit_tokens: None,
            context_limit_source: ContextLimitSourceV4::Unknown,
            serialized_request_bytes: None,
            image_count: None,
            image_bound_tokens: None,
            breakdown: None,
        }
    }

    fn observation(
        request: &ModelRequestStartedV4,
        sample_index: u32,
        state: UsageObservationStateV4,
        aggregation: UsageAggregationV4,
        input: Option<u64>,
        output: Option<u64>,
        reasoning: Option<u64>,
        cache: Option<u64>,
        total: Option<u64>,
    ) -> ModelUsageObservationV4 {
        ModelUsageObservationV4 {
            logical_request_id: request.logical_request_id,
            attempt_id: request.attempt_id,
            sample_index,
            model_profile_id: request.model_profile_id,
            model_configuration_hash: request.model_configuration_hash.clone(),
            state,
            aggregation,
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: reasoning,
            cache_read_input_tokens: cache,
            cache_creation_input_tokens: None,
            reported_total_tokens: total,
            context_tokens: None,
            context_limit_tokens: None,
            context_limit_source: ContextLimitSourceV4::Unknown,
            serialized_request_bytes: None,
            image_bound_tokens: None,
        }
    }

    fn event_run(events: Vec<(DateTime<Utc>, AgentEventKindV4)>) -> UsageEventRun {
        let run_id = Uuid::from_u128(501);
        let project_id = Uuid::from_u128(502);
        let conversation_id = Uuid::from_u128(503);
        let mut built = Vec::new();
        for (at, kind) in events {
            let event = if let Some(previous) = built.last() {
                AgentEventV4::next(previous, at, kind)
            } else {
                AgentEventV4::first(run_id, project_id, conversation_id, at, kind)
            };
            built.push(event);
        }
        UsageEventRun {
            run_id,
            events: built
                .into_iter()
                .enumerate()
                .map(|(index, event)| UsageEventRow {
                    rowid: index as i64 + 1,
                    run_id,
                    project_id,
                    conversation_id,
                    sequence: event.sequence,
                    occurred_at_ms: event.occurred_at.timestamp_millis(),
                    value_json: serde_json::to_string(&event).unwrap(),
                })
                .collect(),
        }
    }

    fn all_time() -> NormalizedFilter {
        NormalizedFilter {
            project_id: None,
            from_ms: None,
            until_ms: None,
        }
    }

    #[test]
    fn merges_cumulative_and_delta_attempts_without_double_counting_samples_or_facets() {
        let profile = Uuid::from_u128(9);
        let first = request(1, 11, profile);
        let retry = request(1, 12, profile);
        let at = Utc.with_ymd_and_hms(2026, 9, 15, 10, 0, 0).unwrap();
        let run = event_run(vec![
            (
                at,
                AgentEventKindV4::ModelRequestStarted {
                    request: first.clone(),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &first,
                        0,
                        UsageObservationStateV4::Partial,
                        UsageAggregationV4::Cumulative,
                        Some(10),
                        Some(2),
                        Some(1),
                        Some(4),
                        Some(12),
                    ),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &first,
                        1,
                        UsageObservationStateV4::Final,
                        UsageAggregationV4::Cumulative,
                        Some(20),
                        Some(3),
                        Some(2),
                        Some(8),
                        Some(23),
                    ),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &first,
                        1,
                        UsageObservationStateV4::Final,
                        UsageAggregationV4::Cumulative,
                        Some(999),
                        None,
                        None,
                        None,
                        None,
                    ),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelRequestStarted {
                    request: retry.clone(),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &retry,
                        0,
                        UsageObservationStateV4::Partial,
                        UsageAggregationV4::Delta,
                        Some(10),
                        None,
                        None,
                        None,
                        Some(10),
                    ),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &retry,
                        1,
                        UsageObservationStateV4::Final,
                        UsageAggregationV4::Delta,
                        Some(20),
                        None,
                        None,
                        None,
                        Some(20),
                    ),
                },
            ),
        ]);
        let projection = project_runs(&[run], all_time(), &BTreeMap::new(), &[]);
        assert_eq!(projection.totals.input_tokens.known, Some(50));
        assert_eq!(projection.totals.output_tokens.known, Some(3));
        assert_eq!(projection.totals.reasoning_tokens.known, Some(2));
        assert_eq!(projection.totals.cache_read_input_tokens.known, Some(8));
        assert_eq!(projection.totals.reported_total_tokens.known, Some(53));
        assert_eq!(projection.totals.observed_attempts, 2);
        let streamed_final = UsageTotalsV4 {
            observed_attempts: 1,
            final_attempts: 1,
            partial_attempts: 1,
            ..UsageTotalsV4::default()
        };
        assert!(!totals_incomplete(&streamed_final));
    }

    #[test]
    fn keeps_full_configuration_hashes_distinct_in_model_groups() {
        let profile = Uuid::from_u128(9);
        let mut first = request(8, 81, profile);
        let mut second = request(9, 91, profile);
        first.model_configuration_hash = Some(format!("{}{}", "a".repeat(12), "1".repeat(52)));
        second.model_configuration_hash = Some(format!("{}{}", "a".repeat(12), "2".repeat(52)));
        let at = Utc.with_ymd_and_hms(2026, 9, 15, 10, 0, 0).unwrap();
        let run = event_run(vec![
            (
                at,
                AgentEventKindV4::ModelRequestStarted {
                    request: first.clone(),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &first,
                        0,
                        UsageObservationStateV4::Final,
                        UsageAggregationV4::Cumulative,
                        Some(1),
                        None,
                        None,
                        None,
                        None,
                    ),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelRequestStarted {
                    request: second.clone(),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &second,
                        0,
                        UsageObservationStateV4::Final,
                        UsageAggregationV4::Cumulative,
                        Some(2),
                        None,
                        None,
                        None,
                        None,
                    ),
                },
            ),
        ]);

        let projection = project_runs(&[run], all_time(), &BTreeMap::new(), &[]);
        assert_eq!(projection.models.len(), 2);
        let keys = projection.models.keys().collect::<Vec<_>>();
        assert_ne!(keys[0], keys[1]);
    }

    #[test]
    fn shortens_malformed_non_ascii_hash_labels_without_panicking() {
        let profile = Uuid::from_u128(9);
        let mut malformed = request(10, 101, profile);
        malformed.model_configuration_hash = Some("aéé".repeat(10));
        let attempt = Attempt {
            start: Some((Utc::now(), malformed)),
            ..Attempt::default()
        };

        let (key, label) = model_identity(&attempt, &[], false, &[]);
        assert!(key.contains("aééaéé"));
        assert_eq!(label.chars().filter(|value| *value == 'é').count(), 8);
    }

    #[test]
    fn preserves_reported_zero_and_marks_a_started_attempt_without_usage_unknown() {
        let profile = Uuid::from_u128(9);
        let zero = request(2, 21, profile);
        let missing = request(3, 31, profile);
        let at = Utc.with_ymd_and_hms(2026, 9, 15, 10, 0, 0).unwrap();
        let run = event_run(vec![
            (
                at,
                AgentEventKindV4::ModelRequestStarted {
                    request: zero.clone(),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &zero,
                        0,
                        UsageObservationStateV4::Final,
                        UsageAggregationV4::Cumulative,
                        Some(0),
                        Some(0),
                        None,
                        None,
                        Some(0),
                    ),
                },
            ),
            (
                at,
                AgentEventKindV4::ModelRequestStarted { request: missing },
            ),
        ]);
        let projection = project_runs(&[run], all_time(), &BTreeMap::new(), &[]);
        assert_eq!(projection.totals.input_tokens.known, Some(0));
        assert_eq!(projection.totals.output_tokens.known, Some(0));
        assert_eq!(projection.totals.unknown_attempts, 1);
        assert_eq!(projection.totals.interrupted_attempts, 1);
        assert!(totals_incomplete(&projection.totals));
    }

    #[test]
    fn attributes_cross_midnight_usage_to_the_start_day_and_counts_only_real_dispatches() {
        let profile = Uuid::from_u128(9);
        let started = request(4, 41, profile);
        let before_midnight = Utc.with_ymd_and_hms(2026, 9, 14, 23, 59, 0).unwrap();
        let after_midnight = Utc.with_ymd_and_hms(2026, 9, 15, 0, 1, 0).unwrap();
        let outcome = ToolOutcomeV4 {
            call_id: "call-1".into(),
            tool_id: "project.read".into(),
            succeeded: true,
            model_content: "SENSITIVE_SENTINEL".into(),
            data: json!({"raw":"SENSITIVE_SENTINEL"}),
            provenance: vec![],
        };
        let run = event_run(vec![
            (
                before_midnight,
                AgentEventKindV4::ModelRequestStarted {
                    request: started.clone(),
                },
            ),
            (
                after_midnight,
                AgentEventKindV4::ModelUsageObserved {
                    observation: observation(
                        &started,
                        0,
                        UsageObservationStateV4::Final,
                        UsageAggregationV4::Cumulative,
                        Some(42),
                        None,
                        None,
                        None,
                        None,
                    ),
                },
            ),
            (
                before_midnight,
                AgentEventKindV4::ToolDispatchStarted {
                    call_id: "call-1".into(),
                    tool_id: "project.read".into(),
                    effect: ToolEffectV4::ReadOnly,
                    idempotency_key: "key".into(),
                },
            ),
            (
                before_midnight,
                AgentEventKindV4::ToolDispatchStarted {
                    call_id: "call-1".into(),
                    tool_id: "project.read".into(),
                    effect: ToolEffectV4::ReadOnly,
                    idempotency_key: "duplicate".into(),
                },
            ),
            (after_midnight, AgentEventKindV4::ToolFinished { outcome }),
        ]);
        let filter = NormalizedFilter {
            project_id: None,
            from_ms: Some(
                Utc.with_ymd_and_hms(2026, 9, 14, 0, 0, 0)
                    .unwrap()
                    .timestamp_millis(),
            ),
            until_ms: Some(
                Utc.with_ymd_and_hms(2026, 9, 15, 0, 0, 0)
                    .unwrap()
                    .timestamp_millis(),
            ),
        };
        let projection = project_runs(&[run], filter, &BTreeMap::new(), &[]);
        assert_eq!(projection.totals.input_tokens.known, Some(42));
        assert_eq!(projection.days["2026-09-14"].0, 1);
        assert_eq!(projection.tools["project.read"].dispatched, 1);
        assert_eq!(projection.tools["project.read"].succeeded, 1);
        assert!(
            !serde_json::to_string(&projection.tools.into_values().collect::<Vec<_>>())
                .unwrap()
                .contains("SENSITIVE_SENTINEL")
        );
    }

    #[test]
    fn rejects_a_cursor_when_the_filter_changes() {
        let filter = all_time();
        let cursor = encode_cursor(UsageCursor {
            version: CURSOR_VERSION,
            kind: "aggregate".into(),
            snapshot_rowid: 2,
            snapshot_event_hash: "hash".into(),
            snapshot_event_count: 2,
            snapshot_at_ms: 3,
            filter_hash: filter_hash(filter),
            after_run_id: Some(Uuid::from_u128(1)),
            after_activity_ms: None,
            after_conversation_id: None,
        });
        let changed = NormalizedFilter {
            project_id: Some(Uuid::from_u128(2)),
            from_ms: None,
            until_ms: None,
        };
        assert_eq!(
            decode_cursor(&cursor, "aggregate", &filter_hash(changed)).unwrap_err(),
            "usage cursor does not match this request"
        );
    }
}
