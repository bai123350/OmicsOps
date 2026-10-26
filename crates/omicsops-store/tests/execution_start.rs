use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::StopRunRequestV4;
use omicsops_protocol::*;
use omicsops_store::Store;
use serde_json::json;
use uuid::Uuid;

async fn fixture(kind: RunExecutionKindV4, status: &str) -> (Store, RunSpecV4, AgentEventV4) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "start fixture",
        "C:\\fixture",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    let conversation = Conversation::new(Uuid::new_v4(), project.id, "resume", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    let plan = ExecutionPlanV4 {
        schema_version: 4,
        objective: "retain evidence".into(),
        steps: vec!["inspect".into()],
        completion_criteria: vec!["verifiable".into()],
        requested_capabilities: Default::default(),
    };
    let selection = ComputeSelectionV4 {
        schema_version: 4,
        backend_id: "local".into(),
        backend_kind: ComputeBackendKindV4::Local,
        autonomy_mode: AutonomyModeV4::Supervised,
        approval_policy: ApprovalPolicyV4::RiskBased,
        environment: "system".into(),
        network_policy: NetworkPolicyV4::HostInherited,
        container_image: None,
    };
    let run_id = Uuid::new_v4();
    let model_id = Uuid::new_v4();
    let approval = RunSpecV4::approval_hash_for(
        run_id,
        project.id,
        conversation.id,
        model_id,
        &plan,
        &selection,
    )
    .unwrap();
    let mut spec = RunSpecV4::freeze_with_compute(
        run_id,
        project.id,
        conversation.id,
        model_id,
        plan,
        selection,
        &approval,
        Utc::now(),
    )
    .unwrap();
    spec.execution_kind = kind;
    spec.spec_hash = Some(spec.calculate_spec_hash().unwrap());
    store
        .save_agent_run_v4(
            run_id,
            project.id,
            conversation.id,
            status,
            &json!({"status":status, "spec":spec, "retained_metadata":{"value":42}}),
        )
        .await
        .unwrap();
    let first = AgentEventV4::first(
        run_id,
        project.id,
        conversation.id,
        Utc::now(),
        AgentEventKindV4::RunCreated {
            mode: RunModeV4::Execute,
        },
    );
    store.append_agent_event_v4(&first).await.unwrap();
    let head = AgentEventV4::next(
        &first,
        Utc::now(),
        AgentEventKindV4::RunSpecFrozen {
            approval_hash: approval,
            spec_hash: spec.spec_hash.clone().unwrap(),
        },
    );
    store.append_agent_event_v4(&head).await.unwrap();
    (store, spec, head)
}

#[tokio::test]
async fn execution_start_preserves_frozen_state_and_events_for_both_modes() {
    for kind in [
        RunExecutionKindV4::OrdinaryAgent,
        RunExecutionKindV4::ApprovedPlan,
    ] {
        for status in ["waiting_for_input", "waiting_for_approval", "running"] {
            let (store, spec, head) = fixture(kind, status).await;
            let mut expected = store.agent_run_v4(spec.run_id).await.unwrap().unwrap();
            expected["status"] = json!("running");
            let history = store.agent_events_v4(spec.run_id).await.unwrap();
            store
                .start_agent_execution_v4(&spec, status, &head.event_hash)
                .await
                .unwrap();
            assert_eq!(
                store.agent_run_v4(spec.run_id).await.unwrap().unwrap(),
                expected
            );
            assert_eq!(store.agent_events_v4(spec.run_id).await.unwrap(), history);
            store
                .start_agent_execution_v4(&spec, "running", &head.event_hash)
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn execution_start_rejects_changed_sources_and_stop_without_rewriting_state() {
    for change in [
        "head",
        "spec",
        "status",
        "terminal",
        "stop",
        "anchor",
        "approval_anchor",
    ] {
        let (store, mut spec, mut head) =
            fixture(RunExecutionKindV4::ApprovedPlan, "waiting_for_input").await;
        let mut expected_head = head.event_hash.clone();
        let mut expected_status = "waiting_for_input";
        match change {
            "spec" => {
                spec.plan.objective = "tampered".into();
            }
            "status" => expected_status = "running",
            "stop" => {
                store
                    .request_run_stop_v4(&StopRunRequestV4 {
                        request_id: Uuid::new_v4(),
                        project_id: spec.project_id,
                        conversation_id: spec.conversation_id,
                        run_id: spec.run_id,
                    })
                    .await
                    .unwrap();
            }
            _ => {
                let event = match change {
                    "terminal" => AgentEventKindV4::RunFailed {
                        message: "fixture terminal without stop".into(),
                    },
                    "anchor" | "approval_anchor" => AgentEventKindV4::RunSpecFrozen {
                        spec_hash: if change == "anchor" {
                            "0".repeat(64)
                        } else {
                            spec.spec_hash.clone().unwrap()
                        },
                        approval_hash: if change == "approval_anchor" {
                            "0".repeat(64)
                        } else {
                            spec.approval_hash.clone().unwrap()
                        },
                    },
                    _ => AgentEventKindV4::ModelText {
                        text: "new event".into(),
                    },
                };
                head = AgentEventV4::next(&head, Utc::now(), event);
                store.append_agent_event_v4(&head).await.unwrap();
                if change != "head" {
                    expected_head = head.event_hash.clone();
                }
            }
        }
        let before = store.agent_run_v4(spec.run_id).await.unwrap();
        let history = store.agent_events_v4(spec.run_id).await.unwrap();
        assert!(
            store
                .start_agent_execution_v4(&spec, expected_status, &expected_head)
                .await
                .is_err(),
            "accepted {change}"
        );
        assert_eq!(store.agent_run_v4(spec.run_id).await.unwrap(), before);
        assert_eq!(store.agent_events_v4(spec.run_id).await.unwrap(), history);
    }
}
