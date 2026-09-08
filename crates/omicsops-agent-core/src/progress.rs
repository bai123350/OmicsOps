//! A bounded no-progress heuristic reconstructed only from durable outcomes.
use omicsops_protocol::{AgentEventKindV4, AgentEventV4, ToolCallV4, ToolOutcomeV4};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

const WINDOW: usize = 32;

fn fingerprint(value: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    hasher.finish()
}

fn stable_result(mut value: Value) -> Value {
    // Ignore transport timing only at the result root. Scientific timestamps,
    // nested measurements and runtime/job state remain meaningful evidence.
    if let Some(object) = value.as_object_mut() {
        for key in ["duration_ms", "elapsed_ms", "observed_at"] {
            object.remove(key);
        }
    }
    value
}

fn observation(call: &ToolCallV4, outcome: &ToolOutcomeV4) -> u64 {
    let content = serde_json::from_str::<Value>(&outcome.model_content)
        .map(stable_result)
        .unwrap_or_else(|_| json!(outcome.model_content));
    fingerprint(&json!({"tool":call.tool_id, "arguments":call.arguments,
        "succeeded":outcome.succeeded, "content":content, "data":stable_result(outcome.data.clone())}))
}

fn repeated_suffix(window: &VecDeque<u64>, repetitions: u32) -> bool {
    let repeats = repetitions.max(2) as usize;
    (1..=window.len() / repeats).any(|width| {
        let start = window.len() - width * repeats;
        (width..width * repeats).all(|index| window[start + index] == window[start + index % width])
    })
}

pub(crate) fn stalled(events: &[AgentEventV4], repetitions: u32) -> bool {
    let mut calls = BTreeMap::<String, &ToolCallV4>::new();
    let mut batched = BTreeSet::<String>::new();
    let mut results = BTreeMap::<String, u64>::new();
    let mut window = VecDeque::new();
    for event in events {
        let next = match &event.event {
            AgentEventKindV4::UserInputAnswered { .. }
            | AgentEventKindV4::GuidanceConsumed { .. }
            | AgentEventKindV4::ScientificStateChanged { .. } => {
                window.clear();
                None
            }
            AgentEventKindV4::ToolRequested { call } => {
                calls.insert(call.call_id.clone(), call);
                None
            }
            AgentEventKindV4::ToolBatchStarted { call_ids, .. } => {
                batched.extend(call_ids.iter().cloned());
                None
            }
            AgentEventKindV4::ToolFinished { outcome }
            | AgentEventKindV4::ToolOutcomeReused { outcome, .. } => {
                if let Some(call) = calls.get(&outcome.call_id) {
                    let key = observation(call, outcome);
                    if batched.contains(&outcome.call_id) {
                        results.insert(outcome.call_id.clone(), key);
                        None
                    } else {
                        Some(key)
                    }
                } else {
                    None
                }
            }
            AgentEventKindV4::ToolBatchFinished { call_ids, .. } => {
                let ordered = call_ids
                    .iter()
                    .map(|id| results.remove(id))
                    .collect::<Option<Vec<_>>>();
                for id in call_ids {
                    batched.remove(id);
                }
                ordered
                    .filter(|values| !values.is_empty())
                    .map(|values| fingerprint(&json!(values)))
            }
            _ => None,
        };
        if let Some(next) = next {
            window.push_back(next);
            if window.len() > WINDOW {
                window.pop_front();
            }
        }
    }
    repeated_suffix(&window, repetitions)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_suffix_detects_a_and_ab_but_not_changed_observations() {
        assert!(repeated_suffix(&VecDeque::from([1, 1, 1]), 3));
        assert!(repeated_suffix(&VecDeque::from([1, 2, 1, 2, 1, 2]), 3));
        assert!(!repeated_suffix(&VecDeque::from([1, 2, 1, 2, 1, 3]), 3));
        assert!(!repeated_suffix(&VecDeque::from([1]), 1));
    }

    #[test]
    fn ids_and_transport_timing_do_not_fake_progress_but_job_state_does() {
        let call = ToolCallV4 {
            call_id: "a".into(),
            tool_id: "monitor".into(),
            arguments: json!({"job":1}),
        };
        let mut outcome = ToolOutcomeV4 {
            call_id: "a".into(),
            tool_id: "monitor".into(),
            succeeded: true,
            model_content: "running".into(),
            data: json!({"state":"running","duration_ms":1}),
            provenance: vec![],
        };
        let first = observation(&call, &outcome);
        outcome.call_id = "b".into();
        outcome.data["duration_ms"] = json!(900);
        assert_eq!(first, observation(&call, &outcome));
        outcome.data["state"] = json!("completed");
        assert_ne!(first, observation(&call, &outcome));
    }
}
