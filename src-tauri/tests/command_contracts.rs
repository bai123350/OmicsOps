use chrono::{TimeZone, Utc};
use omicsops_core::domain::AuthenticationMethod;
use omicsops_desktop_lib::{
    agent_commands::{SubmitMessageRequest, user_message_from_request},
    commands::{PrivateKeySecret, parse_authentication_secret},
    inspection::parse_server_inspection,
    workspace_commands::{CreateProjectRequest, project_from_request, write_project_manifest},
};
use uuid::Uuid;

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

#[test]
fn project_requests_become_local_first_workspace_records() {
    let project = project_from_request(
        CreateProjectRequest {
            name: "PBMC atlas".into(),
            description: "Two-batch comparison".into(),
            local_root: "E:/Science/pbmc-atlas".into(),
            template: "single_cell_rna_seq".into(),
        },
        Uuid::nil(),
        Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap(),
    )
    .unwrap();

    assert_eq!(project.name, "PBMC atlas");
    assert_eq!(project.description, "Two-batch comparison");
    assert!(matches!(
        project.template,
        omicsops_core::workspace::ProjectTemplate::SingleCellRnaSeq
    ));
}

#[test]
fn project_manifest_initialization_is_confined_to_the_selected_root() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = project_from_request(
        CreateProjectRequest {
            name: "PBMC atlas".into(),
            description: String::new(),
            local_root: directory.path().to_string_lossy().into_owned(),
            template: "blank".into(),
        },
        Uuid::nil(),
        Utc.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap(),
    )
    .unwrap();
    project.remote_root = Some("/srv/pbmc".into());

    write_project_manifest(&project).unwrap();

    let manifest = directory.path().join(".omicsops").join("project.json");
    assert!(manifest.exists());
    assert!(
        std::fs::read_to_string(manifest)
            .unwrap()
            .contains("PBMC atlas")
    );
}

#[test]
fn submitted_research_messages_receive_stable_sequence_and_identity() {
    let project_id = Uuid::new_v4();
    let conversation_id = Uuid::new_v4();
    let message = user_message_from_request(
        SubmitMessageRequest {
            project_id,
            conversation_id,
            markdown: "Check doublet rates".into(),
            sequence: 7,
        },
        Uuid::nil(),
        Utc.with_ymd_and_hms(2026, 8, 11, 1, 0, 0).unwrap(),
    )
    .unwrap();
    assert_eq!(message.project_id, project_id);
    assert_eq!(message.conversation_id, conversation_id);
    assert_eq!(message.sequence, 7);
    assert!(
        user_message_from_request(
            SubmitMessageRequest {
                markdown: "  ".into(),
                ..SubmitMessageRequest {
                    project_id,
                    conversation_id,
                    markdown: String::new(),
                    sequence: 8
                }
            },
            Uuid::nil(),
            Utc::now()
        )
        .is_err()
    );
}
