use omicsops_adapters::ssh::{SshAuthentication, SshSession};
use omicsops_core::domain::{AuthenticationMethod, ConnectionProfile};
use uuid::Uuid;

fn live_profile() -> Option<ConnectionProfile> {
    let host = std::env::var("OMICSOPS_LIVE_SSH_HOST").ok()?;
    let port = std::env::var("OMICSOPS_LIVE_SSH_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(22);
    let username = std::env::var("OMICSOPS_LIVE_SSH_USER").ok()?;
    Some(ConnectionProfile {
        id: Uuid::new_v4(),
        label: "opt-in live compatibility test".into(),
        host,
        port,
        username,
        authentication: AuthenticationMethod::Password,
        authentication_reference: "test-only/environment".into(),
        host_key_fingerprint: None,
    })
}

#[tokio::test]
#[ignore = "requires explicit OMICSOPS_LIVE_SSH_* environment variables and real network access"]
async fn live_server_supports_host_key_probe_without_sending_credentials() {
    let profile = live_profile().expect("live SSH host, port, and user must be provided");
    let fingerprint = SshSession::probe_host_key(&profile).await.unwrap();
    assert!(fingerprint.starts_with("SHA256:"));
}

#[tokio::test]
#[ignore = "requires explicit OMICSOPS_LIVE_SSH_PASSWORD and sends it only after the pinned host key is supplied"]
async fn live_password_session_executes_diagnostics_after_host_key_pin() {
    let mut profile = live_profile().expect("live SSH host, port, and user must be provided");
    profile.host_key_fingerprint = Some(
        std::env::var("OMICSOPS_LIVE_SSH_FINGERPRINT")
            .expect("the independently verified host fingerprint must be pinned"),
    );
    let password = std::env::var("OMICSOPS_LIVE_SSH_PASSWORD")
        .expect("live SSH password must be provided through the process environment");
    let session = SshSession::connect(&profile, SshAuthentication::Password(password))
        .await
        .unwrap();
    let output = session
        .execute_checked("uname -s; id -un; command -v python3; command -v Rscript || true")
        .await
        .unwrap();
    assert!(output.stdout.lines().next().is_some());
    session.disconnect().await.unwrap();
}
