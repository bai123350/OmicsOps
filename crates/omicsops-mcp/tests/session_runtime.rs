#![cfg(feature = "test-fixtures")]

use std::{fs, path::Path};

use omicsops_mcp::{McpRuntimeError, McpServerConfig, McpSessionManager};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn config(project_id: Uuid, counter: &Path, timeout_secs: u64) -> McpServerConfig {
    McpServerConfig {
        project_id,
        server_id: Uuid::new_v4(),
        name: "fake".into(),
        command: env!("CARGO_BIN_EXE_omicsops-mcp-fake").into(),
        args: vec![],
        cwd: None,
        env: vec![(
            "OMICSOPS_MCP_FAKE_COUNTER".into(),
            counter.to_string_lossy().into_owned(),
        )],
        timeout_secs,
    }
}

fn count(path: &Path, value: &str) -> usize {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == value)
        .count()
}

fn schema_digest(value: &serde_json::Value) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(value).unwrap()))
}

fn advertised_schema(inspection: &omicsops_mcp::McpInspection, tool: &str) -> String {
    let entry = inspection
        .tools
        .iter()
        .find(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(tool))
        .unwrap();
    schema_digest(
        entry
            .get("inputSchema")
            .or_else(|| entry.get("input_schema"))
            .unwrap(),
    )
}

#[tokio::test]
async fn calls_reuse_one_process_and_projects_are_isolated() {
    let temp = tempfile::tempdir().unwrap();
    let counter = temp.path().join("counter.txt");
    let manager = McpSessionManager::new();
    let first = config(Uuid::new_v4(), &counter, 5);
    manager
        .call(first.clone(), "echo", json!({"value":"one"}), None, None)
        .await
        .unwrap();
    manager
        .call(first.clone(), "echo", json!({"value":"two"}), None, None)
        .await
        .unwrap();
    assert_eq!(count(&counter, "start"), 1);
    assert_eq!(count(&counter, "echo"), 2);

    let mut other_project = first;
    other_project.project_id = Uuid::new_v4();
    manager
        .call(other_project, "echo", json!({"value":"three"}), None, None)
        .await
        .unwrap();
    assert_eq!(count(&counter, "start"), 2);
    manager.shutdown().await;
}

#[tokio::test]
async fn timed_out_tool_is_not_retried_and_invalidates_the_session() {
    let temp = tempfile::tempdir().unwrap();
    let counter = temp.path().join("counter.txt");
    let manager = McpSessionManager::new();
    let config = config(Uuid::new_v4(), &counter, 1);
    let error = manager
        .call(config, "sleep", json!({"millis":1500}), None, None)
        .await
        .unwrap_err();
    assert!(matches!(error, McpRuntimeError::Timeout(1)));
    assert_eq!(count(&counter, "sleep"), 1);
    manager.shutdown().await;
}

#[tokio::test]
async fn crashed_tool_reports_stderr_and_is_not_retried() {
    let temp = tempfile::tempdir().unwrap();
    let counter = temp.path().join("counter.txt");
    let manager = McpSessionManager::new();
    let config = config(Uuid::new_v4(), &counter, 5);
    let error = manager
        .call(config, "crash", json!({}), None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("fake-mcp-crash-marker"), "{error}");
    assert_eq!(count(&counter, "crash"), 1);
    assert_eq!(count(&counter, "start"), 1);
    manager.shutdown().await;
}

#[tokio::test]
async fn plan_read_only_gate_rejects_unannotated_and_false_tools_before_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let counter = temp.path().join("counter.txt");
    let manager = McpSessionManager::new();
    let config = config(Uuid::new_v4(), &counter, 5);
    let inspection = manager.inspect(config.clone()).await.unwrap();
    let catalog = inspection.tool_catalog_sha256.as_str();

    let echo_schema = advertised_schema(&inspection, "echo");
    let missing = manager
        .call_read_only(
            config.clone(),
            "echo",
            json!({"value":"missing"}),
            Some(catalog),
            Some(&echo_schema),
        )
        .await
        .unwrap_err();
    assert!(matches!(missing, McpRuntimeError::ReadOnlyHintMissing));
    assert_eq!(count(&counter, "echo"), 0);

    let false_schema = advertised_schema(&inspection, "non_readonly_echo");
    let not_true = manager
        .call_read_only(
            config.clone(),
            "non_readonly_echo",
            json!({"value":"false"}),
            Some(catalog),
            Some(&false_schema),
        )
        .await
        .unwrap_err();
    assert!(matches!(not_true, McpRuntimeError::ReadOnlyHintNotTrue));
    assert_eq!(count(&counter, "non_readonly_echo"), 0);

    let readonly_schema = advertised_schema(&inspection, "readonly_echo");
    let invocation = manager
        .call_read_only(
            config,
            "readonly_echo",
            json!({"value":"true"}),
            Some(catalog),
            Some(&readonly_schema),
        )
        .await
        .unwrap();
    assert_eq!(invocation.result["content"][0]["text"], "true");
    assert_eq!(count(&counter, "readonly_echo"), 1);
    assert_eq!(count(&counter, "echo"), 0);
    assert_eq!(count(&counter, "non_readonly_echo"), 0);
    manager.shutdown().await;
}

#[tokio::test]
async fn plan_read_only_gate_rejects_catalog_or_schema_snapshot_before_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let counter = temp.path().join("counter.txt");
    let manager = McpSessionManager::new();
    let config = config(Uuid::new_v4(), &counter, 5);
    let inspection = manager.inspect(config.clone()).await.unwrap();
    let schema = advertised_schema(&inspection, "readonly_echo");
    let wrong_catalog = manager
        .call_read_only(
            config.clone(),
            "readonly_echo",
            json!({"value":"wrong catalog"}),
            Some("wrong-catalog"),
            Some(&schema),
        )
        .await
        .unwrap_err();
    assert!(matches!(wrong_catalog, McpRuntimeError::SchemaChanged));
    assert_eq!(count(&counter, "readonly_echo"), 0);

    let mut second_config = config;
    second_config.server_id = Uuid::new_v4();
    let wrong_schema = manager
        .call_read_only(
            second_config,
            "readonly_echo",
            json!({"value":"wrong schema"}),
            Some(&inspection.tool_catalog_sha256),
            Some("wrong-schema"),
        )
        .await
        .unwrap_err();
    assert!(matches!(wrong_schema, McpRuntimeError::SchemaChanged));
    assert_eq!(count(&counter, "readonly_echo"), 0);
    manager.shutdown().await;
}
