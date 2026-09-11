//! Domain-scoped bundled science MCP service. Stdout is reserved for JSON-RPC.
use std::sync::Arc;

use omicsops_bio::{NativeBio, catalog, domain_metadata};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
};
use serde_json::Value;

/// Only supported credentials are read. Endpoint overrides used by upstream tests
/// are deliberately excluded from the desktop process environment contract.
pub const CREDENTIAL_ENV_NAMES: &[&str] = &[
    "NCBI_API_KEY",
    "NCBI_EMAIL",
    "NCBI_ADMIN_EMAIL",
    "OPERON_CONTACT_EMAIL",
    "OPENALEX_API_KEY",
    "OPENFDA_API_KEY",
    "CIVIC_API_KEY",
];

pub struct BioMcpServer {
    client: NativeBio,
    tools: Vec<Tool>,
    secrets: Vec<String>,
}

/// The same compiled catalog is used for discovery and stdio responses.
pub(crate) fn bundled_tools(domain: &str) -> Vec<Tool> {
    catalog()
        .into_iter()
        .filter(|(slug, _)| *slug == domain)
        .map(|(_, schema)| {
            let function = schema.function;
            let mut annotations = ToolAnnotations::default();
            // Submission creates a remote job and must stay approval-sensitive.
            annotations.read_only_hint = Some(!matches!(
                function.name.as_str(),
                "search_sequence"
                    | "zinc_search_by_id"
                    | "zinc_search_by_supplier"
                    | "zinc_get_3d"
                    | "zinc_random_sample"
            ));
            annotations.destructive_hint = Some(false);
            annotations.open_world_hint = Some(true);
            let mut tool = Tool::new(
                function.name,
                function.description,
                Arc::new(
                    function
                        .parameters
                        .as_object()
                        .expect("bundled tool schema is an object")
                        .clone(),
                ),
            );
            tool.annotations = Some(annotations);
            tool
        })
        .collect()
}

impl BioMcpServer {
    pub fn new(domain: &str, credentials: &[(String, String)]) -> Result<Self, String> {
        if domain_metadata(domain).is_none() {
            return Err("unknown bundled science MCP domain".into());
        }
        let credentials: Vec<_> = credentials
            .iter()
            .filter(|(name, value)| {
                CREDENTIAL_ENV_NAMES.contains(&name.as_str()) && !value.trim().is_empty()
            })
            .cloned()
            .collect();
        let tools = bundled_tools(domain);
        let client = NativeBio::new(&credentials)
            .map_err(|_| "could not initialize bundled scientific HTTP client".to_string())?;
        Ok(Self {
            client,
            tools,
            secrets: credentials.into_iter().map(|(_, value)| value).collect(),
        })
    }

    async fn invoke(&self, name: &str, arguments: Value) -> CallToolResult {
        if !self.tools.iter().any(|tool| tool.name == name) {
            return CallToolResult::error(vec![ContentBlock::text(
                "tool is not available in this science MCP domain",
            )]);
        }
        match self.client.call(name, &arguments).await {
            Ok(mut value) => {
                redact_value(&mut value, &self.secrets);
                CallToolResult::structured(value)
            }
            Err(error) => {
                let mut message = Value::String(error.to_string());
                redact_value(&mut message, &self.secrets);
                CallToolResult::error(vec![ContentBlock::text(
                    message.as_str().unwrap_or("scientific request failed"),
                )])
            }
        }
    }
}

fn redact_value(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in secrets {
                *text = text.replace(secret, "[REDACTED]");
                let encoded: String =
                    url::form_urlencoded::byte_serialize(secret.as_bytes()).collect();
                *text = text.replace(&encoded, "[REDACTED]");
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item, secrets);
            }
        }
        Value::Object(object) => {
            for item in object.values_mut() {
                redact_value(item, secrets);
            }
        }
        _ => {}
    }
}

impl ServerHandler for BioMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tools.clone(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        Ok(self
            .invoke(
                &request.name,
                Value::Object(request.arguments.unwrap_or_default()),
            )
            .await
            .into())
    }
}

pub async fn run_stdio_from_env(domain: &str) -> Result<(), String> {
    let mut credentials: Vec<_> = CREDENTIAL_ENV_NAMES
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| ((*name).to_string(), value))
        })
        .collect();
    if !credentials.iter().any(|(name, _)| name == "NCBI_EMAIL") {
        if let Some((_, email)) = credentials
            .iter()
            .find(|(name, _)| name == "NCBI_ADMIN_EMAIL")
        {
            credentials.push(("NCBI_EMAIL".into(), email.clone()));
        }
    }
    let server = BioMcpServer::new(domain, &credentials)?;
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|_| "science MCP transport initialization failed".to_string())?;
    service
        .waiting()
        .await
        .map_err(|_| "science MCP transport failed".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn discovery_is_strictly_domain_scoped() {
        let domains: std::collections::BTreeSet<_> =
            catalog().into_iter().map(|(domain, _)| domain).collect();
        assert_eq!(domains.len(), 23);
        for domain in domains {
            let server = BioMcpServer::new(domain, &[]).unwrap();
            let expected: Vec<_> = catalog()
                .into_iter()
                .filter(|(slug, _)| *slug == domain)
                .map(|(_, schema)| schema.function.name)
                .collect();
            assert_eq!(
                server
                    .tools
                    .iter()
                    .map(|tool| tool.name.to_string())
                    .collect::<Vec<_>>(),
                expected
            );
        }
        assert!(BioMcpServer::new("unknown", &[]).is_err());
    }

    #[tokio::test]
    async fn cross_domain_unknown_and_malformed_calls_are_tool_errors() {
        let server = BioMcpServer::new("pubmed", &[]).unwrap();
        for (name, args) in [
            ("search_sequence", json!({})),
            ("unknown", json!({})),
            ("search_articles", Value::Null),
            ("search_articles", json!({"__unknown": true})),
            ("search_articles", json!({"query": 42})),
        ] {
            assert_eq!(server.invoke(name, args).await.is_error, Some(true));
        }
    }

    #[test]
    fn sequence_submission_is_not_read_only() {
        let server = BioMcpServer::new("rna", &[]).unwrap();
        let tool = server
            .tools
            .iter()
            .find(|tool| tool.name == "search_sequence")
            .unwrap();
        assert_eq!(
            tool.annotations.as_ref().unwrap().read_only_hint,
            Some(false)
        );
        let zinc = BioMcpServer::new("zinc", &[]).unwrap();
        for tool in zinc
            .tools
            .iter()
            .filter(|tool| tool.name != "zinc_search_by_smiles")
        {
            assert_eq!(
                tool.annotations.as_ref().unwrap().read_only_hint,
                Some(false)
            );
        }
    }

    #[tokio::test]
    async fn mcp_transport_discovers_domain_and_returns_tool_errors() {
        let server = BioMcpServer::new("pubmed", &[]).unwrap();
        let (server_io, client_io) = tokio::io::duplex(65536);
        let server_task = tokio::spawn(async move { server.serve(server_io).await.unwrap() });
        let client = ().serve(client_io).await.unwrap();
        let running_server = server_task.await.unwrap();
        let discovery = client.peer().list_tools(None).await.unwrap();
        assert_eq!(discovery.tools.len(), 7);
        for request in [
            CallToolRequestParams::new("search_sequence"),
            CallToolRequestParams::new("search_articles")
                .with_arguments(json!({"query": 42}).as_object().unwrap().clone()),
        ] {
            let result = client.peer().call_tool(request).await.unwrap();
            assert_eq!(result.is_error, Some(true));
        }
        client.cancel().await.unwrap();
        running_server.cancel().await.unwrap();
    }

    #[test]
    fn credentials_are_allowlisted_and_redacted_in_nested_payloads() {
        let server = BioMcpServer::new(
            "pubmed",
            &[
                ("OTHER_SECRET".into(), "unrelated".into()),
                ("NCBI_API_KEY".into(), "secret&key".into()),
            ],
        )
        .unwrap();
        assert_eq!(server.secrets, ["secret&key"]);
        let mut payload =
            json!({"items": ["secret&key", "https://example.test/?key=secret%26key"]});
        redact_value(&mut payload, &server.secrets);
        assert!(!payload.to_string().contains("secret"));
    }
}
