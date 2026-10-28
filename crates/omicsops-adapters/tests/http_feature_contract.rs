//! Guard the resolved client capabilities, rather than a particular manifest spelling.
//! Without system-proxy, reqwest ignores Windows' proxy and login can take a
//! different route from the system browser even when HTTP(S)_PROXY is unset.
//! Resolve the native runner's dependencies: an offline Linux/macOS test must not
//! require Windows-only crates that its build never downloaded.
#[test]
fn desktop_http_client_resolves_system_proxy_support() {
    let rustc = std::path::Path::new(env!("CARGO"))
        .with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let version = std::process::Command::new(rustc)
        .arg("-vV")
        .output()
        .expect("the Cargo toolchain must include rustc");
    assert!(version.status.success(), "rustc version query must succeed");
    let version = String::from_utf8(version.stdout).unwrap();
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc must report its native host triple");
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version=1",
            "--offline",
            "--locked",
            "--filter-platform",
        ])
        .arg(host)
        .arg("--manifest-path")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("Cargo metadata must be available to the test runner");
    assert!(
        output.status.success(),
        "offline dependency resolution failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let nodes = metadata["resolve"]["nodes"].as_array().unwrap();
    for name in ["omicsops-adapters", "omicsops-desktop"] {
        let package = packages.iter().find(|p| p["name"] == name).unwrap();
        let node = nodes.iter().find(|n| n["id"] == package["id"]).unwrap();
        let client = node["deps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["name"] == "reqwest")
            .unwrap();
        let client_node = nodes.iter().find(|n| n["id"] == client["pkg"]).unwrap();
        assert!(
            client_node["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f == "system-proxy"),
            "{name}'s resolved HTTP client must honor Windows system proxies"
        );
    }
}
