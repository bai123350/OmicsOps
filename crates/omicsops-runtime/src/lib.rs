use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use omicsops_protocol::{ExecutionContextKeyV4, RuntimeResultV4};
use tokio::sync::Mutex;
use uuid::Uuid;

#[async_trait]
pub trait KernelProcessV4: Send + Sync {
    fn session_id(&self) -> Uuid;
    fn process_identity(&self) -> &str;
    async fn execute(
        &self,
        code: String,
        capture_paths: Vec<String>,
    ) -> Result<RuntimeResultV4, String>;
    async fn interrupt(&self) -> Result<(), String>;
}

#[async_trait]
pub trait KernelBackendV4: Send + Sync {
    async fn launch(&self, key: &ExecutionContextKeyV4)
    -> Result<Arc<dyn KernelProcessV4>, String>;
}

pub struct RuntimeManagerV4 {
    backend: Arc<dyn KernelBackendV4>,
    sessions: Mutex<HashMap<ExecutionContextKeyV4, Arc<dyn KernelProcessV4>>>,
}

impl RuntimeManagerV4 {
    pub fn new(backend: Arc<dyn KernelBackendV4>) -> Self {
        Self {
            backend,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub async fn acquire(
        &self,
        key: &ExecutionContextKeyV4,
    ) -> Result<Arc<dyn KernelProcessV4>, String> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(key) {
            return Ok(session.clone());
        }
        let session = self.backend.launch(key).await?;
        sessions.insert(key.clone(), session.clone());
        Ok(session)
    }

    pub async fn execute(
        &self,
        key: &ExecutionContextKeyV4,
        code: String,
        capture_paths: Vec<String>,
    ) -> Result<RuntimeResultV4, String> {
        self.acquire(key).await?.execute(code, capture_paths).await
    }

    pub async fn interrupt(&self, key: &ExecutionContextKeyV4) -> Result<(), String> {
        let session = self.sessions.lock().await.remove(key);
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
            keys.into_iter()
                .filter_map(|key| guard.remove(&key))
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
    use omicsops_protocol::KernelLanguageV4;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct FakeBackend(AtomicUsize);
    struct FakeProcess {
        id: Uuid,
        identity: String,
        values: Mutex<HashMap<String, String>>,
    }
    #[async_trait]
    impl KernelBackendV4 for FakeBackend {
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
            })
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
}
