use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use omicsops_core::domain::ConnectionProfile;
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{PrivateKeyWithHashAlg, load_secret_key, ssh_key},
};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::{AdapterError, AdapterResult};

#[derive(Debug, Clone)]
pub enum SshAuthentication {
    Password(String),
    PrivateKey {
        path: PathBuf,
        passphrase: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutput {
    pub status: u32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
struct HostKeyHandler {
    expected: Option<String>,
    observed: Arc<Mutex<Option<String>>>,
}

impl client::Handler for HostKeyHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .fingerprint(ssh_key::HashAlg::Sha256)
            .to_string();
        *self.observed.lock().expect("host key lock") = Some(fingerprint.clone());
        Ok(self
            .expected
            .as_ref()
            .is_none_or(|expected| expected == &fingerprint))
    }
}

pub struct SshSession {
    handle: client::Handle<HostKeyHandler>,
    fingerprint: String,
}

impl SshSession {
    pub async fn connect(
        profile: &ConnectionProfile,
        authentication: SshAuthentication,
    ) -> AdapterResult<Self> {
        let observed = Arc::new(Mutex::new(None));
        let handler = HostKeyHandler {
            expected: profile.host_key_fingerprint.clone(),
            observed: observed.clone(),
        };
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(30)),
            ..Default::default()
        });
        let mut handle = client::connect(config, (profile.host.as_str(), profile.port), handler)
            .await
            .map_err(ssh_error)?;
        let fingerprint = observed
            .lock()
            .expect("host key lock")
            .clone()
            .ok_or_else(|| AdapterError::Ssh("server did not present a host key".into()))?;
        if let Some(expected) = &profile.host_key_fingerprint {
            if expected != &fingerprint {
                return Err(AdapterError::HostKeyChanged {
                    expected: expected.clone(),
                    received: fingerprint,
                });
            }
        }

        let authenticated = match authentication {
            SshAuthentication::Password(password) => handle
                .authenticate_password(&profile.username, password)
                .await
                .map_err(ssh_error)?
                .success(),
            SshAuthentication::PrivateKey { path, passphrase } => {
                let key = load_secret_key(path, passphrase.as_deref())
                    .map_err(|error| AdapterError::Ssh(error.to_string()))?;
                let hash = handle
                    .best_supported_rsa_hash()
                    .await
                    .map_err(ssh_error)?
                    .flatten();
                handle
                    .authenticate_publickey(
                        &profile.username,
                        PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await
                    .map_err(ssh_error)?
                    .success()
            }
        };
        if !authenticated {
            return Err(AdapterError::Authentication);
        }
        Ok(Self {
            handle,
            fingerprint,
        })
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub async fn execute(&self, command: &str) -> AdapterResult<CommandOutput> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(ssh_error)?;
        channel.exec(true, command).await.map_err(ssh_error)?;
        let mut status = None;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                _ => {}
            }
        }
        Ok(CommandOutput {
            status: status.unwrap_or(255),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    pub async fn execute_checked(&self, command: &str) -> AdapterResult<CommandOutput> {
        let output = self.execute(command).await?;
        if output.status != 0 {
            return Err(AdapterError::RemoteCommand {
                status: output.status,
                stderr: output.stderr,
            });
        }
        Ok(output)
    }

    async fn sftp(&self) -> AdapterResult<SftpSession> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(ssh_error)?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(ssh_error)?;
        SftpSession::new(channel.into_stream())
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))
    }

    pub async fn upload_text(&self, remote_path: &str, contents: &str) -> AdapterResult<()> {
        let sftp = self.sftp().await?;
        let mut file = sftp
            .open_with_flags(
                remote_path,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        file.write_all(contents.as_bytes()).await?;
        file.shutdown().await?;
        Ok(())
    }

    pub async fn upload_file(&self, local_path: &Path, remote_path: &str) -> AdapterResult<()> {
        let mut local = tokio::fs::File::open(local_path).await?;
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open_with_flags(
                remote_path,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        tokio::io::copy(&mut local, &mut remote).await?;
        remote.flush().await?;
        remote.shutdown().await?;
        Ok(())
    }

    pub async fn download_atomic_verified(
        &self,
        remote_path: &str,
        local_path: &Path,
        expected_sha256: &str,
    ) -> AdapterResult<()> {
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        write_verified_atomic(&mut remote, local_path, expected_sha256).await
    }

    pub async fn disconnect(self) -> AdapterResult<()> {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await
            .map_err(ssh_error)
    }
}

pub async fn write_verified_atomic<R: AsyncRead + Unpin>(
    source: &mut R,
    local_path: &Path,
    expected_sha256: &str,
) -> AdapterResult<()> {
    let temporary = local_path.with_extension(format!(
        "{}.part",
        local_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
    ));
    let result = async {
        let mut local = tokio::fs::File::create(&temporary).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            let read = source.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            local.write_all(&buffer[..read]).await?;
        }
        local.flush().await?;
        local.sync_all().await?;
        drop(local);
        let received = format!("{:x}", hasher.finalize());
        if !received.eq_ignore_ascii_case(expected_sha256) {
            return Err(AdapterError::Integrity {
                expected: expected_sha256.to_owned(),
                received,
            });
        }
        tokio::fs::rename(&temporary, local_path).await?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

fn ssh_error(error: russh::Error) -> AdapterError {
    AdapterError::Ssh(error.to_string())
}
