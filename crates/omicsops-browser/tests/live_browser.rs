use std::path::PathBuf;

use omicsops_browser::{BrowserRuntime, BrowserSessionKind};

#[tokio::test]
#[ignore = "requires a real installed Chrome/Edge/Chromium plus the packaged OmicsOps extension"]
async fn real_shared_browser_extension_handshake() {
    let data = std::env::var("OMICSOPS_LIVE_BROWSER_DATA_DIR")
        .map(PathBuf::from)
        .expect("OMICSOPS_LIVE_BROWSER_DATA_DIR");
    let extension = std::env::var("OMICSOPS_LIVE_BROWSER_EXTENSION_DIR")
        .map(PathBuf::from)
        .expect("OMICSOPS_LIVE_BROWSER_EXTENSION_DIR");
    let runtime = BrowserRuntime::new(data, extension);
    let status = runtime
        .setup(BrowserSessionKind::Shared, true)
        .await
        .expect("real shared browser extension handshake");
    assert!(status.connected);
}

#[tokio::test]
#[ignore = "requires a real installed Chrome/Edge/Chromium and an isolated disposable profile"]
async fn real_workspace_browser_extension_handshake() {
    let data = std::env::var("OMICSOPS_LIVE_BROWSER_DATA_DIR")
        .map(PathBuf::from)
        .expect("OMICSOPS_LIVE_BROWSER_DATA_DIR");
    let extension = std::env::var("OMICSOPS_LIVE_BROWSER_EXTENSION_DIR")
        .map(PathBuf::from)
        .expect("OMICSOPS_LIVE_BROWSER_EXTENSION_DIR");
    let runtime = BrowserRuntime::new(data, extension);
    let status = runtime
        .setup(BrowserSessionKind::Workspace, true)
        .await
        .expect("real workspace browser extension handshake");
    assert!(status.connected);
}
