use omicsops_mcp::{McpServerConfig, McpSessionManager};
use uuid::Uuid;

#[tokio::test]
async fn official_client_inspects_the_bundled_pubmed_stdio_server() {
    let manager = McpSessionManager::new();
    let inspection = manager
        .inspect(McpServerConfig {
            project_id: Uuid::new_v4(),
            server_id: Uuid::new_v4(),
            name: "PubMed integration test".into(),
            command: env!("CARGO_BIN_EXE_omicsops-desktop").into(),
            args: vec!["--omicsops-pubmed-mcp".into()],
            cwd: None,
            env: vec![],
            timeout_secs: 15,
        })
        .await
        .expect("bundled PubMed MCP should initialize");

    let names = inspection
        .tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert!(names.contains(&"pubmed_search"));
    assert!(names.contains(&"pubmed_fetch_records"));
    assert!(!inspection.tool_catalog_sha256.is_empty());
    manager.shutdown().await;
}
