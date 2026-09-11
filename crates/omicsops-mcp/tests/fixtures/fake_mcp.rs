use std::{env, fs::OpenOptions, io::Write, time::Duration};

use rmcp::{
    ErrorData as McpError, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars::JsonSchema,
    tool, tool_router,
    transport::stdio,
};
use serde::Deserialize;

#[derive(Clone)]
struct FakeServer;

#[derive(Debug, Deserialize, JsonSchema)]
struct EchoParams {
    value: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SleepParams {
    millis: u64,
}

fn record(kind: &str) {
    let Some(path) = env::var_os("OMICSOPS_MCP_FAKE_COUNTER") else {
        return;
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "{kind}").unwrap();
}

#[tool_router(server_handler)]
impl FakeServer {
    #[tool(description = "Echo a value without side effects")]
    async fn echo(
        &self,
        Parameters(params): Parameters<EchoParams>,
    ) -> Result<CallToolResult, McpError> {
        record("echo");
        if params.value == "reject" {
            return Err(McpError::invalid_params("query exceeds limit", None));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(
            params.value,
        )]))
    }

    #[tool(
        description = "Read-only echo with an explicit MCP annotation",
        annotations(read_only_hint = true)
    )]
    async fn readonly_echo(
        &self,
        Parameters(params): Parameters<EchoParams>,
    ) -> Result<CallToolResult, McpError> {
        record("readonly_echo");
        Ok(CallToolResult::success(vec![ContentBlock::text(
            params.value,
        )]))
    }

    #[tool(
        description = "Echo with an explicit non-read-only MCP annotation",
        annotations(read_only_hint = false)
    )]
    async fn non_readonly_echo(
        &self,
        Parameters(params): Parameters<EchoParams>,
    ) -> Result<CallToolResult, McpError> {
        record("non_readonly_echo");
        Ok(CallToolResult::success(vec![ContentBlock::text(
            params.value,
        )]))
    }

    #[tool(description = "Sleep for timeout testing")]
    async fn sleep(
        &self,
        Parameters(params): Parameters<SleepParams>,
    ) -> Result<CallToolResult, McpError> {
        record("sleep");
        tokio::time::sleep(Duration::from_millis(params.millis)).await;
        Ok(CallToolResult::success(vec![ContentBlock::text("awake")]))
    }

    #[tool(description = "Exit after writing a diagnostic to stderr")]
    async fn crash(&self) -> Result<CallToolResult, McpError> {
        record("crash");
        eprintln!("fake-mcp-crash-marker");
        std::process::exit(17)
    }
}

fn main() {
    record("start");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let service = FakeServer.serve(stdio()).await.unwrap();
            service.waiting().await.unwrap();
        });
}
