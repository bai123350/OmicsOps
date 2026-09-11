#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--omicsops-bio-mcp")
    {
        let Some(domain) = arguments.get(index + 1) else {
            eprintln!("OmicsOps science MCP requires a domain");
            std::process::exit(1);
        };
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .and_then(|runtime| {
                runtime
                    .block_on(omicsops_desktop_lib::bio_mcp::run_stdio_from_env(domain))
                    .map_err(std::io::Error::other)
            });
        if let Err(error) = result {
            eprintln!("OmicsOps science MCP server failed: {error}");
            std::process::exit(1);
        }
        return;
    }
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
