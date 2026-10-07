//! Read-only observation of the original detached dispatch. A remote deadline
//! bounds connection, trust/root checks and status, not the atomic local commit.
use async_trait::async_trait;
use chrono::Utc;
use omicsops_adapters::ssh::SshSession;
use omicsops_core::{domain::ConnectionProfile, workspace::Project};
use omicsops_dto::*;
use omicsops_protocol::{RuntimeJobStateV4, RuntimeJobV4, RuntimeResultV4};
use omicsops_store::Store;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tauri::State;
use tokio::time::{Instant, timeout_at};
use uuid::Uuid;

use crate::{
    commands::{AppState, connect_profile, find_profile, require_trusted_host},
    composer_references::public_text,
    remote_jobs_v4::{self, Transport},
};

const REJECTED: &str = "remote_job_rejected";
const UNAVAILABLE: &str = "remote_job_unavailable";
const TIMEOUT: &str = "remote_job_timeout";
const NOT_FOUND: &str = "remote_job_not_found";

pub(crate) struct BoundRemoteJobTransport {
    pub transport: Box<dyn Transport>,
    pub canonical_root: String,
}

#[async_trait]
pub(crate) trait RemoteJobConnector: Send + Sync {
    /// Owns connection cleanup, including cancellation while resolving root.
    async fn connect(
        &self,
        project: &Project,
        job: &RuntimeJobV4,
    ) -> Result<BoundRemoteJobTransport, RemoteJobObservationStatusV4>;
}

struct DesktopConnector<'a> {
    state: &'a AppState,
}

// Drop also runs when the root/deadline future is cancelled. Disconnect is
// asynchronous and never prolongs the user's status deadline.
struct OwnedSshTransport {
    session: Option<Arc<SshSession>>,
}
impl Drop for OwnedSshTransport {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            tauri::async_runtime::spawn(async move {
                if let Ok(session) = Arc::try_unwrap(session) {
                    let _ = session.disconnect().await;
                }
            });
        }
    }
}
#[async_trait]
impl Transport for OwnedSshTransport {
    fn host_key(&self) -> &str {
        self.session
            .as_ref()
            .expect("owned SSH session")
            .fingerprint()
    }
    async fn invoke(&self, payload: Value) -> Result<Value, String> {
        remote_jobs_v4::SshTransport {
            session: self.session.as_ref().expect("owned SSH session").clone(),
        }
        .invoke(payload)
        .await
    }
}
#[async_trait]
impl RemoteJobConnector for DesktopConnector<'_> {
    async fn connect(
        &self,
        project: &Project,
        job: &RuntimeJobV4,
    ) -> Result<BoundRemoteJobTransport, RemoteJobObservationStatusV4> {
        let profile = current_profile(&self.state.repository, project, job)
            .await
            .map_err(|_| RemoteJobObservationStatusV4::Rejected)?;
        let session = connect_profile(self.state, &profile)
            .await
            .map_err(|_| RemoteJobObservationStatusV4::Unavailable)?;
        let transport = OwnedSshTransport {
            session: Some(Arc::new(session)),
        };
        let root = crate::sync_commands::canonical_remote_root_for_composer(
            transport.session.as_ref().expect("owned SSH session"),
            project
                .remote_root
                .as_deref()
                .ok_or(RemoteJobObservationStatusV4::Rejected)?,
        )
        .await
        .map_err(|_| RemoteJobObservationStatusV4::Unavailable)?;
        Ok(BoundRemoteJobTransport {
            transport: Box::new(transport),
            canonical_root: root,
        })
    }
}

fn public_bounded(text: &str, limit: usize) -> String {
    let text = public_text(text);
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
fn summary(
    job: &RuntimeJobV4,
    conversation_id: Uuid,
    eligibility: RemoteJobQueryEligibilityV4,
) -> RemoteJobSummaryV4 {
    RemoteJobSummaryV4 {
        project_id: job.context.project_id,
        conversation_id,
        job_id: job.job_id,
        run_id: job.context.run_id,
        call_id: public_bounded(&job.call_id, 256),
        backend_id: public_bounded(&job.context.backend_id, 256),
        language: match job.context.language {
            omicsops_protocol::KernelLanguageV4::Python => "python",
            omicsops_protocol::KernelLanguageV4::R => "r",
        }
        .into(),
        environment: public_bounded(&job.context.environment, 256),
        request_sha256: public_bounded(&job.request_sha256, 64),
        recorded_state: match job.state {
            RuntimeJobStateV4::Reserved => RemoteJobRecordedStateV4::Reserved,
            RuntimeJobStateV4::Running => RemoteJobRecordedStateV4::Running,
            RuntimeJobStateV4::Unknown => RemoteJobRecordedStateV4::Unknown,
            RuntimeJobStateV4::Succeeded => RemoteJobRecordedStateV4::Succeeded,
            RuntimeJobStateV4::Failed => RemoteJobRecordedStateV4::Failed,
        },
        query_eligibility: eligibility,
        recorded_at: None,
    }
}
fn terminal(job: &RuntimeJobV4) -> bool {
    matches!(
        job.state,
        RuntimeJobStateV4::Succeeded | RuntimeJobStateV4::Failed
    )
}
fn metadata_eligibility(
    project: &Project,
    job: &RuntimeJobV4,
    has_receipt: bool,
) -> RemoteJobQueryEligibilityV4 {
    if terminal(job) && has_receipt {
        // Only local availability; selected read must validate the body.
        RemoteJobQueryEligibilityV4::RecordedReceipt
    } else if !job.context.backend_id.starts_with("ssh:") || project.remote_root.is_none() {
        RemoteJobQueryEligibilityV4::Unsupported
    } else if project
        .connection_id
        .is_none_or(|id| job.context.backend_id != format!("ssh:{id}"))
    {
        RemoteJobQueryEligibilityV4::BindingChanged
    } else {
        RemoteJobQueryEligibilityV4::NeedsRootCheck
    }
}
pub(crate) async fn list_remote_jobs_with_store(
    store: &Store,
    request: ListRemoteJobsV4Request,
) -> Result<RemoteJobsPageV4, String> {
    let project = store
        .get_project(request.project_id)
        .await
        .map_err(|_| UNAVAILABLE)?
        .ok_or(NOT_FOUND)?;
    let rows = store
        .remote_job_metadata_v4(request.project_id, request.conversation_id)
        .await
        .map_err(|_| REJECTED)?;
    Ok(RemoteJobsPageV4 {
        jobs: rows
            .into_iter()
            .map(|row| {
                summary(
                    &row.job,
                    row.conversation_id,
                    metadata_eligibility(&project, &row.job, row.has_receipt),
                )
            })
            .collect(),
        limit: 100,
        remote_liveness_checked: false,
    })
}

async fn current_profile(
    store: &Store,
    project: &Project,
    job: &RuntimeJobV4,
) -> Result<ConnectionProfile, String> {
    let connection_id = project.connection_id.ok_or(REJECTED)?;
    if job.context.backend_id != format!("ssh:{connection_id}")
        || project
            .remote_root
            .as_ref()
            .is_none_or(|root| root.trim().is_empty())
    {
        return Err(REJECTED.into());
    }
    let profile = find_profile(store, connection_id)
        .await
        .map_err(|_| REJECTED)?;
    require_trusted_host(&profile).map_err(|_| REJECTED)?;
    if profile.host_key_fingerprint.as_deref() != job.remote_host_key.as_deref()
        || profile
            .host_key_fingerprint
            .as_deref()
            .is_none_or(|key| key.trim().is_empty())
    {
        return Err(REJECTED.into());
    }
    Ok(profile)
}

struct GuardedStatusTransport<'a> {
    store: &'a Store,
    project: &'a Project,
    profile: &'a ConnectionProfile,
    job: &'a RuntimeJobV4,
    bound: &'a BoundRemoteJobTransport,
    deadline: Instant,
}
#[async_trait]
impl Transport for GuardedStatusTransport<'_> {
    fn host_key(&self) -> &str {
        self.bound.transport.host_key()
    }
    async fn invoke(&self, payload: Value) -> Result<Value, String> {
        let operation = async {
            let project = self
                .store
                .get_project(self.project.id)
                .await
                .map_err(|_| UNAVAILABLE)?
                .ok_or(REJECTED)?;
            if project.connection_id != self.project.connection_id
                || project.remote_root != self.project.remote_root
            {
                return Err(REJECTED.into());
            }
            let profile = current_profile(self.store, &project, self.job).await?;
            if profile != *self.profile
                || self.bound.canonical_root.as_str()
                    != self.job.remote_root.as_deref().ok_or(REJECTED)?
                || self.host_key() != self.job.remote_host_key.as_deref().ok_or(REJECTED)?
                || payload["action"] != "status"
                || payload.get("identity") != Some(&remote_jobs_v4::identity(self.job))
            {
                return Err(REJECTED.into());
            }
            // Expiry before dispatch must not issue a new remote command. Once
            // dispatched, a read-only SSH request cannot be atomically recalled.
            if Instant::now() >= self.deadline {
                return Err(TIMEOUT.into());
            }
            self.bound
                .transport
                .invoke(payload)
                .await
                .map_err(|_| UNAVAILABLE.into())
        };
        timeout_at(self.deadline, operation)
            .await
            .map_err(|_| TIMEOUT.to_owned())?
    }
}

fn observation(
    job: &RuntimeJobV4,
    conversation_id: Uuid,
    status: RemoteJobObservationStatusV4,
    result: Option<&RuntimeResultV4>,
) -> RemoteJobObservationV4 {
    let mut captures = vec![];
    if let Some(result) = result {
        for (stream, capture) in [
            ("stdout", &result.stdout_capture),
            ("stderr", &result.stderr_capture),
        ] {
            if let Some(capture) = capture {
                captures.push(RemoteJobCaptureReferenceV4 {
                    relative_path: format!(".omicsops/remote-jobs/{}/{stream}.txt", job.job_id),
                    sha256: capture.sha256.clone(),
                    size_bytes: Some(capture.total_bytes),
                });
            }
        }
        // The public combined stream/artifact list has its own 128-item budget.
        captures.extend(
            result
                .artifacts
                .iter()
                .take(128 - captures.len())
                .map(|artifact| RemoteJobCaptureReferenceV4 {
                    relative_path: artifact.relative_path.clone(),
                    sha256: artifact.sha256.clone(),
                    size_bytes: Some(artifact.size_bytes),
                }),
        );
    }
    RemoteJobObservationV4 {
        job: summary(
            job,
            conversation_id,
            if result.is_some() {
                RemoteJobQueryEligibilityV4::RecordedReceipt
            } else {
                RemoteJobQueryEligibilityV4::NeedsRootCheck
            },
        ),
        status,
        observed_at: Utc::now().to_rfc3339(),
        stdout_excerpt: result
            .map(|r| public_bounded(&r.stdout, 4096))
            .unwrap_or_default(),
        stderr_excerpt: result
            .map(|r| public_bounded(&r.stderr, 4096))
            .unwrap_or_default(),
        captures,
        resubmitted: false,
    }
}

pub(crate) async fn refresh_remote_job_with_connector(
    store: &Store,
    connector: &dyn RemoteJobConnector,
    request: RefreshRemoteJobV4Request,
) -> Result<RemoteJobObservationV4, String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let recorded = store
        .remote_job_record_v4(request.project_id, request.job_id)
        .await
        .map_err(|_| REJECTED)?
        .ok_or(NOT_FOUND)?;
    if let Some(result) = &recorded.result {
        remote_jobs_v4::validate_recorded_result(&recorded.job, result).map_err(|_| REJECTED)?;
        return Ok(observation(
            &recorded.job,
            recorded.conversation_id,
            RemoteJobObservationStatusV4::Completed,
            Some(result),
        ));
    }
    let initial = async {
        let project = store
            .get_project(request.project_id)
            .await
            .map_err(|_| RemoteJobObservationStatusV4::Unavailable)?
            .ok_or(RemoteJobObservationStatusV4::Rejected)?;
        let profile = current_profile(store, &project, &recorded.job)
            .await
            .map_err(|_| RemoteJobObservationStatusV4::Rejected)?;
        let bound = connector.connect(&project, &recorded.job).await?;
        if bound.canonical_root.as_str()
            != recorded
                .job
                .remote_root
                .as_deref()
                .ok_or(RemoteJobObservationStatusV4::Rejected)?
            || bound.transport.host_key()
                != recorded
                    .job
                    .remote_host_key
                    .as_deref()
                    .ok_or(RemoteJobObservationStatusV4::Rejected)?
        {
            return Err(RemoteJobObservationStatusV4::Rejected);
        }
        Ok((project, profile, bound))
    };
    let (project, profile, bound) = match timeout_at(deadline, initial).await {
        Ok(Ok(bound)) => bound,
        Ok(Err(RemoteJobObservationStatusV4::Rejected)) => return Err(REJECTED.into()),
        Ok(Err(status)) => {
            return Ok(observation(
                &recorded.job,
                recorded.conversation_id,
                status,
                None,
            ));
        }
        Err(_) => {
            return Ok(observation(
                &recorded.job,
                recorded.conversation_id,
                RemoteJobObservationStatusV4::Timeout,
                None,
            ));
        }
    };
    let transport = GuardedStatusTransport {
        store,
        project: &project,
        profile: &profile,
        job: &recorded.job,
        bound: &bound,
        deadline,
    };
    // Only the guarded remote status is deadline-bound. After a valid receipt,
    // query's existing SQLite CAS must finish atomically without cancellation.
    let value = match remote_jobs_v4::query(
        store,
        &transport,
        request.project_id,
        &recorded.job.context.backend_id,
        &bound.canonical_root,
        Some(request.job_id),
    )
    .await
    {
        Ok(value) => value,
        Err(error) if error == TIMEOUT => {
            return Ok(observation(
                &recorded.job,
                recorded.conversation_id,
                RemoteJobObservationStatusV4::Timeout,
                None,
            ));
        }
        Err(error) if error == UNAVAILABLE => {
            return Ok(observation(
                &recorded.job,
                recorded.conversation_id,
                RemoteJobObservationStatusV4::Unavailable,
                None,
            ));
        }
        Err(_) => return Err(REJECTED.into()),
    };
    let job: RuntimeJobV4 = serde_json::from_value(value["job"].clone()).map_err(|_| REJECTED)?;
    if job.job_id != recorded.job.job_id
        || job.context != recorded.job.context
        || job.call_id != recorded.job.call_id
        || job.request_sha256 != recorded.job.request_sha256
        || job.remote_root != recorded.job.remote_root
        || job.remote_host_key != recorded.job.remote_host_key
        || value["resubmitted"] != false
    {
        return Err(REJECTED.into());
    }
    let result: Option<RuntimeResultV4> =
        serde_json::from_value(value["result"].clone()).map_err(|_| REJECTED)?;
    if let Some(result) = &result {
        remote_jobs_v4::validate_recorded_result(&job, result).map_err(|_| REJECTED)?;
    }
    let status = match value["status"].as_str() {
        Some("running") => RemoteJobObservationStatusV4::Running,
        Some("unknown") => RemoteJobObservationStatusV4::Unknown,
        Some("completed") if result.is_some() => RemoteJobObservationStatusV4::Completed,
        _ => return Err(REJECTED.into()),
    };
    Ok(observation(
        &job,
        recorded.conversation_id,
        status,
        result.as_ref(),
    ))
}

#[tauri::command]
pub async fn list_remote_jobs_v4(
    state: State<'_, AppState>,
    request: ListRemoteJobsV4Request,
) -> Result<RemoteJobsPageV4, String> {
    list_remote_jobs_with_store(&state.repository, request).await
}
#[tauri::command]
pub async fn refresh_remote_job_v4(
    state: State<'_, AppState>,
    request: RefreshRemoteJobV4Request,
) -> Result<RemoteJobObservationV4, String> {
    refresh_remote_job_with_connector(
        &state.repository,
        &DesktopConnector { state: &state },
        request,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use omicsops_core::{
        domain::{AuthenticationMethod, ConnectionProfile},
        workspace::{Conversation, Project, ProjectTemplate},
    };
    use omicsops_protocol::*;
    use omicsops_store::Store;
    use serde_json::{Value, json};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    use uuid::Uuid;

    #[derive(Clone)]
    struct FakeTransport {
        key: String,
        calls: Arc<Mutex<Vec<Value>>>,
        response: Arc<Mutex<Option<Value>>>,
        wait: Duration,
        fail: bool,
        hold_connection: Option<(
            Store,
            Arc<Mutex<Option<sqlx::pool::PoolConnection<sqlx::Sqlite>>>>,
        )>,
        barrier: Option<Arc<tokio::sync::Barrier>>,
    }
    #[async_trait]
    impl crate::remote_jobs_v4::Transport for FakeTransport {
        fn host_key(&self) -> &str {
            &self.key
        }
        async fn invoke(&self, payload: Value) -> Result<Value, String> {
            let wait = tokio::time::sleep(self.wait);
            self.calls.lock().unwrap().push(payload.clone());
            if !self.wait.is_zero() {
                wait.await;
            }
            if let Some(barrier) = &self.barrier {
                barrier.wait().await;
            }
            if self.fail {
                return Err("password=secret-sentinel".into());
            }
            if let Some((store, slot)) = &self.hold_connection {
                *slot.lock().unwrap() = Some(store.pool().acquire().await.unwrap());
            }
            Ok(self
                .response
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(json!({"identity":payload["identity"],"status":"running"})))
        }
    }
    struct FakeConnector {
        transport: FakeTransport,
        connects: Arc<AtomicUsize>,
        root: String,
        wait: Duration,
        root_wait: Duration,
        roots: Arc<AtomicUsize>,
        change: Option<(Store, Project)>,
        profile_change: Option<(Store, ConnectionProfile)>,
        job_change: Option<(Store, RuntimeJobV4)>,
    }
    #[async_trait]
    impl RemoteJobConnector for FakeConnector {
        async fn connect(
            &self,
            _: &Project,
            _: &RuntimeJobV4,
        ) -> Result<BoundRemoteJobTransport, RemoteJobObservationStatusV4> {
            let wait = tokio::time::sleep(self.wait);
            self.connects.fetch_add(1, Ordering::SeqCst);
            if !self.wait.is_zero() {
                wait.await;
            }
            let root_wait = tokio::time::sleep(self.root_wait);
            self.roots.fetch_add(1, Ordering::SeqCst);
            if !self.root_wait.is_zero() {
                root_wait.await;
            }
            if let Some((store, project)) = &self.change {
                store.save_project(project).await.unwrap();
            }
            if let Some((store, profile)) = &self.profile_change {
                store.save_connection(profile).await.unwrap();
            }
            if let Some((store, job)) = &self.job_change {
                sqlx::query("UPDATE runtime_jobs_v4 SET value_json=?1 WHERE job_id=?2")
                    .bind(serde_json::to_string(job).unwrap())
                    .bind(job.job_id.to_string())
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
            Ok(BoundRemoteJobTransport {
                transport: Box::new(self.transport.clone()),
                canonical_root: self.root.clone(),
            })
        }
    }
    fn connector() -> FakeConnector {
        FakeConnector {
            transport: FakeTransport {
                key: "test-host".into(),
                calls: Default::default(),
                response: Default::default(),
                wait: Duration::ZERO,
                fail: false,
                hold_connection: None,
                barrier: None,
            },
            connects: Arc::new(AtomicUsize::new(0)),
            root: "/project".into(),
            wait: Duration::ZERO,
            root_wait: Duration::ZERO,
            roots: Default::default(),
            change: None,
            profile_change: None,
            job_change: None,
        }
    }
    async fn fixture(store: &Store) -> (Project, RuntimeJobV4) {
        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            label: "fixture".into(),
            host: "offline.example".into(),
            port: 22,
            username: "fixture".into(),
            authentication: AuthenticationMethod::Password,
            authentication_reference: "fixture-only".into(),
            host_key_fingerprint: Some("test-host".into()),
        };
        store.save_connection(&profile).await.unwrap();
        let mut project = Project::new(
            Uuid::new_v4(),
            "remote",
            "synthetic",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        project.connection_id = Some(profile.id);
        project.remote_root = Some("/project".into());
        store.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "jobs", Utc::now());
        store.save_conversation(&conversation).await.unwrap();
        let plan = ExecutionPlanV4 {
            schema_version: 4,
            objective: "observe".into(),
            steps: vec!["submit".into()],
            completion_criteria: vec!["receipt".into()],
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
                project.id,
                conversation.id,
                "running",
                &json!({"spec":spec}),
            )
            .await
            .unwrap();
        let first = AgentEventV4::first(
            spec.run_id,
            project.id,
            conversation.id,
            Utc::now(),
            AgentEventKindV4::RunCreated {
                mode: RunModeV4::Execute,
            },
        );
        store.append_agent_event_v4(&first).await.unwrap();
        let call = ToolCallV4 {
            call_id: "background".into(),
            tool_id: "runtime.execute".into(),
            arguments: json!({"language":"python","code":"print(42)","background":true}),
        };
        let requested = AgentEventV4::next(
            &first,
            Utc::now(),
            AgentEventKindV4::ToolRequested { call: call.clone() },
        );
        store.append_agent_event_v4(&requested).await.unwrap();
        let dispatch = AgentEventV4::next(
            &requested,
            Utc::now(),
            AgentEventKindV4::ToolDispatchStarted {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                effect: ToolEffectV4::Runtime,
                idempotency_key: call.call_id.clone(),
            },
        );
        store.append_agent_event_v4(&dispatch).await.unwrap();
        let context = ExecutionContextKeyV4 {
            project_id: project.id,
            run_id: spec.run_id,
            backend_id: format!("ssh:{}", profile.id),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        };
        let (job, _) = store
            .reserve_runtime_job_with_remote_root_v4(
                &context,
                &call.call_id,
                &call.canonical_hash().unwrap(),
                Some("/project"),
                Some("test-host"),
            )
            .await
            .unwrap();
        let job = store
            .advance_runtime_job_v4(&job, RuntimeJobStateV4::Running, Some(job.job_id), None)
            .await
            .unwrap();
        (project, job)
    }
    fn receipt(job: &RuntimeJobV4) -> Value {
        let capture = |stream: &str| json!({"excerpt":"42","total_bytes":2,"sha256":"a".repeat(64),"archive_path":format!("/project/.omicsops/remote-jobs/{}/{stream}.txt",job.job_id),"truncated":false});
        json!({"identity":{"job_id":job.job_id,"context":job.context,"request_sha256":job.request_sha256,"remote_root":"/project","remote_host_key":"test-host"},"status":"completed","result":{"request_id":job.job_id,"session_id":job.job_id,"process_identity":format!("ssh-detached:{}",job.job_id),"stdout":"42","stderr":"42","stdout_capture":capture("stdout"),"stderr_capture":capture("stderr"),"succeeded":true,"artifacts":[],"software_versions":{}}})
    }
    fn request(project: &Project, job: &RuntimeJobV4) -> RefreshRemoteJobV4Request {
        RefreshRemoteJobV4Request {
            project_id: project.id,
            job_id: job.job_id,
        }
    }
    async fn unchanged(store: &Store, job: &RuntimeJobV4) {
        let actual = store
            .remote_job_record_v4(job.context.project_id, job.job_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual.job, *job);
        assert!(actual.result.is_none());
    }
    async fn fixture_phase_ready(ready: impl Fn() -> bool, phase: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "fake-clock fixture stalled at {phase}"
            );
            tokio::task::yield_now().await;
        }
    }
    #[tokio::test]
    async fn metadata_is_local_and_status_only_preserves_original_identity() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let connector = connector();
        let page = list_remote_jobs_with_store(
            &store,
            ListRemoteJobsV4Request {
                project_id: project.id,
                conversation_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(page.jobs.len(), 1);
        assert!(!page.remote_liveness_checked);
        assert_eq!(connector.connects.load(Ordering::SeqCst), 0);
        let observation =
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap();
        assert_eq!(observation.status, RemoteJobObservationStatusV4::Running);
        assert!(!observation.resubmitted);
        let calls = connector.transport.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["action"], "status");
        assert_eq!(calls[0]["identity"]["job_id"], job.job_id.to_string());
        assert_eq!(calls[0]["identity"]["request_sha256"], job.request_sha256);
        assert!(calls[0].get("code").is_none());
        unchanged(&store, &job).await;
    }
    #[tokio::test]
    async fn terminal_receipt_is_local_and_projects_bounded_safe_references() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let mut value = receipt(&job);
        let text = "λ".repeat(32768);
        value["result"]["stdout"] = json!(text);
        value["result"]["stdout_capture"]["excerpt"] = json!(text);
        value["result"]["stdout_capture"]["total_bytes"] = json!(65536);
        value["result"]["artifacts"] =
            json!([{"relative_path":"plots/result.png","sha256":"b".repeat(64),"size_bytes":12}]);
        let result: RuntimeResultV4 = serde_json::from_value(value["result"].clone()).unwrap();
        let terminal = store
            .advance_runtime_job_v4(&job, RuntimeJobStateV4::Succeeded, None, Some(&result))
            .await
            .unwrap();
        let mut changed = project.clone();
        changed.connection_id = None;
        store.save_project(&changed).await.unwrap();
        let connector = connector();
        let page = list_remote_jobs_with_store(
            &store,
            ListRemoteJobsV4Request {
                project_id: project.id,
                conversation_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            page.jobs[0].query_eligibility,
            RemoteJobQueryEligibilityV4::RecordedReceipt
        );
        let observation =
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap();
        assert_eq!(observation.status, RemoteJobObservationStatusV4::Completed);
        assert_eq!(
            observation.job.query_eligibility,
            RemoteJobQueryEligibilityV4::RecordedReceipt
        );
        assert_eq!(connector.connects.load(Ordering::SeqCst), 0);
        assert!(observation.stdout_excerpt.len() <= 4096);
        assert!(
            observation
                .captures
                .iter()
                .all(|capture| !capture.relative_path.starts_with('/'))
        );
        assert!(
            !serde_json::to_string(&observation)
                .unwrap()
                .contains("/project")
        );
        assert_eq!(
            store
                .remote_job_record_v4(project.id, job.job_id)
                .await
                .unwrap()
                .unwrap()
                .job,
            terminal
        );
    }
    #[tokio::test]
    async fn changed_actual_binding_and_mid_connect_binding_reject_without_status() {
        for mode in 0..7 {
            let store = Store::open_in_memory().await.unwrap();
            let (mut project, job) = fixture(&store).await;
            let mut connector = connector();
            match mode {
                0 => {
                    let mut profile = store.list_connections().await.unwrap().remove(0);
                    profile.id = Uuid::new_v4();
                    store.save_connection(&profile).await.unwrap();
                    project.connection_id = Some(profile.id);
                    store.save_project(&project).await.unwrap();
                }
                1 => connector.root = "/other".into(),
                2 => connector.transport.key = "other-key".into(),
                3 => {
                    let mut changed = project.clone();
                    changed.remote_root = Some("/other".into());
                    connector.change = Some((store.clone(), changed));
                }
                4 => {
                    let mut profile = store.list_connections().await.unwrap().remove(0);
                    profile.host_key_fingerprint = None;
                    store.save_connection(&profile).await.unwrap();
                }
                5 => {
                    let mut profile = store.list_connections().await.unwrap().remove(0);
                    profile.host_key_fingerprint = Some("changed-host".into());
                    store.save_connection(&profile).await.unwrap();
                }
                _ => {
                    let mut profile = store.list_connections().await.unwrap().remove(0);
                    profile.host = "changed.invalid".into();
                    connector.profile_change = Some((store.clone(), profile));
                }
            }
            assert_eq!(
                refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                    .await
                    .unwrap_err(),
                "remote_job_rejected"
            );
            assert!(connector.transport.calls.lock().unwrap().is_empty());
            unchanged(&store, &job).await;
        }
    }
    #[tokio::test]
    async fn sensitive_receipts_are_rejected_before_cas_and_insert() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let connector = connector();
        let good = receipt(&job);
        for mode in 0..7 {
            let mut value = good.clone();
            match mode {
                0 | 1 => {
                    let text = format!("{}\npassword=secret-sentinel", "x".repeat(5000));
                    let stream = if mode == 0 { "stdout" } else { "stderr" };
                    value["result"][stream] = json!(text);
                    value["result"][format!("{stream}_capture")]["excerpt"] = json!(text);
                    value["result"][format!("{stream}_capture")]["total_bytes"] = json!(text.len());
                }
                2 => value["result"]["software_versions"] = json!({"password":"secret-sentinel"}),
                3 => {
                    value["result"]["software_versions"] =
                        json!({"python":"Authorization: Bearer secret-sentinel"})
                }
                4 => {
                    value["result"]["artifacts"] = json!([{"relative_path":"plots/../secret-sentinel","sha256":"a".repeat(64),"size_bytes":1}])
                }
                5 => {
                    value["result"]["artifacts"] = json!([{"relative_path":"plots/.omicsops/secret-sentinel","sha256":"a".repeat(64),"size_bytes":1}])
                }
                _ => value["result"]["session_id"] = json!(Uuid::new_v4()),
            }
            *connector.transport.response.lock().unwrap() = Some(value);
            let error =
                refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                    .await
                    .unwrap_err();
            assert_eq!(error, "remote_job_rejected");
            assert!(!error.contains("secret-sentinel"));
            unchanged(&store, &job).await;
        }
    }
    #[tokio::test]
    async fn whole_deadline_covers_connect_root_and_status_without_writes() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        tokio::time::pause();
        // SQLx uses a real worker thread; prevent Tokio's automatic clock jump
        // from timing out the pool while that thread finishes a local read.
        let keep_awake = tokio::spawn(async {
            loop {
                tokio::task::yield_now().await;
            }
        });
        for (connect_wait, root_wait, status_wait, want_calls) in
            [(21, 0, 0, 0), (5, 16, 0, 0), (0, 19, 2, 1)]
        {
            let mut connector = connector();
            connector.wait = Duration::from_secs(connect_wait);
            connector.root_wait = Duration::from_secs(root_wait);
            connector.transport.wait = Duration::from_secs(status_wait);
            let connector = Arc::new(connector);
            let task_store = store.clone();
            let task_connector = connector.clone();
            let request = request(&project, &job);
            let task = tokio::spawn(async move {
                refresh_remote_job_with_connector(&task_store, &*task_connector, request).await
            });
            fixture_phase_ready(|| connector.connects.load(Ordering::SeqCst) > 0, "connect").await;
            if want_calls == 0 {
                if root_wait > 0 {
                    tokio::time::advance(
                        Duration::from_secs(connect_wait) + Duration::from_millis(1),
                    )
                    .await;
                    fixture_phase_ready(|| connector.roots.load(Ordering::SeqCst) > 0, "root")
                        .await;
                    tokio::time::advance(Duration::from_secs(20 - connect_wait)).await;
                } else {
                    tokio::time::advance(Duration::from_secs(20) + Duration::from_millis(1)).await;
                }
            } else {
                fixture_phase_ready(|| connector.roots.load(Ordering::SeqCst) > 0, "root").await;
                tokio::time::advance(Duration::from_secs(19) + Duration::from_millis(1)).await;
                fixture_phase_ready(
                    || !connector.transport.calls.lock().unwrap().is_empty(),
                    "status",
                )
                .await;
                tokio::time::advance(Duration::from_secs(1)).await;
            }
            fixture_phase_ready(|| task.is_finished(), "remote timeout response").await;
            let observation = task.await.unwrap().unwrap();
            assert_eq!(observation.status, RemoteJobObservationStatusV4::Timeout);
            assert!(!observation.resubmitted);
            assert_eq!(connector.transport.calls.lock().unwrap().len(), want_calls);
            unchanged(&store, &job).await;
        }
        keep_awake.abort();
        tokio::time::resume();
    }

    #[tokio::test]
    async fn unavailable_status_has_safe_error_and_no_effects() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let mut connector = connector();
        connector.transport.fail = true;
        let observation =
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap();
        assert_eq!(
            observation.status,
            RemoteJobObservationStatusV4::Unavailable
        );
        assert!(
            !serde_json::to_string(&observation)
                .unwrap()
                .contains("secret-sentinel")
        );
        assert!(!observation.resubmitted);
        unchanged(&store, &job).await;
    }
    #[tokio::test]
    async fn changed_original_request_during_connect_is_rejected_before_status_dispatch() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let mut connector = connector();
        let mut changed = job.clone();
        changed.request_sha256 = "f".repeat(64);
        connector.job_change = Some((store.clone(), changed.clone()));
        assert_eq!(
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap_err(),
            "remote_job_rejected"
        );
        assert!(
            connector.transport.calls.lock().unwrap().is_empty(),
            "query must use the originally validated identity, not newly changed job metadata"
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runtime_job_results_v4")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let serialized: String =
            sqlx::query_scalar("SELECT value_json FROM runtime_jobs_v4 WHERE job_id=?1")
                .bind(job.job_id.to_string())
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<RuntimeJobV4>(&serialized).unwrap(),
            changed,
            "only the intentional test tamper remains; service adds no state change"
        );
    }
    #[tokio::test]
    async fn concurrent_terminal_observations_accept_only_identical_receipts() {
        for identical in [true, false] {
            let store = Store::open_in_memory().await.unwrap();
            let (project, job) = fixture(&store).await;
            let barrier = Arc::new(tokio::sync::Barrier::new(2));
            let mut a = connector();
            let mut b = connector();
            a.transport.barrier = Some(barrier.clone());
            b.transport.barrier = Some(barrier);
            *a.transport.response.lock().unwrap() = Some(receipt(&job));
            let mut other = receipt(&job);
            if !identical {
                other["result"]["stdout"] = json!("43");
                other["result"]["stdout_capture"]["excerpt"] = json!("43");
            }
            *b.transport.response.lock().unwrap() = Some(other);
            let (first, second) = tokio::join!(
                refresh_remote_job_with_connector(&store, &a, request(&project, &job)),
                refresh_remote_job_with_connector(&store, &b, request(&project, &job))
            );
            if identical {
                assert_eq!(
                    first.unwrap().status,
                    RemoteJobObservationStatusV4::Completed
                );
                assert_eq!(
                    second.unwrap().status,
                    RemoteJobObservationStatusV4::Completed
                );
            } else {
                assert!(first.is_ok() != second.is_ok());
                assert_eq!(first.err().or(second.err()).unwrap(), "remote_job_rejected");
            }
            let stored = store
                .remote_job_record_v4(project.id, job.job_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(stored.job.state, RuntimeJobStateV4::Succeeded);
            assert!(stored.result.is_some());
            assert_eq!(a.transport.calls.lock().unwrap().len(), 1);
            assert_eq!(b.transport.calls.lock().unwrap().len(), 1);
        }
    }
    #[tokio::test]
    async fn successful_status_commits_atomically_after_remote_deadline_during_local_db_wait() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let slot = Arc::new(Mutex::new(None));
        let mut connector = connector();
        connector.transport.hold_connection = Some((store.clone(), slot.clone()));
        *connector.transport.response.lock().unwrap() = Some(receipt(&job));
        tokio::time::pause();
        let keep_awake = tokio::spawn(async {
            loop {
                tokio::task::yield_now().await;
            }
        });
        let task_store = store.clone();
        let request = request(&project, &job);
        let task = tokio::spawn(async move {
            refresh_remote_job_with_connector(&task_store, &connector, request).await
        });
        fixture_phase_ready(
            || slot.lock().unwrap().is_some(),
            "successful status/local CAS pool wait",
        )
        .await;
        // Transport completed and the actual Store CAS is waiting for its pool
        // connection. Passing the remote deadline must not cancel this commit.
        tokio::time::advance(Duration::from_secs(21)).await;
        drop(slot.lock().unwrap().take());
        fixture_phase_ready(|| task.is_finished(), "local CAS completion").await;
        let observation = task.await.unwrap().unwrap();
        assert_eq!(observation.status, RemoteJobObservationStatusV4::Completed);
        let stored = store
            .remote_job_record_v4(project.id, job.job_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.job.state, RuntimeJobStateV4::Succeeded);
        assert!(stored.result.is_some());
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM runtime_job_results_v4 WHERE job_id=?1")
                .bind(job.job_id.to_string())
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(count, 1);
        keep_awake.abort();
        tokio::time::resume();
    }
    #[tokio::test]
    async fn decoded_replacement_capture_preserves_raw_hash_and_bytes() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let connector = connector();
        let mut value = receipt(&job);
        value["result"]["stdout"] = json!("�");
        value["result"]["stdout_capture"]["excerpt"] = json!("�");
        value["result"]["stdout_capture"]["total_bytes"] = json!(1);
        let original: RuntimeResultV4 = serde_json::from_value(value["result"].clone()).unwrap();
        *connector.transport.response.lock().unwrap() = Some(value);
        let observation =
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap();
        assert_eq!(observation.status, RemoteJobObservationStatusV4::Completed);
        let stored = store
            .remote_job_record_v4(project.id, job.job_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.result, Some(original.clone()));
        use sha2::{Digest, Sha256};
        assert_eq!(
            stored.job.result_sha256,
            Some(hex::encode(Sha256::digest(
                serde_json::to_vec(&original).unwrap()
            )))
        );
    }
    #[tokio::test]
    async fn local_corrupt_receipt_rejects_without_connect_or_fallback() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let result: RuntimeResultV4 =
            serde_json::from_value(receipt(&job)["result"].clone()).unwrap();
        store
            .advance_runtime_job_v4(&job, RuntimeJobStateV4::Succeeded, None, Some(&result))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE runtime_job_results_v4 SET result_json=result_json || ' ' WHERE job_id=?1",
        )
        .bind(job.job_id.to_string())
        .execute(store.pool())
        .await
        .unwrap();
        let connector = connector();
        assert_eq!(
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap_err(),
            "remote_job_rejected"
        );
        assert_eq!(connector.connects.load(Ordering::SeqCst), 0);
        assert!(connector.transport.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn reopen_retired_receipt_unknown_and_alias_use_original_status_only() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("jobs.sqlite");
        let store = Store::open(&path).await.unwrap();
        let (mut project, job) = fixture(&store).await;
        let connector = connector();
        let mut unknown = receipt(&job);
        unknown.as_object_mut().unwrap().remove("result");
        unknown["status"] = json!("unknown");
        *connector.transport.response.lock().unwrap() = Some(unknown);
        project.remote_root = Some("/alias".into());
        store.save_project(&project).await.unwrap();
        drop(store);
        let store = Store::open(&path).await.unwrap();
        let observation =
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap();
        assert_eq!(observation.status, RemoteJobObservationStatusV4::Unknown);
        unchanged(&store, &job).await;
        *connector.transport.response.lock().unwrap() = Some(receipt(&job));
        let observation =
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap();
        assert_eq!(observation.status, RemoteJobObservationStatusV4::Completed);
        sqlx::query("DELETE FROM runtime_job_results_v4 WHERE job_id=?1")
            .bind(job.job_id.to_string())
            .execute(store.pool())
            .await
            .unwrap();
        let page = list_remote_jobs_with_store(
            &store,
            ListRemoteJobsV4Request {
                project_id: project.id,
                conversation_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            page.jobs[0].query_eligibility,
            RemoteJobQueryEligibilityV4::NeedsRootCheck
        );
        let observation =
            refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                .await
                .unwrap();
        assert_eq!(observation.status, RemoteJobObservationStatusV4::Completed);
        assert!(
            connector
                .transport
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|call| call["action"] == "status"
                    && call["identity"]["remote_root"] == "/project")
        );
    }
    #[tokio::test]
    async fn capture_consistency_and_software_budget_reject_without_writes() {
        let store = Store::open_in_memory().await.unwrap();
        let (project, job) = fixture(&store).await;
        let connector = connector();
        for mode in 0..5 {
            let mut value = receipt(&job);
            match mode {
                0 => value["result"]["stdout_capture"]["total_bytes"] = json!(0),
                1 => value["result"]["stdout_capture"]["total_bytes"] = json!(1),
                2 => value["result"]["stdout_capture"]["truncated"] = json!(true),
                3 => value["result"]["software_versions"] = json!({"python":"x".repeat(65537)}),
                _ => {
                    value["result"]["software_versions"] = Value::Object(
                        (0..129)
                            .map(|i| (format!("software-{i}"), json!("1")))
                            .collect(),
                    )
                }
            }
            *connector.transport.response.lock().unwrap() = Some(value);
            assert_eq!(
                refresh_remote_job_with_connector(&store, &connector, request(&project, &job))
                    .await
                    .unwrap_err(),
                "remote_job_rejected"
            );
            unchanged(&store, &job).await;
        }
    }
}
