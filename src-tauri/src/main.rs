#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|argument| argument == "--omicsops-pubmed-mcp") {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .and_then(|runtime| {
                runtime
                    .block_on(omicsops_desktop_lib::pubmed_mcp::run_stdio_from_env())
                    .map_err(std::io::Error::other)
            });
        if let Err(error) = result {
            eprintln!("OmicsOps PubMed MCP server failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    omicsops_desktop_lib::run();
}
