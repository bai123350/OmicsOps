//! Detached one-shot SSH jobs: local launch authority, remote durable receipt.
use async_trait::async_trait;
use base64::Engine as _;
use omicsops_adapters::ssh::SshSession;
use omicsops_core::project::shell_quote;
use omicsops_protocol::{ExecutionContextKeyV4, RuntimeJobV4, RuntimeJobStateV4, RuntimeResultV4, ToolCallV4};
use omicsops_store::Store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub(crate) trait Transport: Send + Sync {
    fn host_key(&self) -> &str;
    async fn invoke(&self, payload: Value) -> Result<Value, String>;
}

pub(crate) struct SshTransport { pub session: Arc<SshSession> }
#[async_trait]
impl Transport for SshTransport {
    fn host_key(&self) -> &str { self.session.fingerprint() }
    async fn invoke(&self, payload: Value) -> Result<Value, String> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&payload).map_err(|e| e.to_string())?);
        let command = format!("python3 -c {} {}", shell_quote(include_str!("remote_job_bridge.py")), shell_quote(&encoded));
        let output = self.session.execute_checked(&command).await.map_err(|_| "remote job transport unavailable; query the same job again".to_owned())?;
        if output.stdout.len() > 512 * 1024 { return Err("remote job receipt exceeds limit".into()); }
        serde_json::from_str(&output.stdout).map_err(|_| "invalid remote job response".into())
    }
}

fn identity(job: &RuntimeJobV4) -> Value {
    json!({"job_id":job.job_id,"context":job.context,"request_sha256":job.request_sha256,"remote_root":job.remote_root,"remote_host_key":job.remote_host_key})
}

pub(crate) async fn submit(
    store: &Store, transport: &dyn Transport, root: &str,
    key: &ExecutionContextKeyV4, call: &ToolCallV4,
) -> Result<Value, String> {
    let code = call.arguments.get("code").and_then(Value::as_str).ok_or("code required")?;
    let captures = call.arguments.get("capture_paths").cloned().unwrap_or(json!([]));
    if code.len() > 16 * 1024 || captures.to_string().len() > 16 * 1024
        || captures.as_array().is_none_or(|paths| paths.len() > 128 || paths.iter().any(|path| !path.is_string()))
        || call.arguments.get("analysis").is_some() {
        return Err("background jobs require bounded code/captures and no inline analysis declaration".into());
    }
    let hash = call.canonical_hash().map_err(|e| e.to_string())?;
    let (job, acquired) = store.reserve_runtime_job_with_remote_root_v4(key, &call.call_id, &hash, Some(root), Some(transport.host_key())).await.map_err(|e| e.to_string())?;
    if !acquired { return Ok(json!({"job":job,"status":"already_reserved","resubmitted":false})); }
    let job = store.advance_runtime_job_v4(&job, RuntimeJobStateV4::Running, Some(job.job_id), None).await.map_err(|e| e.to_string())?;
    let response = transport.invoke(json!({"action":"submit","identity":identity(&job),"code":code,"captures":captures})).await;
    let observed = response.as_ref().is_ok_and(|value| value.get("identity") == Some(&identity(&job)) && value.get("status").and_then(Value::as_str) == Some("submitted"));
    // A lost acknowledgement must not turn a second submission into a retry.
    Ok(json!({"job":job,"status":if observed {"submitted"} else {"unknown"},"submission_observed":observed,"resubmitted":false}))
}

fn validate_observation(job: &RuntimeJobV4, value: &Value) -> Result<Option<RuntimeResultV4>, String> {
    if value.get("identity") != Some(&identity(job)) { return Err("remote job identity mismatch".into()); }
    match value.get("status").and_then(Value::as_str) {
        Some("running" | "unknown") if value.get("result").is_none() => Ok(None),
        Some("completed") => {
            let result: RuntimeResultV4 = serde_json::from_value(value.get("result").cloned().ok_or("remote receipt missing")?).map_err(|_| "invalid remote receipt")?;
            if result.request_id != job.job_id || result.session_id != job.job_id
                || result.process_identity != format!("ssh-detached:{}", job.job_id)
                || result.stdout.len() > 65536 || result.stderr.len() > 65536 || result.artifacts.len() > 128 {
                return Err("remote receipt identity or size mismatch".into());
            }
            let root = job.remote_root.as_deref().ok_or("job is not detached")?;
            for (stream, capture, text) in [("stdout", &result.stdout_capture, &result.stdout), ("stderr", &result.stderr_capture, &result.stderr)] {
                let capture = capture.as_ref().ok_or("remote output capture missing")?;
                if capture.archive_path != format!("{root}/.omicsops/remote-jobs/{}/{stream}.txt", job.job_id)
                    || capture.excerpt != *text || !valid_hash(&capture.sha256) {
                    return Err("remote output capture mismatch".into());
                }
            }
            let paths = result.artifacts.iter().map(|a| a.relative_path.clone()).collect::<Vec<_>>();
            omicsops_adapters::kernel::validate_capture_paths(&paths).map_err(|e| e.to_string())?;
            if result.artifacts.iter().any(|a| !valid_hash(&a.sha256)) { return Err("invalid artifact digest".into()); }
            Ok(Some(result))
        }
        _ => Err("invalid remote job status".into()),
    }
}
fn valid_hash(value: &str) -> bool { value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) }

pub(crate) async fn query(
    store: &Store, transport: &dyn Transport, project_id: Uuid, backend: &str, root: &str, job_id: Option<Uuid>,
) -> Result<Value, String> {
    let jobs = store.remote_runtime_jobs_v4(project_id, backend, root, job_id, transport.host_key()).await.map_err(|e| e.to_string())?;
    if job_id.is_none() { return Ok(json!({"jobs":jobs,"limit":100,"remote_liveness_checked":false})); }
    let job = jobs.into_iter().next().ok_or("remote job not found in this project/backend/root")?;
    let value = transport.invoke(json!({"action":"status","identity":identity(&job)})).await?;
    let result = validate_observation(&job, &value)?;
    let mut recorded = job.clone();
    if let Some(result) = &result {
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(result).map_err(|e| e.to_string())?));
        if job.state == RuntimeJobStateV4::Running {
            recorded = match store.advance_runtime_job_v4(&job, if result.succeeded {RuntimeJobStateV4::Succeeded} else {RuntimeJobStateV4::Failed}, None, Some(result)).await {
                Ok(updated) => updated,
                Err(error) => {
                    let current = store.remote_runtime_jobs_v4(project_id, backend, root, job_id, transport.host_key()).await.map_err(|e| e.to_string())?.into_iter().next().ok_or("job disappeared")?;
                    if current.result_sha256.as_deref() != Some(&digest) { return Err(error.to_string()); }
                    current
                }
            };
        } else if job.result_sha256.as_deref() != Some(&digest) { return Err("remote result differs from recorded receipt".into()); }
    }
    Ok(json!({"job":recorded,"status":value["status"],"result":result,"resubmitted":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use omicsops_core::workspace::{Project, ProjectTemplate, Conversation};
    use omicsops_protocol::*;
    use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};

    struct FakeTransport { submissions: AtomicUsize, observation: Mutex<Option<Value>>, lose_ack: bool, host_key: &'static str }
    #[async_trait]
    impl Transport for FakeTransport {
        fn host_key(&self) -> &str { self.host_key }
        async fn invoke(&self, payload: Value) -> Result<Value, String> {
            if payload["action"] == "submit" {
                self.submissions.fetch_add(1, Ordering::SeqCst);
                if self.lose_ack { return Err("disconnected after dispatch".into()); }
                return Ok(json!({"identity":payload["identity"],"status":"submitted"}));
            }
            assert_eq!(payload["action"], "status");
            assert!(payload.get("code").is_none());
            Ok(self.observation.lock().unwrap().clone().unwrap_or(json!({"identity":payload["identity"],"status":"running"})))
        }
    }
    async fn fixture(store: &Store) -> (ExecutionContextKeyV4, ToolCallV4) {
        fixture_with_code(store, "print(42)").await
    }
    async fn fixture_with_code(store: &Store, code: &str) -> (ExecutionContextKeyV4, ToolCallV4) {
        let project = Project::new(Uuid::new_v4(), "remote", "synthetic", ProjectTemplate::Blank, Utc::now());
        store.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "jobs", Utc::now());
        store.save_conversation(&conversation).await.unwrap();
        let plan = ExecutionPlanV4 { schema_version:4, objective:"submit".into(), steps:vec!["submit".into()], completion_criteria:vec!["job id".into()], requested_capabilities: Default::default() };
        let spec = RunSpecV4::freeze(Uuid::new_v4(), project.id, conversation.id, Uuid::new_v4(), plan.clone(), &plan.canonical_hash().unwrap(), Utc::now()).unwrap();
        store.save_agent_run_v4_if_unlocked(spec.run_id, project.id, conversation.id, "running", &json!({"spec":spec})).await.unwrap();
        let first = AgentEventV4::first(spec.run_id, project.id, conversation.id, Utc::now(), AgentEventKindV4::RunCreated { mode:RunModeV4::Execute });
        store.append_agent_event_v4(&first).await.unwrap();
        let call = ToolCallV4 { call_id:"background".into(), tool_id:"runtime.execute".into(), arguments:json!({"language":"python","code":code,"background":true}) };
        let request = AgentEventV4::next(&first, Utc::now(), AgentEventKindV4::ToolRequested {call:call.clone()});
        store.append_agent_event_v4(&request).await.unwrap();
        let dispatch = AgentEventV4::next(&request, Utc::now(), AgentEventKindV4::ToolDispatchStarted {call_id:call.call_id.clone(),tool_id:call.tool_id.clone(),effect:ToolEffectV4::Runtime,idempotency_key:call.call_id.clone()});
        store.append_agent_event_v4(&dispatch).await.unwrap();
        (ExecutionContextKeyV4 {project_id:project.id,run_id:spec.run_id,backend_id:"ssh:fixture".into(),language:KernelLanguageV4::Python,environment:"system".into()},call)
    }
    fn receipt(job: &RuntimeJobV4) -> Value {
        let capture = |stream: &str| json!({"excerpt":"42","total_bytes":2,"sha256":"a".repeat(64),"archive_path":format!("{}/.omicsops/remote-jobs/{}/{stream}.txt",job.remote_root.as_deref().unwrap(),job.job_id),"truncated":false});
        json!({"identity":identity(job),"status":"completed","result":{"request_id":job.job_id,"session_id":job.job_id,"process_identity":format!("ssh-detached:{}",job.job_id),"stdout":"42","stderr":"42","stdout_capture":capture("stdout"),"stderr_capture":capture("stderr"),"succeeded":true,"artifacts":[],"software_versions":{}}})
    }

    #[tokio::test]
    #[ignore = "requires explicit pinned Linux SSH credentials and an empty disposable OMICSOPS_LIVE_REMOTE_JOB_ROOT"]
    async fn live_remote_job_survives_disconnect_and_store_reopen() {
        use omicsops_adapters::ssh::SshAuthentication;
        use omicsops_core::domain::{ConnectionProfile, AuthenticationMethod};
        let profile = ConnectionProfile {
            id:Uuid::new_v4(), label:"remote job acceptance".into(),
            host:std::env::var("OMICSOPS_LIVE_SSH_HOST").expect("host"),
            port:std::env::var("OMICSOPS_LIVE_SSH_PORT").ok().map(|p| p.parse().expect("port")).unwrap_or(22),
            username:std::env::var("OMICSOPS_LIVE_SSH_USER").expect("user"),
            authentication:AuthenticationMethod::Password, authentication_reference:"acceptance".into(),
            host_key_fingerprint:Some(std::env::var("OMICSOPS_LIVE_SSH_FINGERPRINT").expect("pinned fingerprint")),
        };
        assert!(!profile.host_key_fingerprint.as_ref().unwrap().trim().is_empty());
        let connect = || SshSession::connect(&profile, SshAuthentication::Password(std::env::var("OMICSOPS_LIVE_SSH_PASSWORD").expect("password")));
        let session = connect().await.expect("initial connection");
        let configured_root = std::env::var("OMICSOPS_LIVE_REMOTE_JOB_ROOT").expect("empty disposable root");
        assert!(configured_root.starts_with('/') && configured_root != "/");
        let checked = session.execute_checked(&format!("python3 -c {} {}", shell_quote("import pathlib,sys; p=pathlib.Path(sys.argv[1]).resolve(strict=True); assert p.is_dir() and not any(p.iterdir()); print(p)"),shell_quote(&configured_root))).await.expect("root must be empty");
        let root = checked.stdout.trim().to_owned();
        let temp = tempfile::tempdir().unwrap(); let path = temp.path().join("jobs.sqlite");
        let store = Store::open(&path).await.unwrap();
        let (key,call) = fixture_with_code(&store, "import pathlib,time\np=pathlib.Path('dispatch-count.txt')\nwith p.open('x') as f: f.write('once')\ntime.sleep(3)\nprint('remote-job-completed')").await;
        let transport = SshTransport {session:Arc::new(session)};
        let submitted = submit(&store,&transport,&root,&key,&call).await.unwrap();
        assert_eq!(submitted["status"],"submitted");
        let job_id = Uuid::parse_str(submitted["job"]["job_id"].as_str().unwrap()).unwrap();
        let session = Arc::try_unwrap(transport.session).ok().expect("owned SSH connection");
        session.disconnect().await.unwrap();
        drop(store);
        let reopened = Store::open(&path).await.unwrap();
        let reconnected = SshTransport {session:Arc::new(connect().await.expect("reconnection"))};
        let observed = tokio::time::timeout(std::time::Duration::from_secs(45), async {
            loop {
                let value = query(&reopened,&reconnected,key.project_id,&key.backend_id,&root,Some(job_id)).await.unwrap();
                if value["status"] == "completed" { break value; }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }).await.expect("remote completion deadline");
        assert_eq!(observed["job"]["state"],"succeeded");
        assert!(observed["result"]["stdout"].as_str().unwrap().contains("remote-job-completed"));
        let count = reconnected.session.execute_checked(&format!("cat -- {}",shell_quote(&format!("{root}/dispatch-count.txt")))).await.unwrap();
        assert_eq!(count.stdout,"once");
    }

    #[tokio::test]
    async fn remote_job_reconnects_after_store_reopen_without_resubmission() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("jobs.sqlite");
        let store = Store::open(&path).await.unwrap();
        let (key, call) = fixture(&store).await;
        let transport = FakeTransport {submissions:AtomicUsize::new(0),observation:Mutex::new(None),lose_ack:true,host_key:"test-host"};
        let (a,b) = tokio::join!(submit(&store,&transport,"/project",&key,&call),submit(&store,&transport,"/project",&key,&call));
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(transport.submissions.load(Ordering::SeqCst),1);
        let job = store.remote_runtime_jobs_v4(key.project_id,&key.backend_id,"/project",None,"test-host").await.unwrap().remove(0);
        let last = store.agent_events_v4(key.run_id).await.unwrap().pop().unwrap();
        store.append_agent_event_v4(&AgentEventV4::next(&last,Utc::now(),AgentEventKindV4::RunCancelled)).await.unwrap();
        drop(store);
        let reopened = Store::open(&path).await.unwrap();
        let observer = FakeTransport {submissions:AtomicUsize::new(0),observation:Mutex::new(None),lose_ack:false,host_key:"test-host"};
        let pending = query(&reopened,&observer,key.project_id,&key.backend_id,"/project",Some(job.job_id)).await.unwrap();
        assert_eq!(pending["status"],"running");
        assert_eq!(pending["job"]["state"],"running");
        *observer.observation.lock().unwrap() = Some(receipt(&job));
        let (a,b) = tokio::join!(query(&reopened,&observer,key.project_id,&key.backend_id,"/project",Some(job.job_id)),query(&reopened,&observer,key.project_id,&key.backend_id,"/project",Some(job.job_id)));
        assert_eq!(a.unwrap()["job"]["state"],"succeeded");
        assert_eq!(b.unwrap()["job"]["state"],"succeeded");
        assert_eq!(observer.submissions.load(Ordering::SeqCst),0);
        let different_host = FakeTransport {submissions:AtomicUsize::new(0),observation:Mutex::new(None),lose_ack:false,host_key:"other-host"};
        assert!(query(&reopened,&different_host,key.project_id,&key.backend_id,"/project",Some(job.job_id)).await.is_err());
        for (project,backend,root) in [(Uuid::new_v4(),key.backend_id.as_str(),"/project"),(key.project_id,"ssh:other","/project"),(key.project_id,key.backend_id.as_str(),"/other")] {
            assert!(query(&reopened,&observer,project,backend,root,Some(job.job_id)).await.is_err());
        }
    }

    #[tokio::test]
    async fn remote_job_rejects_forged_receipts_and_changed_launch_identity() {
        let store = Store::open_in_memory().await.unwrap();
        let (key,call) = fixture(&store).await;
        let transport = FakeTransport {submissions:AtomicUsize::new(0),observation:Mutex::new(None),lose_ack:false,host_key:"test-host"};
        submit(&store,&transport,"/project",&key,&call).await.unwrap();
        let job = store.remote_runtime_jobs_v4(key.project_id,&key.backend_id,"/project",None,"test-host").await.unwrap().remove(0);
        let good = receipt(&job);
        for pointer in ["/identity/request_sha256","/result/session_id","/result/stdout_capture/archive_path","/result/stdout_capture/sha256"] {
            let mut bad = good.clone();
            *bad.pointer_mut(pointer).unwrap() = json!("mismatch");
            assert!(validate_observation(&job,&bad).is_err());
        }
        assert!(submit(&store,&transport,"/other",&key,&call).await.is_err());
        let mut changed = call.clone(); changed.arguments["code"] = json!("print(99)");
        assert!(submit(&store,&transport,"/project",&key,&changed).await.is_err());
        assert_eq!(transport.submissions.load(Ordering::SeqCst),1);
        *transport.observation.lock().unwrap() = Some(json!({"identity":identity(&job),"status":"unknown"}));
        let unknown = query(&store,&transport,key.project_id,&key.backend_id,"/project",Some(job.job_id)).await.unwrap();
        assert_eq!(unknown["status"],"unknown");
        assert!(unknown["result"].is_null());
    }
}
