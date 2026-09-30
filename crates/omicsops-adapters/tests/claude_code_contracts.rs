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
