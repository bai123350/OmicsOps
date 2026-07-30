use std::path::PathBuf;

use omicsops_adapters::ssh::{SshAuthentication, SshSession};
use omicsops_core::domain::{AuthenticationMethod, ConnectionProfile};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires the loopback-only WSL sshd fixture"]
async fn real_ssh_session_executes_sftp_and_verified_download() {
    let key_path = std::env::var("OMICSOPS_E2E_KEY").expect("OMICSOPS_E2E_KEY");
    let port = std::env::var("OMICSOPS_E2E_PORT")
        .unwrap_or_else(|_| "40222".into())
        .parse()
        .unwrap();
    let username = std::env::var("OMICSOPS_E2E_USER").unwrap_or_else(|_| "jin".into());
    let profile = ConnectionProfile {
        id: Uuid::new_v4(),
        label: "WSL loopback fixture".into(),
        host: "127.0.0.1".into(),
        port,
        username,
        authentication: AuthenticationMethod::PrivateKey,
        authentication_reference: "test-only".into(),
        host_key_fingerprint: None,
    };
    let session = SshSession::connect(
        &profile,
        SshAuthentication::PrivateKey {
            path: PathBuf::from(key_path),
            passphrase: None,
        },
    )
    .await
    .unwrap();
    assert!(session.fingerprint().starts_with("SHA256:"));

    let remote_root = format!("/home/jin/.cache/omicsops-ssh-test-{}", Uuid::new_v4());
    session
        .execute_checked(&format!("mkdir -p '{remote_root}'"))
        .await
        .unwrap();
    let contents = b"verified OmicsOps transfer\n";
    let remote_path = format!("{remote_root}/artifact.txt");
    session
        .upload_text(&remote_path, std::str::from_utf8(contents).unwrap())
        .await
        .unwrap();
    let output = session
        .execute_checked(&format!("cat '{remote_path}'"))
        .await
        .unwrap();
    assert_eq!(output.stdout.as_bytes(), contents);

    let expected = format!("{:x}", Sha256::digest(contents));
    let local_directory = tempdir().unwrap();
    let local = local_directory.path().join("artifact.txt");
    session
        .download_atomic_verified(&remote_path, &local, &expected)
        .await
        .unwrap();
    assert_eq!(std::fs::read(local).unwrap(), contents);
    session.disconnect().await.unwrap();
}
