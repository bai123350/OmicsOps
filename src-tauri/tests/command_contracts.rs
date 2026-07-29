use omicsops_core::domain::AuthenticationMethod;
use omicsops_desktop_lib::{
    commands::{PrivateKeySecret, parse_authentication_secret},
    inspection::parse_server_inspection,
};

#[test]
fn password_secret_remains_opaque() {
    let authentication =
        parse_authentication_secret(AuthenticationMethod::Password, "hunter2").unwrap();
    assert!(matches!(
        authentication,
        omicsops_adapters::ssh::SshAuthentication::Password(value) if value == "hunter2"
    ));
}

#[test]
fn private_key_secret_is_validated_json() {
    let secret = serde_json::to_string(&PrivateKeySecret {
        path: "C:\\keys\\omicsops".into(),
        passphrase: Some("secret".into()),
    })
    .unwrap();
    let authentication =
        parse_authentication_secret(AuthenticationMethod::PrivateKey, &secret).unwrap();
    assert!(matches!(
        authentication,
        omicsops_adapters::ssh::SshAuthentication::PrivateKey { .. }
    ));
}

#[test]
fn server_inspection_parser_preserves_machine_limits() {
    let raw = "OS=Linux 6.8\nCPU=16\nMEM_KIB=65536000\nDISK_KIB=104857600\nHOME=/home/omicsops\nMAMBA=/usr/bin/micromamba\n";
    let inspection = parse_server_inspection(raw).unwrap();

    assert_eq!(inspection.cpu_cores, 16);
    assert_eq!(inspection.memory_kib, 65_536_000);
    assert_eq!(inspection.disk_available_kib, 104_857_600);
    assert_eq!(
        inspection.micromamba.as_deref(),
        Some("/usr/bin/micromamba")
    );
}
