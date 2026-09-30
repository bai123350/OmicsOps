use async_trait::async_trait;
use omicsops_adapters::{AdapterResult, claude_code::*};
use omicsops_process::managed_child::{BackgroundLaunchSpec, ManagedBackgroundChild};
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Runner {
    verified: bool,
    kind: ClaudeAuthKind,
    spawned: AtomicUsize,
}
#[async_trait]
impl ClaudeProcessRunner for Runner {
    async fn inspect(&self, _: &Path) -> AdapterResult<ClaudePreflight> {
        Ok(ClaudePreflight {
            executable_fingerprint: "fixture hash".into(),
            version: "2.1.281".into(),
            auth_kind: self.kind,
            account_fingerprint: Some("opaque_fixture".into()),
            restrictions_verified: self.verified,
        })
    }
    async fn spawn(&self, _: BackgroundLaunchSpec) -> AdapterResult<ManagedBackgroundChild> {
        self.spawned.fetch_add(1, Ordering::SeqCst);
        panic!("preflight cannot spawn generation")
    }
}
#[tokio::test]
async fn claude_preflight_rejects_unverifiable_policy_and_non_subscription_auth() {
    for (verified, kind) in [
        (false, ClaudeAuthKind::Subscription),
        (true, ClaudeAuthKind::Api),
        (true, ClaudeAuthKind::Unknown),
    ] {
        let runner = Runner {
            verified,
            kind,
            spawned: AtomicUsize::new(0),
        };
        assert!(
            inspect_claude_preflight(Path::new("C:\\fixture\\claude.exe"), &runner)
                .await
                .is_err()
        );
        assert_eq!(runner.spawned.load(Ordering::SeqCst), 0);
    }
    let runner = Runner {
        verified: true,
        kind: ClaudeAuthKind::Subscription,
        spawned: AtomicUsize::new(0),
    };
    for invalid in [
        "claude.cmd",
        "C:\\fixture\\claude.bat",
        "C:\\fixture\\claude.exe --flag",
    ] {
        assert!(
            inspect_claude_preflight(Path::new(invalid), &runner)
                .await
                .is_err()
        );
    }
    #[cfg(windows)]
    assert!(
        inspect_claude_preflight(Path::new("C:\\fixture\\claude.exe"), &runner)
            .await
            .is_ok()
    );
}
#[test]
fn claude_preflight_invalidates_changed_binary_and_policy() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("claude.exe");
    std::fs::write(&exe, b"fixture one").unwrap();
    let old = claude_executable_fingerprint(&exe).unwrap();
    std::fs::write(&exe, b"fixture two").unwrap();
    assert_ne!(claude_executable_fingerprint(&exe).unwrap(), old);
    assert!(parse_claude_version("unknown format SECRET_SENTINEL").is_err());
    assert!(parse_claude_version("2.1.247 (Claude Code)").is_err());
    assert_eq!(
        parse_claude_version("2.1.281 (Claude Code)").unwrap(),
        "2.1.281"
    );
}
#[tokio::test]
async fn claude_production_runner_cannot_bypass_unverified_policy() {
    let spec = BackgroundLaunchSpec {
        program: "C:\\fixture\\claude.exe".into(),
        args: vec![],
        cwd: "C:\\fixture".into(),
        env: Default::default(),
    };
    let error = SystemClaudeProcessRunner.spawn(spec).await.err().unwrap();
    assert!(error.to_string().contains("claude_policy_unverifiable"));
}

fn request() -> omicsops_agent::provider::ProviderRequest {
    use omicsops_agent::{
        ModelMessage,
        provider::{ProviderRequest, ProviderToolSpec},
    };
    ProviderRequest {
        system: "host policy".into(),
        messages: vec![ModelMessage {
            role: "user".into(),
            content: "中文 `literal` $() \"quote\"\nnewline".into(),
        }],
        tools: vec![ProviderToolSpec {
            id: "host.read".into(),
            description: "Read".into(),
            input_schema: serde_json::json!({"type":"object"}),
        }],
        require_strict_json_fallback: false,
        replay: vec![],
    }
}
#[cfg(windows)]
#[test]
fn claude_invocation_is_tool_free_subscription_only_and_stdin_only() {
    let client = ClaudeCodeClient::new(
        uuid::Uuid::new_v4(),
        "C:\\fixture\\claude.exe".into(),
        "sonnet".into(),
        std::sync::Arc::new(Runner {
            verified: true,
            kind: ClaudeAuthKind::Subscription,
            spawned: AtomicUsize::new(0),
        }),
    )
    .unwrap();
    let invocation = client.build_invocation(&request()).unwrap();
    let args: Vec<_> = invocation
        .launch
        .args
        .iter()
        .map(|a| a.to_str().unwrap())
        .collect();
    let tools = args.iter().position(|v| *v == "--tools").unwrap();
    assert_eq!(args[tools + 1], "");
    assert!(
        args.contains(&"--restricted")
            && args.contains(&"--safe-mode")
            && args.contains(&"--strict-mcp-config")
    );
    assert!(!args.contains(&"--bare") && !args.contains(&"--json-schema"));
    assert!(!args.iter().any(|v| v.contains("中文")));
    assert!(invocation.stdin.windows(3).any(|w| w == b"$()"));
    assert!(
        !invocation
            .launch
            .env
            .contains_key(std::ffi::OsStr::new("ANTHROPIC_API_KEY"))
    );
    assert!(
        !invocation
            .launch
            .env
            .contains_key(std::ffi::OsStr::new("CLAUDE_CODE_OAUTH_TOKEN"))
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&invocation.stdin).unwrap()["system"],
        "host policy"
    );
}
#[cfg(windows)]
#[test]
fn claude_budget_measures_the_same_stdin_and_system_prompt_it_validates() {
    let client = ClaudeCodeClient::new(
        uuid::Uuid::new_v4(),
        "C:\\fixture\\claude.exe".into(),
        "sonnet".into(),
        std::sync::Arc::new(Runner {
            verified: true,
            kind: ClaudeAuthKind::Subscription,
            spawned: AtomicUsize::new(0),
        }),
    )
    .unwrap();
    let bytes = client
        .measure_model_request(&request())
        .unwrap()
        .serialized_request_bytes as u32;
    let budget = omicsops_adapters::llm::RequestBudget {
        context_window_tokens: bytes + 120,
        reserved_output_tokens: 100,
        safety_margin_tokens: 20,
    };
    assert!(
        client
            .clone()
            .with_request_budget(budget)
            .build_invocation(&request())
            .is_ok()
    );
    assert!(
        client
            .with_request_budget(omicsops_adapters::llm::RequestBudget {
                context_window_tokens: bytes + 119,
                ..budget
            })
            .build_invocation(&request())
            .is_err()
    );
}
#[test]
fn claude_stream_rejects_invalid_final_and_native_execution() {
    use serde_json::json;
    let request = request();
    let calls = json!({"kind":"tool_calls","calls":[{"call_id":"one","tool_id":"host.read","arguments":{"path":"x"}}]});
    let events = parse_claude_envelope(&calls, &request.tools).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                omicsops_agent::provider::ProviderStreamEvent::ToolCallCompleted { .. }
            ))
            .count(),
        1
    );
    assert!(parse_claude_envelope(&json!({"kind":"final","text":"done"}), &request.tools).is_ok());
    for invalid in [
        json!({"kind":"final","text":"done","calls":[]}),
        json!({"kind":"tool_calls","calls":[]}),
        json!({"kind":"tool_calls","calls":[{"call_id":"one","tool_id":"ungiven","arguments":{}}]}),
        json!({"kind":"tool_calls","calls":[{"call_id":"one","tool_id":"host.read","arguments":{}},{"call_id":"one","tool_id":"host.read","arguments":{}}]}),
    ] {
        assert!(parse_claude_envelope(&invalid, &request.tools).is_err());
    }
    for native in [
        json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"native","name":"Bash","input":{"command":"SECRET_SENTINEL"}}]}}),
        json!({"type":"system","subtype":"init","tools":["Read"]}),
        json!({"type":"system","subtype":"hook_started"}),
    ] {
        let mut d = ClaudeStreamDecoder::for_request(&request);
        let err = d.push(format!("{native}\n").as_bytes()).err().unwrap();
        assert!(!err.to_string().contains("SECRET_SENTINEL"));
    }
    let mut d = ClaudeStreamDecoder::for_request(&request);
    assert!(d.finish(true).is_err());
    let result = json!({"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"{\"kind\":\"final\",\"text\":\"done\"}"});
    d.push(format!("{result}\n").as_bytes()).unwrap();
    assert!(d.finish(false).is_err());
}
#[test]
fn claude_stream_bounds_io_and_counts_only_content_progress() {
    use omicsops_agent::provider::ProviderStreamEvent;
    use serde_json::json;
    let r = request();
    let mut d = ClaudeStreamDecoder::for_request(&r);
    for event in [
        json!({"type":"system","subtype":"init","tools":[],"mcp_servers":[]}),
        json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":""}}}),
        json!({"type":"rate_limit_event"}),
    ] {
        assert!(d.push(format!("{event}\n").as_bytes()).unwrap().is_empty());
    }
    let result = json!({"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"{\"kind\":\"final\",\"text\":\"done\"}","usage":{"output_tokens":17,"private":"SECRET_SENTINEL"}});
    d.push(format!("{result}\n").as_bytes()).unwrap();
    let events = d.finish(true).unwrap();
    let sample = events
        .iter()
        .find_map(|e| {
            if let ProviderStreamEvent::UsageObserved { sample } = e {
                Some(sample)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(sample.output_tokens, Some(17));
    assert_eq!(sample.input_tokens, None);
    assert!(!format!("{events:?}").contains("SECRET_SENTINEL"));
    assert!(
        ClaudeStreamDecoder::for_request(&r)
            .push(&vec![b'x'; 1024 * 1024 + 1])
            .is_err()
    );
    let padding = format!(
        "{}\n",
        json!({"type":"rate_limit_event","padding":"x".repeat(900_000)})
    );
    let mut oversized = ClaudeStreamDecoder::for_request(&r);
    for _ in 0..9 {
        oversized.push(padding.as_bytes()).unwrap();
    }
    assert!(oversized.push(padding.as_bytes()).is_err());
    let many = json!({"kind":"tool_calls","calls":(0..17).map(|n|json!({"call_id":format!("c{n}"),"tool_id":"host.read","arguments":{}})).collect::<Vec<_>>()});
    assert!(parse_claude_envelope(&many, &r.tools).is_err());
    #[cfg(windows)]
    {
        let client = ClaudeCodeClient::new(
            uuid::Uuid::new_v4(),
            "C:\\fixture\\claude.exe".into(),
            "sonnet".into(),
            std::sync::Arc::new(Runner {
                verified: true,
                kind: ClaudeAuthKind::Subscription,
                spawned: AtomicUsize::new(0),
            }),
        )
        .unwrap();
        let mut big = r;
        big.system = "x".repeat(8 * 1024 * 1024);
        assert!(client.build_invocation(&big).is_err());
    }
}
#[cfg(windows)]
struct ScriptRunner {
    script: String,
    spawns: AtomicUsize,
    inspections: AtomicUsize,
    change_identity: bool,
}
#[cfg(windows)]
#[async_trait]
impl ClaudeProcessRunner for ScriptRunner {
    async fn inspect(&self, _: &Path) -> AdapterResult<ClaudePreflight> {
        let n = self.inspections.fetch_add(1, Ordering::SeqCst);
        Ok(ClaudePreflight {
            executable_fingerprint: "fixture".into(),
            version: "2.1.281".into(),
            auth_kind: ClaudeAuthKind::Subscription,
            account_fingerprint: Some(
                if self.change_identity && n > 0 {
                    "changed"
                } else {
                    "fixture"
                }
                .into(),
            ),
            restrictions_verified: true,
        })
    }
    async fn spawn(&self, mut spec: BackgroundLaunchSpec) -> AdapterResult<ManagedBackgroundChild> {
        self.spawns.fetch_add(1, Ordering::SeqCst);
        spec.program = std::path::PathBuf::from(std::env::var_os("SYSTEMROOT").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        spec.args = [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &self.script,
        ]
        .map(std::ffi::OsString::from)
        .to_vec();
        ManagedBackgroundChild::spawn(spec).map_err(omicsops_adapters::AdapterError::Io)
    }
}
#[cfg(windows)]
fn success_script() -> String {
    use serde_json::json;
    let assistant = json!({"type":"assistant","message":{"model":"claude-sonnet-5","content":[{"type":"text","text":"envelope"}]}});
    let result = json!({"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"{\"kind\":\"final\",\"text\":\"done\"}","usage":{"output_tokens":17}});
    format!(
        "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); $null=[Console]::In.ReadToEnd(); [Console]::Out.WriteLine('{}'); [Console]::Out.WriteLine('{}');",
        assistant.to_string().replace('\'', "''"),
        result.to_string().replace('\'', "''")
    )
}
#[cfg(windows)]
#[tokio::test]
async fn claude_stream_publishes_validated_success_after_one_dispatch() {
    use omicsops_agent::provider::ProviderStreamEvent;
    let runner = std::sync::Arc::new(ScriptRunner {
        script: success_script(),
        spawns: AtomicUsize::new(0),
        inspections: AtomicUsize::new(0),
        change_identity: false,
    });
    let client = ClaudeCodeClient::new(
        uuid::Uuid::new_v4(),
        "C:\\fixture\\claude.exe".into(),
        "sonnet".into(),
        runner.clone(),
    )
    .unwrap();
    let mut events = vec![];
    client
        .stream_once(request(), |event| events.push(event))
        .await
        .unwrap();
    assert!(
        events.iter().any(
            |event| matches!(event, ProviderStreamEvent::TextDelta { text } if text == "done")
        )
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ProviderStreamEvent::Completed))
            .count(),
        1
    );
    assert_eq!(runner.spawns.load(Ordering::SeqCst), 1);
    assert_eq!(runner.inspections.load(Ordering::SeqCst), 2);
}
#[cfg(windows)]
#[tokio::test]
async fn claude_stream_drains_stderr_concurrently_and_rejects_identity_change() {
    use omicsops_agent::provider::ProviderStreamEvent;
    let runner = std::sync::Arc::new(ScriptRunner {
        script: "$null=[Console]::In.ReadToEnd(); [Console]::Error.Write(('x'*65537));".into(),
        spawns: AtomicUsize::new(0),
        inspections: AtomicUsize::new(0),
        change_identity: false,
    });
    let client = ClaudeCodeClient::new(
        uuid::Uuid::new_v4(),
        "C:\\fixture\\claude.exe".into(),
        "sonnet".into(),
        runner.clone(),
    )
    .unwrap()
    .with_request_timeout(std::time::Duration::from_secs(3));
    let error = client.stream_once(request(), |_| {}).await.err().unwrap();
    assert!(
        error.to_string().contains("claude_stderr_too_large"),
        "{error}"
    );
    assert_eq!(runner.spawns.load(Ordering::SeqCst), 1);
    let runner = std::sync::Arc::new(ScriptRunner {
        script: success_script(),
        spawns: AtomicUsize::new(0),
        inspections: AtomicUsize::new(0),
        change_identity: true,
    });
    let client = ClaudeCodeClient::new(
        uuid::Uuid::new_v4(),
        "C:\\fixture\\claude.exe".into(),
        "sonnet".into(),
        runner.clone(),
    )
    .unwrap();
    let mut events = vec![];
    assert!(
        client
            .stream_once(request(), |e| events.push(e))
            .await
            .is_err()
    );
    assert!(!events.iter().any(|e| matches!(
        e,
        ProviderStreamEvent::Completed
            | ProviderStreamEvent::TextDelta { .. }
            | ProviderStreamEvent::ToolCallStarted { .. }
    )));
    assert_eq!(runner.spawns.load(Ordering::SeqCst), 1);
}
