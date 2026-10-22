//! Opt-in production SSH startup acceptance; never opens the real Store.

use std::{path::PathBuf, sync::Arc, time::Duration};

use omicsops_adapters::{
    credentials::{CredentialVault, SystemCredentialVault},
    ssh::SshSession,
};
use omicsops_core::domain::ConnectionProfile;
use omicsops_protocol::{ExecutionContextKeyV4, KernelLanguageV4};
use omicsops_runtime::RuntimeManagerV4;
use sqlx::{Connection, sqlite::SqliteConnectOptions};
use uuid::Uuid;

use super::{SshKernelBackendV4, resolve_root, shell_quote};

async fn selected_connection() -> Result<ConnectionProfile, String> {
    let id = std::env::var("OMICSOPS_LIVE_SSH_CONNECTION_ID")
        .map_err(|_| "set OMICSOPS_LIVE_SSH_CONNECTION_ID to one saved connection UUID")?
        .parse::<Uuid>()
        .map_err(|_| "OMICSOPS_LIVE_SSH_CONNECTION_ID must be a UUID")?;
    let database = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or("run this acceptance in the Windows desktop user session")?
        .join("io.omicsops.desktop")
        .join("omicsops.db");
    if !database.is_file() {
        return Err("the desktop database does not exist".into());
    }
    let options = SqliteConnectOptions::new()
        .filename(database)
        .create_if_missing(false)
        .read_only(true);
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .map_err(|_| "could not open the desktop database read-only")?;
    let row = sqlx::query_scalar::<_, String>("SELECT profile_json FROM connections WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(&mut connection)
        .await
        .map_err(|_| "could not read the selected connection");
    connection
        .close()
        .await
        .map_err(|_| "could not close the read-only connection")?;
    let profile: ConnectionProfile =
        serde_json::from_str(&row?.ok_or("the selected connection does not exist")?)
            .map_err(|_| "the selected connection is invalid")?;
    if profile.id != id
        || profile
            .host_key_fingerprint
            .as_deref()
            .is_none_or(|fingerprint| fingerprint.trim().is_empty())
    {
        return Err("the selected connection must have a matching ID and pinned host key".into());
    }
    crate::commands::require_trusted_host(&profile)?;
    Ok(profile)
}

fn absolute_parent(parent: &str) -> Result<&str, String> {
    let parent = parent.trim_end_matches('/');
    if !parent.starts_with('/') || parent.is_empty() || parent.chars().any(char::is_control) {
        return Err("the disposable parent must be an absolute non-root Linux directory".into());
    }
    Ok(parent)
}

struct Fixture {
    id: Uuid,
    session: Option<Arc<SshSession>>,
    root: Option<String>,
    creation_unconfirmed: bool,
    runtime: Option<RuntimeManagerV4>,
    key: Option<ExecutionContextKeyV4>,
    kernel_session_id: Option<Uuid>,
    completed_requests: Vec<Uuid>,
}

impl Fixture {
    async fn run(&mut self) -> Result<(), String> {
        let parent = std::env::var("OMICSOPS_LIVE_SSH_DISPOSABLE_PARENT")
            .map_err(|_| "set OMICSOPS_LIVE_SSH_DISPOSABLE_PARENT to an existing directory")?;
        let parent = absolute_parent(&parent)?;
        let profile = selected_connection().await?;
        let secret = SystemCredentialVault
            .get(&profile.authentication_reference)
            .map_err(|_| "could not read the selected keyring credential")?
            .ok_or("the selected keyring credential is missing")?;
        let authentication =
            crate::commands::parse_authentication_secret(profile.authentication, &secret)
                .map_err(|_| "the selected keyring credential is invalid")?;
        let session = Arc::new(
            SshSession::connect(&profile, authentication)
                .await
                .map_err(|_| "SSH connection or pinned host validation failed")?,
        );
        self.session = Some(session.clone());
        let parent = resolve_root(&session, parent)
            .await
            .map_err(|_| "the disposable parent does not resolve to an existing directory")?;
        let parent = absolute_parent(&parent)?;
        let root = format!("{parent}/.omicsops-startup-acceptance-{}", self.id);
        self.creation_unconfirmed = true;
        session
            .execute_checked(&format!("mkdir -- {}", shell_quote(&root)))
            .await
            .map_err(|_| "could not confirm exclusive creation of the disposable child")?;
        self.root = Some(root.clone());
        self.creation_unconfirmed = false;
        let canonical = resolve_root(&session, &root)
            .await
            .map_err(|_| "could not verify the disposable child path")?;
        if canonical != root {
            return Err(
                "the disposable child is not a direct canonical child of its parent".into(),
            );
        }

        let key = ExecutionContextKeyV4 {
            project_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            backend_id: format!("ssh:{}", profile.id),
            language: KernelLanguageV4::Python,
            environment: "system".into(),
        };
        self.runtime = Some(RuntimeManagerV4::new(Arc::new(SshKernelBackendV4 {
            session,
            root,
            project_id: key.project_id,
            backend_id: key.backend_id.clone(),
        })));
        self.key = Some(key.clone());
        let runtime = self.runtime.as_ref().expect("runtime was initialized");
        let process = runtime
            .acquire(&key)
            .await
            .map_err(|_| "production SSH kernel launch failed")?;
        self.kernel_session_id = Some(process.session_id());
        let nonce = Uuid::new_v4().to_string();
        let first = runtime
            .execute(&key, format!("startup_nonce = '{nonce}'"), vec![])
            .await
            .map_err(|_| "the first production Python cell failed")?;
        self.completed_requests.push(first.request_id);
        let second = runtime
            .execute(&key, "print(startup_nonce)".into(), vec![])
            .await
            .map_err(|_| "the second production Python cell failed")?;
        self.completed_requests.push(second.request_id);
        if !first.succeeded
            || !second.succeeded
            || first.session_id != second.session_id
            || Some(first.session_id) != self.kernel_session_id
            || first.process_identity != second.process_identity
            || second.stdout.trim() != nonce
        {
            return Err("the production kernel did not preserve the two-cell nonce state".into());
        }
        Ok(())
    }

    async fn cleanup(&self) -> Result<(), String> {
        // Interrupt even after an incomplete cell. This is only the isolated
        // fixture kernel; it never addresses an existing project or remote job.
        let interrupt = if let (Some(runtime), Some(key)) = (&self.runtime, &self.key) {
            runtime.interrupt(key).await
        } else {
            Ok(())
        };
        if self.creation_unconfirmed {
            return Err(
                "disposable child creation was unconfirmed; automatic cleanup refused".into(),
            );
        }
        let (Some(session), Some(root)) = (&self.session, &self.root) else {
            return interrupt.map_err(|_| "fixture kernel shutdown was not confirmed".into());
        };
        let canonical = resolve_root(session, root)
            .await
            .map_err(|_| "could not verify the fixture root before cleanup")?;
        if canonical != *root {
            return Err("fixture path changed; automatic cleanup refused".into());
        }
        let mut files = vec![];
        if let Some(id) = self.kernel_session_id {
            files.push(format!("{root}/.omicsops/kernels/v4-{id}.driver.py"));
        }
        let mut directories = vec![];
        if let Some(key) = &self.key {
            let run_dir = format!("{root}/.omicsops/runs/{}", key.run_id);
            for id in &self.completed_requests {
                for stream in ["stdout", "stderr"] {
                    files.push(format!("{run_dir}/outputs/{id}.{stream}.txt"));
                }
            }
            directories.extend([format!("{run_dir}/outputs"), run_dir]);
        }
        directories.extend([
            format!("{root}/.omicsops/runs"),
            format!("{root}/.omicsops/kernels"),
            format!("{root}/.omicsops"),
            root.clone(),
        ]);
        // No glob or recursive deletion: remove only exact returned session /
        // request files. Unknown partial files cause rmdir to fail and remain.
        let mut commands = directories
            .iter()
            .map(|path| format!("test ! -L {}", shell_quote(path)))
            .collect::<Vec<_>>();
        for path in files {
            let path = shell_quote(&path);
            commands.push(format!(
                "test -f {path} && test ! -L {path} && rm -- {path}"
            ));
        }
        for path in directories {
            let path = shell_quote(&path);
            commands.push(format!("if test -d {path}; then rmdir -- {path}; fi"));
        }
        session
            .execute_checked(&commands.join(" && "))
            .await
            .map_err(
                |_| "fixture cleanup incomplete; exact leftover paths require manual review",
            )?;
        interrupt.map_err(|_| "fixture kernel shutdown was not confirmed".into())
    }
}

#[tokio::test]
#[ignore = "requires one selected saved SSH connection, pinned host, keyring credential and explicit disposable parent"]
async fn live_ssh_kernel_startup_preserves_two_python_cells() -> Result<(), String> {
    let started = tokio::time::Instant::now();
    let mut fixture = Fixture {
        id: Uuid::new_v4(),
        session: None,
        root: None,
        creation_unconfirmed: false,
        runtime: None,
        key: None,
        kernel_session_id: None,
        completed_requests: vec![],
    };
    let outcome = tokio::time::timeout_at(started + Duration::from_secs(65), fixture.run())
        .await
        .unwrap_or_else(|_| {
            Err("SSH startup acceptance exceeded its 65-second work deadline".into())
        });
    let cleanup = tokio::time::timeout_at(started + Duration::from_secs(90), fixture.cleanup())
        .await
        .unwrap_or_else(|_| Err("fixture cleanup exceeded the 90-second total deadline".into()));
    if outcome.is_err() || cleanup.is_err() {
        // Only a generated fixture ID and fixed categories, never host output,
        // credentials, model reasoning, or original project contents.
        eprintln!("SSH startup fixture {}: cleanup={cleanup:?}", fixture.id);
    }
    cleanup?;
    outcome?;
    eprintln!(
        "SSH startup acceptance passed: cells=2 same_session=true cleanup=complete elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(())
}
