use omicsops_adapters::kernel::{kernel_driver, validate_capture_paths, validate_kernel_code};
use omicsops_agent::KernelLanguage;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

#[test]
fn kernel_inputs_reject_empty_code_and_project_escape_paths() {
    assert!(validate_kernel_code(" ").is_err());
    assert!(validate_kernel_code("print('ok')").is_ok());
    for path in [
        "../secret",
        "/etc/passwd",
        "C:\\secret",
        ".omicsops/audit.jsonl",
    ] {
        assert!(
            validate_capture_paths(&[path.into()]).is_err(),
            "accepted {path}"
        );
    }
    assert!(validate_capture_paths(&["results/figure.png".into()]).is_ok());
}

#[test]
fn both_kernel_drivers_are_jsonl_processes() {
    assert!(kernel_driver(KernelLanguage::Python).contains("json.loads"));
    assert!(kernel_driver(KernelLanguage::R).contains("fromJSON"));
}

#[test]
fn python_kernel_driver_uses_utf8_for_unicode_jsonl_on_ascii_hosts() {
    if Command::new("python").arg("--version").output().is_err() {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    let driver = project.path().join("driver.py");
    std::fs::write(&driver, kernel_driver(KernelLanguage::Python)).unwrap();
    let request_id = "3e0f52bd-3268-4cc0-9733-0fe68b191c6c";
    let session_id = "97a1a147-cbbc-4309-8bf7-d93f8f788c58";
    let mut child = Command::new("python")
        .arg("-u")
        .arg(driver)
        .arg(project.path())
        .arg("91783118-8d04-4185-9a33-8feee051007a")
        .arg(session_id)
        .env("PYTHONIOENCODING", "ascii")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        json!({
            "action": "execute",
            "session_id": session_id,
            "request_id": request_id,
            "code": "print('中文 🧬')",
            "capture_paths": [],
        })
    )
    .unwrap();
    let mut events = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let event: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let terminal = matches!(
            event["event"]["kind"].as_str(),
            Some("completed" | "failed")
        );
        events.push(event);
        if terminal {
            break;
        }
    }
    child.kill().unwrap();

    assert!(events.iter().all(|event| event["request_id"] == request_id));
    assert!(events.iter().any(|event| {
        event["event"]["kind"] == "stdout"
            && event["event"]["payload"]
                .as_str()
                .unwrap_or_default()
                .contains("中文 🧬")
    }));
    assert_eq!(events.last().unwrap()["event"]["kind"], "completed");
}
