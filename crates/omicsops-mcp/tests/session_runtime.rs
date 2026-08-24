#![cfg(feature = "test-fixtures")]

use std::{fs, path::Path};

use omicsops_mcp::{McpRuntimeError, McpServerConfig, McpSessionManager};
use serde_json::json;
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
