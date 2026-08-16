use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use omicsops_agent::harness_v3::{
    AuthorityDecisionV3, CancellationTokenV3, NoopToolAuditSinkV3, RiskLevelV3, ToolAuthorityV3,
    ToolCallRequestV3, ToolConcurrencyV3, ToolDefinitionV3, ToolOutcomeStatusV3, ToolOutcomeV3,
    ToolRouterErrorV3, ToolRouterV3, ToolRuntimeV3,
};
use serde_json::json;

fn definition(id: &str, read_only: bool, timeout_ms: u64) -> ToolDefinitionV3 {
    ToolDefinitionV3 {
        id: id.into(),
        description: id.into(),
        input_schema: json!({
            "type":"object",
            "required":["path"],
            "properties":{"path":{"type":"string"}}
        }),
        output_schema: json!({"type":"object"}),
        capabilities: vec![if read_only {
            "remote_read".into()
        } else {
            "remote_write".into()
        }],
        risk: if read_only {
            RiskLevelV3::Low
        } else {
            RiskLevelV3::Medium
        },
        read_only,
        concurrency: if read_only {
            ToolConcurrencyV3::ParallelReadOnly
        } else {
            ToolConcurrencyV3::Serial
        },
        timeout_ms,
    }
}

#[derive(Clone)]
struct FixedAuthority(AuthorityDecisionV3);

impl ToolAuthorityV3 for FixedAuthority {
    fn authorize(
        &self,
        _definition: &ToolDefinitionV3,
        _request: &ToolCallRequestV3,
    ) -> AuthorityDecisionV3 {
        self.0.clone()
    }
}

#[derive(Default)]
struct RuntimeStats {
    active: AtomicUsize,
    max_active: AtomicUsize,
    calls: AtomicUsize,
    call_ids: Mutex<Vec<String>>,
}

struct FixedRuntime {
    stats: Arc<RuntimeStats>,
    delay: Duration,
    content: String,
}

#[async_trait]
impl ToolRuntimeV3 for FixedRuntime {
    async fn execute(
        &self,
        _definition: &ToolDefinitionV3,
        request: &ToolCallRequestV3,
        cancellation: &CancellationTokenV3,
    ) -> ToolOutcomeV3 {
        self.stats.calls.fetch_add(1, Ordering::SeqCst);
        self.stats
            .call_ids
            .lock()
            .unwrap()
            .push(request.call_id.clone());
        let active = self.stats.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.stats.max_active.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.stats.active.fetch_sub(1, Ordering::SeqCst);
        if cancellation.is_cancelled() {
            return ToolOutcomeV3 {
                call_id: request.call_id.clone(),
                status: ToolOutcomeStatusV3::Cancelled,
                model_content: "cancelled".into(),
                structured_result: None,
                error: Some("cancelled".into()),
                truncated: false,
                provenance: vec![],
            };
        }
        ToolOutcomeV3 {
            call_id: request.call_id.clone(),
            status: ToolOutcomeStatusV3::Succeeded,
            model_content: self.content.clone(),
            structured_result: Some(json!({"ok":true})),
            error: None,
            truncated: false,
            provenance: vec!["fixture".into()],
        }
    }
}

fn request(call_id: &str, tool_id: &str, key: &str) -> ToolCallRequestV3 {
    ToolCallRequestV3 {
        call_id: call_id.into(),
        tool_id: tool_id.into(),
        arguments: json!({"path":"results/a.txt"}),
        idempotency_key: key.into(),
    }
}

fn router(
    definitions: Vec<ToolDefinitionV3>,
    authority: AuthorityDecisionV3,
    runtime: FixedRuntime,
) -> ToolRouterV3 {
    ToolRouterV3::new(
        definitions,
        Arc::new(FixedAuthority(authority)),
        Arc::new(runtime),
        Arc::new(NoopToolAuditSinkV3),
        4,
    )
    .unwrap()
}

#[tokio::test]
async fn router_validates_schema_and_rechecks_authority_before_runtime() {
    let stats = Arc::new(RuntimeStats::default());
    let denied = router(
        vec![definition("remote.read", true, 1_000)],
        AuthorityDecisionV3::Denied {
            reason: "approval revoked".into(),
        },
        FixedRuntime {
            stats: stats.clone(),
            delay: Duration::ZERO,
            content: "ok".into(),
        },
    );
    let error = denied
        .execute(
            request("read-1", "remote.read", "key-1"),
            CancellationTokenV3::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ToolRouterErrorV3::Denied(_)));
    assert_eq!(stats.calls.load(Ordering::SeqCst), 0);

    let allowed = router(
        vec![definition("remote.read", true, 1_000)],
        AuthorityDecisionV3::Allowed,
        FixedRuntime {
            stats: stats.clone(),
            delay: Duration::ZERO,
            content: "ok".into(),
        },
    );
    let mut malformed = request("read-2", "remote.read", "key-2");
    malformed.arguments = json!({"path": 4});
    assert!(matches!(
        allowed
            .execute(malformed, CancellationTokenV3::new())
            .await
            .unwrap_err(),
        ToolRouterErrorV3::InvalidArguments(_)
    ));
    assert_eq!(stats.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn router_caps_parallel_reads_at_four_and_serializes_side_effects() {
    let read_stats = Arc::new(RuntimeStats::default());
    let reads = Arc::new(router(
        vec![definition("remote.read", true, 1_000)],
        AuthorityDecisionV3::Allowed,
        FixedRuntime {
            stats: read_stats.clone(),
            delay: Duration::from_millis(20),
            content: "ok".into(),
        },
    ));
    let mut read_tasks = Vec::new();
    for index in 0..8 {
        let reads = reads.clone();
        read_tasks.push(tokio::spawn(async move {
            reads
                .execute(
                    request(
                        &format!("read-{index}"),
                        "remote.read",
                        &format!("key-{index}"),
                    ),
                    CancellationTokenV3::new(),
                )
                .await
                .unwrap();
        }));
    }
    for task in read_tasks {
        task.await.unwrap();
    }
    assert_eq!(read_stats.max_active.load(Ordering::SeqCst), 4);

    let write_stats = Arc::new(RuntimeStats::default());
    let writes = Arc::new(router(
        vec![definition("remote.write", false, 1_000)],
        AuthorityDecisionV3::Allowed,
        FixedRuntime {
            stats: write_stats.clone(),
            delay: Duration::from_millis(20),
            content: "ok".into(),
        },
    ));
    let first = {
        let writes = writes.clone();
        tokio::spawn(async move {
            writes
                .execute(
                    request("write-1", "remote.write", "write-key-1"),
                    CancellationTokenV3::new(),
                )
                .await
                .unwrap()
        })
    };
    let second = {
        let writes = writes.clone();
        tokio::spawn(async move {
            writes
                .execute(
                    request("write-2", "remote.write", "write-key-2"),
                    CancellationTokenV3::new(),
                )
                .await
                .unwrap()
        })
    };
    first.await.unwrap();
    second.await.unwrap();
    assert_eq!(write_stats.max_active.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn router_enforces_timeout_cancellation_preview_limit_and_idempotency() {
    let timeout_stats = Arc::new(RuntimeStats::default());
    let timed = router(
        vec![definition("remote.read", true, 5)],
        AuthorityDecisionV3::Allowed,
        FixedRuntime {
            stats: timeout_stats,
            delay: Duration::from_millis(30),
            content: "late".into(),
        },
    );
    let outcome = timed
        .execute(
            request("slow", "remote.read", "slow-key"),
            CancellationTokenV3::new(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.status, ToolOutcomeStatusV3::TimedOut);

    let stats = Arc::new(RuntimeStats::default());
    let long = "测".repeat(20_000);
    let cached = router(
        vec![definition("remote.read", true, 1_000)],
        AuthorityDecisionV3::Allowed,
        FixedRuntime {
            stats: stats.clone(),
            delay: Duration::ZERO,
            content: long,
        },
    );
    let first = cached
        .execute(
            request("first", "remote.read", "same-key"),
            CancellationTokenV3::new(),
        )
        .await
        .unwrap();
    assert!(first.truncated);
    assert!(first.model_content.len() <= 32 * 1024);
    let replayed = cached
        .execute(
            request("second", "remote.read", "same-key"),
            CancellationTokenV3::new(),
        )
        .await
        .unwrap();
    assert_eq!(stats.calls.load(Ordering::SeqCst), 1);
    assert_eq!(replayed.call_id, "second");

    let restarted_stats = Arc::new(RuntimeStats::default());
    let restarted = router(
        vec![definition("remote.read", true, 1_000)],
        AuthorityDecisionV3::Allowed,
        FixedRuntime {
            stats: restarted_stats.clone(),
            delay: Duration::ZERO,
            content: "must not run".into(),
        },
    );
    restarted.seed_successful_outcome("same-key", first.clone());
    let recovered = restarted
        .execute(
            request("after-restart", "remote.read", "same-key"),
            CancellationTokenV3::new(),
        )
        .await
        .unwrap();
    assert_eq!(restarted_stats.calls.load(Ordering::SeqCst), 0);
    assert_eq!(recovered.call_id, "after-restart");

    let cancellation = CancellationTokenV3::new();
    cancellation.cancel();
    let error = cached
        .execute(
            request("cancelled", "remote.read", "cancelled-key"),
            cancellation,
        )
        .await
        .unwrap_err();
    assert_eq!(error, ToolRouterErrorV3::Cancelled);
}
