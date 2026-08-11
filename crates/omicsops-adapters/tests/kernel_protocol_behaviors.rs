use omicsops_adapters::kernel::{kernel_driver, validate_capture_paths, validate_kernel_code};
use omicsops_agent::KernelLanguage;

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
