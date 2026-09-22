//! Bounded model-only projections. Durable events remain the source of truth.
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, DelegationGraphOutcomeV4, DelegationNodeOutcomeV4, RunSpecV4,
    ToolCallV4, ToolOutcomeV4,
};
use serde_json::{Value, json};

pub(crate) const READ_RESULT_TOOL: &str = "agent.read_tool_result";
const VIEW_BYTES: usize = 8_192;

pub(crate) fn bounded_text(text: &str) -> String {
    if text.len() <= VIEW_BYTES {
        return text.to_owned();
    }
    let mut head = VIEW_BYTES / 2 - 128;
    let mut tail = text.len() - head;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}\n[model view shortened; retrieve the original using its result_reference]\n{}",
        &text[..head],
        &text[tail..]
    )
}

pub(crate) fn event_view(event: &AgentEventV4) -> Value {
    let mut view = serde_json::to_value(event).expect("serializable event");
    // The containing context is already scoped to one run. Keep the event's
    // stable lookup identity, but do not repeat run identity and hash-chain
    // fields in every model-only entry.
    let envelope = view.as_object_mut().expect("event object");
    for field in [
        "schema_version",
        "run_id",
        "project_id",
        "conversation_id",
        "occurred_at",
        "previous_hash",
    ] {
        envelope.remove(field);
    }
    view["model_projection"] = json!(true);
    let delegation = match &event.event {
        AgentEventKindV4::DelegationNodeFinished { outcome, .. } => Some(node_view(outcome)),
        AgentEventKindV4::DelegationGraphFinished { outcome, .. } => Some(graph_view(outcome)),
        _ => None,
    };
    if let Some(summary) = delegation {
        view["event"]["outcome"] = summary;
        attach_reference(&mut view, event, "data");
        return view;
    }
    let outcome = match &event.event {
        AgentEventKindV4::ToolFinished { outcome }
        | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => outcome,
        _ => return view,
    };
    if outcome.tool_id == "agent.delegate" {
        if let Ok(graph) = serde_json::from_value::<DelegationGraphOutcomeV4>(outcome.data.clone())
        {
            let summary = graph_view(&graph);
            view["event"]["outcome"]["model_content"] = json!(
                "Delegation finished; see data for node conclusions and result_reference for the full trace."
            );
            view["event"]["outcome"]["data"] = summary;
            attach_reference(&mut view, event, "data");
            return view;
        }
    }
    let data_bytes = serde_json::to_vec(&outcome.data)
        .expect("serializable data")
        .len();
    let model_content_duplicates_data =
        serde_json::to_string(&outcome.data).is_ok_and(|data| data == outcome.model_content);
    if outcome.succeeded && data_bytes <= VIEW_BYTES && model_content_duplicates_data {
        let original_bytes = serde_json::to_vec(&view)
            .expect("serializable model view")
            .len();
        let mut deduplicated = view.clone();
        deduplicated["event"]["outcome"]
            .as_object_mut()
            .expect("outcome object")
            .remove("model_content");
        // A result page already carries the complete page and continuation
        // metadata in data. Pointing it back at its own duplicate model_content
        // invites recursive reads without making any information recoverable.
        if outcome.tool_id != READ_RESULT_TOOL {
            attach_reference(&mut deduplicated, event, "model_content");
        }
        if serde_json::to_vec(&deduplicated)
            .expect("serializable model view")
            .len()
            < original_bytes
        {
            return deduplicated;
        }
    }
    if outcome.model_content.len() <= VIEW_BYTES && data_bytes <= VIEW_BYTES {
        return view;
    }
    // This object is explicitly a model projection, never a new signed event.
    view["model_projection"] = json!(true);
    view["result_reference"] = json!({
        "tool": READ_RESULT_TOOL, "sequence": event.sequence,
        "event_hash": event.event_hash, "call_id": outcome.call_id,
        "model_content_bytes": outcome.model_content.len(), "data_bytes": data_bytes,
    });
    view["event"]["outcome"]["model_content"] = json!(bounded_text(&outcome.model_content));
    if data_bytes > VIEW_BYTES {
        view["event"]["outcome"]["data"] = json!({"omitted_from_model_view": true,
            "read_field": "data", "bytes": data_bytes});
    }
    view
}

fn attach_reference(view: &mut Value, event: &AgentEventV4, field: &str) {
    view["model_projection"] = json!(true);
    view["result_reference"] = json!({
        "tool": READ_RESULT_TOOL, "sequence": event.sequence,
        "event_hash": event.event_hash, "field": field,
    });
}

fn node_view(outcome: &DelegationNodeOutcomeV4) -> Value {
    let mut view = serde_json::to_value(outcome).expect("serializable node");
    view.as_object_mut()
        .expect("node object")
        .remove("tool_outcomes");
    view["tool_call_count"] = json!(outcome.tool_outcomes.len());
    view["failed_tool_call_count"] = json!(
        outcome
            .tool_outcomes
            .iter()
            .filter(|tool| !tool.succeeded)
            .count()
    );
    // Old durable nodes may predate child output bounds. Keep their original
    // structured output retrievable rather than publishing invalid partial JSON.
    if serde_json::to_vec(&outcome.output)
        .expect("serializable output")
        .len()
        > VIEW_BYTES
    {
        view["output"] = json!({"omitted_from_model_view": true, "read_field": "data"});
    }
    if let Some(error) = &outcome.error {
        view["error"] = json!(bounded_text(error));
    }
    view
}

fn graph_view(outcome: &DelegationGraphOutcomeV4) -> Value {
    let nodes: serde_json::Map<String, Value> = outcome
        .nodes
        .iter()
        .map(|(id, node)| (id.clone(), node_view(node)))
        .collect();
    json!({"schema_version": outcome.schema_version, "nodes": nodes})
}

pub(crate) fn read_result(
    spec: &RunSpecV4,
    events: &[AgentEventV4],
    call: &ToolCallV4,
) -> Result<Value, String> {
    let args = &call.arguments;
    let sequence = args
        .get("sequence")
        .and_then(Value::as_u64)
        .ok_or("sequence is required")?;
    let hash = args
        .get("event_hash")
        .and_then(Value::as_str)
        .ok_or("event_hash is required")?;
    let field = args
        .get("field")
        .and_then(Value::as_str)
        .ok_or("field is required")?;
    let offset = args
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or("offset is required")?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .ok_or("limit is required")?;
    if !(4..=8_192).contains(&limit) {
        return Err("limit must be 4..8192 UTF-8 bytes".into());
    }
    let event = events
        .iter()
        .find(|event| {
            event.sequence == sequence
                && event.run_id == spec.run_id
                && event.project_id == spec.project_id
                && event.conversation_id == spec.conversation_id
                && event.event_hash == hash
        })
        .ok_or("result reference does not belong to this run or its hash changed")?;
    event
        .verify()
        .map_err(|_| "result event failed integrity verification")?;
    let text = match &event.event {
        AgentEventKindV4::ToolFinished { outcome }
        | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => match field {
            "model_content" => outcome.model_content.clone(),
            "data" => serde_json::to_string(&outcome.data).map_err(|error| error.to_string())?,
            _ => return Err("field must be model_content or data".into()),
        },
        AgentEventKindV4::DelegationNodeFinished { outcome, .. } if field == "data" => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())?
        }
        AgentEventKindV4::DelegationGraphFinished { outcome, .. } if field == "data" => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())?
        }
        _ => return Err("reference is not a tool outcome".into()),
    };
    let offset = usize::try_from(offset).map_err(|_| "offset exceeds platform size")?;
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err("offset must be an in-range UTF-8 boundary; use next_offset".into());
    }
    let mut requested_end = offset.saturating_add(limit as usize).min(text.len());
    while !text.is_char_boundary(requested_end) {
        requested_end -= 1;
    }
    let requested = result_page(&text, offset, requested_end, sequence, hash, field);
    if serde_json::to_vec(&requested)
        .expect("serializable result page")
        .len()
        <= VIEW_BYTES
    {
        return Ok(requested);
    }

    // `limit` bounds the raw UTF-8 slice, while the page is subsequently
    // serialized into model_content. Quotes, backslashes, control characters,
    // and page metadata can therefore make an 8192-byte slice exceed the model
    // view budget. Find the largest UTF-8 prefix whose complete page remains
    // inline. Callers must continue from the returned next_offset.
    let mut boundaries = vec![offset];
    boundaries.extend(
        text[offset..requested_end]
            .char_indices()
            .skip(1)
            .map(|(relative, _)| offset + relative),
    );
    boundaries.push(requested_end);
    let mut low = 0;
    let mut high = boundaries.len() - 1;
    while low < high {
        let middle = (low + high + 1) / 2;
        let candidate_end = boundaries[middle];
        // Use a numeric continuation while searching so serialized size stays
        // monotonic even when candidate_end happens to equal the text length.
        let candidate = json!({"content": &text[offset..candidate_end],
            "next_offset": candidate_end, "total_bytes": text.len(),
            "sequence": sequence, "event_hash": hash, "field": field});
        if serde_json::to_vec(&candidate)
            .expect("serializable result page")
            .len()
            <= VIEW_BYTES
        {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let end = boundaries[low];
    if end == offset {
        return Err("result page metadata exceeds the model view budget".into());
    }
    Ok(result_page(&text, offset, end, sequence, hash, field))
}

fn result_page(
    text: &str,
    offset: usize,
    end: usize,
    sequence: u64,
    hash: &str,
    field: &str,
) -> Value {
    json!({"content": &text[offset..end], "next_offset": (end < text.len()).then_some(end),
        "total_bytes": text.len(), "sequence": sequence, "event_hash": hash, "field": field})
}

pub(crate) fn read_outcome(
    spec: &RunSpecV4,
    events: &[AgentEventV4],
    call: ToolCallV4,
) -> ToolOutcomeV4 {
    let result = read_result(spec, events, &call);
    let (succeeded, data) = match result {
        Ok(data) => (true, data),
        Err(message) => (
            false,
            json!({"error_kind": "result_reference", "message": message}),
        ),
    };
    ToolOutcomeV4 {
        call_id: call.call_id,
        tool_id: call.tool_id,
        succeeded,
        model_content: data.to_string(),
        data,
        provenance: vec!["host-scoped-result-read-v4".into()],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::Utc;
    use omicsops_protocol::{
        AgentEventKindV4, AgentEventV4, ExecutionPlanV4, RunSpecV4, ToolCallV4, ToolOutcomeV4,
    };
    use serde_json::json;
    use uuid::Uuid;

    use super::{READ_RESULT_TOOL, VIEW_BYTES, event_view, read_outcome};

    fn spec() -> RunSpecV4 {
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "read a stored result".into(),
            steps: vec!["read pages".into()],
            completion_criteria: vec!["restore the original".into()],
            requested_capabilities: BTreeSet::from([READ_RESULT_TOOL.into()]),
        };
        let hash = plan.canonical_hash().unwrap();
        RunSpecV4::freeze(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            plan,
            &hash,
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn read_result_pages_stay_inline_after_event_projection_with_utf8_escapes() {
        let spec = spec();
        let original = "quoted: \\\"line\\\\break\n\t; unicode: 数据🧬; ".repeat(700);
        let source = AgentEventV4::first(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            Utc::now(),
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: "source".into(),
                    tool_id: "project.read".into(),
                    succeeded: true,
                    model_content: original.clone(),
                    data: json!({"content": original}),
                    provenance: vec![],
                },
            },
        );
        let mut offset = 0_u64;
        let mut restored = String::new();
        let mut page_index = 0_u64;

        loop {
            let outcome = read_outcome(
                &spec,
                std::slice::from_ref(&source),
                ToolCallV4 {
                    call_id: format!("page-{page_index}"),
                    tool_id: READ_RESULT_TOOL.into(),
                    arguments: json!({
                        "sequence": source.sequence,
                        "event_hash": source.event_hash,
                        "field": "model_content",
                        "offset": offset,
                        "limit": 8192,
                    }),
                },
            );
            assert!(outcome.succeeded);
            assert!(
                outcome.model_content.len() <= VIEW_BYTES,
                "serialized page exceeded the model-view budget: {} bytes",
                outcome.model_content.len()
            );

            let page_event = AgentEventV4::first(
                spec.run_id,
                spec.project_id,
                spec.conversation_id,
                Utc::now(),
                AgentEventKindV4::ToolFinished {
                    outcome: outcome.clone(),
                },
            );
            let view = event_view(&page_event);
            assert_eq!(view["event"]["outcome"]["data"], outcome.data);
            assert!(view["event"]["outcome"].get("model_content").is_none());
            assert!(view.get("result_reference").is_none());

            let page = &view["event"]["outcome"]["data"];
            let content = page["content"].as_str().unwrap();
            restored.push_str(content);
            let Some(next_offset) = page["next_offset"].as_u64() else {
                break;
            };
            assert!(next_offset > offset);
            assert!(original.is_char_boundary(next_offset as usize));
            if page_index == 0 {
                assert!(
                    content.len() < 8192,
                    "escaping overhead must consume budget"
                );
                assert_ne!(next_offset, 8192, "callers must follow next_offset");
            }
            offset = next_offset;
            page_index += 1;
        }

        assert_eq!(restored, original);
    }
}
