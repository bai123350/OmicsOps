use omicsops_agent::{
    AgentEvent, ApprovalDecision, ApprovalGate, Capability, SpecialistDispatcher, ToolRegistry,
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
