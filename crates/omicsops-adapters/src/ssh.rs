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
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf,
};

use crate::{AdapterError, AdapterResult};

fn authenticated_client_config() -> client::Config {
    client::Config {
        inactivity_timeout: Some(Duration::from_secs(300)),
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        ..Default::default()
    }
}

#[cfg(test)]
mod client_config_tests {
    use super::authenticated_client_config;
    use std::time::Duration;

    #[test]
    fn long_running_sessions_have_protocol_keepalive() {
        let config = authenticated_client_config();
        assert_eq!(config.keepalive_interval, Some(Duration::from_secs(15)));
        assert_eq!(config.keepalive_max, 3);
        assert_eq!(config.inactivity_timeout, Some(Duration::from_secs(300)));
    }
}

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

pub struct SshJsonlProcess {
    reader: BufReader<ReadHalf<russh::ChannelStream<client::Msg>>>,
    writer: WriteHalf<russh::ChannelStream<client::Msg>>,
}

impl SshJsonlProcess {
    pub async fn send<T: Serialize>(&mut self, value: &T) -> AdapterResult<()> {
        let mut line = serde_json::to_vec(value)?;
        line.push(b'\n');
        self.writer.write_all(&line).await?;
        self.writer.flush().await?;
        Ok(())
    }

    pub async fn receive<T: serde::de::DeserializeOwned>(&mut self) -> AdapterResult<Option<T>> {
        let mut line = String::new();
        let bytes = self.reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(None);
        }
        if bytes > 1024 * 1024 {
            return Err(AdapterError::InvalidInput(
                "kernel JSONL message exceeds 1 MiB".into(),
            ));
        }
        Ok(Some(serde_json::from_str(line.trim_end())?))
    }

    pub async fn shutdown(mut self) -> AdapterResult<()> {
        self.writer.shutdown().await?;
        Ok(())
    }
}

impl SshSession {
    pub async fn probe_host_key(profile: &ConnectionProfile) -> AdapterResult<String> {
        let observed = Arc::new(Mutex::new(None));
        let handler = HostKeyHandler {
            expected: None,
            observed: observed.clone(),
        };
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(15)),
            ..Default::default()
        });
        let handle = tokio::time::timeout(
            Duration::from_secs(15),
            client::connect(config, (profile.host.as_str(), profile.port), handler),
        )
        .await
        .map_err(|_| AdapterError::Ssh("connection timed out during host key probe".into()))?
        .map_err(ssh_error)?;
        let fingerprint = observed
            .lock()
            .expect("host key lock")
            .clone()
            .ok_or_else(|| AdapterError::Ssh("server did not present a host key".into()))?;
        handle
            .disconnect(Disconnect::ByApplication, "host key probe complete", "en")
            .await
            .map_err(ssh_error)?;
        Ok(fingerprint)
    }

    pub async fn connect(
        profile: &ConnectionProfile,
        authentication: SshAuthentication,
    ) -> AdapterResult<Self> {
        let observed = Arc::new(Mutex::new(None));
        let handler = HostKeyHandler {
            expected: profile.host_key_fingerprint.clone(),
            observed: observed.clone(),
        };
        let config = Arc::new(authenticated_client_config());
        let mut handle = tokio::time::timeout(
            Duration::from_secs(15),
            client::connect(config, (profile.host.as_str(), profile.port), handler),
        )
        .await
        .map_err(|_| AdapterError::Ssh("connection timed out".into()))?
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
            SshAuthentication::Password(password) => tokio::time::timeout(
                Duration::from_secs(15),
                handle.authenticate_password(&profile.username, password),
            )
            .await
            .map_err(|_| AdapterError::Ssh("password authentication timed out".into()))?
            .map_err(ssh_error)?
            .success(),
            SshAuthentication::PrivateKey { path, passphrase } => {
                let key = load_secret_key(path, passphrase.as_deref())
                    .map_err(|error| AdapterError::Ssh(error.to_string()))?;
                let hash =
                    tokio::time::timeout(Duration::from_secs(15), handle.best_supported_rsa_hash())
                        .await
                        .map_err(|_| AdapterError::Ssh("key negotiation timed out".into()))?
                        .map_err(ssh_error)?
                        .flatten();
                tokio::time::timeout(
                    Duration::from_secs(15),
                    handle.authenticate_publickey(
                        &profile.username,
                        PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    ),
                )
                .await
                .map_err(|_| AdapterError::Ssh("public key authentication timed out".into()))?
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
        self.execute_streaming(command, |_, _| {}).await
    }

    pub async fn execute_streaming(
        &self,
        command: &str,
        mut on_output: impl FnMut(bool, &str),
    ) -> AdapterResult<CommandOutput> {
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
                ChannelMsg::Data { data } => {
                    on_output(false, &String::from_utf8_lossy(&data));
                    stdout.extend_from_slice(&data);
                }
                ChannelMsg::ExtendedData { data, .. } => {
                    on_output(true, &String::from_utf8_lossy(&data));
                    stderr.extend_from_slice(&data);
                }
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

    pub async fn probe_sftp(&self) -> AdapterResult<()> {
        let _session = self.sftp().await?;
        Ok(())
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

    pub async fn open_jsonl_process(&self, command: &str) -> AdapterResult<SshJsonlProcess> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(ssh_error)?;
        channel.exec(true, command).await.map_err(ssh_error)?;
        let stream = channel.into_stream();
        let (reader, writer) = tokio::io::split(stream);
        Ok(SshJsonlProcess {
            reader: BufReader::new(reader),
            writer,
        })
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

    pub async fn read_file_limited(
        &self,
        remote_path: &str,
        max_bytes: u64,
    ) -> AdapterResult<Vec<u8>> {
        let sftp = self.sftp().await?;
        let remote = sftp
            .open(remote_path)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        let mut limited = remote.take(max_bytes.saturating_add(1));
        let mut bytes = Vec::new();
        limited.read_to_end(&mut bytes).await?;
        if bytes.len() as u64 > max_bytes {
            return Err(AdapterError::Ssh(format!(
                "remote file exceeds preview limit of {max_bytes} bytes"
            )));
        }
        Ok(bytes)
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
