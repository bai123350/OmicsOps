//! Official CLI credential ownership. No OAuth tokens are read or routed by OmicsOps.
use crate::{AdapterError, AdapterResult};
use async_trait::async_trait;
use omicsops_process::managed_child::{BackgroundLaunchSpec, ManagedBackgroundChild};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncReadExt;

pub(crate) fn error(code: &str) -> AdapterError {
    AdapterError::Llm(code.into())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeAuthKind {
    Subscription,
    Api,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudePreflight {
    pub executable_fingerprint: String,
    pub version: String,
    pub auth_kind: ClaudeAuthKind,
    pub account_fingerprint: Option<String>,
    pub restrictions_verified: bool,
}
#[async_trait]
pub trait ClaudeProcessRunner: Send + Sync {
    async fn inspect(&self, executable: &Path) -> AdapterResult<ClaudePreflight>;
    async fn spawn(&self, spec: BackgroundLaunchSpec) -> AdapterResult<ManagedBackgroundChild>;
}
pub fn validate_claude_executable(path: &Path) -> AdapterResult<()> {
    if !cfg!(windows) {
        return Err(error("claude_windows_only"));
    }
    if !path.is_absolute()
        || !path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("claude.exe"))
    {
        return Err(error("claude_executable_path_invalid"));
    }
    Ok(())
}
pub fn resolve_claude_executable(explicit: Option<&str>) -> AdapterResult<PathBuf> {
    if let Some(path) = explicit {
        let path = PathBuf::from(path);
        validate_claude_executable(&path)?;
        return Ok(path);
    }
    if !cfg!(windows) {
        return Err(error("claude_windows_only"));
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = dir.join("claude.exe");
        if path.is_absolute() && path.is_file() {
            return Ok(path);
        }
    }
    Err(error("claude_not_installed"))
}
pub fn claude_executable_fingerprint(path: &Path) -> AdapterResult<String> {
    let mut file = std::fs::File::open(path).map_err(|_| error("claude_not_installed"))?;
    if file
        .metadata()
        .map_err(|_| error("claude_binary_unreadable"))?
        .len()
        > 512 * 1024 * 1024
    {
        return Err(error("claude_binary_too_large"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0usize;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| error("claude_binary_unreadable"))?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        if total > 512 * 1024 * 1024 {
            return Err(error("claude_binary_too_large"));
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
pub fn parse_claude_version(output: &str) -> AdapterResult<String> {
    let value = output
        .trim()
        .strip_suffix(" (Claude Code)")
        .ok_or_else(|| error("claude_version_unknown"))?;
    let components: Vec<_> = value
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<_, _>>()
        .map_err(|_| error("claude_version_unknown"))?;
    if components.len() != 3 || components[0] != 2 || components[1] != 1 || components[2] < 248 {
        return Err(error("claude_restrictions_version_unsupported"));
    }
    Ok(value.into())
}
pub async fn inspect_claude_preflight(
    path: &Path,
    runner: &dyn ClaudeProcessRunner,
) -> AdapterResult<ClaudePreflight> {
    validate_claude_executable(path)?;
    let preflight = tokio::time::timeout(Duration::from_secs(15), runner.inspect(path))
        .await
        .map_err(|_| error("claude_preflight_timeout"))??;
    parse_claude_version(&format!("{} (Claude Code)", preflight.version))?;
    if preflight.auth_kind != ClaudeAuthKind::Subscription {
        return Err(error("claude_subscription_login_required"));
    }
    if !preflight.restrictions_verified {
        return Err(error("claude_policy_unverifiable"));
    }
    if preflight.executable_fingerprint.is_empty() {
        return Err(error("claude_binary_identity_missing"));
    }
    Ok(preflight)
}
pub(crate) fn subscription_environment() -> BTreeMap<OsString, OsString> {
    const ALLOWED: &[&str] = &[
        "SYSTEMROOT",
        "WINDIR",
        "SYSTEMDRIVE",
        "COMSPEC",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "PATH",
        "PATHEXT",
    ];
    std::env::vars_os()
        .filter(|(key, _)| ALLOWED.contains(&key.to_string_lossy().to_uppercase().as_str()))
        .collect()
}
#[derive(Default)]
pub struct SystemClaudeProcessRunner;
impl SystemClaudeProcessRunner {
    async fn read_only(
        executable: &Path,
        args: &[&str],
    ) -> AdapterResult<(std::process::ExitStatus, Vec<u8>)> {
        let dir = tempfile::tempdir().map_err(|_| error("claude_temporary_directory_failed"))?;
        let spec = BackgroundLaunchSpec {
            program: executable.to_path_buf(),
            args: args.iter().map(OsString::from).collect(),
            cwd: dir.path().to_path_buf(),
            env: subscription_environment(),
        };
        let mut child = ManagedBackgroundChild::spawn(spec)
            .map_err(|_| error("claude_process_start_failed"))?;
        drop(child.take_stdin());
        let mut stdout = child
            .take_stdout()
            .ok_or_else(|| error("claude_stdout_missing"))?
            .take(64 * 1024 + 1);
        let mut stderr = child
            .take_stderr()
            .ok_or_else(|| error("claude_stderr_missing"))?
            .take(64 * 1024 + 1);
        let outcome = tokio::time::timeout(Duration::from_secs(15), async {
            let (out, err, status) = tokio::try_join!(
                async {
                    let mut bytes = vec![];
                    stdout.read_to_end(&mut bytes).await?;
                    Ok::<_, std::io::Error>(bytes)
                },
                async {
                    let mut bytes = vec![];
                    stderr.read_to_end(&mut bytes).await?;
                    Ok::<_, std::io::Error>(bytes)
                },
                child.wait()
            )
            .map_err(|_| error("claude_inspection_io_failed"))?;
            if out.len() > 64 * 1024 || err.len() > 64 * 1024 {
                return Err(error("claude_inspection_output_too_large"));
            }
            Ok((status, out))
        })
        .await;
        match outcome {
            Ok(Ok(result)) => Ok(result),
            _ => {
                let _ = child.terminate_and_wait().await;
                Err(error("claude_inspection_failed_or_timed_out"))
            }
        }
    }
}
#[async_trait]
impl ClaudeProcessRunner for SystemClaudeProcessRunner {
    async fn inspect(&self, executable: &Path) -> AdapterResult<ClaudePreflight> {
        validate_claude_executable(executable)?;
        let fingerprint = claude_executable_fingerprint(executable)?;
        let (status, version) = Self::read_only(executable, &["--version"]).await?;
        if !status.success() {
            return Err(error("claude_version_check_failed"));
        }
        let version = parse_claude_version(
            std::str::from_utf8(&version).map_err(|_| error("claude_version_unknown"))?,
        )?;
        let (status, auth) = Self::read_only(executable, &["auth", "status"]).await?;
        let auth: Value =
            serde_json::from_slice(&auth).map_err(|_| error("claude_auth_status_unknown"))?;
        let kind = if status.success()
            && auth["loggedIn"] == true
            && auth["authMethod"] == "claude.ai"
            && auth["apiProvider"] == "firstParty"
        {
            ClaudeAuthKind::Subscription
        } else if auth["authMethod"] == "api_key" || auth["authMethod"] == "apiKey" {
            ClaudeAuthKind::Api
        } else {
            ClaudeAuthKind::Unknown
        };
        let account = auth["orgId"]
            .as_str()
            .zip(auth["email"].as_str())
            .map(|(org, email)| {
                let mut hash = Sha256::new();
                hash.update(org);
                hash.update([0]);
                hash.update(email);
                hex::encode(hash.finalize())
            });
        if fingerprint != claude_executable_fingerprint(executable)? {
            return Err(error("claude_binary_changed"));
        }
        // Official docs expose /status after session startup, not a complete read-only policy
        // snapshot before SessionStart. Managed hooks remain active in --safe-mode. Neither
        // missing local files nor host --settings prove they are disabled. Do not generate.
        // https://code.claude.com/docs/en/cli-reference
        // https://code.claude.com/docs/en/managed-settings
        Ok(ClaudePreflight {
            executable_fingerprint: fingerprint,
            version,
            auth_kind: kind,
            account_fingerprint: account,
            restrictions_verified: false,
        })
    }
    async fn spawn(&self, _spec: BackgroundLaunchSpec) -> AdapterResult<ManagedBackgroundChild> {
        // No caller can bypass the production preflight by invoking the runner directly.
        Err(error("claude_policy_unverifiable"))
    }
}
