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
    let mut recent_observations = VecDeque::<Vec<u64>>::new();
    let mut units_without_novelty = 0_u32;
    for event in events {
        let next = match &event.event {
            AgentEventKindV4::UserInputAnswered { .. }
            | AgentEventKindV4::GuidanceConsumed { .. }
            | AgentEventKindV4::ScientificStateChanged { .. } => {
                window.clear();
                recent_observations.clear();
                units_without_novelty = 0;
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
                        Some(vec![key])
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
                ordered.filter(|values| !values.is_empty())
            }
            _ => None,
        };
        if let Some(observations) = next {
            if observations
                .iter()
                .any(|value| !recent_observations.iter().any(|unit| unit.contains(value)))
            {
                units_without_novelty = 0;
            } else {
                units_without_novelty = units_without_novelty.saturating_add(1);
            }
            recent_observations.push_back(observations.clone());
            if recent_observations.len() > WINDOW {
                recent_observations.pop_front();
            }
            window.push_back(fingerprint(&json!(observations)));
            if window.len() > WINDOW {
                window.pop_front();
            }
        }
    }
    repeated_suffix(&window, repetitions) || units_without_novelty >= repetitions.max(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn push(events: &mut Vec<AgentEventV4>, event: AgentEventKindV4) {
        let next = match events.last() {
            Some(previous) => AgentEventV4::next(previous, Utc::now(), event),
            None => AgentEventV4::first(
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                Utc::now(),
                event,
            ),
        };
        events.push(next);
    }

    fn complete_observation(
        events: &mut Vec<AgentEventV4>,
        id: &str,
        tool: &str,
        args: Value,
        data: Value,
    ) {
        push(
            events,
            AgentEventKindV4::ToolRequested {
                call: ToolCallV4 {
                    call_id: id.into(),
                    tool_id: tool.into(),
                    arguments: args,
                },
            },
        );
        push(
            events,
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: id.into(),
                    tool_id: tool.into(),
                    succeeded: true,
                    model_content: "result".into(),
                    data,
                    provenance: vec![],
                },
            },
        );
    }
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
        let completed = observation(&call, &outcome);
        assert_ne!(first, completed);
        outcome.model_content = "completed with paper citation".into();
        assert_ne!(completed, observation(&call, &outcome));
    }

    #[test]
    fn varied_repeats_stop_only_after_two_complete_units_without_new_observations() {
        let mut events = Vec::new();
        for (id, tool, data) in [
            ("read-1", "project.read", json!({"paper":"p1"})),
            ("list-1", "project.list", json!({"files":["p1"]})),
            ("verify-1", "artifact.verify", json!({"checksum":"sha1"})),
            ("list-2", "project.list", json!({"files":["p1"]})),
        ] {
            complete_observation(&mut events, id, tool, json!({}), data);
        }
        assert!(!stalled(&events, 2));
        complete_observation(
            &mut events,
            "read-2",
            "project.read",
            json!({}),
            json!({"paper":"p1"}),
        );
        assert!(stalled(&events, 2));
    }

    #[test]
    fn changed_scientific_result_and_page_are_new_observations() {
        let mut events = Vec::new();
        complete_observation(
            &mut events,
            "a",
            "project.read",
            json!({"offset":0}),
            json!({"paper":"p1","checksum":"sha1","job":"running"}),
        );
        complete_observation(
            &mut events,
            "b",
            "project.read",
            json!({"offset":0}),
            json!({"paper":"p2","checksum":"sha1","job":"running"}),
        );
        complete_observation(
            &mut events,
            "c",
            "project.read",
            json!({"offset":0}),
            json!({"paper":"p2","checksum":"sha2","job":"running"}),
        );
        complete_observation(
            &mut events,
            "d",
            "project.read",
            json!({"offset":0}),
            json!({"paper":"p2","checksum":"sha2","job":"completed"}),
        );
        complete_observation(
            &mut events,
            "e",
            "project.read",
            json!({"offset":50}),
            json!({"paper":"p2","checksum":"sha2","job":"completed"}),
        );
        assert!(!stalled(&events, 2));
    }

    #[test]
    fn input_guidance_and_scientific_state_each_reset_novelty_streak() {
        for reset in [
            AgentEventKindV4::UserInputAnswered {
                question_id: "q".into(),
                answer: "continue".into(),
            },
            AgentEventKindV4::GuidanceConsumed {
                message_id: Uuid::new_v4(),
                markdown: "continue".into(),
            },
            AgentEventKindV4::ScientificStateChanged {
                revision: 1,
                state_sha256: "new-state".into(),
                changes: vec![],
            },
        ] {
            let mut events = Vec::new();
            complete_observation(
                &mut events,
                "a",
                "project.read",
                json!({}),
                json!({"paper":"p1"}),
            );
            complete_observation(
                &mut events,
                "b",
                "project.read",
                json!({}),
                json!({"paper":"p1"}),
            );
            push(&mut events, reset);
            complete_observation(
                &mut events,
                "c",
                "project.read",
                json!({}),
                json!({"paper":"p1"}),
            );
            assert!(!stalled(&events, 2));
            complete_observation(
                &mut events,
                "d",
                "project.read",
                json!({}),
                json!({"paper":"p2"}),
            );
            assert!(!stalled(&events, 2));
            complete_observation(
                &mut events,
                "e",
                "project.read",
                json!({}),
                json!({"paper":"p1"}),
            );
            assert!(!stalled(&events, 2));
        }
    }
}
