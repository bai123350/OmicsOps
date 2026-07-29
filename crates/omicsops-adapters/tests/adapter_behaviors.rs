use std::fs;

use omicsops_adapters::{
    credentials::{CredentialVault, MemoryCredentialVault, credential_account},
    document::extract_plan_text,
    llm::{SseToolCallAccumulator, parse_tool_call_response},
    persistence::Repository,
};
use omicsops_core::domain::{AuthenticationMethod, ConnectionProfile, RunEvent, RunState};
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn markdown_plan_extraction_normalizes_text() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("plan.md");
    fs::write(&path, "# PBMC\n\n  Download data  \n\nRun Scanpy.").unwrap();

    let document = extract_plan_text(&path).unwrap();

    assert_eq!(document.format, "markdown");
    assert_eq!(document.text, "# PBMC\n\nDownload data\n\nRun Scanpy.");
}

#[test]
fn tool_call_response_extracts_only_the_expected_function() {
    let response = serde_json::json!({
        "choices": [{
            "message": {
                "tool_calls": [{
                    "function": {
                        "name": "submit_analysis_plan",
                        "arguments": "{\"title\":\"PBMC\",\"stages\":[]}"
                    }
                }]
            }
        }]
    });

    let value = parse_tool_call_response(&response, "submit_analysis_plan").unwrap();

    assert_eq!(value["title"], "PBMC");
}

#[test]
fn streaming_tool_arguments_survive_chunk_boundaries() {
    let mut accumulator = SseToolCallAccumulator::new("submit_analysis_plan");
    accumulator
        .push_data(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"submit_analysis_plan","arguments":"{\"title\":"}}]}}]}"#)
        .unwrap();
    accumulator
        .push_data(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"PBMC\",\"stages\":[]}"}}]}}]}"#)
        .unwrap();

    let value = accumulator.finish().unwrap();
    assert_eq!(value["title"], "PBMC");
}

#[test]
fn repository_round_trips_profiles_without_secrets_and_orders_events() {
    let repository = Repository::open_in_memory().unwrap();
    let profile = ConnectionProfile {
        id: Uuid::new_v4(),
        label: "analysis server".into(),
        host: "example.org".into(),
        port: 22,
        username: "omicsops".into(),
        authentication: AuthenticationMethod::Password,
        authentication_reference: "ssh/profile-1".into(),
        host_key_fingerprint: None,
    };
    repository.save_connection(&profile).unwrap();

    let loaded = repository.list_connections().unwrap();
    assert_eq!(loaded, vec![profile.clone()]);
    let serialized = serde_json::to_string(&loaded).unwrap();
    assert!(!serialized.contains("hunter2"));

    let run_id = Uuid::new_v4();
    for sequence in [2, 1] {
        repository
            .append_event(&RunEvent {
                sequence,
                timestamp: chrono::Utc::now(),
                run_id,
                stage_id: None,
                step_id: None,
                attempt: 0,
                action: "test".into(),
                state: RunState::Running,
                log_reference: None,
                reason: String::new(),
            })
            .unwrap();
    }
    let events = repository.events_for_run(run_id).unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        [1, 2]
    );
}

#[test]
fn credential_vault_uses_stable_names_and_never_lists_values() {
    let vault = MemoryCredentialVault::default();
    let account = credential_account("ssh", Uuid::nil());

    vault.set(&account, "hunter2").unwrap();

    assert_eq!(vault.get(&account).unwrap().as_deref(), Some("hunter2"));
    assert_eq!(vault.accounts(), vec![account.clone()]);
    vault.delete(&account).unwrap();
    assert_eq!(vault.get(&account).unwrap(), None);
}
