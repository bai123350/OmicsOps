//! Bounded model-only projections. Durable events remain the source of truth.
use omicsops_protocol::{
    AgentEventKindV4, AgentEventV4, DelegationGraphOutcomeV4, DelegationNodeOutcomeV4, RunSpecV4,
    ToolCallV4, ToolOutcomeV4,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

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
    if outcome.tool_id == "search_mcp_tools" && outcome.succeeded {
        if let Ok(directory) = mcp_directory_text(&outcome.data, None) {
            let selector = json!({"view":"directory"});
            view["event"]["outcome"]
                .as_object_mut()
                .expect("outcome object")
                .remove("model_content");
            attach_reference(&mut view, event, "data");
            view["result_reference"]["mcp_selector"] = selector.clone();
            // Count the entire projected envelope, including escaped content and
            // the signed reference, when choosing the first compact page.
            let mut limit = 4_096;
            loop {
                let page = page_text(
                    &directory,
                    0,
                    limit,
                    event.sequence,
                    &event.event_hash,
                    "data",
                    Some(&selector),
                    VIEW_BYTES,
                )
                .expect("directory page fits result budget");
                view["event"]["outcome"]["data"] = page;
                if view.to_string().len() <= VIEW_BYTES {
                    return view;
                }
                limit /= 2;
                if limit < 4 {
                    break;
                }
            }
        }
    }
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
    let selector = args.get("mcp_selector");
    if selector.is_some() && field != "data" {
        return Err("mcp_selector requires field data".into());
    }
    if selector.is_some()
        && !matches!(&event.event,
            AgentEventKindV4::ToolFinished { outcome }
                | AgentEventKindV4::ToolOutcomeReused { outcome, .. }
                if outcome.tool_id == "search_mcp_tools" && outcome.succeeded)
    {
        return Err("mcp_selector requires a successful search_mcp_tools result".into());
    }
    let text = match &event.event {
        AgentEventKindV4::ToolFinished { outcome }
        | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => {
            if let Some(selector) = selector {
                selected_mcp_text(&outcome.data, selector)?
            } else {
                match field {
                    "model_content" => outcome.model_content.clone(),
                    "data" => {
                        serde_json::to_string(&outcome.data).map_err(|error| error.to_string())?
                    }
                    _ => return Err("field must be model_content or data".into()),
                }
            }
        }
        AgentEventKindV4::DelegationNodeFinished { outcome, .. } if field == "data" => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())?
        }
        AgentEventKindV4::DelegationGraphFinished { outcome, .. } if field == "data" => {
            serde_json::to_string(outcome).map_err(|error| error.to_string())?
        }
        _ => return Err("reference is not a tool outcome".into()),
    };
    let offset = usize::try_from(offset).map_err(|_| "offset exceeds platform size")?;
    // Selected pages are later wrapped in a tool event. Reserve envelope
    // space so the complete model projection remains within the same budget.
    let budget = if selector.is_some() {
        6_144
    } else {
        VIEW_BYTES
    };
    page_text(
        &text,
        offset,
        limit as usize,
        sequence,
        hash,
        field,
        selector,
        budget,
    )
}

fn page_text(
    text: &str,
    offset: usize,
    limit: usize,
    sequence: u64,
    hash: &str,
    field: &str,
    selector: Option<&Value>,
    budget: usize,
) -> Result<Value, String> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err("offset must be an in-range UTF-8 boundary; use next_offset".into());
    }
    let mut requested_end = offset.saturating_add(limit).min(text.len());
    while !text.is_char_boundary(requested_end) {
        requested_end -= 1;
    }
    let requested = result_page(text, offset, requested_end, sequence, hash, field, selector);
    if serde_json::to_vec(&requested)
        .expect("serializable result page")
        .len()
        <= budget
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
        let mut candidate =
            result_page(text, offset, candidate_end, sequence, hash, field, selector);
        candidate["next_offset"] = json!(candidate_end);
        if serde_json::to_vec(&candidate)
            .expect("serializable result page")
            .len()
            <= budget
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
    Ok(result_page(
        text, offset, end, sequence, hash, field, selector,
    ))
}

fn result_page(
    text: &str,
    offset: usize,
    end: usize,
    sequence: u64,
    hash: &str,
    field: &str,
    selector: Option<&Value>,
) -> Value {
    let mut page = json!({"content": &text[offset..end], "next_offset": (end < text.len()).then_some(end),
        "total_bytes": text.len(), "sequence": sequence, "event_hash": hash, "field": field});
    if let Some(selector) = selector {
        page["mcp_selector"] = selector.clone();
    }
    page
}

fn selected_mcp_text(data: &Value, selector: &Value) -> Result<String, String> {
    let object = selector
        .as_object()
        .ok_or("mcp_selector must be an object")?;
    match object.get("view").and_then(Value::as_str) {
        Some("directory") => {
            if object.keys().any(|key| key != "view" && key != "server_id") {
                return Err("invalid directory selector field".into());
            }
            let server = match object.get("server_id") {
                None => None,
                Some(Value::String(id)) if !id.is_empty() => Some(id.as_str()),
                _ => return Err("server_id must be a nonempty string".into()),
            };
            mcp_directory_text(data, server)
        }
        Some("tool") => {
            if object.len() != 3
                || object
                    .keys()
                    .any(|key| key != "view" && key != "server_id" && key != "tool_name")
            {
                return Err("tool selector requires only view, server_id and tool_name".into());
            }
            let server = object
                .get("server_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("server_id is required")?;
            let name = object
                .get("tool_name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or("tool_name is required")?;
            let tools = mcp_tools(data)?;
            let mut found = None;
            let mut identities = BTreeSet::new();
            for tool in tools {
                let tool_server = tool
                    .get("server_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or("MCP tool has no server_id")?;
                let tool_name = tool
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or("MCP tool has no tool_name")?;
                if !identities.insert((tool_server, tool_name)) {
                    return Err("duplicate MCP tool identity".into());
                }
                if tool.get("server_id").and_then(Value::as_str) == Some(server)
                    && tool.get("tool_name").and_then(Value::as_str) == Some(name)
                {
                    found = Some(tool);
                }
            }
            serde_json::to_string(found.ok_or("MCP tool identity not found")?)
                .map_err(|error| error.to_string())
        }
        _ => Err("mcp_selector view must be directory or tool".into()),
    }
}

fn mcp_tools(data: &Value) -> Result<&Vec<Value>, String> {
    data.get("tools")
        .and_then(Value::as_array)
        .ok_or("MCP directory has no tools array".into())
}

fn mcp_directory_text(data: &Value, filter: Option<&str>) -> Result<String, String> {
    let mut identities = BTreeSet::new();
    let mut servers: BTreeMap<&str, (&str, Vec<Value>)> = BTreeMap::new();
    for entry in mcp_tools(data)? {
        let server = entry
            .get("server_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("MCP tool has no server_id")?;
        let name = entry
            .get("tool_name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("MCP tool has no tool_name")?;
        if !identities.insert((server, name)) {
            return Err("duplicate MCP tool identity".into());
        }
        if filter.is_some_and(|selected| selected != server) {
            continue;
        }
        let server_name = entry
            .get("server_name")
            .and_then(Value::as_str)
            .unwrap_or(server);
        let description = entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut end = description.len().min(96);
        while !description.is_char_boundary(end) {
            end -= 1;
        }
        servers
            .entry(server)
            .or_insert_with(|| (server_name, Vec::new()))
            .1
            .push(json!({"tool_name":name, "description":&description[..end]}));
    }
    if filter.is_some() && servers.is_empty() {
        return Err("MCP server identity not found".into());
    }
    let groups: Vec<_> = servers
        .into_iter()
        .map(|(id, (name, tools))| json!({"server_id":id,"server_name":name,"tools":tools}))
        .collect();
    Ok(json!({"servers":groups}).to_string())
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
    use std::collections::{BTreeMap, BTreeSet};

    use chrono::Utc;
    use omicsops_protocol::{
        AgentEventKindV4, AgentEventV4, DelegationGraphOutcomeV4, DelegationNodeOutcomeV4,
        DelegationNodeStatusV4, ExecutionPlanV4, RunSpecV4, ToolCallV4, ToolOutcomeV4,
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

    fn large_directory(spec: &RunSpecV4) -> (AgentEventV4, serde_json::Value) {
        let tools: Vec<_> = (0..247)
            .map(|index| {
                json!({
                    "server_id": if index % 2 == 0 { "server-a" } else { "server-b" },
                    "server_name": if index % 2 == 0 { "Server A" } else { "Server B" },
                    "tool_name": format!("tool_{index:03}"),
                    "description": format!("Search research papers 数据🧬 {index}; {}", "x".repeat(300)),
                    "input_schema": {"type":"object", "properties": {"query": {"type":"string", "description": "quoted \\\" schema 数据🧬 ".repeat(19)}}},
                    "tool_catalog_sha256": "catalog-hash",
                    "schema_sha256": format!("schema-{index}"),
                    "launch_approved": true,
                })
            })
            .collect();
        let data = json!({"tools": tools, "guidance": "complete enabled directory"});
        let event = AgentEventV4::first(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            Utc::now(),
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: "directory".into(),
                    tool_id: "search_mcp_tools".into(),
                    succeeded: true,
                    model_content: data.to_string(),
                    data: data.clone(),
                    provenance: vec![],
                },
            },
        );
        (event, data)
    }

    fn directory_call(
        event: &AgentEventV4,
        selector: serde_json::Value,
        offset: u64,
    ) -> ToolCallV4 {
        ToolCallV4 {
            call_id: "directory-page".into(),
            tool_id: READ_RESULT_TOOL.into(),
            arguments: json!({"sequence":event.sequence, "event_hash":event.event_hash,
                "field":"data", "offset":offset, "limit":8192, "mcp_selector":selector}),
        }
    }

    #[test]
    fn mcp_directory_projection_pages_compact_names_and_exact_middle_schema() {
        let spec = spec();
        let (event, original) = large_directory(&spec);
        let original_bytes = serde_json::to_vec(&original).unwrap().len();
        assert!(
            original_bytes > 250_000,
            "fixture must represent a large catalog"
        );
        let view = event_view(&event);
        assert!(view.to_string().len() <= VIEW_BYTES);
        assert_eq!(
            view["result_reference"]["mcp_selector"],
            json!({"view":"directory"})
        );
        let mut directory = String::new();
        let mut next = view["event"]["outcome"]["data"].clone();
        let mut pages = 0;
        loop {
            directory.push_str(next["content"].as_str().unwrap());
            pages += 1;
            let Some(offset) = next["next_offset"].as_u64() else {
                break;
            };
            let result = read_outcome(
                &spec,
                std::slice::from_ref(&event),
                directory_call(&event, json!({"view":"directory"}), offset),
            );
            assert!(result.succeeded, "{}", result.model_content);
            assert!(result.model_content.len() <= VIEW_BYTES);
            next = result.data;
            assert!(pages < 50);
        }
        let compact: serde_json::Value = serde_json::from_str(&directory).unwrap();
        let names: Vec<_> = compact["servers"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|server| server["tools"].as_array().unwrap().iter())
            .map(|tool| tool["tool_name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 247);
        assert!(names.contains(&"tool_123"));
        assert!(pages < 10, "directory needed {pages} compact pages");
        let filtered = read_outcome(
            &spec,
            std::slice::from_ref(&event),
            directory_call(
                &event,
                json!({"view":"directory","server_id":"server-b"}),
                0,
            ),
        );
        assert!(filtered.succeeded);
        assert_eq!(
            filtered.data["mcp_selector"],
            json!({"view":"directory","server_id":"server-b"})
        );
        let mut filtered_call = directory_call(
            &event,
            json!({"view":"directory","server_id":"server-b"}),
            0,
        );
        let mut filtered_text = String::new();
        loop {
            let page = read_outcome(&spec, std::slice::from_ref(&event), filtered_call.clone());
            assert!(page.succeeded);
            filtered_text.push_str(page.data["content"].as_str().unwrap());
            let Some(offset) = page.data["next_offset"].as_u64() else {
                break;
            };
            filtered_call.arguments["offset"] = json!(offset);
        }
        let filtered_directory: serde_json::Value = serde_json::from_str(&filtered_text).unwrap();
        assert_eq!(filtered_directory["servers"].as_array().unwrap().len(), 1);
        assert_eq!(filtered_directory["servers"][0]["server_id"], "server-b");
        assert_eq!(
            filtered_directory["servers"][0]["tools"]
                .as_array()
                .unwrap()
                .len(),
            123
        );

        let selected = read_outcome(
            &spec,
            std::slice::from_ref(&event),
            directory_call(
                &event,
                json!({"view":"tool","server_id":"server-b","tool_name":"tool_123"}),
                0,
            ),
        );
        assert!(selected.succeeded, "{}", selected.model_content);
        assert!(selected.model_content.len() <= VIEW_BYTES);
        assert_eq!(selected.data["next_offset"], serde_json::Value::Null);
        let entry: serde_json::Value =
            serde_json::from_str(selected.data["content"].as_str().unwrap()).unwrap();
        assert_eq!(entry, original["tools"][123]);

        let mut legacy = directory_call(&event, json!({"view":"directory"}), 0);
        legacy
            .arguments
            .as_object_mut()
            .unwrap()
            .remove("mcp_selector");
        let mut legacy_pages = 0;
        let mut restored = String::new();
        loop {
            let page = read_outcome(&spec, std::slice::from_ref(&event), legacy.clone());
            assert!(page.succeeded);
            restored.push_str(page.data["content"].as_str().unwrap());
            legacy_pages += 1;
            let Some(offset) = page.data["next_offset"].as_u64() else {
                break;
            };
            legacy.arguments["offset"] = json!(offset);
        }
        assert_eq!(restored, original.to_string());
        assert!(
            legacy_pages > 30,
            "fixture should require many original pages"
        );
        eprintln!(
            "synthetic MCP fixture: original={original_bytes} bytes; compact directory={pages} pages; legacy original={legacy_pages} pages; exact tool=1 page; first projection={} bytes",
            view.to_string().len()
        );
    }

    #[test]
    fn mcp_selection_rejects_wrong_scope_identity_and_selector_shape() {
        let spec = spec();
        let (event, _) = large_directory(&spec);
        for selector in [
            json!({"view":"tool","server_id":"server-b","tool_name":"missing"}),
            json!({"view":"tool","server_id":"server-a","tool_name":"tool_123"}),
            json!({"view":"directory","unexpected":true}),
            json!({"view":"tool","server_id":"server-b"}),
            json!({"view":"unknown"}),
        ] {
            assert!(
                !read_outcome(
                    &spec,
                    std::slice::from_ref(&event),
                    directory_call(&event, selector, 0)
                )
                .succeeded
            );
        }
        let mut wrong_field = directory_call(&event, json!({"view":"directory"}), 0);
        wrong_field.arguments["field"] = json!("model_content");
        assert!(!read_outcome(&spec, std::slice::from_ref(&event), wrong_field).succeeded);
        let mut wrong_hash = directory_call(&event, json!({"view":"directory"}), 0);
        wrong_hash.arguments["event_hash"] = json!("changed");
        assert!(!read_outcome(&spec, std::slice::from_ref(&event), wrong_hash).succeeded);
        let non_directory = AgentEventV4::first(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            Utc::now(),
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: "other".into(),
                    tool_id: "project.read".into(),
                    succeeded: true,
                    model_content: "{}".into(),
                    data: json!({"tools":[]}),
                    provenance: vec![],
                },
            },
        );
        assert!(
            !read_outcome(
                &spec,
                std::slice::from_ref(&non_directory),
                directory_call(&non_directory, json!({"view":"directory"}), 0)
            )
            .succeeded
        );
        let duplicate = AgentEventV4::first(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            Utc::now(),
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: "dup".into(),
                    tool_id: "search_mcp_tools".into(),
                    succeeded: true,
                    model_content: "{}".into(),
                    data: json!({"tools":[{"server_id":"s","tool_name":"t"},{"server_id":"s","tool_name":"t"}]}),
                    provenance: vec![],
                },
            },
        );
        assert!(
            !read_outcome(
                &spec,
                std::slice::from_ref(&duplicate),
                directory_call(
                    &duplicate,
                    json!({"view":"tool","server_id":"s","tool_name":"t"}),
                    0
                )
            )
            .succeeded
        );
    }

    #[test]
    fn delegation_reads_reject_mcp_selectors_and_preserve_legacy_data() {
        let spec = spec();
        let node = DelegationNodeOutcomeV4 {
            node_id: "reader".into(),
            status: DelegationNodeStatusV4::Succeeded,
            output: Some(json!({"finding":"verified"})),
            error: None,
            tool_outcomes: vec![],
        };
        let graph = DelegationGraphOutcomeV4 {
            schema_version: 4,
            nodes: BTreeMap::from([("reader".into(), node.clone())]),
        };
        for (kind, expected) in [
            (
                AgentEventKindV4::DelegationNodeFinished {
                    call_id: "node".into(),
                    outcome: node.clone(),
                },
                json!(node),
            ),
            (
                AgentEventKindV4::DelegationGraphFinished {
                    call_id: "graph".into(),
                    outcome: graph.clone(),
                },
                json!(graph),
            ),
        ] {
            let event = AgentEventV4::first(
                spec.run_id,
                spec.project_id,
                spec.conversation_id,
                Utc::now(),
                kind,
            );
            let mut call = directory_call(&event, json!({"view":"directory"}), 0);
            let selected = read_outcome(&spec, std::slice::from_ref(&event), call.clone());
            assert!(
                !selected.succeeded,
                "delegation data accepted an MCP selector"
            );
            call.arguments["mcp_selector"] = json!({"unexpected":true});
            let malformed = read_outcome(&spec, std::slice::from_ref(&event), call.clone());
            assert!(!malformed.succeeded, "malformed selector was ignored");
            call.arguments
                .as_object_mut()
                .unwrap()
                .remove("mcp_selector");
            let legacy = read_outcome(&spec, std::slice::from_ref(&event), call);
            assert!(legacy.succeeded, "{}", legacy.model_content);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(legacy.data["content"].as_str().unwrap())
                    .unwrap(),
                expected
            );
        }
    }

    #[test]
    fn oversized_selected_schema_pages_without_losing_utf8_or_exceeding_projection_budget() {
        let spec = spec();
        let entry = json!({"server_id":"server-a", "tool_name":"large",
            "input_schema":{"description":"quoted \\\" 数据🧬 ".repeat(5_000)}});
        let data = json!({"tools":[entry.clone()]});
        let event = AgentEventV4::first(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            Utc::now(),
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: "large".into(),
                    tool_id: "search_mcp_tools".into(),
                    succeeded: true,
                    model_content: data.to_string(),
                    data,
                    provenance: vec![],
                },
            },
        );
        let mut call = directory_call(
            &event,
            json!({"view":"tool","server_id":"server-a","tool_name":"large"}),
            0,
        );
        let mut restored = String::new();
        let mut pages = 0;
        loop {
            let result = read_outcome(&spec, std::slice::from_ref(&event), call.clone());
            assert!(result.succeeded, "{}", result.model_content);
            let page_event = AgentEventV4::first(
                spec.run_id,
                spec.project_id,
                spec.conversation_id,
                Utc::now(),
                AgentEventKindV4::ToolFinished {
                    outcome: result.clone(),
                },
            );
            assert!(event_view(&page_event).to_string().len() <= VIEW_BYTES);
            restored.push_str(result.data["content"].as_str().unwrap());
            pages += 1;
            let Some(offset) = result.data["next_offset"].as_u64() else {
                break;
            };
            call.arguments["offset"] = json!(offset);
            assert!(pages < 100);
        }
        assert!(pages > 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&restored).unwrap(),
            entry
        );
    }
}
