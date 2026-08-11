use omicsops_agent::{
    ActiveTurnCoordinator, AgentEvent, ApprovalDecision, ApprovalGate, Capability,
    ModelStreamEvent, SpecialistDispatcher, SpecialistFinding, SpecialistKind,
    SpecialistProposedAction, SpecialistReport, ToolArgumentBuffer, ToolRegistry,
};
use uuid::Uuid;

#[test]
fn every_mutating_or_remote_capability_requires_explicit_approval() {
    let gate = ApprovalGate::default();
    assert_eq!(
        gate.decision(Capability::ReadLocal),
        ApprovalDecision::Allowed
    );
    assert_eq!(
        gate.decision(Capability::WriteLocal),
        ApprovalDecision::Required
    );
    assert_eq!(
        gate.decision(Capability::ExecuteRemote),
        ApprovalDecision::Required
    );
    assert_eq!(
        gate.decision(Capability::QueryResearchSource),
        ApprovalDecision::Required
    );
}

#[test]
fn provider_neutral_tool_arguments_are_assembled_and_name_checked() {
    let mut buffer = ToolArgumentBuffer::new("submit_plan");
    buffer
        .push(ModelStreamEvent::ToolArgumentsDelta {
            name: "submit_plan".into(),
            json_fragment: "{\"schema_".into(),
        })
        .unwrap();
    buffer
        .push(ModelStreamEvent::ToolArgumentsDelta {
            name: "".into(),
            json_fragment: "version\":2}".into(),
        })
        .unwrap();
    assert_eq!(buffer.finish().unwrap()["schema_version"], 2);
    let mut wrong = ToolArgumentBuffer::new("submit_plan");
    assert!(
        wrong
            .push(ModelStreamEvent::ToolArgumentsDelta {
                name: "other".into(),
                json_fragment: "{}".into()
            })
            .is_err()
    );
}

#[test]
fn tool_registry_rejects_undeclared_capabilities() {
    let mut tools = ToolRegistry::default();
    tools.register("pubmed.search", [Capability::QueryResearchSource]);
    assert!(
        tools
            .authorize("pubmed.search", Capability::QueryResearchSource)
            .is_ok()
    );
    assert!(
        tools
            .authorize("pubmed.search", Capability::WriteLocal)
            .is_err()
    );
    assert!(tools.authorize("unknown", Capability::ReadLocal).is_err());
}

#[test]
fn specialist_dispatcher_caps_parallel_work_at_three() {
    let mut dispatcher = SpecialistDispatcher::default();
    for name in ["literature", "statistics", "code-review"] {
        dispatcher.start(name).unwrap();
    }
    assert!(dispatcher.start("fourth").is_err());
    dispatcher.finish("statistics");
    assert!(dispatcher.start("replacement").is_ok());
}

#[test]
fn specialist_mutations_return_to_the_main_approval_gate() {
    let report = SpecialistReport {
        task_id: Uuid::new_v4(),
        kind: SpecialistKind::CodeReview,
        findings: vec![SpecialistFinding {
            summary: "Use a sparse matrix".into(),
            evidence: vec!["memory estimate".into()],
        }],
        proposed_actions: vec![
            SpecialistProposedAction {
                tool: "workspace.read".into(),
                capability: Capability::ReadLocal,
                arguments: serde_json::json!({"path":"analysis.py"}),
            },
            SpecialistProposedAction {
                tool: "remote.execute".into(),
                capability: Capability::ExecuteRemote,
                arguments: serde_json::json!({"command":"python analysis.py"}),
            },
        ],
    };
    let required = report.actions_requiring_main_approval(&ApprovalGate);
    assert_eq!(required.len(), 1);
    assert_eq!(required[0].tool, "remote.execute");
}

#[test]
fn active_conversation_turns_are_capped_at_three() {
    let mut turns = ActiveTurnCoordinator::default();
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for id in ids {
        turns.start(id).unwrap();
    }
    assert!(turns.start(Uuid::new_v4()).is_err());
    turns.finish(ids[1]);
    assert!(turns.start(Uuid::new_v4()).is_ok());
}

#[test]
fn stable_agent_events_keep_project_conversation_and_turn_identity() {
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let turn_id = Uuid::new_v4();
    let event = AgentEvent::text_delta(project_id, conversation_id, turn_id, "正在检查数据");
    assert_eq!(event.project_id, project_id);
    assert_eq!(event.conversation_id, conversation_id);
    assert_eq!(event.turn_id, turn_id);
    assert_eq!(event.sequence, 0);
}
