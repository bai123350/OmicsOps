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
    let delegation = match &event.event {
        AgentEventKindV4::DelegationNodeFinished { outcome, .. } => Some(node_view(outcome)),
        AgentEventKindV4::DelegationGraphFinished { outcome, .. } => Some(graph_view(outcome)),
        _ => None,
    };
    if let Some(summary) = delegation {
        view["event"]["outcome"] = summary;
        attach_reference(&mut view, event);
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
            attach_reference(&mut view, event);
            return view;
        }
    }
    let data_bytes = serde_json::to_vec(&outcome.data)
        .expect("serializable data")
        .len();
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

fn attach_reference(view: &mut Value, event: &AgentEventV4) {
    view["model_projection"] = json!(true);
    view["result_reference"] = json!({
        "tool": READ_RESULT_TOOL, "sequence": event.sequence,
        "event_hash": event.event_hash, "field": "data",
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
    let mut end = offset.saturating_add(limit as usize).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"content": &text[offset..end], "next_offset": (end < text.len()).then_some(end),
        "total_bytes": text.len(), "sequence": sequence, "event_hash": hash, "field": field}),
    )
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
