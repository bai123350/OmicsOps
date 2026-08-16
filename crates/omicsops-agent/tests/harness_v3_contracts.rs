use chrono::{TimeZone, Utc};
use omicsops_agent::ModelMessage;
use omicsops_agent::harness_v3::{
    AgentRunEventKindV3, AgentRunEventV3, AgentRunSpecV3, AgentRunStateV3, ArtifactEvidenceV3,
    CompletionCriterionV3, CompletionLedgerV3, ContextSourceV3, FindingSeverityV3, ModelRequestV2,
    ModelStreamEventV2, ModelToolSpec, ReviewFindingV3, ReviewReportV3, RiskLevelV3, RunLimitsV3,
    RunStatusV3, ToolCallAccumulatorV2, ToolCallRequestV3, ToolConcurrencyV3, ToolDefinitionV3,
    ToolOutcomeStatusV3, ToolOutcomeV3, build_context, validate_event_chain,
};
use serde_json::json;
use uuid::Uuid;

fn ids() -> (Uuid, Uuid, Uuid) {
    (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4())
}

fn spec() -> AgentRunSpecV3 {
    let (run_id, project_id, conversation_id) = ids();
    AgentRunSpecV3::new(
        run_id,
        project_id,
        conversation_id,
        "Analyze PBMC3k without a prewritten workflow",
        vec![CompletionCriterionV3::new(
            "criterion-1",
            "Deliver a verified h5ad",
        )],
        vec![],
        vec![],
    )
}

#[test]
fn run_spec_freezes_v3_defaults() {
    let spec = spec();

    assert_eq!(spec.harness_version, 3);
    assert_eq!(spec.harness_id, "agent.harness_v3@3.0.0");
    assert_eq!(spec.limits, RunLimitsV3::default());
    assert_eq!(spec.limits.max_model_steps, 64);
    assert_eq!(spec.limits.max_tool_calls, 48);
    assert_eq!(spec.limits.max_reviewer_corrections, 2);
    assert_eq!(spec.limits.max_parallel_read_only, 4);
}

#[test]
fn model_protocol_preserves_multiple_call_ids_and_usage() {
    let request = ModelRequestV2 {
        system: "Use tools".into(),
        messages: vec![ModelMessage {
            role: "user".into(),
            content: "inspect two files".into(),
        }],
        tools: vec![ModelToolSpec {
            id: "remote.read".into(),
            description: "Read a project file".into(),
            input_schema: json!({"type":"object","required":["path"]}),
        }],
        require_strict_json_fallback: true,
    };
    assert_eq!(request.tools.len(), 1);

    let events = vec![
        ModelStreamEventV2::ToolCallStarted {
            call_id: "a".into(),
            index: 0,
            tool_id: "remote.read".into(),
        },
        ModelStreamEventV2::ToolArgumentsDelta {
            call_id: "b".into(),
            index: 1,
            arguments: "{\"path\":\"b\"}".into(),
        },
        ModelStreamEventV2::Usage {
            input_tokens: 120,
            output_tokens: 30,
            provider_json: json!({"cache_read": 4}),
        },
    ];

    assert!(matches!(
        &events[1],
        ModelStreamEventV2::ToolArgumentsDelta { call_id, index: 1, .. } if call_id == "b"
    ));
}

#[test]
fn tool_call_accumulator_assembles_interleaved_calls_and_repairs_only_once() {
    let mut accumulator = ToolCallAccumulatorV2::default();
    for event in [
        ModelStreamEventV2::ToolCallStarted {
            call_id: "a".into(),
            index: 0,
            tool_id: "remote.list".into(),
        },
        ModelStreamEventV2::ToolCallStarted {
            call_id: "b".into(),
            index: 1,
            tool_id: "remote.read".into(),
        },
        ModelStreamEventV2::ToolArgumentsDelta {
            call_id: "b".into(),
            index: 1,
            arguments: "{\"path\":".into(),
        },
        ModelStreamEventV2::ToolArgumentsDelta {
            call_id: "a".into(),
            index: 0,
            arguments: "{}".into(),
        },
        ModelStreamEventV2::ToolArgumentsDelta {
            call_id: "b".into(),
            index: 1,
            arguments: "\"a.txt\"}".into(),
        },
    ] {
        accumulator.push(&event).unwrap();
    }
    let calls = accumulator.finish().unwrap();
    assert_eq!(calls[0].call_id, "a");
    assert_eq!(calls[1].arguments, json!({"path":"a.txt"}));

    let mut malformed = ToolCallAccumulatorV2::default();
    malformed
        .push(&ModelStreamEventV2::ToolCallStarted {
            call_id: "bad".into(),
            index: 0,
            tool_id: "remote.read".into(),
        })
        .unwrap();
    malformed
        .push(&ModelStreamEventV2::ToolArgumentsDelta {
            call_id: "bad".into(),
            index: 0,
            arguments: "{path:".into(),
        })
        .unwrap();
    assert!(malformed.finish().is_err());
    malformed
        .repair_once("bad", "{\"path\":\"fixed.txt\"}")
        .unwrap();
    assert!(malformed.repair_once("bad", "{}").is_err());
    assert_eq!(
        malformed.finish().unwrap()[0].arguments,
        json!({"path":"fixed.txt"})
    );
}

#[test]
fn hash_chain_is_deterministic_and_rejects_tampering() {
    let spec = spec();
    let at = Utc.with_ymd_and_hms(2026, 8, 16, 8, 0, 0).unwrap();
    let first = AgentRunEventV3::first(&spec, at, AgentRunEventKindV3::RunStarted).unwrap();
    let second = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ModelStepStarted { step: 1 },
    )
    .unwrap();

    let duplicate = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ModelStepStarted { step: 1 },
    )
    .unwrap();
    assert_eq!(second.event_hash, duplicate.event_hash);
    validate_event_chain(&[first.clone(), second.clone()]).unwrap();

    let mut tampered = second;
    tampered.event = AgentRunEventKindV3::ModelStepStarted { step: 2 };
    let error = validate_event_chain(&[first, tampered]).unwrap_err();
    assert!(error.to_string().contains("hash"));
}

#[test]
fn reducer_marks_dispatched_side_effect_without_outcome_uncertain() {
    let spec = spec();
    let at = Utc.with_ymd_and_hms(2026, 8, 16, 8, 0, 0).unwrap();
    let first = AgentRunEventV3::first(&spec, at, AgentRunEventKindV3::RunStarted).unwrap();
    let request = ToolCallRequestV3 {
        call_id: "write-1".into(),
        tool_id: "remote.write".into(),
        arguments: json!({"path":"results/a.txt","content":"x"}),
        idempotency_key: "write-results-a-v1".into(),
    };
    let dispatched = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ToolCallDispatched {
            request,
            read_only: false,
        },
    )
    .unwrap();

    let state = AgentRunStateV3::replay(&spec, &[first, dispatched]).unwrap();
    assert_eq!(state.status, RunStatusV3::Recovering);
    assert_eq!(state.uncertain_side_effects, vec!["write-1"]);
    assert!(state.can_execute_side_effects().is_err());
}

#[test]
fn successful_idempotent_call_is_reconstructed_from_events() {
    let spec = spec();
    let at = Utc.with_ymd_and_hms(2026, 8, 16, 8, 0, 0).unwrap();
    let first = AgentRunEventV3::first(&spec, at, AgentRunEventKindV3::RunStarted).unwrap();
    let request = ToolCallRequestV3 {
        call_id: "read-1".into(),
        tool_id: "remote.read".into(),
        arguments: json!({"path":"input/matrix.mtx"}),
        idempotency_key: "read-input-v1".into(),
    };
    let dispatched = AgentRunEventV3::next(
        &first,
        at,
        AgentRunEventKindV3::ToolCallDispatched {
            request: request.clone(),
            read_only: true,
        },
    )
    .unwrap();
    let outcome = ToolOutcomeV3 {
        call_id: request.call_id.clone(),
        status: ToolOutcomeStatusV3::Succeeded,
        model_content: "matrix exists".into(),
        structured_result: Some(json!({"bytes": 20})),
        error: None,
        truncated: false,
        provenance: vec!["remote:input/matrix.mtx".into()],
    };
    let finished = AgentRunEventV3::next(
        &dispatched,
        at,
        AgentRunEventKindV3::ToolCallFinished { outcome },
    )
    .unwrap();

    let state = AgentRunStateV3::replay(&spec, &[first, dispatched, finished]).unwrap();
    assert!(state.successful_idempotency_keys.contains("read-input-v1"));
    assert!(state.uncertain_side_effects.is_empty());
}

#[test]
fn tool_definition_validates_required_arguments_and_mcp_defaults() {
    let definition = ToolDefinitionV3 {
        id: "remote.read".into(),
        description: "read".into(),
        input_schema: json!({
            "type":"object",
            "required":["path"],
            "properties":{"path":{"type":"string"}}
        }),
        output_schema: json!({"type":"object"}),
        capabilities: vec!["remote_read".into()],
        risk: RiskLevelV3::Low,
        read_only: true,
        concurrency: ToolConcurrencyV3::ParallelReadOnly,
        timeout_ms: 5_000,
    };
    assert!(definition.validate_arguments(&json!({"path":"a"})).is_ok());
    assert!(
        definition
            .validate_arguments(&json!({"path":2}))
            .unwrap_err()
            .to_string()
            .contains("path")
    );

    let mcp = ToolDefinitionV3::mcp("server", "lookup", "Lookup a record", json!({}));
    assert_eq!(mcp.id, "mcp::server::lookup");
    assert_eq!(mcp.risk, RiskLevelV3::Medium);
    assert_eq!(mcp.concurrency, ToolConcurrencyV3::Serial);
    assert_eq!(mcp.timeout_ms, 30_000);
}

#[test]
fn context_compaction_preserves_ledger_errors_and_recent_steps() {
    let spec = spec();
    let mut ledger = CompletionLedgerV3::from_spec(&spec);
    ledger.unresolved_errors.push("seed missing".into());
    ledger.verified_artifacts.push(ArtifactEvidenceV3 {
        path: "results/pbmc.h5ad".into(),
        size_bytes: 42,
        sha256: "a".repeat(64),
        evidence_sequence: 9,
    });
    let sources = (0..12)
        .map(|index| ContextSourceV3::ToolStep {
            sequence: index,
            tool_id: "remote.read".into(),
            content: format!("step-{index}"),
        })
        .collect::<Vec<_>>();

    let context = build_context(&spec, &ledger, sources, 100, 76);
    assert!(context.compaction_required);
    assert_eq!(context.recent_tool_steps.len(), 8);
    assert_eq!(context.recent_tool_steps[0].sequence(), Some(4));
    assert!(context.rendered.contains("seed missing"));
    assert!(context.rendered.contains("results/pbmc.h5ad"));
    assert!(context.rendered.contains("UNTRUSTED_TOOL_OUTPUT"));
}

#[test]
fn completion_gate_requires_evidence_for_every_criterion_and_verified_artifacts() {
    let spec = spec();
    let mut ledger = CompletionLedgerV3::from_spec(&spec);
    assert!(ledger.can_complete().is_err());

    ledger.satisfy("criterion-1", vec![7]).unwrap();
    ledger.verified_artifacts.push(ArtifactEvidenceV3 {
        path: "results/pbmc.h5ad".into(),
        size_bytes: 42,
        sha256: "a".repeat(64),
        evidence_sequence: 8,
    });
    assert!(ledger.can_complete().is_ok());

    ledger.unresolved_errors.push("report drift".into());
    assert!(ledger.can_complete().is_err());
}

#[test]
fn reviewer_errors_allow_only_two_correction_cycles_and_findings_are_capped() {
    let findings = (0..10)
        .map(|index| ReviewFindingV3 {
            severity: if index == 0 {
                FindingSeverityV3::Error
            } else {
                FindingSeverityV3::Warn
            },
            summary: format!("finding-{index}"),
            evidence: vec![format!("event:{}", index + 1)],
        })
        .collect();
    let report = ReviewReportV3::new(1, findings);

    assert_eq!(report.findings.len(), 8);
    assert!(report.has_errors());
    assert_eq!(report.next_status(0, 2), RunStatusV3::Running);
    assert_eq!(report.next_status(1, 2), RunStatusV3::Running);
    assert_eq!(report.next_status(2, 2), RunStatusV3::NeedsAttention);
}
