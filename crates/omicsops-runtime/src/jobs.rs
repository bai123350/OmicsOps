//! Bounded in-process workers. Durable launch authority remains in Store.
use crate::KernelProcessV4;
use omicsops_protocol::{ExecutionContextKeyV4, RuntimeResultV4};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::watch;
use uuid::Uuid;

type Outcome = Option<Result<Arc<RuntimeResultV4>, String>>;

#[derive(Clone)]
pub struct RuntimeJobHandleV4 {
    pub job_id: Uuid,
    result: watch::Receiver<Outcome>,
}

impl RuntimeJobHandleV4 {
    /// None means no terminal result has been observed; it is not a liveness probe.
    pub fn result(&self) -> Outcome {
        self.result.borrow().clone()
    }

    /// Dropping this future leaves the worker intact. No timer or model poll.
    pub async fn wait(&self) -> Result<Arc<RuntimeResultV4>, String> {
        let mut result = self.result.clone();
        loop {
            if let Some(outcome) = result.borrow_and_update().clone() {
                return outcome;
            }
            result
                .changed()
                .await
                .map_err(|_| "runtime worker result channel closed".to_owned())?;
        }
    }
}

struct Entry {
    context: ExecutionContextKeyV4,
    fingerprint: String,
    result: watch::Sender<Outcome>,
    abort: tokio::task::AbortHandle,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

#[derive(Default)]
pub(crate) struct Jobs {
    entries: HashMap<Uuid, Entry>,
}

impl Jobs {
    pub fn start(
        &mut self,
        context: &ExecutionContextKeyV4,
        job_id: Uuid,
        session: Arc<dyn KernelProcessV4>,
        code: String,
        captures: Vec<String>,
    ) -> Result<RuntimeJobHandleV4, String> {
        let fingerprint = hex::encode(Sha256::digest(
            serde_json::to_vec(&(context, session.session_id(), &code, &captures))
                .map_err(|error| error.to_string())?,
        ));
        if let Some(entry) = self.entries.get(&job_id) {
            if entry.context != *context || entry.fingerprint != fingerprint {
                return Err("runtime worker identity conflicts with an existing request".into());
            }
            return Ok(RuntimeJobHandleV4 {
                job_id,
                result: entry.result.subscribe(),
            });
        }
        if self.entries.len() >= 128 {
            return Err("runtime worker capacity reached; release completed jobs first".into());
        }
        let (result, receiver) = watch::channel(None);
        let worker =
            tokio::spawn(async move { session.execute(code, captures).await.map(Arc::new) });
        let abort = worker.abort_handle();
        let publisher = result.clone();
        tokio::spawn(async move {
            let outcome = worker.await.unwrap_or_else(|_| {
                Err("runtime worker interrupted or failed; remote outcome is unknown".into())
            });
            publisher.send_if_modified(|value| {
                if value.is_some() {
                    return false;
                }
                *value = Some(outcome);
                true
            });
        });
        self.entries.insert(
            job_id,
            Entry {
                context: context.clone(),
                fingerprint,
                result,
                abort,
            },
        );
        Ok(RuntimeJobHandleV4 {
            job_id,
            result: receiver,
        })
    }

    pub fn get(
        &self,
        context: &ExecutionContextKeyV4,
        job_id: Uuid,
    ) -> Result<Option<RuntimeJobHandleV4>, String> {
        let Some(entry) = self.entries.get(&job_id) else {
            return Ok(None);
        };
        if entry.context != *context {
            return Err("runtime worker context mismatch".into());
        }
        Ok(Some(RuntimeJobHandleV4 {
            job_id,
            result: entry.result.subscribe(),
        }))
    }

    pub fn release(&mut self, context: &ExecutionContextKeyV4, job_id: Uuid) -> Result<(), String> {
        if let Some(handle) = self.get(context, job_id)? {
            if handle.result().is_none() {
                return Err("cannot release a running runtime worker".into());
            }
            self.entries.remove(&job_id);
        }
        Ok(())
    }

    pub fn interrupt(&mut self, context: &ExecutionContextKeyV4) {
        for entry in self
            .entries
            .values_mut()
            .filter(|entry| entry.context == *context)
        {
            entry.result.send_if_modified(|value| {
                if value.is_some() {
                    return false;
                }
                *value = Some(Err(
                    "runtime wait interrupted; remote termination is unconfirmed".into(),
                ));
                true
            });
            entry.abort.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KernelBackendV4, RuntimeManagerV4};
    use async_trait::async_trait;
    use omicsops_protocol::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    struct Process {
        id: Uuid,
        entered: tokio::sync::Notify,
        release: tokio::sync::Notify,
        calls: AtomicUsize,
        interrupts: AtomicUsize,
        panic: bool,
    }

    fn process(panic: bool) -> Arc<Process> {
        Arc::new(Process {
            id: Uuid::new_v4(),
            entered: Default::default(),
            release: Default::default(),
            calls: AtomicUsize::new(0),
            interrupts: AtomicUsize::new(0),
            panic,
        })
    }
    fn context() -> ExecutionContextKeyV4 {
        ExecutionContextKeyV4 {
            project_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            backend_id: "local".into(),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        }
    }

    #[async_trait]
    impl KernelProcessV4 for Process {
        fn session_id(&self) -> Uuid {
            self.id
        }
        fn process_identity(&self) -> &str {
            "fake"
        }
        async fn execute(&self, _: String, _: Vec<String>) -> Result<RuntimeResultV4, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            assert!(!self.panic, "synthetic worker failure");
            self.release.notified().await;
            Ok(RuntimeResultV4 {
                request_id: Uuid::new_v4(),
                session_id: self.id,
                process_identity: "fake".into(),
                stdout: "42".into(),
                stderr: String::new(),
                stdout_capture: None,
                stderr_capture: None,
                succeeded: true,
                artifacts: vec![RuntimeArtifactV4 {
                    relative_path: "result.tsv".into(),
                    size_bytes: 2,
                    sha256: "a".repeat(64),
                }],
                software_versions: Default::default(),
            })
        }
        async fn interrupt(&self) -> Result<(), String> {
            self.interrupts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Backend(Arc<Process>);
    #[async_trait]
    impl KernelBackendV4 for Backend {
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
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn dropped_waiters_and_repeated_waits_preserve_one_execution_and_artifacts() {
        let process = process(false);
        let key = context();
        let mut jobs = Jobs::default();
        let id = Uuid::new_v4();
        let first = jobs
            .start(&key, id, process.clone(), "code".into(), vec![])
            .unwrap();
        process.entered.notified().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(10), first.wait())
                .await
                .is_err()
        );
        drop(first);
        let resumed = jobs.get(&key, id).unwrap().unwrap();
        let duplicate = jobs
            .start(&key, id, process.clone(), "code".into(), vec![])
            .unwrap();
        assert!(
            jobs.start(&key, id, process.clone(), "different code".into(), vec![])
                .is_err()
        );
        let wrong = ExecutionContextKeyV4 {
            run_id: Uuid::new_v4(),
            ..key.clone()
        };
        assert!(jobs.get(&wrong, id).is_err());
        assert!(jobs.release(&key, id).is_err());
        process.release.notify_one();
        let (a, b) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(resumed.wait(), duplicate.wait())
        })
        .await
        .unwrap();
        let a = a.unwrap();
        assert!(Arc::ptr_eq(&a, &b.unwrap()));
        assert_eq!(
            resumed.result().unwrap().unwrap().artifacts[0].relative_path,
            "result.tsv"
        );
        assert_eq!(process.calls.load(Ordering::SeqCst), 1);
        assert_eq!(process.interrupts.load(Ordering::SeqCst), 0);
        jobs.release(&key, id).unwrap();
        assert!(jobs.get(&key, id).unwrap().is_none());
        assert_eq!(resumed.wait().await.unwrap().stdout, "42");
    }

    #[tokio::test]
    async fn explicit_interrupt_wakes_waiters_and_isolates_other_contexts() {
        let process = process(false);
        let manager = RuntimeManagerV4::new(Arc::new(Backend(process.clone())));
        let key = context();
        let session = manager.acquire(&key).await.unwrap();
        let handle = manager
            .start_job(
                &key,
                Uuid::new_v4(),
                session.session_id(),
                "code".into(),
                vec![],
            )
            .await
            .unwrap();
        process.entered.notified().await;
        manager.interrupt_run(Uuid::new_v4()).await.unwrap();
        assert!(handle.result().is_none());
        manager.interrupt_run(key.run_id).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), handle.wait())
                .await
                .unwrap()
                .unwrap_err()
                .contains("unconfirmed")
        );
        assert_eq!(process.interrupts.load(Ordering::SeqCst), 1);
        process.release.notify_one();
        assert!(handle.wait().await.is_err());
        assert!(
            manager
                .start_job(
                    &key,
                    Uuid::new_v4(),
                    session.session_id(),
                    "code".into(),
                    vec![]
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn worker_panics_wake_waiters_as_unknown() {
        let process = process(true);
        let mut jobs = Jobs::default();
        let handle = jobs
            .start(&context(), Uuid::new_v4(), process, "code".into(), vec![])
            .unwrap();
        let error = tokio::time::timeout(Duration::from_secs(1), handle.wait())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.contains("unknown"));
        assert!(!error.contains("synthetic worker failure"));
    }

    #[tokio::test]
    async fn retained_workers_are_bounded() {
        let mut jobs = Jobs::default();
        let process = process(false);
        let key = context();
        for _ in 0..128 {
            jobs.start(&key, Uuid::new_v4(), process.clone(), "code".into(), vec![])
                .unwrap();
        }
        assert!(
            jobs.start(&key, Uuid::new_v4(), process, "code".into(), vec![])
                .is_err()
        );
        assert_eq!(jobs.entries.len(), 128);
    }
}
