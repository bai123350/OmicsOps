use chrono::{Duration, Utc};
use omicsops_core::workspace::{Artifact, Conversation, Message, MessageRole, Project, ProjectTemplate, SkillPackage};
use omicsops_dto::*;
use omicsops_store::{Store, StoreError};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{composer_references::{composer_reference_catalog_for_repository, resolve_composer_references}, workspace_commands::*, workspace_navigation::{resolve_source, save_publication}};

async fn fixture() -> (Store, Project, Conversation, Conversation) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(Uuid::new_v4(), "visibility", "fixture-only", ProjectTemplate::Blank, Utc::now());
    store.save_project(&project).await.unwrap();
    let current = Conversation::new(Uuid::new_v4(), project.id, "current", Utc::now());
    let saved = Conversation::new(Uuid::new_v4(), project.id, "QC and clustering", Utc::now());
    store.save_conversation(&current).await.unwrap();
    store.save_conversation(&saved).await.unwrap();
    (store, project, current, saved)
}

fn request(project_id: Uuid, conversation_id: Uuid, revision: u64, visibility: ConversationVisibilityV1) -> SetConversationVisibilityRequestV1 {
    SetConversationVisibilityRequestV1 {request_id: Uuid::new_v4(), project_id, conversation_id, expected_revision: revision, visibility}
}

fn list_request(project_id: Uuid) -> ConversationVisibilityListRequestV1 {
    ConversationVisibilityListRequestV1 {project_id, visibility: ConversationVisibilityFilterV1::Visible, query: String::new(), offset: 0, limit: 50}
}

#[tokio::test]
async fn visibility_catalog_vs_resolve() {
    let (store, project, current, saved) = fixture().await;
    let message = Message::markdown(Uuid::new_v4(), project.id, saved.id, 0, MessageRole::User, "Saved observation", Utc::now());
    store.save_message(&message).await.unwrap();
    let artifact = Artifact {id: Uuid::new_v4(), project_id: project.id, run_id: None, relative_path: "result.txt".into(), remote_path: None, media_type: "text/plain".into(), size_bytes: 1, sha256: "a".repeat(64), verified: true, created_at: Utc::now()};
    store.save_artifact_v3(&artifact).await.unwrap();
    let skill_root = tempfile::tempdir().unwrap();
    std::fs::write(skill_root.path().join("SKILL.md"), "# Reference skill\nRead saved evidence.\n").unwrap();
    let skill = SkillPackage {id: Uuid::new_v4(), name: "reference skill".into(), version: "1".into(), source_path: skill_root.path().to_string_lossy().into_owned(), sha256: String::new(), enabled: true, capabilities: vec![], category: None};
    store.save_skill_package(&skill).await.unwrap();
    let source = WorkspaceSourceRef {project_id: project.id, kind: SourceKind::Message, id: message.id.to_string(), conversation_id: Some(saved.id), run_id: None, start: Some(0), end: Some(message.markdown.len()), content_sha256: Some(hex::encode(Sha256::digest(message.markdown.as_bytes()))), sequence: None, event_hash: None};
    let publication_request = SavePublicationRequest {request_id: Uuid::new_v4(), project_id: project.id, publication_id: None, expected_revision: 0, title: "Draft".into(), markdown: "Draft text".into(), sources: vec![source.clone()]};
    let publication = save_publication(&store, publication_request.clone()).await.unwrap();
    let before = composer_reference_catalog_for_repository(&store, project.id).await.unwrap();
    let session = ComposerReference::Session {project_id: project.id, id: saved.id};
    assert!(before.iter().any(|item| item.reference == session));
    workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 0, ConversationVisibilityV1::Shelved)).await.unwrap();
    let after = composer_reference_catalog_for_repository(&store, project.id).await.unwrap();
    assert!(!after.iter().any(|item| item.reference == session));
    let non_session = |items: Vec<ComposerCatalogItem>| items.into_iter().filter(|item| !matches!(item.reference, ComposerReference::Session {..})).collect::<Vec<_>>();
    assert_eq!(non_session(before), non_session(after));
    assert!(resolve_composer_references(&store, project.id, current.id, &[session]).await.unwrap().contains("Saved observation"));
    assert!(resolve_source(&store, &source).await.is_ok());
    let replay = save_publication(&store, publication_request).await.unwrap();
    assert_eq!(replay.publication.id, publication.publication.id);
    assert_eq!(replay.revisions[0].references[0].availability, SourceAvailability::Available);
    // Shelved sessions can also own the active request, rather than merely its source.
    assert!(resolve_composer_references(&store, project.id, saved.id, &[]).await.is_ok());
    // A malformed hidden transcript must never be loaded during discovery.
    sqlx::query("UPDATE messages SET role='invalid-role' WHERE id=?").bind(message.id.to_string()).execute(store.pool()).await.unwrap();
    assert!(composer_reference_catalog_for_repository(&store, project.id).await.is_ok());
}

#[tokio::test]
async fn visibility_direct_getter_default_and_foreign_owner() {
    let (store, project, _, mut saved) = fixture().await;
    saved.title = "password=fixture-secret title".into();
    store.save_conversation(&saved).await.unwrap();
    let state = workspace_get_conversation_visibility_response(&store, project.id, saved.id).await.unwrap();
    assert_eq!(state.revision, 0);
    assert_eq!(state.visibility, ConversationVisibilityV1::Visible);
    assert_eq!(state.changed_at, None);
    workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 0, ConversationVisibilityV1::Shelved)).await.unwrap();
    let direct = workspace_get_conversation_response(&store, project.id, saved.id).await.unwrap();
    assert_eq!(direct.id, saved.id);
    assert!(!direct.title.contains("fixture-secret"));
    assert_eq!(store.workspace_source_conversation(project.id, saved.id).await.unwrap().unwrap().title, saved.title);
    let foreign = Project::new(Uuid::new_v4(), "foreign", "fixture-only", ProjectTemplate::Blank, Utc::now());
    store.save_project(&foreign).await.unwrap();
    assert!(workspace_get_conversation_response(&store, foreign.id, saved.id).await.is_err());
    assert_eq!(workspace_get_conversation_visibility_response(&store, foreign.id, saved.id).await.unwrap_err().code, ConversationVisibilityErrorCodeV1::OwnershipMismatch);
    assert_eq!(workspace_get_conversation_visibility_response(&store, project.id, Uuid::new_v4()).await.unwrap_err().code, ConversationVisibilityErrorCodeV1::NotFound);
    assert_eq!(workspace_get_conversation_response(&store, Uuid::new_v4(), saved.id).await.unwrap_err(), "project was not found");
}

#[tokio::test]
async fn visibility_command_shape_and_errors() {
    let (store, project, _, saved) = fixture().await;
    let page = workspace_list_conversation_visibility_response(&store, list_request(project.id)).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(serde_json::to_value(&page).unwrap()["items"][0].get("model_profile_id").is_none());
    let shelf = request(project.id, saved.id, 0, ConversationVisibilityV1::Shelved);
    let receipt = workspace_set_conversation_visibility_response(&store, shelf.clone()).await.unwrap();
    workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 1, ConversationVisibilityV1::Visible)).await.unwrap();
    assert_eq!(workspace_set_conversation_visibility_response(&store, shelf.clone()).await.unwrap(), receipt);
    assert_eq!(workspace_get_conversation_visibility_response(&store, project.id, saved.id).await.unwrap().revision, 2);
    let mut conflict = shelf;
    conflict.visibility = ConversationVisibilityV1::Visible;
    assert_eq!(workspace_set_conversation_visibility_response(&store, conflict).await.unwrap_err().code, ConversationVisibilityErrorCodeV1::RequestConflict);
    assert_eq!(workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 0, ConversationVisibilityV1::Visible)).await.unwrap_err().code, ConversationVisibilityErrorCodeV1::RevisionConflict);
    let mut invalid = list_request(project.id);
    invalid.limit = 0;
    assert_eq!(workspace_list_conversation_visibility_response(&store, invalid).await.unwrap_err().code, ConversationVisibilityErrorCodeV1::InvalidInput);
    for (tag, code) in [("invalid_input", ConversationVisibilityErrorCodeV1::InvalidInput), ("not_found", ConversationVisibilityErrorCodeV1::NotFound), ("ownership_mismatch", ConversationVisibilityErrorCodeV1::OwnershipMismatch), ("revision_conflict", ConversationVisibilityErrorCodeV1::RevisionConflict), ("request_conflict", ConversationVisibilityErrorCodeV1::RequestConflict)] {
        assert_eq!(conversation_visibility_error(StoreError::InvalidInput(format!("conversation_visibility_{tag}"))).code, code);
    }
    let error = conversation_visibility_error(StoreError::Migration("raw SQL password=fixture-secret".into()));
    assert_eq!(error.code, ConversationVisibilityErrorCodeV1::StorageFailed);
    assert!(!serde_json::to_string(&error).unwrap().contains("fixture-secret"));
    sqlx::query("DROP TABLE conversation_visibility_v4").execute(store.pool()).await.unwrap();
    assert_eq!(workspace_list_conversation_visibility_response(&store, list_request(project.id)).await.unwrap_err(), error);
    assert_eq!(workspace_get_conversation_visibility_response(&store, project.id, saved.id).await.unwrap_err(), error);
    assert_eq!(workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 2, ConversationVisibilityV1::Shelved)).await.unwrap_err(), error);
}

#[tokio::test]
async fn visibility_no_runtime_effect() {
    let (store, project, _, saved) = fixture().await;
    let before = serde_json::to_value(store.workspace_source_conversation(project.id, saved.id).await.unwrap()).unwrap();
    workspace_list_conversation_visibility_response(&store, list_request(project.id)).await.unwrap();
    workspace_get_conversation_response(&store, project.id, saved.id).await.unwrap();
    workspace_get_conversation_visibility_response(&store, project.id, saved.id).await.unwrap();
    workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 0, ConversationVisibilityV1::Shelved)).await.unwrap();
    assert_eq!(serde_json::to_value(store.workspace_source_conversation(project.id, saved.id).await.unwrap()).unwrap(), before);
    // Concrete structural capability boundary: every command delegates only its Store,
    // and its response helper accepts Store, with no runtime/credential/SSH capability.
    let source = include_str!("workspace_commands.rs");
    for name in ["workspace_list_conversation_visibility", "workspace_set_conversation_visibility", "workspace_get_conversation", "workspace_get_conversation_visibility"] {
        let body = function_body(source, name);
        assert_eq!(body.matches("state.").count(), 1, "{name}");
        assert!(body.contains("&state.repository"));
        let helper = function_body(source, &format!("{name}_response"));
        for forbidden in ["AppState", "state.", "active_runs", "kernel", "credentials", "ssh", "mcp", "agent_v4", "restore"] { assert!(!helper.contains(forbidden), "{name}: {forbidden}"); }
        assert!(include_str!("lib.rs").contains(&format!("workspace_commands::{name},")));
    }
    // The existing presentation command alone retains placeholder-title repair.
    let visible = list_conversations_response(&store, project.id).await.unwrap();
    assert!(!visible.iter().any(|item| item.id == saved.id));
    workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 1, ConversationVisibilityV1::Visible)).await.unwrap();
    store.save_message(&Message::markdown(Uuid::new_v4(), project.id, saved.id, 0, MessageRole::User, "First question", Utc::now())).await.unwrap();
    assert_eq!(workspace_get_conversation_response(&store, project.id, saved.id).await.unwrap().title, "QC and clustering");
    assert_eq!(list_conversations_response(&store, project.id).await.unwrap().into_iter().find(|item| item.id == saved.id).unwrap().title, "First question");
}

fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source.find(&format!("fn {name}(" )).unwrap();
    let body_start = start + source[start..].find('{').unwrap();
    let mut depth = 0;
    for (index, ch) in source[body_start..].char_indices() { match ch { '{' => depth += 1, '}' => { depth -= 1; if depth == 0 {return &source[body_start..=body_start+index];}}, _ => {} } }
    panic!("unclosed function {name}");
}

#[tokio::test]
async fn visibility_latest_default() {
    let (store, project, current, saved) = fixture().await;
    assert!(latest_used_conversation_response(&store, project.id).await.unwrap().is_none());
    let now = Utc::now();
    for (conversation, sequence, role, time) in [(saved.id, 0, MessageRole::User, now-Duration::seconds(20)), (current.id, 0, MessageRole::User, now-Duration::seconds(10)), (saved.id, 1, MessageRole::Tool, now)] {store.save_message(&Message::markdown(Uuid::new_v4(), project.id, conversation, sequence, role, "turn", time)).await.unwrap();}
    assert_eq!(store.latest_used_conversation(project.id).await.unwrap().unwrap().id, saved.id);
    assert_eq!(latest_used_conversation_response(&store, project.id).await.unwrap().unwrap().id, current.id);
    workspace_set_conversation_visibility_response(&store, request(project.id, current.id, 0, ConversationVisibilityV1::Shelved)).await.unwrap();
    assert_eq!(latest_used_conversation_response(&store, project.id).await.unwrap().unwrap().id, saved.id);
    workspace_set_conversation_visibility_response(&store, request(project.id, saved.id, 0, ConversationVisibilityV1::Shelved)).await.unwrap();
    assert!(latest_used_conversation_response(&store, project.id).await.unwrap().is_none());
}

// Type-check exact native response shapes without constructing runtime capabilities.
#[allow(dead_code)]
async fn native_shapes(state: tauri::State<'_, crate::commands::AppState>, list: ConversationVisibilityListRequestV1, set: SetConversationVisibilityRequestV1, project: Uuid, conversation: Uuid) {
    let _: Result<ConversationVisibilityPageV1, ConversationVisibilityErrorV1> = workspace_list_conversation_visibility(state.clone(), list).await;
    let _: Result<ConversationVisibilityReceiptV1, ConversationVisibilityErrorV1> = workspace_set_conversation_visibility(state.clone(), set).await;
    let _: Result<Conversation, String> = workspace_get_conversation(state.clone(), project, conversation).await;
    let _: Result<ConversationVisibilityRecordV1, ConversationVisibilityErrorV1> = workspace_get_conversation_visibility(state, project, conversation).await;
}
