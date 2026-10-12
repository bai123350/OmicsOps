use super::*;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const MANIFEST: &str = include_str!("data/v4-scenarios.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportWriteErrorCodeV4 {
    InvalidPath,
    WriteFailed,
}

fn evaluation_target() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("fixed workspace layout")
        .join("target")
}

fn report_bytes(report: &SuiteReportV4) -> Result<Vec<u8>, ReportWriteErrorCodeV4> {
    // The curated suite has closed, public identities. Reject arbitrary string
    // fields instead of allowing malformed fixtures to leak into a CI artifact.
    let allowed_ids = [
        "compact_preserve_sources",
        "correct_mcp_arguments",
        "deny_approval",
        "read_evidence",
        "recover_uncertain",
    ];
    let mut scenarios: Vec<_> = report.scenarios.iter().collect();
    scenarios.sort_by(|a, b| a.id.cmp(&b.id));
    if report.schema_version != 1
        || report.runner_contract_version != 1
        || report.suite_id != "offline-v4-controls"
        || report.suite_hash.len() != 64
        || !report
            .suite_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || scenarios
            .iter()
            .map(|scenario| scenario.id.as_str())
            .collect::<Vec<_>>()
            != allowed_ids
    {
        return Err(ReportWriteErrorCodeV4::WriteFailed);
    }
    let mut value =
        serde_json::to_value(report).map_err(|_| ReportWriteErrorCodeV4::WriteFailed)?;
    value["scenarios"] =
        serde_json::to_value(scenarios).map_err(|_| ReportWriteErrorCodeV4::WriteFailed)?;
    serde_json::to_vec(&value).map_err(|_| ReportWriteErrorCodeV4::WriteFailed)
}

fn report_path_alias(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    false
}

fn report_destination_in(workspace: &Path, path: &Path) -> Result<PathBuf, ReportWriteErrorCodeV4> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || path.file_name().is_none()
    {
        return Err(ReportWriteErrorCodeV4::InvalidPath);
    }
    if !path.is_absolute() && !path.starts_with("target") {
        return Err(ReportWriteErrorCodeV4::InvalidPath);
    }
    let destination = if path.is_absolute() {
        path.to_owned()
    } else {
        workspace.join(path)
    };
    let canonical_workspace = workspace
        .canonicalize()
        .map_err(|_| ReportWriteErrorCodeV4::InvalidPath)?;
    let canonical_target = canonical_workspace.join("target");
    let parent = destination
        .parent()
        .ok_or(ReportWriteErrorCodeV4::InvalidPath)?;
    let ancestor = parent
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .ok_or(ReportWriteErrorCodeV4::InvalidPath)?;
    if !ancestor.is_dir()
        || parent.ancestors().any(|ancestor| {
            std::fs::symlink_metadata(ancestor).is_ok_and(|metadata| report_path_alias(&metadata))
        })
        || std::fs::symlink_metadata(&destination)
            .is_ok_and(|metadata| report_path_alias(&metadata))
    {
        return Err(ReportWriteErrorCodeV4::InvalidPath);
    }
    // Reconstruct the missing suffix from a validated existing ancestor before
    // creating anything. This also works with a custom Cargo build directory.
    let future_parent = ancestor
        .canonicalize()
        .map_err(|_| ReportWriteErrorCodeV4::InvalidPath)?
        .join(
            parent
                .strip_prefix(ancestor)
                .map_err(|_| ReportWriteErrorCodeV4::InvalidPath)?,
        );
    if !future_parent.starts_with(&canonical_target) {
        return Err(ReportWriteErrorCodeV4::InvalidPath);
    }
    std::fs::create_dir_all(&future_parent).map_err(|_| ReportWriteErrorCodeV4::WriteFailed)?;
    let canonical_parent = future_parent
        .canonicalize()
        .map_err(|_| ReportWriteErrorCodeV4::InvalidPath)?;
    if !canonical_parent.starts_with(canonical_target) {
        return Err(ReportWriteErrorCodeV4::InvalidPath);
    }
    // Also reject a destination symlink rather than replacing a path alias.
    if std::fs::symlink_metadata(&destination).is_ok_and(|metadata| report_path_alias(&metadata)) {
        return Err(ReportWriteErrorCodeV4::InvalidPath);
    }
    Ok(canonical_parent.join(
        destination
            .file_name()
            .ok_or(ReportWriteErrorCodeV4::InvalidPath)?,
    ))
}

fn write_report_atomically(
    bytes: &[u8],
    path: &Path,
    persist: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<(), ReportWriteErrorCodeV4>,
) -> Result<(), ReportWriteErrorCodeV4> {
    let target = evaluation_target();
    let workspace = target.parent().ok_or(ReportWriteErrorCodeV4::InvalidPath)?;
    write_report_atomically_in(bytes, path, workspace, persist)
}

fn write_report_atomically_in(
    bytes: &[u8],
    path: &Path,
    workspace: &Path,
    persist: impl FnOnce(tempfile::NamedTempFile, &Path) -> Result<(), ReportWriteErrorCodeV4>,
) -> Result<(), ReportWriteErrorCodeV4> {
    let destination = report_destination_in(workspace, path)?;
    let parent = destination
        .parent()
        .ok_or(ReportWriteErrorCodeV4::InvalidPath)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| ReportWriteErrorCodeV4::WriteFailed)?;
    temporary
        .write_all(bytes)
        .map_err(|_| ReportWriteErrorCodeV4::WriteFailed)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| ReportWriteErrorCodeV4::WriteFailed)?;
    persist(temporary, &destination)
}

fn emit_report_using(
    report: &SuiteReportV4,
    path: Option<&Path>,
    write: impl FnOnce(&[u8], &Path) -> Result<(), ReportWriteErrorCodeV4>,
) -> Result<(), ReportWriteErrorCodeV4> {
    let bytes = report_bytes(report)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ReportWriteErrorCodeV4::WriteFailed)?;
    println!("OMICSOPS_V4_SCENARIOS={text}");
    if let Some(path) = path {
        write(&bytes, path)?;
    }
    Ok(())
}

fn emit_report(report: &SuiteReportV4, path: Option<&Path>) -> Result<(), ReportWriteErrorCodeV4> {
    emit_report_using(report, path, |bytes, path| {
        write_report_atomically(bytes, path, |temporary, destination| {
            temporary
                .persist(destination)
                .map(|_| ())
                .map_err(|_| ReportWriteErrorCodeV4::WriteFailed)
        })
    })
}

fn assert_suite_passed(report: &SuiteReportV4) {
    assert_eq!(report.failed, 0, "V4 scenario suite failed");
}

#[tokio::test(start_paused = true)]
async fn scenario_evaluation_suite() {
    let manifest = parse_suite(MANIFEST).expect("invalid V4 scenario manifest");
    // All five fixtures are inline in this manifest; there are no external paths.
    let hash = suite_hash(&manifest, &BTreeMap::new()).expect("invalid V4 suite hash input");
    let report = run_suite(&manifest, &hash).await;
    let path = std::env::var_os("OMICSOPS_EVAL_REPORT_PATH").map(PathBuf::from);
    if let Err(code) = emit_report(&report, path.as_deref()) {
        panic!("V4 report emission failed: {code:?}");
    }
    assert_suite_passed(&report);
}

// These exercise the real file writer: a missing opt-in, destructive replacement,
// early failure, or serialization of raw fixture text must break the contract.
fn report_test_directory() -> tempfile::TempDir {
    let target = evaluation_target();
    std::fs::create_dir_all(&target).unwrap();
    tempfile::tempdir_in(target).unwrap()
}

#[tokio::test(start_paused = true)]
async fn report_explicit_creates_absent_target_without_default_write() {
    let workspace = report_test_directory();
    let manifest = parse_suite(MANIFEST).unwrap();
    let hash = suite_hash(&manifest, &BTreeMap::new()).unwrap();
    let report = run_suite(&manifest, &hash).await;
    let path = Path::new("target/evaluation/report.json");
    let emit = |requested_path| {
        emit_report_using(&report, requested_path, |bytes, path| {
            write_report_atomically_in(bytes, path, workspace.path(), |temporary, destination| {
                temporary
                    .persist(destination)
                    .map(|_| ())
                    .map_err(|_| ReportWriteErrorCodeV4::WriteFailed)
            })
        })
    };
    emit(None).unwrap();
    assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
    assert_eq!(
        emit(Some(Path::new("docs/report.json"))),
        Err(ReportWriteErrorCodeV4::InvalidPath)
    );
    assert_eq!(std::fs::read_dir(workspace.path()).unwrap().count(), 0);
    emit(Some(path)).unwrap();
    assert_eq!(
        std::fs::read(workspace.path().join(path)).unwrap(),
        report_bytes(&report).unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn report_default_writes_no_file() {
    let directory = report_test_directory();
    let before_files = std::fs::read_dir(directory.path()).unwrap().count();
    let manifest = parse_suite(MANIFEST).unwrap();
    let hash = suite_hash(&manifest, &BTreeMap::new()).unwrap();
    let report = run_suite(&manifest, &hash).await;
    emit_report(&report, None).unwrap();
    emit_report_using(&report, None, |_, _| {
        panic!("default must never dispatch a file write")
    })
    .unwrap();
    let files_without_explicit_path = std::fs::read_dir(directory.path()).unwrap().count();
    assert_eq!(files_without_explicit_path, before_files);
}

#[tokio::test(start_paused = true)]
async fn report_atomic_replace_retains_previous_on_failure() {
    let directory = report_test_directory();
    let path = directory.path().join("report.json");
    let manifest = parse_suite(MANIFEST).unwrap();
    let hash = suite_hash(&manifest, &BTreeMap::new()).unwrap();
    let report = run_suite(&manifest, &hash).await;
    let old_bytes = b"previous complete report";
    std::fs::write(&path, old_bytes).unwrap();
    let bytes = report_bytes(&report).unwrap();
    // Fail exactly at the persistence boundary, after the temporary file was written.
    assert_eq!(
        write_report_atomically(&bytes, &path, |_, _| Err(
            ReportWriteErrorCodeV4::WriteFailed
        )),
        Err(ReportWriteErrorCodeV4::WriteFailed)
    );
    let old_bytes_after_failed_replace = std::fs::read(&path).unwrap();
    assert_eq!(old_bytes_after_failed_replace, old_bytes);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    emit_report(&report, Some(&path)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    emit_report(&report, Some(&path)).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);

    let existing_directory = directory.path().join("directory");
    std::fs::create_dir(&existing_directory).unwrap();
    let sentinel = existing_directory.join("retain.json");
    std::fs::write(&sentinel, old_bytes).unwrap();
    assert_eq!(
        emit_report(&report, Some(&existing_directory)),
        Err(ReportWriteErrorCodeV4::WriteFailed)
    );
    assert_eq!(std::fs::read(sentinel).unwrap(), old_bytes);
    assert_eq!(
        emit_report(&report, Some(&path.join("child.json"))),
        Err(ReportWriteErrorCodeV4::InvalidPath)
    );
    assert_eq!(
        emit_report(&report, Some(std::path::Path::new("docs/report.json"))),
        Err(ReportWriteErrorCodeV4::InvalidPath)
    );
    assert_eq!(
        emit_report(&report, Some(std::path::Path::new("target/../report.json"))),
        Err(ReportWriteErrorCodeV4::InvalidPath)
    );
    assert_eq!(
        emit_report(&report, Some(std::path::Path::new(""))),
        Err(ReportWriteErrorCodeV4::InvalidPath)
    );
    let nested = directory.path().join("new-parent").join("report.json");
    emit_report(&report, Some(&nested)).unwrap();
    assert_eq!(std::fs::read(nested).unwrap(), bytes);
}

#[tokio::test(start_paused = true)]
async fn report_failure_still_emits_all_scenarios() {
    let directory = report_test_directory();
    let path = directory.path().join("failed.json");
    let mut manifest = parse_suite(MANIFEST).unwrap();
    let hash = suite_hash(&manifest, &BTreeMap::new()).unwrap();
    manifest.scenarios[0].model_script.clear();
    let failed_report = run_suite(&manifest, &hash).await;
    assert_eq!(failed_report.scenarios.len(), 5);
    emit_report(&failed_report, Some(&path)).unwrap();
    let saved: SuiteReportV4 = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved.scenarios.len(), 5);
    assert_eq!((saved.passed, saved.failed), (4, 1));
    assert!(std::panic::catch_unwind(|| assert_suite_passed(&failed_report)).is_err());
}

#[tokio::test(start_paused = true)]
async fn report_stability_and_redaction() {
    let directory = report_test_directory();
    let path = directory.path().join("stable.json");
    let mut manifest = parse_suite(MANIFEST).unwrap();
    let hash = suite_hash(&manifest, &BTreeMap::new()).unwrap();
    let raw_provider_error = "raw-provider-error-sk-secret-C:\\private\\host";
    manifest.scenarios[0].objective = raw_provider_error.into();
    manifest.scenarios[3].tool_script[0].call_id = raw_provider_error.into();
    manifest.scenarios[3].model_script[0].public_text = raw_provider_error.into();
    let mut first = run_suite(&manifest, &hash).await;
    first.scenarios.reverse();
    emit_report(&first, Some(&path)).unwrap();
    let report_bytes_run1 = std::fs::read(&path).unwrap();
    emit_report(&run_suite(&manifest, &hash).await, Some(&path)).unwrap();
    let report_bytes_run2 = std::fs::read(&path).unwrap();
    assert_eq!(report_bytes_run1, report_bytes_run2);
    let failed_report_json = String::from_utf8(report_bytes_run1).unwrap();
    assert!(!failed_report_json.contains(raw_provider_error));
    let value: Value = serde_json::from_str(&failed_report_json).unwrap();
    let ids: Vec<_> = value["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "compact_preserve_sources",
            "correct_mcp_arguments",
            "deny_approval",
            "read_evidence",
            "recover_uncertain"
        ]
    );
    for forbidden in [
        "arguments",
        "outcome",
        "reasoning",
        "transcript",
        "objective",
        "public_text",
        "timestamp",
        "duration",
        "attempt_id",
    ] {
        assert!(!failed_report_json.contains(&format!("\"{forbidden}\":")));
    }
    first.scenarios[0].id = raw_provider_error.into();
    assert_eq!(
        emit_report(&first, Some(&path)),
        Err(ReportWriteErrorCodeV4::WriteFailed)
    );
    first.scenarios[0].id = "compact_preserve_sources".into();
    first.suite_id = raw_provider_error.into();
    assert_eq!(
        emit_report(&first, Some(&path)),
        Err(ReportWriteErrorCodeV4::WriteFailed)
    );
    first.suite_id = "offline-v4-controls".into();
    first.suite_hash = raw_provider_error.into();
    assert_eq!(
        emit_report(&first, Some(&path)),
        Err(ReportWriteErrorCodeV4::WriteFailed)
    );
    assert_eq!(std::fs::read(&path).unwrap(), report_bytes_run2);
}

#[tokio::test(start_paused = true)]
async fn scenarios_run_all_five_and_measure_actual_dispatch() {
    let manifest = parse_suite(MANIFEST).unwrap();
    let hash = suite_hash(&manifest, &BTreeMap::new()).unwrap();
    let report = run_suite(&manifest, &hash).await;
    assert_eq!(report.scenarios.len(), 5);
    assert_eq!((report.passed, report.failed), (5, 0), "{report:?}");
    let denied = &report.scenarios[1];
    let uncertain = &report.scenarios[2];
    assert_eq!(denied.tool_dispatches, 0);
    assert_eq!(uncertain.tool_dispatches, 0);
    assert_eq!(uncertain.recovered_outcomes, 1);
    assert_eq!(report.scenarios[3].tool_dispatches, 2);
    assert_eq!(report.scenarios[3].tool_proposals, 4);
    assert_eq!(report.scenarios[0].tool_dispatches, 0);
    assert_eq!(report.scenarios[4].tool_dispatches, 0);
}

#[tokio::test(start_paused = true)]
async fn scenarios_continue_after_failure_and_require_exact_script_consumption() {
    let mut manifest = parse_suite(MANIFEST).unwrap();
    manifest.scenarios[0].model_script.clear();
    let report = run_suite(&manifest, "fixture-hash").await;
    assert_eq!(report.scenarios.len(), 5);
    assert_eq!(
        report.scenarios[0].failure_reason_code,
        Some(ScenarioFailureCodeV4::ScriptExhausted)
    );
    assert_eq!(report.passed, 4);
    let mut extra = parse_suite(MANIFEST).unwrap().scenarios.remove(0);
    extra.model_script.push(extra.model_script[0].clone());
    assert_eq!(
        run_scenario(&extra).await.failure_reason_code,
        Some(ScenarioFailureCodeV4::UnusedScript)
    );
    let mut extra_tool = parse_suite(MANIFEST).unwrap().scenarios.remove(0);
    extra_tool
        .tool_script
        .push(parse_suite(MANIFEST).unwrap().scenarios[3].tool_script[0].clone());
    assert_eq!(
        run_scenario(&extra_tool).await.failure_reason_code,
        Some(ScenarioFailureCodeV4::UnusedScript)
    );
    let mut exhausted_tool = parse_suite(MANIFEST).unwrap().scenarios.remove(3);
    exhausted_tool.tool_script.pop();
    assert_eq!(
        run_scenario(&exhausted_tool).await.failure_reason_code,
        Some(ScenarioFailureCodeV4::ScriptExhausted)
    );
    let mut wrong = parse_suite(MANIFEST).unwrap().scenarios.remove(3);
    wrong.tool_script[0].call_id = "mismatched-script".into();
    assert_eq!(
        run_scenario(&wrong).await.failure_reason_code,
        Some(ScenarioFailureCodeV4::InvalidFixture)
    );
}

#[tokio::test(start_paused = true)]
async fn scenario_fault_injections_break_expected_assertions() {
    let manifest = parse_suite(MANIFEST).unwrap();
    for scenario in &manifest.scenarios {
        for assertion in &scenario.assertions {
            let result = run_scenario_with_fault(scenario, Some(assertion.name)).await;
            assert_eq!(
                result.status,
                ScenarioStatusV4::Failed,
                "{} {:?}",
                scenario.id,
                assertion.name
            );
            assert!(result.assertions_failed > 0);
            assert_eq!(
                result.failure_reason_code,
                Some(ScenarioFailureCodeV4::AssertionFailed)
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn scenario_report_omits_sensitive_payloads() {
    let mut manifest = parse_suite(MANIFEST).unwrap();
    let secret_sentinel = "secret-sentinel-sk-credential-C:\\private-host";
    for scenario in &mut manifest.scenarios {
        scenario.objective = secret_sentinel.into();
        for turn in &mut scenario.model_script {
            turn.public_text = secret_sentinel.into();
            for call in &mut turn.tool_calls {
                call.arguments["sensitive_fixture"] = json!(secret_sentinel);
            }
        }
        for step in &mut scenario.tool_script {
            if let Some(outcome) = &mut step.outcome {
                outcome.model_content = secret_sentinel.into();
                outcome.provenance.push(secret_sentinel.into());
                outcome.data["sensitive_fixture"] = json!(secret_sentinel);
            }
        }
    }
    // Exercise the fixed failure-code path with a raw sensitive script ID too.
    manifest.scenarios[3].tool_script[0].call_id = secret_sentinel.into();
    let report = run_suite(&manifest, "safe-hash").await;
    assert_eq!(report.scenarios.len(), 5);
    assert_eq!(
        report.scenarios[3].failure_reason_code,
        Some(ScenarioFailureCodeV4::InvalidFixture)
    );
    let serialized_report = serde_json::to_string(&report).unwrap();
    assert!(!serialized_report.contains(secret_sentinel));
    for forbidden in [
        "public_text",
        "arguments",
        "model_content",
        "reasoning",
        "provenance",
        "objective",
        "transcript",
    ] {
        assert!(!serialized_report.contains(&format!("\"{forbidden}\":")));
    }
}

#[tokio::test(start_paused = true)]
async fn scenario_fault_fixtures_fail_real_receipt_budget_and_mcp_classification() {
    let manifest = parse_suite(MANIFEST).unwrap();
    let mut receipt = manifest.scenarios[2].clone();
    receipt.tool_script[1].outcome = None;
    receipt.tool_script[1].failure = Some(ScriptFailureV4::MissingReceipt);
    let failed = run_scenario(&receipt).await;
    assert_eq!(failed.status, ScenarioStatusV4::Failed);
    assert_eq!(failed.recovered_outcomes, 0);
    assert!(failed.assertions_failed >= 2);

    let mut compact = manifest.scenarios[4].clone();
    compact.limits.request_max_bytes = Some(usize::MAX);
    assert!(run_scenario(&compact).await.assertions_failed > 0);

    let mut mcp = manifest.scenarios[3].clone();
    mcp.tool_script[1].failure = None;
    mcp.tool_script[1].outcome = Some(ToolOutcomeV4 {
        call_id: "call-4".into(),
        tool_id: "use_mcp_tool".into(),
        succeeded: false,
        model_content: "business fault incorrectly presented as preflight".into(),
        data: json!({"operation_dispatched":false,"result":{"isError":true,"structuredContent":{"error_kind":"upstream_failure","operation_dispatched":false}}}),
        provenance: vec![],
    });
    let failed = run_scenario(&mcp).await;
    assert_eq!(
        failed.failure_reason_code,
        Some(ScenarioFailureCodeV4::AssertionFailed)
    );
    assert_eq!(failed.assertions_failed, 1);

    let mut missing_seed = manifest.scenarios[2].clone();
    missing_seed.seed_events.clear();
    assert_eq!(
        run_scenario(&missing_seed).await.failure_reason_code,
        Some(ScenarioFailureCodeV4::InvalidFixture)
    );
}

#[tokio::test(start_paused = true)]
async fn scenarios_report_is_stable_across_random_event_metadata() {
    let manifest = parse_suite(MANIFEST).unwrap();
    let hash = suite_hash(&manifest, &BTreeMap::new()).unwrap();
    assert_eq!(
        serde_json::to_value(run_suite(&manifest, &hash).await).unwrap(),
        serde_json::to_value(run_suite(&manifest, &hash).await).unwrap()
    );
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteManifestV4 {
    schema_version: u8,
    suite_id: String,
    runner_contract_version: u32,
    scenarios: Vec<ScenarioManifestV4>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioManifestV4 {
    id: String,
    label: String,
    scenario_kind: ScenarioKindV4,
    objective: String,
    seed: ScenarioSeedV4,
    model_script: Vec<ModelTurnV4>,
    tool_script: Vec<ScriptedToolStepV4>,
    seed_events: Vec<ScenarioSeedEventV4>,
    limits: ScenarioLimitsV4,
    assertions: Vec<AssertionSpecV4>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioKindV4 {
    ReadEvidence,
    DenyApproval,
    RecoverUncertain,
    CorrectMcpArguments,
    CompactPreserveSources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioSeedV4 {
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioLimitsV4 {
    context_max_bytes: Option<usize>,
    max_turns: Option<u32>,
    max_tool_calls: Option<u32>,
    checkpoint_recent_events: Option<usize>,
    #[serde(default)]
    request_max_bytes: Option<usize>,
    #[serde(default)]
    tool_schema_padding_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptedToolStepV4 {
    stage: ToolScriptStageV4,
    call_id: String,
    tool_id: String,
    outcome: Option<ToolOutcomeV4>,
    failure: Option<ScriptFailureV4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToolScriptStageV4 {
    Prepare,
    Execute,
    Recover,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScriptFailureV4 {
    ArgumentPreflight,
    BusinessFailure,
    UncertainDispatch,
    MissingReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ScenarioSeedEventV4 {
    SuccessfulTool { outcome: ToolOutcomeV4 },
    McpDirectory { outcome: ToolOutcomeV4 },
    UncertainDispatch { call: ToolCallV4 },
    UserGuidance { message_id: Uuid, markdown: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertionSpecV4 {
    name: ScenarioAssertionV4,
    expected: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioAssertionV4 {
    ValidEvidenceRef,
    RejectedForeignRef,
    DeniedNoDispatch,
    UncertainNoReplay,
    RecoveredOnce,
    PreflightPreservesServer,
    CorrectedMcpSuccess,
    DiscoveredOnce,
    FullBudgetAdmitted,
    SourceRetrievable,
    FrozenAuthorityRetained,
    ModelAttempts,
    ToolProposals,
    ToolDispatches,
    RecoveredOutcomes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManifestErrorCodeV4 {
    InvalidVersion,
    UnknownField,
    DuplicateId,
    InvalidSeed,
    InvalidScript,
    InvalidAssertion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioStatusV4 {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScenarioFailureCodeV4 {
    AssertionFailed,
    ScriptExhausted,
    UnusedScript,
    InvalidFixture,
    CoreRejected,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioOutcomeV4 {
    id: String,
    status: ScenarioStatusV4,
    model_attempts: u64,
    tool_proposals: u64,
    tool_dispatches: u64,
    recovered_outcomes: u64,
    assertions_passed: u64,
    assertions_failed: u64,
    failure_reason_code: Option<ScenarioFailureCodeV4>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteReportV4 {
    schema_version: u8,
    suite_id: String,
    suite_hash: String,
    runner_contract_version: u32,
    passed: u64,
    failed: u64,
    scenarios: Vec<ScenarioOutcomeV4>,
}

#[derive(Default)]
struct ScenarioCountersV4 {
    model_attempts: u64,
    tool_proposals: u64,
    tool_dispatches: u64,
    recovered_outcomes: u64,
    directory_discoveries: u64,
    dispatched_calls: Vec<String>,
    recovery_calls: Vec<String>,
    failure: Option<ScenarioFailureCodeV4>,
}

impl ScenarioCountersV4 {
    fn fail(&mut self, code: ScenarioFailureCodeV4) {
        self.failure.get_or_insert(code);
    }
}

struct ScenarioModelV4 {
    script: Mutex<VecDeque<ModelTurnV4>>,
    counters: Arc<Mutex<ScenarioCountersV4>>,
    validated: Mutex<Vec<(ModelRequestV4, bool)>>,
    streamed: Mutex<Vec<ModelRequestV4>>,
    request_limit: usize,
}

#[async_trait]
impl ModelPortV4 for ScenarioModelV4 {
    fn validate_request(&self, request: &ModelRequestV4) -> Result<(), ModelFailureV4> {
        let fits = serde_json::to_vec(request)
            .map(|v| v.len() <= self.request_limit)
            .unwrap_or(false);
        self.validated.lock().unwrap().push((request.clone(), fits));
        if fits {
            Ok(())
        } else {
            Err(ModelFailureV4::permanent(
                omicsops_protocol::ModelErrorClassV4::InvalidRequest,
                "fixture request budget",
            ))
        }
    }

    async fn stream(
        &self,
        request: ModelRequestV4,
        on_event: &mut (dyn FnMut(ModelStreamEventV4) + Send),
    ) -> Result<ModelTurnV4, ModelFailureV4> {
        self.streamed.lock().unwrap().push(request);
        self.counters.lock().unwrap().model_attempts += 1;
        let Some(mut turn) = self.script.lock().unwrap().pop_front() else {
            self.counters
                .lock()
                .unwrap()
                .fail(ScenarioFailureCodeV4::ScriptExhausted);
            return Err(ModelFailureV4::permanent(
                omicsops_protocol::ModelErrorClassV4::InvalidRequest,
                "fixture script exhausted",
            ));
        };
        for call in &mut turn.tool_calls {
            if call.tool_id == "agent.complete" {
                if let Some(evidence) = call
                    .arguments
                    .pointer_mut("/criteria/0/evidence/0/sequence")
                {
                    if *evidence == 0 {
                        let request = self.streamed.lock().unwrap();
                        let context: Value = serde_json::from_str(&request.last().unwrap().context)
                            .unwrap_or(Value::Null);
                        if let Some(sequence) = context["recent_events"]
                            .as_array()
                            .and_then(|events| {
                                events
                                    .iter()
                                    .rev()
                                    .find(|e| e["event"]["outcome"]["succeeded"] == true)
                            })
                            .and_then(|e| e["sequence"].as_u64())
                        {
                            *evidence = json!(sequence);
                        } else {
                            self.counters
                                .lock()
                                .unwrap()
                                .fail(ScenarioFailureCodeV4::InvalidFixture);
                        }
                    }
                }
            }
        }
        self.counters.lock().unwrap().tool_proposals += turn.tool_calls.len() as u64;
        if !turn.public_text.is_empty() {
            on_event(ModelStreamEventV4::TextDelta(turn.public_text.clone()));
        }
        Ok(turn)
    }
}

struct ScenarioToolsV4 {
    script: Mutex<VecDeque<ScriptedToolStepV4>>,
    counters: Arc<Mutex<ScenarioCountersV4>>,
    schema_padding: usize,
}

impl ScenarioToolsV4 {
    fn take(
        &self,
        stage: ToolScriptStageV4,
        call: &ToolCallV4,
    ) -> Result<ScriptedToolStepV4, String> {
        let Some(step) = self.script.lock().unwrap().pop_front() else {
            self.counters
                .lock()
                .unwrap()
                .fail(ScenarioFailureCodeV4::ScriptExhausted);
            return Err("fixture script exhausted".into());
        };
        if step.stage != stage || step.call_id != call.call_id || step.tool_id != call.tool_id {
            self.counters
                .lock()
                .unwrap()
                .fail(ScenarioFailureCodeV4::InvalidFixture);
            return Err("fixture script mismatch".into());
        }
        Ok(step)
    }
}

#[async_trait]
impl ToolPortV4 for ScenarioToolsV4 {
    fn descriptors(&self, _: RunModeV4) -> Vec<ToolDescriptorV4> {
        ["read_file", "runtime.execute", "search_mcp_tools", "use_mcp_tool", context_views::READ_RESULT_TOOL].into_iter().map(|id| ToolDescriptorV4 {
            id: id.into(), description: id.into(),
            input_schema: json!({"type":"object", "description":"s".repeat(if id == "read_file" { self.schema_padding } else { 0 })}),
            effect: self.effect(id).unwrap(),
        }).collect()
    }

    fn effect(&self, id: &str) -> Option<ToolEffectV4> {
        match id {
            "runtime.execute" => Some(ToolEffectV4::Runtime),
            "read_file" | "search_mcp_tools" | "use_mcp_tool" | context_views::READ_RESULT_TOOL => {
                Some(ToolEffectV4::ReadOnly)
            }
            _ => None,
        }
    }

    async fn prepare_call(&self, call: &ToolCallV4) -> Result<Option<ToolOutcomeV4>, String> {
        // Only deferred preflight outcomes need script entries. Readiness itself
        // is a pure host check and does not dispatch or consume an execute step.
        let has_preflight = self
            .script
            .lock()
            .unwrap()
            .front()
            .is_some_and(|s| s.stage == ToolScriptStageV4::Prepare);
        if !has_preflight {
            return Ok(None);
        }
        let step = self.take(ToolScriptStageV4::Prepare, call)?;
        match step.failure {
            Some(ScriptFailureV4::ArgumentPreflight) => Ok(Some(ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: false,
                model_content: "fixture argument preflight".into(),
                data: json!({"operation_dispatched":false,"result":{"isError":true,"structuredContent":{"error_kind":"mcp_arguments","operation_dispatched":false}}}),
                provenance: vec![],
            })),
            None => Ok(step.outcome),
            _ => {
                self.counters
                    .lock()
                    .unwrap()
                    .fail(ScenarioFailureCodeV4::InvalidFixture);
                Err("invalid preparation fixture".into())
            }
        }
    }

    async fn execute(&self, _: RunModeV4, call: ToolCallV4) -> Result<ToolOutcomeV4, String> {
        {
            let mut counters = self.counters.lock().unwrap();
            counters.tool_dispatches += 1;
            counters.directory_discoveries += u64::from(call.tool_id == "search_mcp_tools");
            counters.dispatched_calls.push(call.call_id.clone());
        }
        let step = self.take(ToolScriptStageV4::Execute, &call)?;
        match step.failure {
            None => step.outcome.ok_or_else(|| "missing fixture outcome".into()),
            Some(ScriptFailureV4::BusinessFailure) => Ok(ToolOutcomeV4 {
                call_id: call.call_id,
                tool_id: call.tool_id,
                succeeded: false,
                model_content: "fixture business failure".into(),
                data: json!({"result":{"isError":true}}),
                provenance: vec![],
            }),
            Some(ScriptFailureV4::UncertainDispatch) => Err("fixture uncertain dispatch".into()),
            _ => {
                self.counters
                    .lock()
                    .unwrap()
                    .fail(ScenarioFailureCodeV4::InvalidFixture);
                Err("invalid execute fixture".into())
            }
        }
    }

    async fn recover_result(&self, call: &ToolCallV4) -> Result<Option<ToolOutcomeV4>, String> {
        self.counters
            .lock()
            .unwrap()
            .recovery_calls
            .push(call.call_id.clone());
        let step = self.take(ToolScriptStageV4::Recover, call)?;
        match step.failure {
            Some(ScriptFailureV4::MissingReceipt) => Ok(None),
            None => {
                let outcome = step
                    .outcome
                    .ok_or_else(|| "missing receipt fixture".to_owned())?;
                self.counters.lock().unwrap().recovered_outcomes += 1;
                Ok(Some(outcome))
            }
            _ => {
                self.counters
                    .lock()
                    .unwrap()
                    .fail(ScenarioFailureCodeV4::InvalidFixture);
                Err("invalid recovery fixture".into())
            }
        }
    }
}

fn scenario_spec(scenario: &ScenarioManifestV4) -> Result<RunSpecV4, ScenarioFailureCodeV4> {
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: scenario.objective.clone(),
        steps: vec!["fixture control".into()],
        completion_criteria: vec!["verified output".into()],
        requested_capabilities: BTreeSet::from(
            [
                "read_file",
                "runtime.execute",
                "search_mcp_tools",
                "use_mcp_tool",
                context_views::READ_RESULT_TOOL,
            ]
            .map(String::from),
        ),
    };
    let compute = omicsops_protocol::ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "local".into(),
        backend_kind: ComputeBackendKindV4::Local,
        autonomy_mode: omicsops_protocol::AutonomyModeV4::Supervised,
        approval_policy: if matches!(scenario.scenario_kind, ScenarioKindV4::DenyApproval) {
            ApprovalPolicyV4::RequestApproval
        } else {
            ApprovalPolicyV4::RiskBased
        },
        environment: "system".into(),
        network_policy: omicsops_protocol::NetworkPolicyV4::HostInherited,
        container_image: None,
    };
    let profile = Uuid::from_u128(0x40000000000040008000000000000001);
    let hash = RunSpecV4::approval_hash_for(
        scenario.seed.run_id,
        scenario.seed.project_id,
        scenario.seed.conversation_id,
        profile,
        &plan,
        &compute,
    )
    .map_err(|_| ScenarioFailureCodeV4::InvalidFixture)?;
    let mut spec = RunSpecV4::freeze_with_compute(
        scenario.seed.run_id,
        scenario.seed.project_id,
        scenario.seed.conversation_id,
        profile,
        plan,
        compute,
        &hash,
        Utc::now(),
    )
    .map_err(|_| ScenarioFailureCodeV4::InvalidFixture)?;
    spec.conversation_preferences = Some(ConversationAgentPreferencesV4 {
        auto_review: false,
        ..Default::default()
    });
    spec.spec_hash = Some(
        spec.calculate_spec_hash()
            .map_err(|_| ScenarioFailureCodeV4::InvalidFixture)?,
    );
    Ok(spec)
}

fn scenario_limits(scenario: &ScenarioManifestV4) -> AgentLimitsV4 {
    AgentLimitsV4 {
        context_max_bytes: scenario.limits.context_max_bytes.unwrap_or(256 * 1024),
        max_turns: scenario.limits.max_turns.unwrap_or(8),
        max_tool_calls: scenario.limits.max_tool_calls.unwrap_or(8),
        checkpoint_recent_events: scenario.limits.checkpoint_recent_events.unwrap_or(4),
        max_model_retries: 0,
        ..Default::default()
    }
}

fn seed_scenario(store: &MemoryStore, spec: &RunSpecV4, seeds: &[ScenarioSeedEventV4]) {
    seed_execution(store, spec);
    for seed in seeds {
        match seed {
            ScenarioSeedEventV4::SuccessfulTool { outcome }
            | ScenarioSeedEventV4::McpDirectory { outcome } => {
                append_test_event(
                    store,
                    spec.run_id,
                    AgentEventKindV4::ToolFinished {
                        outcome: outcome.clone(),
                    },
                );
            }
            ScenarioSeedEventV4::UncertainDispatch { call } => {
                append_test_event(
                    store,
                    spec.run_id,
                    AgentEventKindV4::ToolRequested { call: call.clone() },
                );
                append_test_event(
                    store,
                    spec.run_id,
                    AgentEventKindV4::ToolDispatchStarted {
                        call_id: call.call_id.clone(),
                        tool_id: call.tool_id.clone(),
                        effect: ToolEffectV4::Runtime,
                        idempotency_key: call.call_id.clone(),
                    },
                );
            }
            ScenarioSeedEventV4::UserGuidance {
                message_id,
                markdown,
            } => {
                append_test_event(
                    store,
                    spec.run_id,
                    AgentEventKindV4::GuidanceConsumed {
                        message_id: *message_id,
                        markdown: markdown.clone(),
                    },
                );
            }
        }
    }
}

fn result_call(source: &AgentEventV4, field: &str) -> ToolCallV4 {
    ToolCallV4 {
        call_id: "fixture-read-back".into(),
        tool_id: context_views::READ_RESULT_TOOL.into(),
        arguments: json!({"sequence":source.sequence,"event_hash":source.event_hash,"field":field,"offset":0,"limit":8192}),
    }
}

fn read_back(
    spec: &RunSpecV4,
    events: &[AgentEventV4],
    source: &AgentEventV4,
    field: &str,
    expected: &str,
) -> bool {
    let mut call = result_call(source, field);
    let mut content = String::new();
    loop {
        let Ok(page) = context_views::read_result(spec, events, &call) else {
            return false;
        };
        let Some(text) = page["content"].as_str() else {
            return false;
        };
        content.push_str(text);
        if content.len() > expected.len() {
            return false;
        }
        match page["next_offset"].as_u64() {
            Some(next) if next > call.arguments["offset"].as_u64().unwrap_or(0) => {
                call.arguments["offset"] = json!(next)
            }
            Some(_) => return false,
            None => return content == expected,
        }
    }
}

fn observation_bool(
    observations: &mut BTreeMap<ScenarioAssertionV4, Value>,
    assertion: ScenarioAssertionV4,
    value: bool,
) {
    observations.insert(assertion, json!(value));
}

fn mcp_preflight_preserves_business_budget(events: &[AgentEventV4]) -> bool {
    // Probe the public classifier with one and then two business failures after
    // the actual core preflight trace. If preflight consumed a slot, the first
    // probe already exhausts the server. These probes never execute a tool.
    let mut probes = events.to_vec();
    if probes.is_empty() {
        return false;
    }
    let mut result = true;
    for index in 1..=2 {
        let call = ToolCallV4 {
            call_id: format!("classification-{index}"),
            tool_id: "use_mcp_tool".into(),
            arguments: json!({"server_id":"fixture-server"}),
        };
        for kind in [
            AgentEventKindV4::ToolRequested { call: call.clone() },
            AgentEventKindV4::ToolFinished {
                outcome: ToolOutcomeV4 {
                    call_id: call.call_id,
                    tool_id: call.tool_id,
                    succeeded: false,
                    model_content: "business classification probe".into(),
                    data: json!({"result":{"isError":true}}),
                    provenance: vec![],
                },
            },
        ] {
            let next = AgentEventV4::next(probes.last().unwrap(), Utc::now(), kind);
            probes.push(next);
        }
        result &= failed_mcp_servers(&probes).contains("fixture-server") == (index == 2);
    }
    result
}

// A narrow test seam can corrupt an observed fixture fact without modifying
// production code. Every listed assertion must fail under its corresponding
// corruption; missing observations never silently satisfy an assertion.
fn evaluate_assertions(
    scenario: &ScenarioManifestV4,
    observations: &BTreeMap<ScenarioAssertionV4, Value>,
    outcome: &mut ScenarioOutcomeV4,
    fault: Option<ScenarioAssertionV4>,
) {
    for assertion in &scenario.assertions {
        let mut actual = observations.get(&assertion.name).cloned();
        if fault == Some(assertion.name) {
            actual = actual.map(|value| {
                if let Some(boolean) = value.as_bool() {
                    json!(!boolean)
                } else {
                    json!(value.as_u64().unwrap_or(0).wrapping_add(1))
                }
            });
        }
        if actual.as_ref() == Some(&assertion.expected) {
            outcome.assertions_passed += 1;
        } else {
            outcome.assertions_failed += 1;
        }
    }
    if outcome.assertions_failed > 0 {
        outcome
            .failure_reason_code
            .get_or_insert(ScenarioFailureCodeV4::AssertionFailed);
    }
    outcome.status = if outcome.failure_reason_code.is_none() {
        ScenarioStatusV4::Passed
    } else {
        ScenarioStatusV4::Failed
    };
}

async fn run_scenario(scenario: &ScenarioManifestV4) -> ScenarioOutcomeV4 {
    run_scenario_with_fault(scenario, None).await
}

async fn run_suite(manifest: &SuiteManifestV4, hash: &str) -> SuiteReportV4 {
    let mut scenarios = Vec::new();
    for scenario in &manifest.scenarios {
        scenarios.push(run_scenario(scenario).await);
    }
    let passed = scenarios
        .iter()
        .filter(|s| s.status == ScenarioStatusV4::Passed)
        .count() as u64;
    SuiteReportV4 {
        schema_version: manifest.schema_version,
        suite_id: manifest.suite_id.clone(),
        suite_hash: hash.into(),
        runner_contract_version: manifest.runner_contract_version,
        passed,
        failed: scenarios.len() as u64 - passed,
        scenarios,
    }
}

async fn run_scenario_with_fault(
    scenario: &ScenarioManifestV4,
    fault: Option<ScenarioAssertionV4>,
) -> ScenarioOutcomeV4 {
    let counters = Arc::new(Mutex::new(ScenarioCountersV4::default()));
    let mut observations = BTreeMap::new();
    let mut outcome = ScenarioOutcomeV4 {
        id: scenario.id.clone(),
        status: ScenarioStatusV4::Failed,
        model_attempts: 0,
        tool_proposals: 0,
        tool_dispatches: 0,
        recovered_outcomes: 0,
        assertions_passed: 0,
        assertions_failed: 0,
        failure_reason_code: None,
    };
    let spec = match scenario_spec(scenario) {
        Ok(spec) => spec,
        Err(code) => {
            outcome.failure_reason_code = Some(code);
            evaluate_assertions(scenario, &observations, &mut outcome, fault);
            return outcome;
        }
    };
    let store = MemoryStore::default();
    let recovery = matches!(scenario.scenario_kind, ScenarioKindV4::RecoverUncertain);
    if recovery
        && (scenario.seed_events.len() != 2
            || !scenario
                .seed_events
                .iter()
                .all(|seed| matches!(seed, ScenarioSeedEventV4::UncertainDispatch { .. })))
    {
        outcome.failure_reason_code = Some(ScenarioFailureCodeV4::InvalidFixture);
        evaluate_assertions(scenario, &observations, &mut outcome, fault);
        return outcome;
    }
    if recovery {
        seed_scenario(&store, &spec, &scenario.seed_events[1..]);
    } else {
        seed_scenario(&store, &spec, &scenario.seed_events);
    }
    let sources = store.load_direct(spec.run_id).unwrap();
    let source = sources
        .iter()
        .find(|event| matches!(event.event, AgentEventKindV4::ToolFinished { .. }))
        .cloned();
    let mut script = scenario.model_script.clone();
    // Closed source binding: only the private fixture marker is replaced, and
    // the exact source event remains independently verified by the core reader.
    for turn in &mut script {
        for call in &mut turn.tool_calls {
            if call.tool_id == context_views::READ_RESULT_TOOL
                && call.arguments["event_hash"] == "seed_source"
            {
                if let Some(source) = &source {
                    call.arguments["sequence"] = json!(source.sequence);
                    call.arguments["event_hash"] = json!(source.event_hash);
                } else {
                    counters
                        .lock()
                        .unwrap()
                        .fail(ScenarioFailureCodeV4::InvalidFixture);
                }
            }
        }
    }
    let model = ScenarioModelV4 {
        script: Mutex::new(script.into()),
        counters: counters.clone(),
        validated: Mutex::new(vec![]),
        streamed: Mutex::new(vec![]),
        request_limit: scenario.limits.request_max_bytes.unwrap_or(usize::MAX),
    };
    let tools = ScenarioToolsV4 {
        script: Mutex::new(scenario.tool_script.clone().into()),
        counters: counters.clone(),
        schema_padding: scenario.limits.tool_schema_padding_bytes.unwrap_or(0),
    };
    let core = AgentCoreV4 {
        model: &model,
        tools: &tools,
        events: &store,
        science: None,
    };
    let limits = scenario_limits(scenario);
    let cancelled = AtomicBool::new(false);

    if recovery {
        let unknown = MemoryStore::default();
        seed_scenario(&unknown, &spec, &scenario.seed_events[..1]);
        let unknown_core = AgentCoreV4 {
            model: &model,
            tools: &tools,
            events: &unknown,
            science: None,
        };
        let unknown_result = unknown_core
            .execute_with_limits(&spec, limits, &cancelled)
            .await;
        let unknown_again = unknown_core
            .execute_with_limits(&spec, limits, &cancelled)
            .await;
        observation_bool(
            &mut observations,
            ScenarioAssertionV4::UncertainNoReplay,
            matches!(
                unknown_result,
                Err(AgentCoreErrorV4::UncertainSideEffect(_))
            ) && matches!(unknown_again, Err(AgentCoreErrorV4::UncertainSideEffect(_)))
                && counters.lock().unwrap().tool_dispatches == 0
                && model.streamed.lock().unwrap().is_empty()
                && unknown
                    .load_direct(spec.run_id)
                    .unwrap()
                    .iter()
                    .any(|e| matches!(e.event, AgentEventKindV4::ToolDispatchUncertain { .. })),
        );
    }

    let initial = core.execute_with_limits(&spec, limits, &cancelled).await;
    let accepted = if matches!(scenario.scenario_kind, ScenarioKindV4::DenyApproval) {
        let waiting = matches!(initial, Err(AgentCoreErrorV4::WaitingForApproval));
        let approval = store
            .load_direct(spec.run_id)
            .unwrap()
            .into_iter()
            .find_map(|event| match event.event {
                AgentEventKindV4::ToolApprovalRequested { request } => Some(request),
                _ => None,
            });
        if let Some(request) = approval {
            let properly_scoped = core
                .approval_request(&spec, request.call.clone(), request.effect)
                .is_ok_and(|expected| {
                    expected.call_hash == request.call_hash
                        && expected.approval_id == request.approval_id
                })
                && request.scope_hash.is_none()
                && request.mode == RunModeV4::Execute;
            append_test_event(
                &store,
                spec.run_id,
                AgentEventKindV4::ToolApprovalDecided {
                    approval_id: request.approval_id,
                    call_hash: request.call_hash,
                    decision: ToolApprovalDecisionV4::Denied,
                },
            );
            let resumed = core.execute_with_limits(&spec, limits, &cancelled).await;
            let events = store.load_direct(spec.run_id).unwrap();
            observation_bool(&mut observations, ScenarioAssertionV4::DeniedNoDispatch, waiting && properly_scoped && matches!(resumed, Err(AgentCoreErrorV4::WaitingForInput))
                && counters.lock().unwrap().tool_dispatches == 0 && !events.iter().any(|e| matches!(e.event, AgentEventKindV4::ToolDispatchStarted { .. }))
                && events.iter().any(|e| matches!(&e.event, AgentEventKindV4::ToolFinished { outcome } if outcome.data["error_kind"] == "approval_denied"))
                && spec.validate_integrity().is_ok() && spec.plan.requested_capabilities == scenario_spec(scenario).unwrap().plan.requested_capabilities);
            waiting && matches!(resumed, Err(AgentCoreErrorV4::WaitingForInput))
        } else {
            false
        }
    } else {
        initial.is_ok()
    };
    if !accepted {
        counters
            .lock()
            .unwrap()
            .fail(ScenarioFailureCodeV4::CoreRejected);
    }
    let events = store.load_direct(spec.run_id).unwrap();
    match scenario.scenario_kind {
        ScenarioKindV4::ReadEvidence => {
            if let Some(source) = &source {
                let expected = match &source.event {
                    AgentEventKindV4::ToolFinished { outcome } => outcome.model_content.as_str(),
                    _ => "",
                };
                let source_read_back_verified = read_back(&spec, &events, source, "model_content", expected)
                    && events.iter().all(|e| e.verify().is_ok()) && events.windows(2).all(|p| p[1].previous_hash == p[0].event_hash)
                    && events.iter().any(|e| matches!(&e.event, AgentEventKindV4::ToolFinished { outcome } if outcome.tool_id == context_views::READ_RESULT_TOOL && outcome.succeeded && outcome.data["content"] == expected));
                observation_bool(
                    &mut observations,
                    ScenarioAssertionV4::ValidEvidenceRef,
                    source_read_back_verified && accepted,
                );
                let call = result_call(source, "model_content");
                let mut corrupted = events.clone();
                if let Some(event) = corrupted.iter_mut().find(|e| e.sequence == source.sequence) {
                    if let AgentEventKindV4::ToolFinished { outcome } = &mut event.event {
                        // Retain the issued hash while changing its payload:
                        // this requires integrity verification, not just lookup.
                        outcome.model_content.push_str("tampered");
                    }
                }
                let foreign_rejected = (0..3).all(|scope| {
                    let mut foreign = spec.clone();
                    match scope {
                        0 => foreign.project_id = Uuid::nil(),
                        1 => foreign.conversation_id = Uuid::nil(),
                        _ => foreign.run_id = Uuid::nil(),
                    }
                    context_views::read_result(&foreign, &events, &call).is_err()
                });
                let no_data: CompletionProposalV4 =
                    serde_json::from_value(completion_arguments(1)).unwrap();
                observation_bool(
                    &mut observations,
                    ScenarioAssertionV4::RejectedForeignRef,
                    foreign_rejected
                        && context_views::read_result(&spec, &corrupted, &call).is_err()
                        && !verify_completion_v4(
                            &spec,
                            &ScientificStateV4::new(spec.project_id),
                            &events,
                            &no_data,
                        )
                        .passed,
                );
            }
        }
        ScenarioKindV4::RecoverUncertain => {
            let checked = core
                .recover_interrupted_dispatches(&spec, limits, &cancelled)
                .await
                .is_ok();
            let counters = counters.lock().unwrap();
            observation_bool(&mut observations, ScenarioAssertionV4::RecoveredOnce, checked && accepted && counters.recovered_outcomes == 1 && counters.recovery_calls.len() == 2
                && events.iter().filter(|e| matches!(&e.event, AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == "receipt-3")).count() == 1);
        }
        ScenarioKindV4::CorrectMcpArguments => {
            let before_corrected: Vec<_> = events.iter().take_while(|e| !matches!(&e.event, AgentEventKindV4::ToolRequested { call } if call.call_id == "corrected-4")).cloned().collect();
            let counters = counters.lock().unwrap();
            observation_bool(&mut observations, ScenarioAssertionV4::PreflightPreservesServer, failed_mcp_servers(&before_corrected).is_empty() && mcp_preflight_preserves_business_budget(&before_corrected)
                && !counters.dispatched_calls.iter().any(|c| c == "call-4")
                && before_corrected.iter().any(|e| matches!(&e.event, AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == "call-4" && !outcome.succeeded && outcome.data["operation_dispatched"] == false)));
            observation_bool(&mut observations, ScenarioAssertionV4::CorrectedMcpSuccess, accepted && events.iter().any(|e| matches!(&e.event, AgentEventKindV4::ToolFinished { outcome } if outcome.call_id == "corrected-4" && outcome.succeeded)));
            let requests = model.streamed.lock().unwrap();
            observation_bool(
                &mut observations,
                ScenarioAssertionV4::DiscoveredOnce,
                counters.directory_discoveries == 1
                    && requests
                        .first()
                        .is_some_and(|r| r.tools.iter().any(|t| t.id == "search_mcp_tools"))
                    && requests.iter().skip(1).all(|r| {
                        !r.tools.iter().any(|t| t.id == "search_mcp_tools")
                            && r.tools.iter().any(|t| t.id == "use_mcp_tool")
                            && r.system.contains("Do not repeat search_mcp_tools")
                    }),
            );
        }
        ScenarioKindV4::CompactPreserveSources => {
            let checks = model.validated.lock().unwrap();
            let streams = model.streamed.lock().unwrap();
            observation_bool(
                &mut observations,
                ScenarioAssertionV4::FullBudgetAdmitted,
                accepted
                    && checks
                        .iter()
                        .any(|(r, fits)| !fits && r.context.len() < limits.context_max_bytes)
                    && streams
                        .iter()
                        .all(|r| serde_json::to_vec(r).unwrap().len() <= model.request_limit)
                    && !streams.is_empty()
                    && store.archives.lock().unwrap().len() >= 1,
            );
            let checkpoint = events
                .iter()
                .find(|e| matches!(e.event, AgentEventKindV4::ContextCheckpointed { .. }));
            let retrieved = checkpoint.is_some_and(|e| match &e.event {
                AgentEventKindV4::ContextCheckpointed { checkpoint } => {
                    read_back(
                        &spec,
                        &events,
                        e,
                        "scientific_state",
                        &checkpoint.scientific_state.to_string(),
                    ) && checkpoint.completion_criteria == spec.plan.completion_criteria
                }
                _ => false,
            });
            observation_bool(
                &mut observations,
                ScenarioAssertionV4::SourceRetrievable,
                retrieved
                    && source.as_ref().is_some_and(|source| match &source.event {
                        AgentEventKindV4::ToolFinished { outcome } => {
                            streams
                                .first()
                                .is_some_and(|request| request.context.contains(&source.event_hash))
                                && read_back(
                                    &spec,
                                    &events,
                                    source,
                                    "model_content",
                                    &outcome.model_content,
                                )
                        }
                        _ => false,
                    }),
            );
            observation_bool(
                &mut observations,
                ScenarioAssertionV4::FrozenAuthorityRetained,
                spec.validate_integrity().is_ok()
                    && streams.iter().all(|r| {
                        serde_json::from_str::<Value>(&r.context).is_ok_and(|context| {
                            context["frozen_plan"] == json!(spec.plan)
                                && context["compute_selection"] == json!(spec.compute_selection)
                                && context["active_guidance"]
                                    .to_string()
                                    .contains("Preserve original source references.")
                        })
                    })
                    && !streams.is_empty(),
            );
        }
        ScenarioKindV4::DenyApproval => {}
    }
    if !model.script.lock().unwrap().is_empty() || !tools.script.lock().unwrap().is_empty() {
        counters
            .lock()
            .unwrap()
            .fail(ScenarioFailureCodeV4::UnusedScript);
    }
    let counters = counters.lock().unwrap();
    outcome.model_attempts = counters.model_attempts;
    outcome.tool_proposals = counters.tool_proposals;
    outcome.tool_dispatches = counters.tool_dispatches;
    outcome.recovered_outcomes = counters.recovered_outcomes;
    outcome.failure_reason_code = counters.failure;
    for (name, value) in [
        (ScenarioAssertionV4::ModelAttempts, outcome.model_attempts),
        (ScenarioAssertionV4::ToolProposals, outcome.tool_proposals),
        (ScenarioAssertionV4::ToolDispatches, outcome.tool_dispatches),
        (
            ScenarioAssertionV4::RecoveredOutcomes,
            outcome.recovered_outcomes,
        ),
    ] {
        observations.insert(name, json!(value));
    }
    evaluate_assertions(scenario, &observations, &mut outcome, fault);
    outcome
}

// Protocol payloads deliberately accept extensible product data. At this private
// fixture boundary their envelopes must be strict; arguments/data remain JSON.
fn check_fields(value: &Value, allowed: &[&str]) -> Result<(), ManifestErrorCodeV4> {
    let object = value
        .as_object()
        .ok_or(ManifestErrorCodeV4::InvalidScript)?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ManifestErrorCodeV4::UnknownField);
    }
    Ok(())
}

fn check_call(value: &Value) -> Result<(), ManifestErrorCodeV4> {
    check_fields(value, &["call_id", "tool_id", "arguments"])
}

fn check_outcome(value: &Value) -> Result<(), ManifestErrorCodeV4> {
    check_fields(
        value,
        &[
            "call_id",
            "tool_id",
            "succeeded",
            "model_content",
            "data",
            "provenance",
        ],
    )
}

fn check_continuation(value: &Value) -> Result<(), ManifestErrorCodeV4> {
    check_fields(value, &["items"])?;
    for item in value["items"]
        .as_array()
        .ok_or(ManifestErrorCodeV4::InvalidScript)?
    {
        match item["kind"].as_str() {
            Some("assistant_text") => check_fields(item, &["kind", "text"])?,
            Some("tool_call") => {
                check_fields(item, &["kind", "call"])?;
                check_call(&item["call"])?;
            }
            Some("responses_reasoning") => {
                check_fields(item, &["kind", "id", "encrypted_content"])?
            }
            // A model fixture cannot fabricate a host tool result.
            _ => return Err(ManifestErrorCodeV4::InvalidScript),
        }
    }
    Ok(())
}

fn parse_suite(source: &str) -> Result<SuiteManifestV4, ManifestErrorCodeV4> {
    let value: Value =
        serde_json::from_str(source).map_err(|_| ManifestErrorCodeV4::InvalidScript)?;
    check_fields(
        &value,
        &[
            "schema_version",
            "suite_id",
            "runner_contract_version",
            "scenarios",
        ],
    )?;
    if value["schema_version"].as_u64() != Some(1)
        || value["runner_contract_version"].as_u64() != Some(1)
    {
        return Err(ManifestErrorCodeV4::InvalidVersion);
    }
    for scenario in value["scenarios"]
        .as_array()
        .ok_or(ManifestErrorCodeV4::InvalidScript)?
    {
        check_fields(
            scenario,
            &[
                "id",
                "label",
                "scenario_kind",
                "objective",
                "seed",
                "model_script",
                "tool_script",
                "seed_events",
                "limits",
                "assertions",
            ],
        )?;
        let seed = &scenario["seed"];
        if !seed.is_object() {
            return Err(ManifestErrorCodeV4::InvalidSeed);
        }
        check_fields(seed, &["project_id", "conversation_id", "run_id"])?;
        serde_json::from_value::<ScenarioSeedV4>(seed.clone())
            .map_err(|_| ManifestErrorCodeV4::InvalidSeed)?;
        check_fields(
            &scenario["limits"],
            &[
                "context_max_bytes",
                "max_turns",
                "max_tool_calls",
                "checkpoint_recent_events",
                "request_max_bytes",
                "tool_schema_padding_bytes",
            ],
        )?;
        for assertion in scenario["assertions"]
            .as_array()
            .ok_or(ManifestErrorCodeV4::InvalidAssertion)?
        {
            check_fields(assertion, &["name", "expected"])?;
            serde_json::from_value::<AssertionSpecV4>(assertion.clone())
                .map_err(|_| ManifestErrorCodeV4::InvalidAssertion)?;
        }
        for turn in scenario["model_script"]
            .as_array()
            .ok_or(ManifestErrorCodeV4::InvalidScript)?
        {
            check_fields(
                turn,
                &["public_text", "tool_calls", "provider_continuation"],
            )?;
            if !turn["provider_continuation"].is_null() {
                check_continuation(&turn["provider_continuation"])?;
            }
            for call in turn["tool_calls"]
                .as_array()
                .ok_or(ManifestErrorCodeV4::InvalidScript)?
            {
                check_call(call)?;
            }
        }
        for step in scenario["tool_script"]
            .as_array()
            .ok_or(ManifestErrorCodeV4::InvalidScript)?
        {
            check_fields(step, &["stage", "call_id", "tool_id", "outcome", "failure"])?;
            if !step["outcome"].is_null() {
                check_outcome(&step["outcome"])?;
            }
        }
        for event in scenario["seed_events"]
            .as_array()
            .ok_or(ManifestErrorCodeV4::InvalidScript)?
        {
            match event["kind"].as_str() {
                Some("successful_tool" | "mcp_directory") => {
                    check_fields(event, &["kind", "outcome"])?;
                    check_outcome(&event["outcome"])?;
                }
                Some("uncertain_dispatch") => {
                    check_fields(event, &["kind", "call"])?;
                    check_call(&event["call"])?;
                }
                Some("user_guidance") => check_fields(event, &["kind", "message_id", "markdown"])?,
                _ => return Err(ManifestErrorCodeV4::InvalidScript),
            }
        }
    }
    let manifest = serde_json::from_value(value).map_err(|_| ManifestErrorCodeV4::InvalidScript)?;
    validate_suite(&manifest)?;
    Ok(manifest)
}

fn valid_call(call: &ToolCallV4) -> bool {
    !call.call_id.trim().is_empty() && !call.tool_id.trim().is_empty() && call.arguments.is_object()
}

fn valid_outcome(outcome: &ToolOutcomeV4) -> bool {
    !outcome.call_id.trim().is_empty() && !outcome.tool_id.trim().is_empty()
}

fn validate_suite(manifest: &SuiteManifestV4) -> Result<(), ManifestErrorCodeV4> {
    if manifest.schema_version != 1 || manifest.runner_contract_version != 1 {
        return Err(ManifestErrorCodeV4::InvalidVersion);
    }
    if manifest.suite_id.trim().is_empty() || manifest.scenarios.is_empty() {
        return Err(ManifestErrorCodeV4::InvalidScript);
    }
    let mut ids = BTreeSet::new();
    let mut conversations = BTreeSet::new();
    let mut runs = BTreeSet::new();
    for scenario in &manifest.scenarios {
        if !ids.insert(normalize_lf(&scenario.id)) {
            return Err(ManifestErrorCodeV4::DuplicateId);
        }
        let seed = &scenario.seed;
        if seed.project_id.is_nil()
            || seed.conversation_id.is_nil()
            || seed.run_id.is_nil()
            || !conversations.insert(seed.conversation_id)
            || !runs.insert(seed.run_id)
        {
            return Err(ManifestErrorCodeV4::InvalidSeed);
        }
        if scenario.id.trim().is_empty()
            || scenario.label.trim().is_empty()
            || scenario.objective.trim().is_empty()
            || scenario.model_script.is_empty()
            || scenario.limits.context_max_bytes == Some(0)
            || scenario.limits.max_turns == Some(0)
        {
            return Err(ManifestErrorCodeV4::InvalidScript);
        }
        let mut calls = BTreeSet::new();
        for turn in &scenario.model_script {
            if turn
                .tool_calls
                .iter()
                .any(|call| !valid_call(call) || !calls.insert(&call.call_id))
                || turn
                    .provider_continuation
                    .as_ref()
                    .is_some_and(|continuation| continuation.validate().is_err())
            {
                return Err(ManifestErrorCodeV4::InvalidScript);
            }
        }
        let mut steps = BTreeSet::new();
        for step in &scenario.tool_script {
            if step.call_id.trim().is_empty()
                || step.tool_id.trim().is_empty()
                || !steps.insert((step.stage, &step.call_id))
                || step.outcome.is_some() == step.failure.is_some()
                || step.outcome.as_ref().is_some_and(|outcome| {
                    !valid_outcome(outcome)
                        || outcome.call_id != step.call_id
                        || outcome.tool_id != step.tool_id
                })
                || matches!(step.failure, Some(ScriptFailureV4::ArgumentPreflight))
                    && step.stage != ToolScriptStageV4::Prepare
                || matches!(step.failure, Some(ScriptFailureV4::UncertainDispatch))
                    && step.stage != ToolScriptStageV4::Execute
                || matches!(step.failure, Some(ScriptFailureV4::MissingReceipt))
                    && step.stage != ToolScriptStageV4::Recover
            {
                return Err(ManifestErrorCodeV4::InvalidScript);
            }
        }
        let mut guidance_ids = BTreeSet::new();
        for event in &scenario.seed_events {
            let valid = match event {
                ScenarioSeedEventV4::SuccessfulTool { outcome }
                | ScenarioSeedEventV4::McpDirectory { outcome } => {
                    valid_outcome(outcome) && outcome.succeeded
                }
                ScenarioSeedEventV4::UncertainDispatch { call } => valid_call(call),
                ScenarioSeedEventV4::UserGuidance {
                    message_id,
                    markdown,
                } => {
                    !message_id.is_nil()
                        && guidance_ids.insert(message_id)
                        && !markdown.trim().is_empty()
                }
            };
            if !valid {
                return Err(ManifestErrorCodeV4::InvalidScript);
            }
        }
        let mut assertions = BTreeSet::new();
        if scenario.assertions.is_empty() {
            return Err(ManifestErrorCodeV4::InvalidAssertion);
        }
        for assertion in &scenario.assertions {
            let numeric = matches!(
                assertion.name,
                ScenarioAssertionV4::ModelAttempts
                    | ScenarioAssertionV4::ToolProposals
                    | ScenarioAssertionV4::ToolDispatches
                    | ScenarioAssertionV4::RecoveredOutcomes
            );
            if !assertions.insert(assertion.name)
                || (if numeric {
                    assertion.expected.as_u64().is_none()
                } else {
                    !assertion.expected.is_boolean()
                })
            {
                return Err(ManifestErrorCodeV4::InvalidAssertion);
            }
        }
    }
    Ok(())
}

fn normalize_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn canonical_value(value: Value) -> Result<Value, ManifestErrorCodeV4> {
    Ok(match value {
        Value::String(text) => Value::String(normalize_lf(&text)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(canonical_value)
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(values) => {
            let mut sorted = BTreeMap::new();
            for (key, value) in values {
                let key = normalize_lf(&key);
                if sorted.insert(key, canonical_value(value)?).is_some() {
                    return Err(ManifestErrorCodeV4::InvalidScript);
                }
            }
            Value::Object(sorted.into_iter().collect())
        }
        other => other,
    })
}

fn suite_hash(
    manifest: &SuiteManifestV4,
    fixtures: &BTreeMap<String, String>,
) -> Result<String, ManifestErrorCodeV4> {
    use sha2::{Digest, Sha256};
    validate_suite(manifest)?;
    if fixtures.keys().any(|path| {
        path.contains(['\\', ':', '\r', '\n'])
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
    }) {
        return Err(ManifestErrorCodeV4::InvalidScript);
    }
    let mut sorted = manifest.clone();
    sorted
        .scenarios
        .sort_by_cached_key(|scenario| normalize_lf(&scenario.id));
    let canonical = canonical_value(json!({"manifest": sorted, "fixtures": fixtures}))?;
    let bytes = serde_json::to_vec(&canonical).map_err(|_| ManifestErrorCodeV4::InvalidScript)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

#[test]
fn manifest_rejects_invalid_version_fields_ids_seeds_and_actions() {
    let source: Value = serde_json::from_str(MANIFEST).unwrap();
    let suite = parse_suite(MANIFEST).unwrap();
    assert_eq!(suite.scenarios.len(), 5);
    let cases = [
        (
            "/schema_version",
            json!(2),
            ManifestErrorCodeV4::InvalidVersion,
        ),
        (
            "/runner_contract_version",
            json!(2),
            ManifestErrorCodeV4::InvalidVersion,
        ),
        (
            "/scenarios/1/id",
            source["scenarios"][0]["id"].clone(),
            ManifestErrorCodeV4::DuplicateId,
        ),
        (
            "/scenarios/0/seed/run_id",
            json!("invalid"),
            ManifestErrorCodeV4::InvalidSeed,
        ),
        (
            "/scenarios/0/seed/run_id",
            json!(Uuid::nil()),
            ManifestErrorCodeV4::InvalidSeed,
        ),
        (
            "/scenarios/0/seed",
            Value::Null,
            ManifestErrorCodeV4::InvalidSeed,
        ),
        (
            "/scenarios/1/seed",
            source["scenarios"][0]["seed"].clone(),
            ManifestErrorCodeV4::InvalidSeed,
        ),
        (
            "/scenarios/0/scenario_kind",
            json!("shell"),
            ManifestErrorCodeV4::InvalidScript,
        ),
        (
            "/scenarios/0/seed_events",
            json!([{"kind":"shell","command":"run"}]),
            ManifestErrorCodeV4::InvalidScript,
        ),
        (
            "/scenarios/0/model_script",
            json!([]),
            ManifestErrorCodeV4::InvalidScript,
        ),
        (
            "/scenarios/0/assertions/0/name",
            json!("approved"),
            ManifestErrorCodeV4::InvalidAssertion,
        ),
        (
            "/scenarios/0/assertions/0/expected",
            json!(1),
            ManifestErrorCodeV4::InvalidAssertion,
        ),
        (
            "/scenarios/1/assertions/1/expected",
            json!(true),
            ManifestErrorCodeV4::InvalidAssertion,
        ),
        (
            "/scenarios/3/tool_script/0/failure",
            json!("business_failure"),
            ManifestErrorCodeV4::InvalidScript,
        ),
        (
            "/scenarios/3/tool_script/0/outcome",
            Value::Null,
            ManifestErrorCodeV4::InvalidScript,
        ),
        (
            "/scenarios/3/tool_script/0/outcome/call_id",
            json!("foreign"),
            ManifestErrorCodeV4::InvalidScript,
        ),
    ];
    for (pointer, replacement, expected) in cases {
        let mut invalid = source.clone();
        *invalid.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            parse_suite(&invalid.to_string()).unwrap_err(),
            expected,
            "{pointer}"
        );
    }
    for pointer in [
        "",
        "/scenarios/0",
        "/scenarios/0/seed",
        "/scenarios/0/limits",
        "/scenarios/0/assertions/0",
        "/scenarios/0/model_script/0",
        "/scenarios/1/model_script/0/tool_calls/0",
        "/scenarios/3/tool_script/0",
        "/scenarios/0/seed_events/0",
        "/scenarios/0/seed_events/0/outcome",
    ] {
        let mut invalid = source.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("self_approved".into(), json!(true));
        assert_eq!(
            parse_suite(&invalid.to_string()).unwrap_err(),
            ManifestErrorCodeV4::UnknownField,
            "{pointer}"
        );
    }
    let mut missing = source.clone();
    missing["scenarios"][0]
        .as_object_mut()
        .unwrap()
        .remove("tool_script");
    assert_eq!(
        parse_suite(&missing.to_string()).unwrap_err(),
        ManifestErrorCodeV4::InvalidScript
    );
    for continuation in [
        json!({"items": [], "self_approved": true}),
        json!({"items": [{"kind":"assistant_text", "text":"fixture", "self_approved": true}]}),
        json!({"items": [{"kind":"tool_call", "call":{"call_id":"continuation", "tool_id":"read_file", "arguments":{}, "self_approved":true}}]}),
    ] {
        let mut invalid = source.clone();
        invalid["scenarios"][0]["model_script"][0]["provider_continuation"] = continuation;
        assert_eq!(
            parse_suite(&invalid.to_string()).unwrap_err(),
            ManifestErrorCodeV4::UnknownField
        );
    }
    for name in [
        "model_attempts",
        "tool_proposals",
        "tool_dispatches",
        "recovered_outcomes",
    ] {
        for expected in [json!(-1), json!(1.5), json!("1"), json!(true), Value::Null] {
            let mut invalid = source.clone();
            invalid["scenarios"][0]["assertions"] = json!([{"name":name, "expected":expected}]);
            assert_eq!(
                parse_suite(&invalid.to_string()).unwrap_err(),
                ManifestErrorCodeV4::InvalidAssertion
            );
        }
        let mut valid = source.clone();
        valid["scenarios"][0]["assertions"] = json!([{"name":name, "expected":u64::MAX}]);
        assert!(parse_suite(&valid.to_string()).is_ok());
    }
    let mut duplicate_assertion = source.clone();
    let assertions = duplicate_assertion["scenarios"][0]["assertions"]
        .as_array_mut()
        .unwrap();
    assertions.push(assertions[0].clone());
    assert_eq!(
        parse_suite(&duplicate_assertion.to_string()).unwrap_err(),
        ManifestErrorCodeV4::InvalidAssertion
    );
    let mut canonical_duplicate = source;
    canonical_duplicate["scenarios"][0]["id"] = json!("duplicate\r\nid");
    canonical_duplicate["scenarios"][1]["id"] = json!("duplicate\nid");
    assert_eq!(
        parse_suite(&canonical_duplicate.to_string()).unwrap_err(),
        ManifestErrorCodeV4::DuplicateId
    );
}

#[test]
fn manifest_hash_is_canonical_across_crlf_and_order() {
    let suite = parse_suite(MANIFEST).unwrap();
    let mut reordered: Value = serde_json::from_str(MANIFEST).unwrap();
    reordered["scenarios"].as_array_mut().unwrap().reverse();
    let lf = parse_suite(&reordered.to_string()).unwrap();
    let crlf = parse_suite(&MANIFEST.replace('\n', "\r\n").replace("\\n", "\\r\\n")).unwrap();
    let fixtures = BTreeMap::from([("data/source.txt".into(), "first\nsecond\n".into())]);
    let windows_fixtures =
        BTreeMap::from([("data/source.txt".into(), "first\r\nsecond\r\n".into())]);
    let hash = suite_hash(&suite, &fixtures).unwrap();
    assert_eq!(hash.len(), 64);
    assert_eq!(
        hash,
        "1dfbe98f1fd482e6c706f617a2649ad6593006fbb37dbee47d5cee2883ff1d3d"
    );
    assert_eq!(hash, suite_hash(&lf, &fixtures).unwrap());
    assert_eq!(hash, suite_hash(&crlf, &windows_fixtures).unwrap());
    // Render reverse object keys explicitly: Value itself may use sorted maps.
    fn reverse_keys(value: &Value) -> String {
        match value {
            Value::Object(object) => format!(
                "{{{}}}",
                object
                    .iter()
                    .rev()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        reverse_keys(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Value::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(reverse_keys)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            other => other.to_string(),
        }
    }
    let reversed = parse_suite(&reverse_keys(&reordered)).unwrap();
    assert_eq!(hash, suite_hash(&reversed, &fixtures).unwrap());
    let mut mixed_ids = suite.clone();
    mixed_ids.scenarios[0].id = "a\r\nb".into();
    mixed_ids.scenarios[1].id = "a\nz".into();
    let mut normalized_ids = mixed_ids.clone();
    normalized_ids.scenarios[0].id = "a\nb".into();
    assert_eq!(
        suite_hash(&mixed_ids, &fixtures).unwrap(),
        suite_hash(&normalized_ids, &fixtures).unwrap()
    );
}

#[test]
fn manifest_hash_changes_with_fixture_content() {
    let suite = parse_suite(MANIFEST).unwrap();
    let fixtures = BTreeMap::from([("data/source.txt".into(), "original".into())]);
    let hash = suite_hash(&suite, &fixtures).unwrap();
    assert_ne!(
        hash,
        suite_hash(
            &suite,
            &BTreeMap::from([("data/source.txt".into(), "changed".into())])
        )
        .unwrap()
    );
    assert_ne!(
        hash,
        suite_hash(
            &suite,
            &BTreeMap::from([("data/renamed.txt".into(), "original".into())])
        )
        .unwrap()
    );
    let mut changed = suite.clone();
    changed.scenarios[0].objective.push_str(" changed");
    assert_ne!(hash, suite_hash(&changed, &fixtures).unwrap());
    for path in [
        "../source.txt",
        "/source.txt",
        "C:/source.txt",
        "data\\source.txt",
    ] {
        assert_eq!(
            suite_hash(&suite, &BTreeMap::from([(path.into(), "fixture".into())])).unwrap_err(),
            ManifestErrorCodeV4::InvalidScript
        );
    }
}
