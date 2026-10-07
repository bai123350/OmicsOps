mod jobs;
mod process_backend;
pub use jobs::RuntimeJobHandleV4;

pub use process_backend::{ContainerKernelBackendV4, LocalKernelBackendV4};

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use omicsops_protocol::{
    AutonomyModeV4, ComputeBackendDescriptorV4, ExecutionContextKeyV4,
    KERNEL_INSPECTION_TIMEOUT_SECONDS, KernelInspectionPayloadV4, KernelInspectionStatusV4,
    RuntimeResultV4, validate_inspection_payload,
};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Closed errors: inspection never returns worker text or user object content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelInspectionErrorV4 {
    Unsupported,
    Busy,
    Unavailable,
    Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelInspectionResultV4 {
    pub status: KernelInspectionStatusV4,
    pub session_id: Option<Uuid>,
    pub payload: Option<KernelInspectionPayloadV4>,
}

#[async_trait]
pub trait KernelProcessV4: Send + Sync {
    fn session_id(&self) -> Uuid;
    fn process_identity(&self) -> &str;
    /// Whether a later cell may safely reuse this process after the previous
    /// operation. Implementations should return false after losing protocol or
    /// transport synchronization; the manager will replace the process without
    /// replaying the failed cell.
    fn is_healthy(&self) -> bool {
        true
    }
    async fn inspect(&self) -> Result<KernelInspectionPayloadV4, KernelInspectionErrorV4> {
        Err(KernelInspectionErrorV4::Unsupported)
    }
    async fn execute(
        &self,
        code: String,
        capture_paths: Vec<String>,
    ) -> Result<RuntimeResultV4, String>;
    async fn interrupt(&self) -> Result<(), String>;
}

#[async_trait]
pub trait KernelBackendV4: Send + Sync {
    fn descriptor(&self) -> ComputeBackendDescriptorV4;

    async fn launch(&self, key: &ExecutionContextKeyV4)
    -> Result<Arc<dyn KernelProcessV4>, String>;
}

#[derive(Default)]
pub struct ComputeBackendRegistryV4 {
    backends: HashMap<String, Arc<dyn KernelBackendV4>>,
}

impl ComputeBackendRegistryV4 {
    pub fn register(&mut self, backend: Arc<dyn KernelBackendV4>) -> Result<(), String> {
        let descriptor = backend.descriptor();
        if descriptor.schema_version != 4 || descriptor.backend_id.trim().is_empty() {
            return Err("invalid V4 compute backend descriptor".into());
        }
        if self
            .backends
            .insert(descriptor.backend_id.clone(), backend)
            .is_some()
        {
            return Err(format!(
                "duplicate V4 compute backend {}",
                descriptor.backend_id
            ));
        }
        Ok(())
    }

    pub fn descriptors(&self) -> Vec<ComputeBackendDescriptorV4> {
        let mut descriptors = self
            .backends
            .values()
            .map(|backend| backend.descriptor())
            .collect::<Vec<_>>();
        descriptors.sort_by(|left, right| left.backend_id.cmp(&right.backend_id));
        descriptors
    }

    pub fn runtime(
        &self,
        backend_id: &str,
        autonomy: AutonomyModeV4,
    ) -> Result<RuntimeManagerV4, String> {
        let backend = self
            .backends
            .get(backend_id)
            .ok_or_else(|| format!("compute backend {backend_id} is not registered"))?;
        let descriptor = backend.descriptor();
        if !descriptor.permits(autonomy) {
            return Err(match autonomy {
                AutonomyModeV4::FullAuto => {
                    "Full Auto requires an available container-isolated backend".into()
                }
                AutonomyModeV4::Supervised => {
                    format!("compute backend {backend_id} is unavailable")
                }
            });
        }
        Ok(RuntimeManagerV4::new(backend.clone()))
    }
}

pub struct RuntimeManagerV4 {
    backend: Arc<dyn KernelBackendV4>,
    sessions: Mutex<HashMap<ExecutionContextKeyV4, Arc<dyn KernelProcessV4>>>,
    jobs: std::sync::Mutex<jobs::Jobs>,
}

impl RuntimeManagerV4 {
    pub fn new(backend: Arc<dyn KernelBackendV4>) -> Self {
        Self {
            backend,
            sessions: Mutex::new(HashMap::new()),
            jobs: Default::default(),
        }
    }

    pub fn backend_descriptor(&self) -> ComputeBackendDescriptorV4 {
        self.backend.descriptor()
    }

    /// Observe only the existing session. Never acquires, replaces, or interrupts it.
    pub async fn inspect_existing(
        &self,
        key: &ExecutionContextKeyV4,
        expected_session_id: Option<Uuid>,
    ) -> KernelInspectionResultV4 {
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(KERNEL_INSPECTION_TIMEOUT_SECONDS);
        let unavailable = |status, session_id| KernelInspectionResultV4 {
            status,
            session_id,
            payload: None,
        };
        let session = match tokio::time::timeout_at(deadline, self.sessions.lock()).await {
            Ok(sessions) => sessions.get(key).cloned(),
            Err(_) => return unavailable(KernelInspectionStatusV4::Busy, None),
        };
        let Some(session) = session else {
            return unavailable(KernelInspectionStatusV4::NotStarted, None);
        };
        let id = session.session_id();
        if expected_session_id.is_some_and(|expected| expected != id) {
            return unavailable(KernelInspectionStatusV4::StaleSession, Some(id));
        }
        if !session.is_healthy() {
            return unavailable(KernelInspectionStatusV4::Unavailable, Some(id));
        }
        let inspection = tokio::time::timeout_at(deadline, session.inspect()).await;
        let sessions = match tokio::time::timeout_at(deadline, self.sessions.lock()).await {
            Ok(sessions) => sessions,
            Err(_) => return unavailable(KernelInspectionStatusV4::Unavailable, Some(id)),
        };
        let current = sessions.get(key);
        if !current
            .is_some_and(|current| Arc::ptr_eq(current, &session) && current.session_id() == id)
        {
            return unavailable(
                KernelInspectionStatusV4::StaleSession,
                current.map(|current| current.session_id()),
            );
        }
        match inspection {
            Ok(Ok(payload))
                if session.is_healthy() && validate_inspection_payload(&payload).is_ok() =>
            {
                KernelInspectionResultV4 {
                    status: KernelInspectionStatusV4::Ready,
                    session_id: Some(id),
                    payload: Some(payload),
                }
            }
            Ok(Err(KernelInspectionErrorV4::Busy)) => {
                unavailable(KernelInspectionStatusV4::Busy, Some(id))
            }
            _ => unavailable(KernelInspectionStatusV4::Unavailable, Some(id)),
        }
    }

    pub async fn acquire(
        &self,
        key: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn KernelProcessV4>, String> {
        loop {
            let mut sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get(key) {
                if session.is_healthy() {
                    return Ok(session.clone());
                }
                let stale = sessions
                    .remove(key)
                    .expect("unhealthy session was read from the same map");
                drop(sessions);
                let _ = stale.interrupt().await;
                continue;
            }
            let session = self.backend.launch(key).await?;
            sessions.insert(key.clone(), session.clone());
            return Ok(session);
        }
    }

    pub async fn execute(
        &self,
        key: &ExecutionContextKeyV4,
        code: String,
        capture_paths: Vec<String>,
    ) -> Result<RuntimeResultV4, String> {
        self.acquire(key).await?.execute(code, capture_paths).await
    }

    /// Requires a durable launch reservation and the already acquired session.
    pub async fn start_job(
        &self,
        key: &ExecutionContextKeyV4,
        job_id: Uuid,
        session_id: Uuid,
        code: String,
        captures: Vec<String>,
    ) -> Result<RuntimeJobHandleV4, String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(key)
            .filter(|session| session.session_id() == session_id)
            .ok_or_else(|| "runtime job session is no longer available".to_owned())?;
        self.jobs
            .lock()
            .map_err(|_| "runtime worker registry unavailable")?
            .start(key, job_id, session.clone(), code, captures)
    }

    pub fn job(
        &self,
        key: &ExecutionContextKeyV4,
        job_id: Uuid,
    ) -> Result<Option<RuntimeJobHandleV4>, String> {
        self.jobs
            .lock()
            .map_err(|_| "runtime worker registry unavailable")?
            .get(key, job_id)
    }

    pub fn release_job(&self, key: &ExecutionContextKeyV4, job_id: Uuid) -> Result<(), String> {
        self.jobs
            .lock()
            .map_err(|_| "runtime worker registry unavailable")?
            .release(key, job_id)
    }

    pub async fn interrupt(&self, key: &ExecutionContextKeyV4) -> Result<(), String> {
        let session = {
            let mut sessions = self.sessions.lock().await;
            self.jobs
                .lock()
                .map_err(|_| "runtime worker registry unavailable")?
                .interrupt(key);
            sessions.remove(key)
        };
        if let Some(session) = session {
            session.interrupt().await?;
        }
        Ok(())
    }

    pub async fn rebuild(
        &self,
        key: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn KernelProcessV4>, String> {
        self.interrupt(key).await?;
        self.acquire(key).await
    }

    pub async fn interrupt_run(&self, run_id: Uuid) -> Result<(), String> {
        let sessions = {
            let mut guard = self.sessions.lock().await;
            let keys = guard
                .keys()
                .filter(|key| key.run_id == run_id)
                .cloned()
                .collect::<Vec<_>>();
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| "runtime worker registry unavailable")?;
            keys.into_iter()
                .filter_map(|key| {
                    jobs.interrupt(&key);
                    guard.remove(&key)
                })
                .collect::<Vec<_>>()
        };
        for session in sessions {
            session.interrupt().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omicsops_protocol::{ComputeBackendKindV4, IsolationStrengthV4, KernelLanguageV4};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn inspection_key() -> ExecutionContextKeyV4 {
        ExecutionContextKeyV4 {
            project_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            backend_id: "fake".into(),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        }
    }

    #[tokio::test]
    async fn inspect_existing_never_acquires() {
        let backend = Arc::new(FakeBackend(AtomicUsize::new(0)));
        let manager = RuntimeManagerV4::new(backend.clone());
        let key = inspection_key();
        let result = manager.inspect_existing(&key, None).await;
        assert_eq!(result.status, KernelInspectionStatusV4::NotStarted);
        assert!(result.payload.is_none());
        assert!(result.session_id.is_none());
        assert_eq!(backend.0.load(Ordering::SeqCst), 0);
        assert!(manager.sessions.lock().await.is_empty());
    }

    #[tokio::test]
    async fn inspect_rejects_replaced_session() {
        let backend = Arc::new(FakeBackend(AtomicUsize::new(0)));
        let manager = RuntimeManagerV4::new(backend.clone());
        let key = inspection_key();
        let old = manager.acquire(&key).await.unwrap().session_id();
        let current = manager.rebuild(&key).await.unwrap().session_id();
        assert_ne!(old, current);
        let result = manager.inspect_existing(&key, Some(old)).await;
        assert_eq!(result.status, KernelInspectionStatusV4::StaleSession);
        assert_eq!(result.session_id, Some(current));
        assert!(result.payload.is_none());
        let result = manager.inspect_existing(&key, Some(current)).await;
        assert_eq!(result.status, KernelInspectionStatusV4::Unavailable);
        assert!(result.payload.is_none());
        assert_eq!(backend.0.load(Ordering::SeqCst), 2);
    }

    struct InspectionProcess {
        id: Uuid,
        result: Result<KernelInspectionPayloadV4, KernelInspectionErrorV4>,
        started: Option<Arc<tokio::sync::Notify>>,
        resume: Option<Arc<tokio::sync::Notify>>,
        healthy: bool,
    }
    #[async_trait]
    impl KernelProcessV4 for InspectionProcess {
        fn session_id(&self) -> Uuid {
            self.id
        }
        fn process_identity(&self) -> &str {
            "inspection-fixture"
        }
        fn is_healthy(&self) -> bool {
            self.healthy
        }
        async fn inspect(&self) -> Result<KernelInspectionPayloadV4, KernelInspectionErrorV4> {
            if let Some(started) = &self.started {
                started.notify_one();
            }
            if let Some(resume) = &self.resume {
                resume.notified().await;
            }
            self.result.clone()
        }
        async fn execute(&self, _: String, _: Vec<String>) -> Result<RuntimeResultV4, String> {
            panic!("inspection must never execute user code")
        }
        async fn interrupt(&self) -> Result<(), String> {
            panic!("inspection must never interrupt")
        }
    }

    #[tokio::test]
    async fn inspect_rechecks_session_after_await() {
        let backend = Arc::new(FakeBackend(AtomicUsize::new(0)));
        let manager = Arc::new(RuntimeManagerV4::new(backend.clone()));
        let key = inspection_key();
        let started = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        let process = Arc::new(InspectionProcess {
            id: Uuid::new_v4(),
            result: Ok(KernelInspectionPayloadV4 {
                objects: vec![],
                truncated: false,
            }),
            started: Some(started.clone()),
            resume: Some(resume.clone()),
            healthy: true,
        });
        manager
            .sessions
            .lock()
            .await
            .insert(key.clone(), process.clone());
        let task = {
            let manager = manager.clone();
            let key = key.clone();
            tokio::spawn(async move { manager.inspect_existing(&key, None).await })
        };
        started.notified().await;
        manager.sessions.lock().await.insert(
            key.clone(),
            Arc::new(InspectionProcess {
                id: process.id,
                result: Ok(KernelInspectionPayloadV4 {
                    objects: vec![],
                    truncated: false,
                }),
                started: None,
                resume: None,
                healthy: true,
            }),
        );
        resume.notify_one();
        let result = task.await.unwrap();
        assert_eq!(result.status, KernelInspectionStatusV4::StaleSession);
        assert!(result.payload.is_none());
        assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn inspect_deadline_bounds_an_unresponsive_process_without_interrupting() {
        let backend = Arc::new(FakeBackend(AtomicUsize::new(0)));
        let manager = RuntimeManagerV4::new(backend.clone());
        let key = inspection_key();
        let id = Uuid::new_v4();
        manager.sessions.lock().await.insert(
            key.clone(),
            Arc::new(InspectionProcess {
                id,
                result: Ok(KernelInspectionPayloadV4 {
                    objects: vec![],
                    truncated: false,
                }),
                started: None,
                resume: Some(Arc::new(tokio::sync::Notify::new())),
                healthy: true,
            }),
        );
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(4),
            manager.inspect_existing(&key, None),
        )
        .await
        .expect("fixed inspection deadline");
        assert_eq!(result.status, KernelInspectionStatusV4::Unavailable);
        assert!(result.payload.is_none());
        assert_eq!(
            manager
                .sessions
                .lock()
                .await
                .get(&key)
                .unwrap()
                .session_id(),
            id
        );
        assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn inspect_maps_safe_errors_and_rejects_invalid_payload() {
        let backend = Arc::new(FakeBackend(AtomicUsize::new(0)));
        let manager = RuntimeManagerV4::new(backend.clone());
        let key = inspection_key();
        for (response, healthy, expected) in [
            (
                Ok(KernelInspectionPayloadV4 {
                    objects: vec![],
                    truncated: true,
                }),
                true,
                KernelInspectionStatusV4::Ready,
            ),
            (
                Err(KernelInspectionErrorV4::Busy),
                true,
                KernelInspectionStatusV4::Busy,
            ),
            (
                Err(KernelInspectionErrorV4::Unsupported),
                true,
                KernelInspectionStatusV4::Unavailable,
            ),
            (
                Err(KernelInspectionErrorV4::Protocol),
                true,
                KernelInspectionStatusV4::Unavailable,
            ),
            (
                Err(KernelInspectionErrorV4::Unavailable),
                true,
                KernelInspectionStatusV4::Unavailable,
            ),
            (
                Ok(KernelInspectionPayloadV4 {
                    objects: vec![],
                    truncated: false,
                }),
                false,
                KernelInspectionStatusV4::Unavailable,
            ),
            (
                Ok(KernelInspectionPayloadV4 {
                    objects: vec![omicsops_protocol::KernelObjectMetadataV4 {
                        name: "x".repeat(257),
                        type_label: "list".into(),
                        dimensions: None,
                        element_count: None,
                        summary: "opaque".into(),
                    }],
                    truncated: false,
                }),
                true,
                KernelInspectionStatusV4::Unavailable,
            ),
        ] {
            manager.sessions.lock().await.insert(
                key.clone(),
                Arc::new(InspectionProcess {
                    id: Uuid::new_v4(),
                    result: response,
                    started: None,
                    resume: None,
                    healthy,
                }),
            );
            let result = manager.inspect_existing(&key, None).await;
            assert_eq!(result.status, expected);
            assert_eq!(
                result.payload.is_some(),
                expected == KernelInspectionStatusV4::Ready
            );
        }
        assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    }
    struct FakeBackend(AtomicUsize);
    struct DescribedBackend(ComputeBackendDescriptorV4);
    struct FakeProcess {
        id: Uuid,
        identity: String,
        values: Mutex<HashMap<String, String>>,
    }
    struct FailingBackend(AtomicUsize);
    struct FailingProcess;
    #[async_trait]
    impl KernelBackendV4 for FakeBackend {
        fn descriptor(&self) -> ComputeBackendDescriptorV4 {
            ComputeBackendDescriptorV4 {
                schema_version: 4,
                backend_id: "fake".into(),
                kind: ComputeBackendKindV4::Local,
                isolation: IsolationStrengthV4::Process,
                available: true,
                supports_python: true,
                supports_r: true,
                supports_network_policy: false,
            }
        }

        async fn launch(
            &self,
            _: &ExecutionContextKeyV4,
        ) -> Result<Arc<dyn KernelProcessV4>, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(FakeProcess {
                id: Uuid::new_v4(),
                identity: "fake-process".into(),
                values: Mutex::new(HashMap::new()),
            }))
        }
    }
    #[async_trait]
    impl KernelBackendV4 for DescribedBackend {
        fn descriptor(&self) -> ComputeBackendDescriptorV4 {
            self.0.clone()
        }

        async fn launch(
            &self,
            _: &ExecutionContextKeyV4,
        ) -> Result<Arc<dyn KernelProcessV4>, String> {
            Err("not used by policy test".into())
        }
    }
    #[async_trait]
    impl KernelBackendV4 for FailingBackend {
        fn descriptor(&self) -> ComputeBackendDescriptorV4 {
            FakeBackend(AtomicUsize::new(0)).descriptor()
        }

        async fn launch(
            &self,
            _: &ExecutionContextKeyV4,
        ) -> Result<Arc<dyn KernelProcessV4>, String> {
            let launch = self.0.fetch_add(1, Ordering::SeqCst);
            if launch == 0 {
                Ok(Arc::new(FailingProcess))
            } else {
                Ok(Arc::new(FakeProcess {
                    id: Uuid::new_v4(),
                    identity: "replacement-process".into(),
                    values: Mutex::new(HashMap::new()),
                }))
            }
        }
    }
    #[async_trait]
    impl KernelProcessV4 for FakeProcess {
        fn session_id(&self) -> Uuid {
            self.id
        }
        fn process_identity(&self) -> &str {
            &self.identity
        }
        async fn execute(&self, code: String, _: Vec<String>) -> Result<RuntimeResultV4, String> {
            let mut values = self.values.lock().await;
            let stdout = if let Some((key, value)) = code.split_once('=') {
                values.insert(key.trim().into(), value.trim().into());
                String::new()
            } else {
                values.get(code.trim()).cloned().unwrap_or_default()
            };
            Ok(RuntimeResultV4 {
                request_id: Uuid::new_v4(),
                session_id: self.id,
                process_identity: self.identity.clone(),
                stdout,
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
    #[async_trait]
    impl KernelProcessV4 for FailingProcess {
        fn session_id(&self) -> Uuid {
            Uuid::nil()
        }
        fn process_identity(&self) -> &str {
            "failed-process"
        }
        fn is_healthy(&self) -> bool {
            false
        }
        async fn execute(&self, _: String, _: Vec<String>) -> Result<RuntimeResultV4, String> {
            Err("simulated kernel transport failure".into())
        }
        async fn interrupt(&self) -> Result<(), String> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn two_cells_reuse_one_process_and_namespace() {
        let backend = Arc::new(FakeBackend(AtomicUsize::new(0)));
        let manager = RuntimeManagerV4::new(backend.clone());
        let key = ExecutionContextKeyV4 {
            project_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            backend_id: "ssh:test".into(),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        };
        let first = manager
            .execute(&key, "answer=42".into(), vec![])
            .await
            .unwrap();
        let second = manager
            .execute(&key, "answer".into(), vec![])
            .await
            .unwrap();
        assert_eq!(first.session_id, second.session_id);
        assert_eq!(second.stdout, "42");
        assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execution_error_discards_cached_process_without_replaying_the_cell() {
        let backend = Arc::new(FailingBackend(AtomicUsize::new(0)));
        let manager = RuntimeManagerV4::new(backend.clone());
        let key = ExecutionContextKeyV4 {
            project_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            backend_id: "fake".into(),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        };

        let failed_session = manager.acquire(&key).await.unwrap();
        let failed_job_id = Uuid::new_v4();
        let failed_job = manager
            .start_job(
                &key,
                failed_job_id,
                failed_session.session_id(),
                "first cell".into(),
                vec![],
            )
            .await
            .unwrap();
        let error = failed_job.wait().await.unwrap_err();
        manager.release_job(&key, failed_job_id).unwrap();
        assert_eq!(error, "simulated kernel transport failure");
        assert_eq!(backend.0.load(Ordering::SeqCst), 1);

        let replacement = manager.acquire(&key).await.unwrap();
        let replacement_job = manager
            .start_job(
                &key,
                Uuid::new_v4(),
                replacement.session_id(),
                "second cell".into(),
                vec![],
            )
            .await
            .unwrap();
        let result = replacement_job.wait().await.unwrap();
        assert_eq!(result.process_identity, "replacement-process");
        assert_eq!(backend.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn rebuild_replaces_process_and_interrupt_run_discards_all_languages() {
        let backend = Arc::new(FakeBackend(AtomicUsize::new(0)));
        let manager = RuntimeManagerV4::new(backend.clone());
        let project_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let python = ExecutionContextKeyV4 {
            project_id,
            run_id,
            backend_id: "ssh:test".into(),
            language: KernelLanguageV4::Python,
            environment: "project".into(),
        };
        let r = ExecutionContextKeyV4 {
            language: KernelLanguageV4::R,
            ..python.clone()
        };
        let original = manager.acquire(&python).await.unwrap().session_id();
        manager.acquire(&r).await.unwrap();
        let rebuilt = manager.rebuild(&python).await.unwrap().session_id();
        assert_ne!(original, rebuilt);
        manager.interrupt_run(run_id).await.unwrap();
        manager.acquire(&r).await.unwrap();
        assert_eq!(backend.0.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn registry_enforces_full_auto_isolation_and_unique_backend_ids() {
        let descriptor = |backend_id: &str, isolation| ComputeBackendDescriptorV4 {
            schema_version: 4,
            backend_id: backend_id.into(),
            kind: if isolation == IsolationStrengthV4::Container {
                ComputeBackendKindV4::Docker
            } else {
                ComputeBackendKindV4::Local
            },
            isolation,
            available: true,
            supports_python: true,
            supports_r: true,
            supports_network_policy: isolation == IsolationStrengthV4::Container,
        };
        let mut registry = ComputeBackendRegistryV4::default();
        registry
            .register(Arc::new(DescribedBackend(descriptor(
                "local",
                IsolationStrengthV4::Process,
            ))))
            .unwrap();
        registry
            .register(Arc::new(DescribedBackend(descriptor(
                "docker",
                IsolationStrengthV4::Container,
            ))))
            .unwrap();
        assert!(
            registry
                .runtime("local", AutonomyModeV4::Supervised)
                .is_ok()
        );
        assert!(registry.runtime("local", AutonomyModeV4::FullAuto).is_err());
        assert!(registry.runtime("docker", AutonomyModeV4::FullAuto).is_ok());
        assert!(
            registry
                .register(Arc::new(DescribedBackend(descriptor(
                    "docker",
                    IsolationStrengthV4::Container,
                ))))
                .unwrap_err()
                .contains("duplicate")
        );
    }
}
