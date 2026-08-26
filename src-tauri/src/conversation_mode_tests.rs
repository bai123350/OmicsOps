use chrono::Utc;
use omicsops_core::workspace::{Conversation, Project, ProjectTemplate};
use omicsops_dto::{SessionAgentModeV4, SetConversationAgentModeRequestV4};
use omicsops_store::Store;
use uuid::Uuid;

use crate::conversation_mode::{
    get_conversation_agent_mode_response, set_conversation_agent_mode_response,
};

async fn fixture() -> (Store, Project, Conversation, Project) {
    let store = Store::open_in_memory().await.unwrap();
    let project = Project::new(
        Uuid::new_v4(),
        "mode project",
        r"C:\data\mode-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    let other_project = Project::new(
        Uuid::new_v4(),
        "other project",
        r"C:\data\other-project",
        ProjectTemplate::Blank,
        Utc::now(),
    );
    store.save_project(&project).await.unwrap();
    store.save_project(&other_project).await.unwrap();
    let conversation =
        Conversation::new(Uuid::new_v4(), project.id, "mode conversation", Utc::now());
    store.save_conversation(&conversation).await.unwrap();
    (store, project, conversation, other_project)
}

#[tokio::test]
async fn command_helpers_return_boundary_response_and_preserve_snake_case() {
    let (store, project, conversation, _) = fixture().await;
    let initial = get_conversation_agent_mode_response(&store, project.id, conversation.id)
        .await
        .unwrap();
    assert_eq!(initial.mode, SessionAgentModeV4::Agent);

    let set = set_conversation_agent_mode_response(
        &store,
        SetConversationAgentModeRequestV4 {
            project_id: project.id,
            conversation_id: conversation.id,
            mode: SessionAgentModeV4::Plan,
        },
    )
    .await
    .unwrap();
    assert_eq!(set.mode, SessionAgentModeV4::Plan);
    let json = serde_json::to_value(set).unwrap();
    assert!(json.get("project_id").is_some());
    assert!(json.get("conversation_id").is_some());
    assert_eq!(json["mode"], "plan");
}

#[tokio::test]
async fn command_helpers_reject_cross_project_requests() {
    let (store, _, conversation, other_project) = fixture().await;
    let get_error = get_conversation_agent_mode_response(&store, other_project.id, conversation.id)
        .await
        .unwrap_err();
    assert!(get_error.contains("does not belong"));

    let set_error = set_conversation_agent_mode_response(
        &store,
        SetConversationAgentModeRequestV4 {
            project_id: other_project.id,
            conversation_id: conversation.id,
            mode: SessionAgentModeV4::Plan,
        },
    )
    .await
    .unwrap_err();
    assert!(set_error.contains("does not belong"));
}
