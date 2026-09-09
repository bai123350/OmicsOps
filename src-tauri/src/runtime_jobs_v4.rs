//! Launch boundary shared by the desktop tool and deterministic fake-backend tests.
use omicsops_protocol::{ExecutionContextKeyV4, RuntimeJobV4, RuntimeResultV4, ToolCallV4};
use omicsops_runtime::RuntimeManagerV4;
use omicsops_store::Store;

pub(crate) async fn execute_reserved(
    repository: &Store,
    runtime: &RuntimeManagerV4,
    key: &ExecutionContextKeyV4,
    call: &ToolCallV4,
    code: String,
    captures: Vec<String>,
) -> Result<(RuntimeJobV4, RuntimeResultV4), String> {
    let recorded_captures = call
        .arguments
        .get("capture_paths")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if call
        .arguments
        .get("code")
        .and_then(serde_json::Value::as_str)
        != Some(code.as_str())
        || captures != recorded_captures
    {
        return Err("runtime job execution differs from recorded request".into());
    }
    let request_hash = call.canonical_hash().map_err(|error| error.to_string())?;
    let (reserved, acquired) = repository
        .reserve_runtime_job_v4(&key, &call.call_id, &request_hash)
        .await
        .map_err(|error| error.to_string())?;
    if !acquired {
        return Err(format!(
            "runtime job {} already exists ({:?}); reconcile its original dispatch before retrying",
            reserved.job_id, reserved.state
        ));
    }
    let session = match runtime.acquire(&key).await {
        Ok(session) => session,
        Err(error) => {
            repository
                .advance_runtime_job_v4(
                    &reserved,
                    omicsops_protocol::RuntimeJobStateV4::Unknown,
                    None,
                    None,
                )
                .await
                .map_err(|error| error.to_string())?;
            return Err(error);
        }
    };
    let running = repository
        .advance_runtime_job_v4(
            &reserved,
            omicsops_protocol::RuntimeJobStateV4::Running,
            Some(session.session_id()),
            None,
        )
        .await
        .map_err(|error| error.to_string())?;
    let execution = async {
        let handle = runtime.start_job(key, running.job_id, session.session_id(), code, captures).await?;
        let outcome = handle.wait().await;
        runtime.release_job(key, running.job_id)?;
        outcome.map(|result| (*result).clone())
    };
    let result = match execution.await {
        Ok(result) => result,
        Err(error) => {
            repository
                .advance_runtime_job_v4(
                    &running,
                    omicsops_protocol::RuntimeJobStateV4::Unknown,
                    None,
                    None,
                )
                .await
                .map_err(|error| error.to_string())?;
            return Err(error);
        }
    };
    Ok((running, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
    use omicsops_protocol::*;
    use omicsops_runtime::{KernelBackendV4, KernelProcessV4};
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use uuid::Uuid;

    struct FakeProcess {
        calls: AtomicUsize,
        fail: bool,
        id: Uuid,
    }
    struct FakeBackend {
        process: Arc<FakeProcess>,
        launches: AtomicUsize,
    }
    #[async_trait]
    impl KernelBackendV4 for FakeBackend {
        fn descriptor(&self) -> ComputeBackendDescriptorV4 {
            ComputeBackendDescriptorV4 {
                schema_version: 4,
                backend_id: "local".into(),
                kind: ComputeBackendKindV4::Local,
                isolation: IsolationStrengthV4::Process,
                available: true,
                supports_python: true,
                supports_r: false,
                supports_network_policy: false,
            }
        }
        async fn launch(
            &self,
            _: &ExecutionContextKeyV4,
        ) -> Result<Arc<dyn KernelProcessV4>, String> {
            self.launches.fetch_add(1, Ordering::SeqCst);
            Ok(self.process.clone())
        }
    }
    #[async_trait]
    impl KernelProcessV4 for FakeProcess {
        fn session_id(&self) -> Uuid {
            self.id
        }
        fn process_identity(&self) -> &str {
            "fake"
        }
        async fn execute(&self, _: String, _: Vec<String>) -> Result<RuntimeResultV4, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err("synthetic connection lost".into());
            }
            Ok(RuntimeResultV4 {
                request_id: Uuid::new_v4(),
                session_id: self.id,
                process_identity: "fake".into(),
                stdout: "42".into(),
                stderr: String::new(),
                stdout_capture: None,
                stderr_capture: None,
                succeeded: true,
                artifacts: vec![],
                software_versions: Default::default(),
            })
        }
        async fn interrupt(&self) -> Result<(), String> {
            Ok(())
        }
    }

    async fn fixture(store: &Store) -> (ExecutionContextKeyV4, ToolCallV4) {
        let project = Project::new(
            Uuid::new_v4(),
            "jobs",
            "synthetic",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        store.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "jobs", Utc::now());
        store.save_conversation(&conversation).await.unwrap();
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "test".into(),
            steps: vec!["compute".into()],
            completion_criteria: vec!["result".into()],
            requested_capabilities: Default::default(),
        };
        let spec = RunSpecV4::freeze(
            Uuid::new_v4(),
            project.id,
            conversation.id,
            Uuid::new_v4(),
            plan.clone(),
            &plan.canonical_hash().unwrap(),
            Utc::now(),
        )
        .unwrap();
        store
            .save_agent_run_v4_if_unlocked(
                spec.run_id,
                spec.project_id,
                spec.conversation_id,
                "running",
                &json!({"spec":spec}),
            )
            .await
            .unwrap();
        let first = AgentEventV4::first(
            spec.run_id,
            spec.project_id,
            spec.conversation_id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        store.append_agent_event_v4(&first).await.unwrap();
        let call = ToolCallV4 {
            call_id: "cell".into(),
            tool_id: "runtime.execute".into(),
            arguments: json!({"language":"python", "code":"print(42)"}),
        };
        let request = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ToolRequested { call: call.clone() },
        );
        store.append_agent_event_v4(&request).await.unwrap();
        let dispatch = AgentEventV4::next(
            &request,
            Utc::now(),
            AgentEventKindV4::ToolDispatchStarted {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                effect: ToolEffectV4::Runtime,
                idempotency_key: call.call_id.clone(),
            },
        );
        store.append_agent_event_v4(&dispatch).await.unwrap();
        (
            ExecutionContextKeyV4 {
                project_id: spec.project_id,
                run_id: spec.run_id,
                backend_id: "local".into(),
                language: KernelLanguageV4::Python,
                environment: "system".into(),
            },
            call,
        )
    }

    #[tokio::test]
    async fn duplicate_dispatch_never_executes_twice_even_after_connection_loss() {
        for fail in [false, true] {
            let store = Store::open_in_memory().await.unwrap();
            let (key, call) = fixture(&store).await;
            let process = Arc::new(FakeProcess {
                calls: AtomicUsize::new(0),
                fail,
                id: Uuid::new_v4(),
            });
            let backend = Arc::new(FakeBackend {
                process: process.clone(),
                launches: AtomicUsize::new(0),
            });
            let runtime = RuntimeManagerV4::new(backend.clone());
            let (a, b) = tokio::join!(
                execute_reserved(&store, &runtime, &key, &call, "print(42)".into(), vec![]),
                execute_reserved(&store, &runtime, &key, &call, "print(42)".into(), vec![])
            );
            assert_eq!(
                usize::from(a.is_ok()) + usize::from(b.is_ok()),
                usize::from(!fail)
            );
            assert_eq!(process.calls.load(Ordering::SeqCst), 1);
            assert_eq!(backend.launches.load(Ordering::SeqCst), 1);
            let job = store
                .runtime_job_v4(&key, &call.call_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                job.state,
                if fail {
                    RuntimeJobStateV4::Unknown
                } else {
                    RuntimeJobStateV4::Running
                }
            );
            // A new manager represents loss of the local interpreter registry.
            let restarted = RuntimeManagerV4::new(backend.clone());
            assert!(
                execute_reserved(&store, &restarted, &key, &call, "print(42)".into(), vec![])
                    .await
                    .is_err()
            );
            assert_eq!(backend.launches.load(Ordering::SeqCst), 1);
        }
    }
}
