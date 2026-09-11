use omicsops_mcp::{McpServerConfig, McpSessionManager};
use uuid::Uuid;

#[tokio::test]
async fn packaged_science_entry_point_discovers_each_domain_without_network() {
    let manager = McpSessionManager::new();
    let presets = omicsops_desktop_lib::bundled_mcp_commands::list_bundled_mcp_presets();
    let total: usize = presets.iter().map(|preset| preset.tool_count).sum();
    eprintln!(
        "Bundled science MCP: {} domains, {total} tools",
        presets.len()
    );
    for preset in presets {
        let inspection = manager
            .inspect(McpServerConfig {
                project_id: Uuid::new_v4(),
                server_id: Uuid::new_v4(),
                name: preset.name,
                command: env!("CARGO_BIN_EXE_omicsops-desktop").into(),
                args: vec!["--omicsops-bio-mcp".into(), preset.id.clone()],
                cwd: None,
                env: vec![],
                timeout_secs: 15,
            })
            .await
            .unwrap_or_else(|error| panic!("{} failed discovery: {error}", preset.id));
        assert_eq!(inspection.tools.len(), preset.tool_count, "{}", preset.id);
        let expected: std::collections::BTreeSet<_> = omicsops_bio::catalog()
            .into_iter()
            .filter(|(domain, _)| *domain == preset.id)
            .map(|(_, schema)| schema.function.name)
            .collect();
        let actual: std::collections::BTreeSet<_> = inspection
            .tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(actual, expected);
    }
    manager.shutdown().await;
}

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
