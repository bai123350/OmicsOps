use chrono::{TimeZone, Utc};
use omicsops_agent::{AgentEventKind, ModelStreamEvent};
use omicsops_core::domain::{AuthenticationMethod, ConnectionProfile};
use omicsops_desktop_lib::{
    agent_commands::{
        SubmitMessageRequest, agent_event_kind_from_model_event, user_message_from_request,
    },
    commands::{PrivateKeySecret, normalize_connection_profile, parse_authentication_secret},
    inspection::parse_server_inspection,
    kernel_commands::{
        PromoteKernelCellRequest, mark_orphaned_kernels_interrupted, promote_saved_kernel_cell,
    },
    model_commands::{SaveModelProfileRequest, model_profile_from_request},
    research_commands::research_cache_key,
    skill_commands::{install_builtin_skills, set_skill_enabled_in_repository},
    sync_commands::{choose_download_relative_path, parse_remote_index, resolve_selected_uploads},
    workspace_commands::{
        CreateProjectRequest, UpdateProjectRemoteRequest, apply_remote_binding,
        project_from_request, write_project_manifest,
    },
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
fn changed_ssh_endpoints_clear_trust_and_use_a_canonical_credential_reference() {
    let id = Uuid::new_v4();
    let previous = ConnectionProfile {
        id,
        label: "Old".into(),
        host: "old.example.org".into(),
        port: 22,
        username: "worker".into(),
        authentication: AuthenticationMethod::Password,
        authentication_reference: "untrusted/reference".into(),
        host_key_fingerprint: Some("SHA256:old".into()),
    };
    let mut updated = previous.clone();
    updated.label = "  Lab SSH  ".into();
    updated.host = "new.example.org".into();
    normalize_connection_profile(&mut updated, Some(&previous)).unwrap();
    assert_eq!(updated.label, "Lab SSH");
    assert_eq!(updated.authentication_reference, format!("ssh/{id}"));
    assert_eq!(updated.host_key_fingerprint, None);

    let mut same_endpoint = previous.clone();
    same_endpoint.host_key_fingerprint = Some("SHA256:client-supplied".into());
    normalize_connection_profile(&mut same_endpoint, Some(&previous)).unwrap();
    assert_eq!(
        same_endpoint.host_key_fingerprint.as_deref(),
        Some("SHA256:old")
    );
}

#[test]
fn project_remote_binding_requires_an_existing_connection_and_absolute_linux_path() {
    let now = Utc.with_ymd_and_hms(2026, 8, 11, 2, 0, 0).unwrap();
    let mut project = project_from_request(
        CreateProjectRequest {
            name: "PBMC".into(),
            description: "".into(),
            local_root: "E:/Science/pbmc".into(),
            template: "single_cell_rna_seq".into(),
            connection_id: None,
            remote_root: None,
        },
        Uuid::new_v4(),
        now,
    )
    .unwrap();
    let project_id = project.id;
    let connection_id = Uuid::new_v4();
    assert!(
        apply_remote_binding(
            &mut project,
            UpdateProjectRemoteRequest {
                project_id,
                connection_id: Some(connection_id),
                remote_root: Some("relative/path".into())
            },
            true,
            now
        )
        .is_err()
    );
    assert!(
        apply_remote_binding(
            &mut project,
            UpdateProjectRemoteRequest {
                project_id,
                connection_id: Some(connection_id),
                remote_root: Some("/home/worker/pbmc".into())
            },
            false,
            now
        )
        .is_err()
    );
    apply_remote_binding(
        &mut project,
        UpdateProjectRemoteRequest {
            project_id,
            connection_id: Some(connection_id),
            remote_root: Some("/home/worker/pbmc/".into()),
        },
        true,
        now,
    )
    .unwrap();
    assert_eq!(project.connection_id, Some(connection_id));
    assert_eq!(project.remote_root.as_deref(), Some("/home/worker/pbmc"));
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
            connection_id: None,
            remote_root: None,
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
            connection_id: None,
            remote_root: None,
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

#[test]
fn model_profile_records_only_credential_references() {
    let id = Uuid::new_v4();
    let profile = model_profile_from_request(SaveModelProfileRequest {
        id: Some(id),
        label: "Lab Claude".into(),
        provider: "anthropic".into(),
        base_url: "https://api.anthropic.com/".into(),
        model: "claude-science".into(),
        credential: Some("secret-key".into()),
    })
    .unwrap();
    assert_eq!(profile.id, id);
    assert_eq!(
        profile.credential_reference.as_deref(),
        Some(format!("model/{id}").as_str())
    );
    assert!(
        !serde_json::to_string(&profile)
            .unwrap()
            .contains("secret-key")
    );

    let ollama = model_profile_from_request(SaveModelProfileRequest {
        id: None,
        label: "Local".into(),
        provider: "ollama".into(),
        base_url: "http://127.0.0.1:11434/".into(),
        model: "qwen3".into(),
        credential: None,
    })
    .unwrap();
    assert!(ollama.credential_reference.is_none());
}

#[test]
fn provider_events_map_to_stable_desktop_agent_events() {
    assert_eq!(
        agent_event_kind_from_model_event(ModelStreamEvent::TextDelta("QC".into())),
        AgentEventKind::TextDelta("QC".into())
    );
    assert_eq!(
        agent_event_kind_from_model_event(ModelStreamEvent::ToolArgumentsDelta {
            name: "submit_plan".into(),
            json_fragment: "{}".into()
        }),
        AgentEventKind::ToolArgumentsDelta {
            name: "submit_plan".into(),
            json_fragment: "{}".into()
        }
    );
    assert_eq!(
        agent_event_kind_from_model_event(ModelStreamEvent::Completed),
        AgentEventKind::TurnCompleted
    );
}

#[test]
fn production_tauri_config_never_points_at_a_development_server() {
    let production: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert!(production.pointer("/build/devUrl").is_none());
    assert_eq!(
        production
            .pointer("/build/frontendDist")
            .and_then(|value| value.as_str()),
        Some("../dist")
    );
    let development: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.dev.conf.json")).unwrap();
    assert_eq!(
        development
            .pointer("/build/devUrl")
            .and_then(|value| value.as_str()),
        Some("http://localhost:1420")
    );
}

#[test]
fn remote_index_parser_preserves_relative_paths_and_metadata() {
    let raw = "results/umap.png\0f\01234\01723200000.25\0analysis\0d\00\01723200001.0\0";
    let entries = parse_remote_index(raw).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].relative_path, "results/umap.png");
    assert_eq!(entries[0].size_bytes, 1234);
    assert!(!entries[0].directory);
    assert!(entries[1].directory);
}

#[test]
fn selected_uploads_never_expand_to_unselected_workspace_files() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("data")).unwrap();
    std::fs::write(directory.path().join("data/selected.csv"), "a,b\n1,2\n").unwrap();
    std::fs::write(directory.path().join("data/private.csv"), "secret").unwrap();
    let selected =
        resolve_selected_uploads(directory.path(), &["data/selected.csv".into()]).unwrap();
    assert_eq!(selected.len(), 1);
    assert!(selected[0].ends_with("selected.csv"));
}

#[test]
fn downloads_use_parallel_conflict_versions_instead_of_overwriting() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("results")).unwrap();
    std::fs::write(directory.path().join("results/markers.csv"), "local").unwrap();
    let chosen = choose_download_relative_path(
        directory.path(),
        "results/markers.csv",
        "different-remote-hash",
    )
    .unwrap();
    assert_eq!(
        chosen.to_string_lossy().replace('\\', "/"),
        "results/markers.conflict-1.csv"
    );
    assert!(directory.path().join("results/markers.csv").exists());
}

#[test]
fn research_cache_keys_include_source_query_limit_and_page_cursor() {
    let first = research_cache_key("pubmed", "PBMC", 20, None);
    assert_eq!(first, research_cache_key("pubmed", "PBMC", 20, None));
    assert_ne!(first, research_cache_key("europe-pmc", "PBMC", 20, None));
    assert_ne!(first, research_cache_key("pubmed", "PBMC", 20, Some("20")));
}

#[test]
fn builtin_research_skills_are_versioned_hashed_and_enabled() {
    let repository = omicsops_adapters::persistence::Repository::open_in_memory().unwrap();
    let directory = tempfile::tempdir().unwrap();
    install_builtin_skills(&repository, directory.path()).unwrap();
    install_builtin_skills(&repository, directory.path()).unwrap();
    let skills = repository.list_skill_packages().unwrap();
    assert_eq!(skills.len(), 3);
    assert!(skills.iter().all(|skill| skill.version == "1.0.0"));
    assert!(
        skills
            .iter()
            .all(|skill| skill.sha256.len() == 64 && skill.enabled)
    );
}

#[test]
fn enabling_a_skill_version_disables_other_versions_with_the_same_name() {
    let repository = omicsops_adapters::persistence::Repository::open_in_memory().unwrap();
    let mut first = omicsops_core::workspace::SkillPackage {
        id: Uuid::new_v4(),
        name: "scrna-qc".into(),
        version: "1.0.0".into(),
        source_path: "one".into(),
        sha256: "1".repeat(64),
        enabled: true,
        capabilities: vec![],
    };
    let second = omicsops_core::workspace::SkillPackage {
        id: Uuid::new_v4(),
        name: "scrna-qc".into(),
        version: "2.0.0".into(),
        source_path: "two".into(),
        sha256: "2".repeat(64),
        enabled: false,
        capabilities: vec![],
    };
    repository.save_skill_package(&first).unwrap();
    repository.save_skill_package(&second).unwrap();
    set_skill_enabled_in_repository(&repository, second.id, true).unwrap();
    let packages = repository.list_skill_packages().unwrap();
    first = packages
        .into_iter()
        .find(|skill| skill.id == first.id)
        .unwrap();
    assert!(!first.enabled);
    assert!(
        repository
            .list_skill_packages()
            .unwrap()
            .into_iter()
            .find(|skill| skill.id == second.id)
            .unwrap()
            .enabled
    );
}

#[test]
fn application_restart_marks_running_exploration_kernels_interrupted() {
    let repository = omicsops_adapters::persistence::Repository::open_in_memory().unwrap();
    let mut session = omicsops_agent::KernelSession::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        omicsops_agent::KernelLanguage::Python,
    );
    session.start().unwrap();
    session.save_cell("x = 1");
    repository
        .put_json("kernel_session", &session.id.to_string(), &session)
        .unwrap();
    assert_eq!(mark_orphaned_kernels_interrupted(&repository).unwrap(), 1);
    let restored: omicsops_agent::KernelSession = repository
        .get_json("kernel_session", &session.id.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(restored.state, omicsops_agent::KernelState::Interrupted);
    assert_eq!(restored.rebuild_cells(), vec!["x = 1"]);
}

#[test]
fn only_saved_exploration_cells_can_be_promoted_to_formal_steps() {
    let repository = omicsops_adapters::persistence::Repository::open_in_memory().unwrap();
    let mut session = omicsops_agent::KernelSession::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        omicsops_agent::KernelLanguage::Python,
    );
    session.start().unwrap();
    session.record_executed_cell("x = 1", true).unwrap();
    session.record_executed_cell("print(x)", false).unwrap();
    repository
        .put_json("kernel_session", &session.id.to_string(), &session)
        .unwrap();

    let proposal = promote_saved_kernel_cell(
        &repository,
        &PromoteKernelCellRequest {
            session_id: session.id,
            cell_index: 0,
            name: "prepare inputs".into(),
            version: 1,
        },
    )
    .unwrap();
    assert_eq!(proposal.code, "x = 1");
    assert_eq!(proposal.code_sha256.len(), 64);
    assert!(
        promote_saved_kernel_cell(
            &repository,
            &PromoteKernelCellRequest {
                session_id: session.id,
                cell_index: 1,
                name: "ephemeral output".into(),
                version: 1,
            },
        )
        .is_err()
    );
}
