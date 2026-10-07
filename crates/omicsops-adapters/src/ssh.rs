use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlledTransfer {
    Completed(u64),
    Paused(u64),
    Canceled(u64),
}

/// Raw bounded bytes for host projection; the EOF probe is never included.
#[derive(Debug)]
pub struct RemoteFilePrefix {
    pub bytes: Vec<u8>,
    pub size_bytes: Option<u64>,
    pub eof: bool,
    pub metadata_stable: bool,
}

type PrefixMetadata = Option<(Option<u64>, Option<u32>)>;
pub const REMOTE_FILE_PREFIX_CHANGED: &str = "remote prefix source changed";

#[async_trait::async_trait]
trait PrefixReader: AsyncRead + Unpin + Send + Sync {
    async fn prefix_metadata(&self) -> PrefixMetadata;
}
#[async_trait::async_trait]
impl PrefixReader for russh_sftp::client::fs::File {
    async fn prefix_metadata(&self) -> PrefixMetadata {
        self.metadata()
            .await
            .ok()
            .map(|metadata| (metadata.size, metadata.mtime))
    }
}
async fn read_handle_prefix(
    reader: &mut impl PrefixReader,
    max_bytes: u64,
) -> AdapterResult<RemoteFilePrefix> {
    let before = reader.prefix_metadata().await;
    let bytes = read_prefix_bytes(reader, max_bytes).await?;
    let after = reader.prefix_metadata().await;
    finish_file_prefix(bytes, max_bytes, before, after)
}

async fn read_prefix_bytes(
    reader: &mut (impl AsyncRead + Unpin),
    max_bytes: u64,
) -> AdapterResult<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

fn finish_file_prefix(
    mut bytes: Vec<u8>,
    max_bytes: u64,
    before: PrefixMetadata,
    after: PrefixMetadata,
) -> AdapterResult<RemoteFilePrefix> {
    if let (Some(before), Some(after)) = (before, after) {
        if matches!((before.0, after.0), (Some(a), Some(b)) if a != b)
            || matches!((before.1, after.1), (Some(a), Some(b)) if a != b)
        {
            return Err(AdapterError::Ssh(REMOTE_FILE_PREFIX_CHANGED.into()));
        }
    }
    let eof = bytes.len() as u64 <= max_bytes;
    bytes.truncate(usize::try_from(max_bytes).unwrap_or(usize::MAX));
    let size_bytes = before.and_then(|metadata| metadata.0);
    let metadata_stable = matches!((before, after), (Some((Some(a), Some(b))), Some((Some(c), Some(d)))) if a == c && b == d);
    if size_bytes
        .is_some_and(|size| size < bytes.len() as u64 || (eof && size != bytes.len() as u64))
    {
        return Err(AdapterError::Ssh(REMOTE_FILE_PREFIX_CHANGED.into()));
    }
    Ok(RemoteFilePrefix {
        bytes,
        size_bytes,
        eof,
        metadata_stable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[tokio::test]
    async fn file_prefix_probe_and_metadata() {
        let mut reader = Cursor::new(b"abcd".to_vec());
        let bytes = read_prefix_bytes(&mut reader, 3).await.unwrap();
        let read = finish_file_prefix(bytes, 3, Some((Some(4), Some(1))), Some((Some(4), Some(1))))
            .unwrap();
        assert_eq!(read.bytes, b"abc");
        assert!(!read.eof);
        assert_eq!(reader.position(), 4);
        assert!(read.metadata_stable);
        assert_eq!(read.size_bytes, Some(4));
        for metadata in [None, Some((None, Some(1))), Some((Some(4), None))] {
            let read = finish_file_prefix(b"abcd".to_vec(), 4, metadata, metadata).unwrap();
            assert!(read.eof);
            assert!(!read.metadata_stable);
            assert_eq!(read.size_bytes, metadata.and_then(|m| m.0));
        }
    }
    #[test]
    fn file_prefix_rejects_observed_changes() {
        for after in [(Some(5), Some(1)), (Some(4), Some(2))] {
            assert!(
                finish_file_prefix(b"abc".to_vec(), 3, Some((Some(4), Some(1))), Some(after))
                    .is_err()
            );
        }
    }
    struct Reader {
        bytes: Cursor<Vec<u8>>,
        metadata: Mutex<std::collections::VecDeque<PrefixMetadata>>,
    }
    impl AsyncRead for Reader {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.bytes).poll_read(cx, buf)
        }
    }
    #[async_trait::async_trait]
    impl PrefixReader for Reader {
        async fn prefix_metadata(&self) -> PrefixMetadata {
            self.metadata.lock().unwrap().pop_front().unwrap()
        }
    }
    #[tokio::test]
    async fn file_prefix_same_reader_observes_metadata_before_and_after() {
        let mut reader = Reader {
            bytes: Cursor::new(b"abcd".to_vec()),
            metadata: Mutex::new([Some((Some(4), Some(1))), Some((Some(5), Some(1)))].into()),
        };
        assert!(read_handle_prefix(&mut reader, 3).await.is_err());
        assert_eq!(reader.bytes.position(), 4);
        assert!(reader.metadata.lock().unwrap().is_empty());
    }
}

use omicsops_core::domain::ConnectionProfile;
use russh::{
    ChannelMsg, Disconnect, client,
    keys::{PrivateKeyWithHashAlg, load_secret_key, ssh_key},
};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, ReadHalf,
    WriteHalf,
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

// ChannelStream's ChannelRx polls its leftover buffer/mpsc receiver directly.
// Control messages wake then return Pending: only an unwoken Pending means
// the currently observable queue is empty. No network-in-flight guarantee.
fn jsonl_ensure_clean(reader: &mut BufReader<impl AsyncRead + Unpin>) -> AdapterResult<()> {
    use std::{
        pin::Pin,
        sync::atomic::{AtomicBool, Ordering},
        task::{Context, Poll, Wake, Waker},
    };
    use tokio::io::AsyncBufRead;
    struct Probe(AtomicBool);
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    if !reader.buffer().is_empty() {
        return Err(AdapterError::InvalidInput(
            "kernel stream contains queued data".into(),
        ));
    }
    let probe = Arc::new(Probe(AtomicBool::new(false)));
    let waker = Waker::from(probe.clone());
    let mut context = Context::from_waker(&waker);
    for _ in 0..4096 {
        probe.0.store(false, Ordering::SeqCst);
        match Pin::new(&mut *reader).poll_fill_buf(&mut context) {
            Poll::Pending if !probe.0.load(Ordering::SeqCst) => return Ok(()),
            Poll::Pending => {}
            Poll::Ready(Ok(_)) => {
                return Err(AdapterError::InvalidInput(
                    "kernel stream is not clean".into(),
                ));
            }
            Poll::Ready(Err(_)) => {
                return Err(AdapterError::InvalidInput(
                    "kernel stream unavailable".into(),
                ));
            }
        }
    }
    Err(AdapterError::InvalidInput(
        "kernel stream readiness limit".into(),
    ))
}

async fn jsonl_receive_bounded<T: serde::de::DeserializeOwned>(
    reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
    limit: usize,
) -> AdapterResult<Option<T>> {
    let mut frame = Vec::new();
    loop {
        let bytes = reader.fill_buf().await?;
        if bytes.is_empty() {
            if frame.is_empty() {
                return Ok(None);
            }
            return Err(AdapterError::InvalidInput(
                "kernel frame lacks newline".into(),
            ));
        }
        let newline = bytes.iter().position(|b| *b == b'\n');
        let count = newline.map_or(bytes.len(), |i| i + 1);
        if frame.len().saturating_add(count) > limit {
            return Err(AdapterError::InvalidInput(
                "kernel frame exceeds limit".into(),
            ));
        }
        frame.extend_from_slice(&bytes[..count]);
        reader.consume(count);
        if newline.is_some() {
            return Ok(Some(serde_json::from_slice(&frame)?));
        }
    }
}

impl SshJsonlProcess {
    /// Only for this ChannelStream transport; no new reader or background I/O.
    pub fn ensure_clean(&mut self) -> AdapterResult<()> {
        jsonl_ensure_clean(&mut self.reader)
    }
    pub async fn receive_bounded<T: serde::de::DeserializeOwned>(
        &mut self,
        limit: usize,
    ) -> AdapterResult<Option<T>> {
        jsonl_receive_bounded(&mut self.reader, limit).await
    }

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

    pub async fn upload_file_resumable(
        &self,
        local_path: &Path,
        remote_path: &str,
        offset: u64,
    ) -> AdapterResult<()> {
        let mut local = tokio::fs::File::open(local_path).await?;
        local.seek(std::io::SeekFrom::Start(offset)).await?;
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open_with_flags(remote_path, OpenFlags::CREATE | OpenFlags::WRITE)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        remote.seek(std::io::SeekFrom::Start(offset)).await?;
        tokio::io::copy(&mut local, &mut remote).await?;
        remote.flush().await?;
        remote.shutdown().await?;
        Ok(())
    }

    pub async fn upload_file_controlled<F: Fn(u64)>(
        &self,
        local_path: &Path,
        remote_path: &str,
        offset: u64,
        control: Arc<AtomicU8>,
        progress: F,
    ) -> AdapterResult<ControlledTransfer> {
        let mut local = tokio::fs::File::open(local_path).await?;
        local.seek(std::io::SeekFrom::Start(offset)).await?;
        let sftp = self.sftp().await?;
        let flags = if offset == 0 {
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
        } else {
            OpenFlags::CREATE | OpenFlags::WRITE
        };
        let mut remote = sftp
            .open_with_flags(remote_path, flags)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        remote.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut transferred = offset;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            match control.load(Ordering::SeqCst) {
                1 => {
                    remote.flush().await?;
                    return Ok(ControlledTransfer::Paused(transferred));
                }
                2 => {
                    remote.flush().await?;
                    return Ok(ControlledTransfer::Canceled(transferred));
                }
                _ => {}
            }
            let read = local.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            remote.write_all(&buffer[..read]).await?;
            transferred += read as u64;
            progress(transferred);
        }
        remote.flush().await?;
        remote.shutdown().await?;
        Ok(ControlledTransfer::Completed(transferred))
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

    pub async fn download_resumable_verified(
        &self,
        remote_path: &str,
        local_path: &Path,
        expected_sha256: &str,
        offset: u64,
    ) -> AdapterResult<()> {
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        remote.seek(std::io::SeekFrom::Start(offset)).await?;
        let temporary = local_path.with_extension(format!(
            "{}.part",
            local_path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
        ));
        let mut local = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&temporary)
            .await?;
        tokio::io::copy(&mut remote, &mut local).await?;
        local.flush().await?;
        local.sync_all().await?;
        drop(local);
        let received = sha256_path(&temporary).await?;
        if !received.eq_ignore_ascii_case(expected_sha256) {
            return Err(AdapterError::Integrity {
                expected: expected_sha256.into(),
                received,
            });
        }
        tokio::fs::rename(&temporary, local_path).await?;
        Ok(())
    }

    pub async fn download_controlled_verified<F: Fn(u64)>(
        &self,
        remote_path: &str,
        local_path: &Path,
        expected_sha256: &str,
        offset: u64,
        control: Arc<AtomicU8>,
        progress: F,
    ) -> AdapterResult<ControlledTransfer> {
        let sftp = self.sftp().await?;
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        remote.seek(std::io::SeekFrom::Start(offset)).await?;
        let temporary = local_path.with_extension(format!(
            "{}.part",
            local_path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
        ));
        let mut local = tokio::fs::OpenOptions::new()
            .create(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .write(true)
            .open(&temporary)
            .await?;
        let mut transferred = offset;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            match control.load(Ordering::SeqCst) {
                1 => {
                    local.flush().await?;
                    return Ok(ControlledTransfer::Paused(transferred));
                }
                2 => {
                    local.flush().await?;
                    return Ok(ControlledTransfer::Canceled(transferred));
                }
                _ => {}
            }
            let read = remote.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            local.write_all(&buffer[..read]).await?;
            transferred += read as u64;
            progress(transferred);
        }
        local.flush().await?;
        local.sync_all().await?;
        drop(local);
        let received = sha256_path(&temporary).await?;
        if !received.eq_ignore_ascii_case(expected_sha256) {
            return Err(AdapterError::Integrity {
                expected: expected_sha256.into(),
                received,
            });
        }
        tokio::fs::rename(&temporary, local_path).await?;
        Ok(ControlledTransfer::Completed(transferred))
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

    pub async fn read_file_prefix(
        &self,
        remote_path: &str,
        max_bytes: u64,
    ) -> AdapterResult<RemoteFilePrefix> {
        let sftp = self.sftp().await?;
        let metadata = sftp
            .symlink_metadata(remote_path)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        if metadata.permissions.is_some() && !metadata.is_regular() {
            return Err(AdapterError::Ssh(
                "remote prefix is not a regular file".into(),
            ));
        }
        let mut remote = sftp
            .open(remote_path)
            .await
            .map_err(|error| AdapterError::Ssh(error.to_string()))?;
        // fstat observes this exact opened handle. Unsupported attributes
        // stay unknown; absent sizes are never converted to zero.
        let result = read_handle_prefix(&mut remote, max_bytes).await;
        let closed = remote.shutdown().await.map_err(AdapterError::from);
        result.and_then(|read| closed.map(|_| read))
    }

    pub async fn disconnect(self) -> AdapterResult<()> {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await
            .map_err(ssh_error)
    }
}

async fn sha256_path(path: &Path) -> AdapterResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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

#[cfg(test)]
mod kernel_transport_tests {
    use super::*;
    use std::{
        collections::VecDeque,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::ReadBuf;
    enum Item {
        Control,
        Data(Vec<u8>),
        Eof,
        Error,
    }
    struct Queue(VecDeque<Item>);
    impl AsyncRead for Queue {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            out: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            match self.0.pop_front() {
                Some(Item::Control) => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Some(Item::Data(mut data)) => {
                    let count = data.len().min(out.remaining());
                    out.put_slice(&data[..count]);
                    data.drain(..count);
                    if !data.is_empty() {
                        self.0.push_front(Item::Data(data));
                    }
                    Poll::Ready(Ok(()))
                }
                Some(Item::Eof) => Poll::Ready(Ok(())),
                Some(Item::Error) => Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into())),
                None => Poll::Pending,
            }
        }
    }
    #[test]
    fn kernel_observable_queue_handles_control_pending() {
        for items in [
            vec![Item::Control, Item::Data(b"duplicate\n".to_vec())],
            vec![Item::Eof],
            vec![Item::Error],
            (0..4096).map(|_| Item::Control).collect(),
        ] {
            assert!(jsonl_ensure_clean(&mut BufReader::new(Queue(items.into()))).is_err());
        }
        assert!(jsonl_ensure_clean(&mut BufReader::new(Queue(vec![Item::Control].into()))).is_ok());
    }
    #[tokio::test]
    async fn kernel_bounded_receive_rejects_before_excess_allocation_and_checks_next_slice() {
        let mut reader = BufReader::with_capacity(
            8,
            Queue(vec![Item::Data(b"{\"x\":1}\nnext".to_vec())].into()),
        );
        let value: Option<serde_json::Value> = jsonl_receive_bounded(&mut reader, 8).await.unwrap();
        assert_eq!(value.unwrap()["x"], 1);
        assert!(jsonl_ensure_clean(&mut reader).is_err());
        let mut reader =
            BufReader::with_capacity(8, Queue(vec![Item::Data(vec![b'x'; 17])].into()));
        assert!(
            jsonl_receive_bounded::<serde_json::Value>(&mut reader, 16)
                .await
                .is_err()
        );
        assert!(
            jsonl_receive_bounded::<serde_json::Value>(
                &mut BufReader::new(Queue(vec![Item::Eof].into())),
                16
            )
            .await
            .unwrap()
            .is_none()
        );
    }
    #[tokio::test]
    async fn kernel_bounded_receive_exact_limit_partial_and_duplicate_boundaries() {
        for size in [8192, 16384] {
            let mut frame = b"{}".to_vec();
            frame.resize(size - 1, b' ');
            frame.push(b'\n');
            let mut unique =
                BufReader::with_capacity(8192, Queue(vec![Item::Data(frame.clone())].into()));
            assert!(
                jsonl_receive_bounded::<serde_json::Value>(&mut unique, 16384)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(jsonl_ensure_clean(&mut unique).is_ok());
            frame.extend_from_slice(b"{}\n");
            let mut duplicate =
                BufReader::with_capacity(8192, Queue(vec![Item::Data(frame)].into()));
            assert!(
                jsonl_receive_bounded::<serde_json::Value>(&mut duplicate, 16384)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(jsonl_ensure_clean(&mut duplicate).is_err());
        }
        let mut over = b"{}".to_vec();
        over.resize(16384, b' ');
        over.push(b'\n');
        assert!(
            jsonl_receive_bounded::<serde_json::Value>(
                &mut BufReader::new(Queue(vec![Item::Data(over)].into())),
                16384
            )
            .await
            .is_err()
        );
        assert!(
            jsonl_receive_bounded::<serde_json::Value>(
                &mut BufReader::new(Queue(vec![Item::Data(b"{}".to_vec()), Item::Eof].into())),
                16384
            )
            .await
            .is_err()
        );
    }
}
