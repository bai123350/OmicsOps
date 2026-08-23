use std::{fs, path::Path};

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn tauri_exposes_only_the_v4_agent_runtime() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib = read(manifest.join("src/lib.rs"));
    for retired in [
        "harness_v3::",
        "start_run_v2",
        "resume_run_v2",
        "approve_plan_v2",
        "list_run_events_v2",
        "propose_plan",
    ] {
        assert!(
            !lib.contains(retired),
            "retired Tauri command remains: {retired}"
        );
    }
    assert!(lib.contains("agent_v4::agent_v4_start_planning"));
    assert!(lib.contains("agent_v4::agent_v4_decide_tool_approval"));
}

#[test]
fn frontend_subscribes_only_to_v4_agent_events() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let frontend = manifest.parent().unwrap().join("src");
    let desktop = read(frontend.join("DesktopApp.tsx"));
    let api = read(frontend.join("tauri-api.ts"));
    assert!(desktop.contains("onAgentV4Event"));
    assert!(api.contains("agent-v4-event"));
    assert!(!desktop.contains("agent-run-v3-event"));
    assert!(!desktop.contains("agent-run-event"));
    assert!(!api.contains("agent-run-v3-event"));
    assert!(!api.contains("agent-run-event"));
}

#[test]
fn cargo_workspace_no_longer_contains_the_retired_runner() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = read(manifest.parent().unwrap().join("Cargo.toml"));
    assert!(!workspace.contains("omicsops-runner"));
}
