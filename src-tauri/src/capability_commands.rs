use std::collections::BTreeSet;

use omicsops_store::Store;
use tauri::State;

use crate::{
    commands::AppState,
    dto::{
        ConversationCapabilitiesV4, ConversationMcpCapabilityV4, ConversationSkillCapabilityV4,
        GetConversationCapabilitiesV4Request,
    },
    p1_commands::McpServerProfile,
    skill_commands::agent_skill_packages,
};

#[tauri::command]
pub async fn get_conversation_capabilities_v4(
    state: State<'_, AppState>,
    request: GetConversationCapabilitiesV4Request,
) -> Result<ConversationCapabilitiesV4, String> {
    conversation_capabilities(&state.repository, &request).await
}

/// Read persisted availability without inspecting MCP servers or changing approvals.
async fn conversation_capabilities(
    repository: &Store,
    request: &GetConversationCapabilitiesV4Request,
) -> Result<ConversationCapabilitiesV4, String> {
    let conversations = repository
        .conversations_for_project(request.project_id)
        .await
        .map_err(|error| error.to_string())?;
    if !conversations
        .iter()
        .any(|item| item.id == request.conversation_id)
    {
        return Err("conversation does not belong to the requested project".into());
    }
    let selected = agent_skill_packages(repository)
        .await?
        .into_iter()
        .map(|skill| skill.id)
        .collect::<BTreeSet<_>>();
    let mut skills = repository
        .list_skill_packages()
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|skill| ConversationSkillCapabilityV4 {
            id: skill.id,
            name: skill.name,
            enabled: selected.contains(&skill.id),
        })
        .collect::<Vec<_>>();
    skills.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    let mut mcp_servers = repository
        .list_json::<McpServerProfile>("mcp_server")
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|profile| {
            // Match V4 indexing: unnamed/malformed tool entries are not discoverable.
            let tool_count = profile
                .tools
                .iter()
                .filter(|tool| {
                    tool.get("name")
                        .and_then(serde_json::Value::as_str)
                        .is_some()
                })
                .count();
            ConversationMcpCapabilityV4 {
                id: profile.id,
                name: profile.name,
                enabled: profile.enabled && tool_count > 0,
                tool_count,
            }
        })
        .collect::<Vec<_>>();
    mcp_servers.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    let project = repository
        .get_project(request.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    let memory_count =
        crate::project_memory::files(std::path::Path::new(&project.local_root))?.len();
    Ok(ConversationCapabilitiesV4 {
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        skills,
        mcp_servers,
        memory_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use omicsops_core::workspace::{
        Conversation, Message, MessageRole, Project, ProjectTemplate, SkillPackage,
    };
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn missing_memory_directory_is_zero_and_is_not_created() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(crate::project_memory::files(root.path()).unwrap().len(), 0);
        assert!(!root.path().join(".omicsops").exists());
    }

    async fn fixture() -> (Store, GetConversationCapabilitiesV4Request) {
        let repository = Store::open_in_memory().await.unwrap();
        let project = Project::new(
            Uuid::new_v4(),
            "test",
            "synthetic",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        repository.save_project(&project).await.unwrap();
        let conversation = Conversation::new(Uuid::new_v4(), project.id, "test", Utc::now());
        repository.save_conversation(&conversation).await.unwrap();
        (
            repository,
            GetConversationCapabilitiesV4Request {
                project_id: project.id,
                conversation_id: conversation.id,
            },
        )
    }

    #[tokio::test]
    async fn empty_snapshot_does_not_count_unconfigured_bundled_presets() {
        let (repository, request) = fixture().await;
        let snapshot = conversation_capabilities(&repository, &request)
            .await
            .unwrap();
        assert!(snapshot.skills.is_empty());
        assert!(snapshot.mcp_servers.is_empty());
        assert_eq!(snapshot.memory_count, 0);
    }

    #[tokio::test]
    async fn unknown_and_cross_project_conversations_are_rejected() {
        let (repository, mut request) = fixture().await;
        let original_project_id = request.project_id;
        let other = Project::new(
            Uuid::new_v4(),
            "other",
            "synthetic",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        repository.save_project(&other).await.unwrap();
        request.project_id = other.id;
        assert!(
            conversation_capabilities(&repository, &request)
                .await
                .is_err()
        );
        request.project_id = original_project_id;
        request.conversation_id = Uuid::new_v4();
        assert!(
            conversation_capabilities(&repository, &request)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn skill_availability_includes_dependency_closure_without_mutation() {
        let (repository, request) = fixture().await;
        let root = tempfile::tempdir().unwrap();
        for (name, enabled, markdown) in [
            (
                "workflow",
                true,
                "---\ndepends_on:\n  - dependency\n---\nWorkflow",
            ),
            (
                "dependency",
                false,
                "---\nname: dependency\n---\nDependency",
            ),
            ("disabled", false, "---\nname: disabled\n---\nDisabled"),
        ] {
            let path = root.path().join(name);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("SKILL.md"), markdown).unwrap();
            repository
                .save_skill_package(&SkillPackage {
                    id: Uuid::new_v4(),
                    name: name.into(),
                    version: "1".into(),
                    source_path: path.to_string_lossy().into_owned(),
                    sha256: name.into(),
                    enabled,
                    capabilities: vec![],
                    category: None,
                })
                .await
                .unwrap();
        }
        let snapshot = conversation_capabilities(&repository, &request)
            .await
            .unwrap();
        assert_eq!(
            snapshot
                .skills
                .iter()
                .map(|skill| (skill.name.as_str(), skill.enabled))
                .collect::<Vec<_>>(),
            vec![
                ("dependency", true),
                ("disabled", false),
                ("workflow", true)
            ]
        );
        assert!(
            !repository
                .list_skill_packages()
                .await
                .unwrap()
                .iter()
                .find(|skill| skill.name == "dependency")
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn mcp_counts_only_persisted_named_tools_and_keeps_approval_unchanged() {
        let (repository, request) = fixture().await;
        for (name, enabled, tools) in [
            ("active", true, json!([{"name":"search"}, {"name":2}, {}])),
            ("disabled", false, json!([{"name":"search"}])),
            ("uninspected", true, json!([])),
        ] {
            let id = Uuid::new_v4();
            repository
                .put_json(
                    "mcp_server",
                    &id.to_string(),
                    &json!({
                        "id":id,"name":name,"command":"never-launch-this.exe","enabled":enabled,
                        "tools":tools,"launch_approved":false,"approved_tools":[],
                        "created_at":Utc::now(),"updated_at":Utc::now(),"last_inspected_at":null
                    }),
                )
                .await
                .unwrap();
        }
        let before = repository
            .list_json::<serde_json::Value>("mcp_server")
            .await
            .unwrap();
        let snapshot = conversation_capabilities(&repository, &request)
            .await
            .unwrap();
        assert_eq!(
            snapshot
                .mcp_servers
                .iter()
                .map(|server| (server.name.as_str(), server.enabled, server.tool_count))
                .collect::<Vec<_>>(),
            vec![
                ("active", true, 1),
                ("disabled", false, 1),
                ("uninspected", false, 0)
            ]
        );
        assert_eq!(
            repository
                .list_json::<serde_json::Value>("mcp_server")
                .await
                .unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn memory_counts_project_markdown_files_instead_of_messages() {
        let (repository, request) = fixture().await;
        let root = tempfile::tempdir().unwrap();
        let mut project = repository
            .get_project(request.project_id)
            .await
            .unwrap()
            .unwrap();
        project.local_root = root.path().to_string_lossy().into_owned();
        repository.save_project(&project).await.unwrap();
        let memory = root.path().join(".omicsops").join("memory");
        std::fs::create_dir_all(memory.join("nested")).unwrap();
        for name in [
            "2026-09-11.md",
            "notes.md",
            "methods.md",
            "ignored.txt",
            "ignored.MD",
            "nested/ignored.md",
        ] {
            std::fs::write(memory.join(name), "multiple notes still count as one file").unwrap();
        }
        let sibling = Conversation::new(Uuid::new_v4(), request.project_id, "sibling", Utc::now());
        repository.save_conversation(&sibling).await.unwrap();
        let other = Project::new(
            Uuid::new_v4(),
            "other",
            "synthetic",
            ProjectTemplate::Blank,
            Utc::now(),
        );
        repository.save_project(&other).await.unwrap();
        let external = Conversation::new(Uuid::new_v4(), other.id, "other", Utc::now());
        repository.save_conversation(&external).await.unwrap();
        for (project_id, conversation_id, role) in [
            (
                request.project_id,
                request.conversation_id,
                MessageRole::User,
            ),
            (request.project_id, sibling.id, MessageRole::User),
            (request.project_id, sibling.id, MessageRole::Assistant),
            (other.id, external.id, MessageRole::User),
        ] {
            repository
                .save_message(&Message::markdown(
                    Uuid::new_v4(),
                    project_id,
                    conversation_id,
                    if role == MessageRole::Assistant { 2 } else { 1 },
                    role,
                    "evidence",
                    Utc::now(),
                ))
                .await
                .unwrap();
        }
        let snapshot = conversation_capabilities(&repository, &request)
            .await
            .unwrap();
        assert_eq!(snapshot.memory_count, 3);
        let sibling_snapshot = conversation_capabilities(
            &repository,
            &GetConversationCapabilitiesV4Request {
                project_id: request.project_id,
                conversation_id: sibling.id,
            },
        )
        .await
        .unwrap();
        assert_eq!(sibling_snapshot.memory_count, snapshot.memory_count);
    }
}
