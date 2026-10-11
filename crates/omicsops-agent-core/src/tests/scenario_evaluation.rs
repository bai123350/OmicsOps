use super::*;
use std::collections::BTreeMap;

const MANIFEST: &str = include_str!("data/v4-scenarios.json");

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
            "/scenarios/1/tool_script/0/failure",
            json!("business_failure"),
            ManifestErrorCodeV4::InvalidScript,
        ),
        (
            "/scenarios/1/tool_script/0/outcome",
            Value::Null,
            ManifestErrorCodeV4::InvalidScript,
        ),
        (
            "/scenarios/1/tool_script/0/outcome/call_id",
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
        "/scenarios/1/tool_script/0",
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
        "4926a6baceccaf5fb519aaffa50bf24f2fcb150e142e659e39fa8dfbac70ff11"
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
