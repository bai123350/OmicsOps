use omicsops_protocol::*;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(crate) fn validated_continuation<'a>(
    request: &crate::ModelRequestV4,
    turn: &'a crate::ModelTurnV4,
) -> Result<&'a ModelProviderContinuationV4, String> {
    let continuation = turn
        .provider_continuation
        .as_ref()
        .ok_or("native provider omitted validated continuation")?;
    continuation.validate()?;
    let native_calls: Vec<_> = continuation
        .items
        .iter()
        .filter_map(|item| match item {
            ModelReplayItemV4::ToolCall { call } => Some(call),
            _ => None,
        })
        .collect();
    if native_calls != turn.tool_calls.iter().collect::<Vec<_>>() {
        return Err("provider continuation differs from host tool proposals".into());
    }
    for call in &turn.tool_calls {
        let descriptor = request
            .tools
            .iter()
            .find(|tool| tool.id == call.tool_id)
            .ok_or("native provider proposed an ungranted tool")?;
        crate::validate_json_schema_subset(&descriptor.input_schema, &call.arguments, "$")?;
        if request.replay.iter().any(|item|matches!(item,ModelReplayItemV4::ToolCall {call:previous} if previous.call_id==call.call_id)) { return Err("native provider reused a prior call identity".into()); }
    }
    let text: String = continuation
        .items
        .iter()
        .filter_map(|item| match item {
            ModelReplayItemV4::AssistantText { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if text != turn.public_text {
        return Err("provider continuation differs from final public text".into());
    }
    Ok(continuation)
}

pub fn project_model_replay(
    project_id: Uuid,
    conversation_id: Uuid,
    binding: &ModelReplayBindingV4,
    events: &[AgentEventV4],
) -> Result<Vec<ModelReplayItemV4>, String> {
    project_replay(project_id, conversation_id, binding, events, true)
}

fn project_replay(
    project_id: Uuid,
    conversation_id: Uuid,
    binding: &ModelReplayBindingV4,
    events: &[AgentEventV4],
    require_finished: bool,
) -> Result<Vec<ModelReplayItemV4>, String> {
    let mut previous: BTreeMap<Uuid, &AgentEventV4> = BTreeMap::new();
    let mut starts: BTreeMap<Uuid, &ModelRequestStartedV4> = BTreeMap::new();
    let mut recorded = BTreeSet::new();
    let mut proposed: BTreeMap<String, (Uuid, ToolCallV4, String)> = BTreeMap::new();
    let mut requested = BTreeSet::new();
    let mut finished = BTreeSet::new();
    let mut output = Vec::new();
    for (event_index, event) in events.iter().enumerate() {
        if event.project_id != project_id
            || event.conversation_id != conversation_id
            || event.verify().is_err()
        {
            return Err("model replay source identity or integrity mismatch".into());
        }
        if let Some(prior) = previous.insert(event.run_id, event) {
            if event.sequence != prior.sequence + 1 || event.previous_hash != prior.event_hash {
                return Err("model replay source chain is incomplete".into());
            }
        } else if event.sequence != 1 || !event.previous_hash.is_empty() {
            return Err("model replay source must include the complete chain".into());
        }
        match &event.event {
            AgentEventKindV4::ModelRequestStarted { request } => {
                starts.insert(event.run_id, request);
            }
            AgentEventKindV4::ModelReplayRecorded { replay } => {
                let start = starts
                    .get(&event.run_id)
                    .ok_or("model replay has no request origin")?;
                if &replay.binding != binding
                    || start.model_profile_id != binding.model_profile_id
                    || start.model_configuration_hash.as_deref()
                        != Some(binding.configuration_hash.as_str())
                    || start.logical_request_id != replay.logical_request_id
                    || start.attempt_id != replay.attempt_id
                    || !recorded.insert((event.run_id, replay.attempt_id))
                {
                    return Err("model replay attempt or frozen binding mismatch".into());
                }
                replay.continuation.validate()?;
                for item in &replay.continuation.items {
                    if let ModelReplayItemV4::ToolCall { call } = item {
                        if proposed
                            .insert(
                                call.call_id.clone(),
                                (event.run_id, call.clone(), event.event_hash.clone()),
                            )
                            .is_some()
                        {
                            return Err("model replay has an ambiguous call identity".into());
                        }
                    }
                    output.push(item.clone());
                }
            }
            AgentEventKindV4::ToolRequested { call } => {
                if let Some((origin_run, proposal, _)) = proposed.get(&call.call_id) {
                    let mut normalized = proposal.clone();
                    crate::bind_mcp_directory(&mut normalized, &events[..event_index]);
                    if *origin_run != event.run_id
                        || &normalized != call
                        || !requested.insert(call.call_id.clone())
                    {
                        return Err(
                            "host tool request does not match the model replay origin".into()
                        );
                    }
                } else if starts.get(&event.run_id).is_some_and(|start| {
                    start.model_profile_id == binding.model_profile_id
                        && start.model_configuration_hash.as_deref()
                            == Some(binding.configuration_hash.as_str())
                }) {
                    return Err("host tool request has no matching provider continuation".into());
                }
            }
            AgentEventKindV4::ToolFinished { outcome }
            | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => {
                if let Some((origin_run, proposal, record_hash)) = proposed.get(&outcome.call_id) {
                    let no_dispatch = !events.iter().any(|candidate| candidate.run_id == event.run_id && matches!(&candidate.event, AgentEventKindV4::ToolDispatchStarted { call_id, .. } if call_id == &outcome.call_id));
                    let closed_without_request = !outcome.succeeded
                        && no_dispatch
                        && outcome.data["error_kind"] == "model_replay_closure"
                        && outcome.data["closure"] == "proposal_not_dispatched"
                        && outcome.data["operation_dispatched"] == false
                        && outcome.data["source_event_hash"].as_str() == Some(record_hash.as_str());
                    let browser_rejected = !outcome.succeeded
                        && no_dispatch
                        && crate::is_browser_tool_id(&proposal.tool_id)
                        && outcome.data["error_kind"] == "browser_validation";
                    if *origin_run != event.run_id
                        || proposal.tool_id != outcome.tool_id
                        || (!requested.contains(&outcome.call_id)
                            && !closed_without_request
                            && !browser_rejected)
                        || !finished.insert(outcome.call_id.clone())
                    {
                        return Err(
                            "model replay contains an orphan or duplicate host result".into()
                        );
                    }
                    output.push(ModelReplayItemV4::ToolResult {
                        call_id: outcome.call_id.clone(),
                        output: serde_json::to_string(&crate::context_views::event_view(event))
                            .map_err(|_| "invalid host result projection")?,
                    });
                } else if starts.get(&event.run_id).is_some_and(|start| {
                    start.model_profile_id == binding.model_profile_id
                        && start.model_configuration_hash.as_deref()
                            == Some(binding.configuration_hash.as_str())
                }) {
                    return Err("model replay contains an unbound host result".into());
                }
            }
            _ => {}
        }
    }
    if require_finished && finished.len() != proposed.len() {
        return Err("model replay contains unfinished host tool calls".into());
    }
    Ok(output)
}

/// Persist host decisions from their verified source events before the next
/// request. Missing requests mean no dispatch; missing results after a recorded
/// request remain unresolved and must be reconciled by normal tool recovery.
pub(crate) fn pending_replay_closures(
    project_id: Uuid,
    conversation_id: Uuid,
    binding: &ModelReplayBindingV4,
    events: &[AgentEventV4],
) -> Result<Vec<ToolOutcomeV4>, String> {
    let replay = project_replay(project_id, conversation_id, binding, events, false)?;
    let finished: BTreeSet<_> = replay
        .iter()
        .filter_map(|item| match item {
            ModelReplayItemV4::ToolResult { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    let mut outcomes = Vec::new();
    for (origin_index, origin) in events.iter().enumerate() {
        let AgentEventKindV4::ModelReplayRecorded { replay } = &origin.event else {
            continue;
        };
        for item in &replay.continuation.items {
            let ModelReplayItemV4::ToolCall { call } = item else {
                continue;
            };
            if finished.contains(call.call_id.as_str()) {
                continue;
            }
            let tail = &events[origin_index + 1..];
            let requested = tail.iter().position(|e| e.run_id == origin.run_id && matches!(&e.event, AgentEventKindV4::ToolRequested { call: actual } if actual.call_id == call.call_id));
            let dispatched = tail.iter().any(|e| e.run_id == origin.run_id && matches!(&e.event, AgentEventKindV4::ToolDispatchStarted { call_id, .. } if call_id == &call.call_id));
            if dispatched {
                continue;
            }
            let mut source = origin;
            let mut closure = "proposal_not_dispatched";
            let mut succeeded = false;
            if let Some(request_index) = requested {
                let decision_events: Vec<_> = tail[request_index + 1..]
                    .iter()
                    .filter(|e| e.run_id == origin.run_id)
                    .take_while(|e| {
                        !matches!(e.event, AgentEventKindV4::ModelRequestStarted { .. })
                    })
                    .collect();
                match call.tool_id.as_str() {
                    "agent.request_input" => {
                        let Some(input_index) = decision_events.iter().position(|e| matches!(&e.event, AgentEventKindV4::InputRequested {question_id,question,..} if question_id == &call.call_id && call.arguments["question"].as_str() == Some(question.as_str()))) else {continue};
                        let answers: Vec<_> = decision_events[input_index+1..].iter().filter(|e| matches!(&e.event, AgentEventKindV4::UserInputAnswered {question_id,answer} if question_id == &call.call_id && !answer.trim().is_empty())).collect();
                        if answers.len() > 1 {
                            return Err("model replay has duplicate clarification answers".into());
                        }
                        let Some(answer) = answers.first() else {
                            continue;
                        };
                        source = answer;
                        closure = "user_input_answered";
                        succeeded = true;
                    }
                    "agent.complete" => {
                        let proposal: CompletionProposalV4 =
                            serde_json::from_value(call.arguments.clone())
                                .map_err(|_| "invalid replay completion proposal")?;
                        if let Some(submitted) = decision_events.iter().position(|e| matches!(&e.event, AgentEventKindV4::CompletionProposalSubmitted {proposal: actual} if actual == &proposal)) {
                            let Some(decision) = decision_events[submitted+1..].iter().rev().find(|e| matches!(e.event, AgentEventKindV4::DeterministicVerificationFinished { .. } | AgentEventKindV4::ReviewerFinished { .. } | AgentEventKindV4::ReviewerCorrectionRequested { .. })) else {continue};
                            source = decision;
                            closure = "completion_decision";
                        }
                    }
                    _ => continue,
                }
            }
            outcomes.push(ToolOutcomeV4 {
                call_id: call.call_id.clone(), tool_id:call.tool_id.clone(), succeeded,
                model_content: if closure == "proposal_not_dispatched" {"Host did not dispatch this proposal. No execution result exists; decide whether a new proposal is still needed.".into()} else {"Host coordinator decision; this is not scientific execution evidence.".into()},
                data: serde_json::json!({"error_kind":"model_replay_closure","closure":closure,"operation_dispatched":false,"recoverable":true,"source_event_hash":source.event_hash,"source":crate::context_views::event_view(source)}),
                provenance:vec![],
            });
        }
    }
    Ok(outcomes)
}

/// Filter only verified host event hashes represented in typed replay. Never derive
/// replay identities from narrative text or remove unrelated scientific state.
pub(crate) fn remove_replayed_context_events(
    context: &str,
    replay: &[ModelReplayItemV4],
    events: &[AgentEventV4],
) -> String {
    let calls: BTreeSet<_> = replay
        .iter()
        .filter_map(|item| match item {
            ModelReplayItemV4::ToolCall { call } => Some(call.call_id.as_str()),
            _ => None,
        })
        .collect();
    let texts: BTreeSet<_> = replay
        .iter()
        .filter_map(|item| match item {
            ModelReplayItemV4::AssistantText { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let hashes: BTreeSet<_> = events
        .iter()
        .filter(|event| match &event.event {
            AgentEventKindV4::ToolRequested { call } => calls.contains(call.call_id.as_str()),
            AgentEventKindV4::ToolFinished { outcome }
            | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => {
                calls.contains(outcome.call_id.as_str())
            }
            AgentEventKindV4::ModelText { text } => texts.contains(text.as_str()),
            _ => false,
        })
        .map(|event| event.event_hash.as_str())
        .collect();
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(context) {
        if let Some(recent) = value
            .get_mut("recent_events")
            .and_then(serde_json::Value::as_array_mut)
        {
            recent.retain(|view| {
                !view
                    .get("event_hash")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|hash| hashes.contains(hash))
            });
            if let Some(steps) = value
                .get_mut("checkpoint")
                .and_then(|checkpoint| checkpoint.get_mut("recent_steps"))
                .and_then(serde_json::Value::as_array_mut)
            {
                steps.retain(|step| {
                    let Some(projection) = step
                        .as_str()
                        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
                    else {
                        return true;
                    };
                    let Some(hash) = projection
                        .get("event_hash")
                        .and_then(serde_json::Value::as_str)
                    else {
                        return true;
                    };
                    let Some(source) = events
                        .iter()
                        .find(|event| event.event_hash == hash && event.verify().is_ok())
                    else {
                        return true;
                    };
                    // A copied hash or narrative match is insufficient. Keep
                    // legacy/changed checkpoint views unless they are exactly
                    // the signed source's current model projection.
                    if crate::context_views::event_view(source) != projection {
                        return true;
                    }
                    match &source.event {
                        AgentEventKindV4::ToolFinished { outcome }
                        | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => {
                            let serialized = projection.to_string();
                            !replay.iter().any(|item| {
                                matches!(item,
                                ModelReplayItemV4::ToolResult { call_id, output }
                                    if call_id == &outcome.call_id && output == &serialized)
                            })
                        }
                        AgentEventKindV4::ModelText { text } => !texts.contains(text.as_str()),
                        _ => true,
                    }
                });
            }
            return value.to_string();
        }
    }
    // Plan context uses a fixed host-owned EVENTS suffix rather than a JSON object.
    if let Some((prefix, suffix)) = context.rsplit_once("\nEVENTS\n") {
        if let Ok(mut views) = serde_json::from_str::<Vec<serde_json::Value>>(suffix) {
            views.retain(|view| {
                !view
                    .get("event_hash")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|hash| hashes.contains(hash))
            });
            if let Ok(serialized) = serde_json::to_string(&views) {
                return format!("{prefix}\nEVENTS\n{serialized}");
            }
        }
    }
    context.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    fn binding() -> ModelReplayBindingV4 {
        ModelReplayBindingV4 {
            model_profile_id: Uuid::from_u128(3),
            configuration_hash: "fixture".into(),
        }
    }
    fn chain() -> Vec<AgentEventV4> {
        let start: ModelRequestStartedV4 = serde_json::from_value(json!({
            "logical_request_id":Uuid::from_u128(4),"attempt_id":Uuid::from_u128(5),
            "model_profile_id":Uuid::from_u128(3),"model_configuration_hash":"fixture","context_limit_source":{"kind":"unknown"}})).unwrap();
        let first = AgentEventV4::first(
            Uuid::from_u128(6),
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            Utc::now(),
            AgentEventKindV4::ModelRequestStarted { request: start },
        );
        let call = ToolCallV4 {
            call_id: "call_1".into(),
            tool_id: "project.read".into(),
            arguments: json!({"path":"nonce.txt"}),
        };
        let record = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ModelReplayRecorded {
                replay: ModelReplayRecordedV4 {
                    logical_request_id: Uuid::from_u128(4),
                    attempt_id: Uuid::from_u128(5),
                    binding: binding(),
                    continuation: ModelProviderContinuationV4 {
                        items: vec![
                            ModelReplayItemV4::ResponsesReasoning {
                                id: "rs_1".into(),
                                encrypted_content: "OPAQUE_SENTINEL".into(),
                            },
                            ModelReplayItemV4::ToolCall { call: call.clone() },
                        ],
                    },
                },
            },
        );
        let requested = AgentEventV4::next(
            &record,
            Utc::now(),
            AgentEventKindV4::ToolRequested { call },
        );
        let result = AgentEventV4::next(
            &requested,
            Utc::now(),
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: "call_1".into(),
                    tool_id: "project.read".into(),
                    succeeded: true,
                    model_content: "nonce-fixture".into(),
                    data: json!({"nonce":"fixture"}),
                    provenance: vec![],
                },
            },
        );
        vec![first, record, requested, result]
    }
    #[test]
    fn model_replay_survives_restart_without_reexecuting_tools() {
        let events = chain();
        let result =
            project_model_replay(Uuid::from_u128(1), Uuid::from_u128(2), &binding(), &events)
                .unwrap();
        assert_eq!(result.len(), 3);
        assert!(
            matches!(&result[2], ModelReplayItemV4::ToolResult { call_id, output } if call_id == "call_1" && output.contains("nonce-fixture"))
        );
        let reloaded =
            serde_json::from_slice::<Vec<AgentEventV4>>(&serde_json::to_vec(&events).unwrap())
                .unwrap();
        assert_eq!(
            result,
            project_model_replay(
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                &binding(),
                &reloaded
            )
            .unwrap()
        );
        assert!(
            project_model_replay(
                Uuid::from_u128(1),
                Uuid::from_u128(2),
                &binding(),
                &events[..3]
            )
            .is_err()
        );
    }
    #[test]
    fn model_replay_removes_checkpoint_copies_without_changing_durable_evidence() {
        let template = chain();
        let mut events = vec![template[0].clone()];
        let mut recorded = template[1].event.clone();
        if let AgentEventKindV4::ModelReplayRecorded { replay } = &mut recorded {
            replay
                .continuation
                .items
                .push(ModelReplayItemV4::AssistantText {
                    text: "checkpoint-public-progress".into(),
                });
        }
        let mut outcome = match &template[3].event {
            AgentEventKindV4::ToolFinished { outcome } => outcome.clone(),
            _ => unreachable!(),
        };
        outcome.data = json!({"finding":"checkpoint-verified-finding", "detail":"x".repeat(4_096)});
        outcome.model_content = outcome.data.to_string();
        for kind in [
            recorded,
            AgentEventKindV4::ModelText {
                text: "checkpoint-public-progress".into(),
            },
            template[2].event.clone(),
            AgentEventKindV4::ToolFinished { outcome },
            AgentEventKindV4::UserInputAnswered {
                question_id: "unreplayed-question".into(),
                answer: "retain this clarification".into(),
            },
            AgentEventKindV4::DelegationNodeFinished {
                call_id: "unreplayed-delegation".into(),
                outcome: DelegationNodeOutcomeV4 {
                    node_id: "reader".into(),
                    status: DelegationNodeStatusV4::Succeeded,
                    output: Some(json!({"finding":"retain delegated conclusion"})),
                    error: None,
                    tool_outcomes: vec![],
                },
            },
        ] {
            events.push(AgentEventV4::next(events.last().unwrap(), Utc::now(), kind));
        }
        let text = crate::context_views::event_view(&events[2]).to_string();
        let result = crate::context_views::event_view(&events[4]).to_string();
        let input = crate::context_views::event_view(&events[5]).to_string();
        let delegation = crate::context_views::event_view(&events[6]).to_string();
        let legacy = "tool project.read: legacy narrative remains".to_owned();
        let checkpoint = ContextCheckpointV4 {
            schema_version: 4,
            through_sequence: events.last().unwrap().sequence,
            completion_criteria: vec!["report the verified finding".into()],
            unresolved_errors: vec!["active correction must remain".into()],
            recent_steps: vec![
                text,
                result,
                input.clone(),
                delegation.clone(),
                legacy.clone(),
            ],
            scientific_state: json!({"project_id":Uuid::from_u128(1), "evidence":"current-science"}),
            task_shape: None,
            phase: None,
            task_revision: None,
            tasks: vec![],
            cycle_id: None,
        };
        events.push(AgentEventV4::next(
            events.last().unwrap(),
            Utc::now(),
            AgentEventKindV4::ContextCheckpointed {
                checkpoint: checkpoint.clone(),
            },
        ));
        let durable_before = serde_json::to_vec(&events).unwrap();
        let replay =
            project_model_replay(Uuid::from_u128(1), Uuid::from_u128(2), &binding(), &events)
                .unwrap();
        let context = json!({
            "frozen_plan":{"objective":"continue"},
            "compute_selection":{"environment":"system"},
            "checkpoint":crate::context_views::checkpoint_view(&checkpoint),
            "recent_events":[],
            "scientific_state":checkpoint.scientific_state,
            "active_guidance":[{"markdown":"preserve the source"}],
        });
        let filtered = remove_replayed_context_events(&context.to_string(), &replay, &events);
        let filtered_value: serde_json::Value = serde_json::from_str(&filtered).unwrap();
        assert_eq!(
            filtered_value["checkpoint"]["recent_steps"],
            json!([input, delegation, legacy])
        );
        assert!(!filtered.contains("checkpoint-verified-finding"));
        assert!(!filtered.contains("checkpoint-public-progress"));
        assert!(replay.iter().any(|item| matches!(item, ModelReplayItemV4::ToolResult { output, .. } if output.contains("checkpoint-verified-finding"))));
        assert!(replay.iter().any(|item| matches!(item, ModelReplayItemV4::AssistantText { text } if text == "checkpoint-public-progress")));
        for field in [
            "frozen_plan",
            "compute_selection",
            "scientific_state",
            "active_guidance",
        ] {
            assert_eq!(filtered_value[field], context[field]);
        }
        assert_eq!(
            filtered_value["checkpoint"]["unresolved_errors"],
            json!(["active correction must remain"])
        );
        assert_eq!(serde_json::to_vec(&events).unwrap(), durable_before);
        for event in &events {
            event.verify().unwrap();
        }
        assert!(context.to_string().len() - filtered.len() > 4_096);
        eprintln!(
            "checkpoint replay fixture: context {} -> {} bytes; removed {} bytes",
            context.to_string().len(),
            filtered.len(),
            context.to_string().len() - filtered.len()
        );
    }
    #[test]
    fn checkpoint_replay_dedup_preserves_unrepresented_or_changed_projections() {
        let mut events = chain();
        let replay =
            project_model_replay(Uuid::from_u128(1), Uuid::from_u128(2), &binding(), &events)
                .unwrap();
        let result = crate::context_views::event_view(&events[3]);
        let mut changed = result.clone();
        changed["event"]["outcome"]["model_content"] = json!("separate checkpoint annotation");
        let foreign = AgentEventV4::first(
            Uuid::from_u128(7),
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            Utc::now(),
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: "call_1".into(),
                    tool_id: "project.read".into(),
                    succeeded: true,
                    model_content: "foreign-run-finding".into(),
                    data: json!({}),
                    provenance: vec![],
                },
            },
        );
        events.push(foreign.clone());
        let steps = json!([
            changed.to_string(),
            crate::context_views::event_view(&foreign).to_string(),
            "plain legacy step",
            "{malformed json",
            json!({"event_hash":"unknown"}).to_string(),
        ]);
        let context = json!({"recent_events":[], "checkpoint":{"recent_steps":steps}});
        let filtered = remove_replayed_context_events(&context.to_string(), &replay, &events);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&filtered).unwrap(),
            context
        );

        let partial_replay: Vec<_> = replay
            .iter()
            .filter(|item| !matches!(item, ModelReplayItemV4::ToolResult { .. }))
            .cloned()
            .collect();
        let context =
            json!({"recent_events":[], "checkpoint":{"recent_steps":[result.to_string()]}});
        let filtered =
            remove_replayed_context_events(&context.to_string(), &partial_replay, &events);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&filtered).unwrap(),
            context
        );

        if let AgentEventKindV4::ToolFinished { outcome } = &mut events[3].event {
            outcome.data = json!({"changed_without_resigning":true});
        }
        assert!(events[3].verify().is_err());
        let filtered = remove_replayed_context_events(&context.to_string(), &replay, &events);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&filtered).unwrap(),
            context
        );
    }
    #[test]
    fn model_replay_rejects_orphans_and_wrong_bindings() {
        let mut events = chain();
        assert!(
            project_model_replay(Uuid::from_u128(7), Uuid::from_u128(2), &binding(), &events)
                .is_err()
        );
        let mut wrong = binding();
        wrong.configuration_hash = "other".into();
        assert!(
            project_model_replay(Uuid::from_u128(1), Uuid::from_u128(2), &wrong, &events).is_err()
        );
        let result = events[3].event.clone();
        events.truncate(2);
        events.push(AgentEventV4::next(&events[1], Utc::now(), result));
        assert!(
            project_model_replay(Uuid::from_u128(1), Uuid::from_u128(2), &binding(), &events)
                .is_err()
        );
        let full = chain();
        let orphan = vec![
            full[0].clone(),
            AgentEventV4::next(&full[0], Utc::now(), full[3].event.clone()),
        ];
        assert!(
            project_model_replay(Uuid::from_u128(1), Uuid::from_u128(2), &binding(), &orphan)
                .is_err()
        );
    }
    #[test]
    fn model_replay_drops_plaintext_reasoning_and_measures_ciphertext() {
        let events = chain();
        let view = crate::context_views::event_view(&events[1]);
        assert!(!view.to_string().contains("OPAQUE_SENTINEL"));
        assert!(
            serde_json::to_vec(
                &project_model_replay(Uuid::from_u128(1), Uuid::from_u128(2), &binding(), &events)
                    .unwrap()
            )
            .unwrap()
            .len()
                > 100
        );
    }
    #[test]
    fn model_replay_accepts_only_verified_host_mcp_binding_normalization() {
        let template = chain();
        let call = ToolCallV4 {
            call_id: "call_1".into(),
            tool_id: "use_mcp_tool".into(),
            arguments: serde_json::json!({"server_id":"fixture-server","tool":"fixture-tool"}),
        };
        let directory = AgentEventKindV4::ToolFinished {
            outcome: ToolOutcomeV4 {
                call_id: "directory".into(),
                tool_id: "search_mcp_tools".into(),
                succeeded: true,
                model_content: "directory".into(),
                data: serde_json::json!({"tools":[{"server_id":"fixture-server","tool_name":"fixture-tool","tool_catalog_sha256":"catalog","schema_sha256":"schema"}]}),
                provenance: vec![],
            },
        };
        for forged in [false, true] {
            let first = AgentEventV4::first(
                template[0].run_id,
                template[0].project_id,
                template[0].conversation_id,
                Utc::now(),
                directory.clone(),
            );
            let mut events = vec![first];
            let mut record = template[1].event.clone();
            if let AgentEventKindV4::ModelReplayRecorded { replay } = &mut record {
                replay.continuation.items =
                    vec![ModelReplayItemV4::ToolCall { call: call.clone() }];
            }
            let mut normalized = call.clone();
            crate::bind_mcp_directory(&mut normalized, &events);
            if forged {
                normalized.arguments["unexpected"] = serde_json::json!(true);
            }
            let mut outcome = match &template[3].event {
                AgentEventKindV4::ToolFinished { outcome } => outcome.clone(),
                _ => unreachable!(),
            };
            outcome.tool_id = "use_mcp_tool".into();
            for kind in [
                template[0].event.clone(),
                record,
                AgentEventKindV4::ToolRequested { call: normalized },
                AgentEventKindV4::ToolFinished { outcome },
            ] {
                events.push(AgentEventV4::next(events.last().unwrap(), Utc::now(), kind));
            }
            assert_eq!(
                project_model_replay(
                    template[0].project_id,
                    template[0].conversation_id,
                    &binding(),
                    &events
                )
                .is_ok(),
                !forged
            );
        }
    }
}
