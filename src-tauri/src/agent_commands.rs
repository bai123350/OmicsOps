use chrono::{DateTime, Utc};
use omicsops_adapters::{
    credentials::CredentialVault,
    llm::{ProviderProtocol, UnifiedModelClient},
};
use omicsops_agent::{
    AgentEvent, AgentEventKind, ModelMessage, ModelRequest, ModelStreamEvent, ToolArgumentBuffer,
};
use omicsops_core::workspace::{AgentTurn, Message, MessageRole, ModelProviderKind, TurnStatus};
use omicsops_core::{
    plan_v2::{AnalysisPlanV2, canonical_plan_hash},
    tools::builtin_tool_catalog,
    validation::{PlanValidation, validate_plan_v2},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use url::Url;
use uuid::Uuid;

use crate::commands::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitMessageRequest {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub markdown: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversationEvent {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub message: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAgentTurnRequest {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub model_profile_id: Uuid,
    pub markdown: String,
    pub message_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposePlanRequest {
    pub project_id: Uuid,
    pub model_profile_id: Uuid,
    pub goal: String,
    pub environment_summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanProposal {
    pub plan: AnalysisPlanV2,
    pub validation: PlanValidation,
    pub plan_hash: String,
}

pub fn agent_event_kind_from_model_event(event: ModelStreamEvent) -> AgentEventKind {
    match event {
        ModelStreamEvent::TextDelta(text) => AgentEventKind::TextDelta(text),
        ModelStreamEvent::ToolArgumentsDelta {
            name,
            json_fragment,
        } => AgentEventKind::ToolArgumentsDelta {
            name,
            json_fragment,
        },
        ModelStreamEvent::Completed => AgentEventKind::TurnCompleted,
    }
}

pub fn user_message_from_request(
    request: SubmitMessageRequest,
    id: Uuid,
    now: DateTime<Utc>,
) -> Result<Message, String> {
    let markdown = request.markdown.trim();
    if markdown.is_empty() {
        return Err("message cannot be empty".into());
    }
    Ok(Message::markdown(
        id,
        request.project_id,
        request.conversation_id,
        request.sequence,
        MessageRole::User,
        markdown,
        now,
    ))
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    conversation_id: Uuid,
) -> Result<Vec<Message>, String> {
    state
        .repository
        .messages_for_conversation(conversation_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn submit_message(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SubmitMessageRequest,
) -> Result<Message, String> {
    let message = user_message_from_request(request, Uuid::new_v4(), Utc::now())?;
    state
        .repository
        .save_message(&message)
        .map_err(|error| error.to_string())?;
    app.emit(
        "conversation-event",
        ConversationEvent {
            project_id: message.project_id,
            conversation_id: message.conversation_id,
            message: message.clone(),
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(message)
}

#[tauri::command]
pub async fn run_agent_turn(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RunAgentTurnRequest,
) -> Result<Uuid, String> {
    let repository = state.repository.clone();
    let credentials = state.credentials;
    let profile = repository
        .get_model_profile(request.model_profile_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "model profile not found".to_string())?;
    let project = repository
        .get_project(request.project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    if project.ollama_only && profile.provider != ModelProviderKind::Ollama {
        return Err("this project permits Ollama models only".into());
    }
    let credential = match &profile.credential_reference {
        Some(reference) => credentials
            .get(reference)
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let protocol = match profile.provider {
        ModelProviderKind::Anthropic => ProviderProtocol::Anthropic,
        ModelProviderKind::OpenAiCompatible => ProviderProtocol::OpenAiCompatible,
        ModelProviderKind::Ollama => ProviderProtocol::Ollama,
    };
    let client = UnifiedModelClient::new(
        profile.id,
        protocol,
        Url::parse(&profile.base_url).map_err(|error| error.to_string())?,
        profile.model,
        credential,
    )
    .map_err(|error| error.to_string())?;

    let user_message = user_message_from_request(
        SubmitMessageRequest {
            project_id: request.project_id,
            conversation_id: request.conversation_id,
            markdown: request.markdown.clone(),
            sequence: request.message_sequence,
        },
        Uuid::new_v4(),
        Utc::now(),
    )?;
    repository
        .save_message(&user_message)
        .map_err(|error| error.to_string())?;
    app.emit(
        "conversation-event",
        ConversationEvent {
            project_id: request.project_id,
            conversation_id: request.conversation_id,
            message: user_message,
        },
    )
    .map_err(|error| error.to_string())?;

    let turn_id = Uuid::new_v4();
    let mut turn = AgentTurn {
        id: turn_id,
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        status: TurnStatus::Streaming,
        model_profile_id: profile.id,
        started_at: Utc::now(),
        finished_at: None,
    };
    repository
        .save_agent_turn(&turn)
        .map_err(|error| error.to_string())?;
    let started = AgentEvent {
        project_id: request.project_id,
        conversation_id: request.conversation_id,
        turn_id,
        sequence: 0,
        occurred_at: Utc::now(),
        event: AgentEventKind::TurnStarted,
    };
    repository
        .append_agent_event(&started)
        .map_err(|error| error.to_string())?;
    app.emit("agent-event", &started)
        .map_err(|error| error.to_string())?;

    let mut sequence = 1_u64;
    let mut assistant_markdown = String::new();
    let mut callback_error: Option<String> = None;
    let stream_result = client.stream_with(
        ModelRequest {
            system: "You are OmicsOps, a careful life-science research assistant. Clarify assumptions, cite evidence identifiers when available, and never claim that code or remote work ran unless a tool event confirms it.".into(),
            messages: vec![ModelMessage { role: "user".into(), content: request.markdown }],
            tool_name: None,
            tool_schema: None,
        },
        |model_event| {
            if let ModelStreamEvent::TextDelta(text) = &model_event {
                assistant_markdown.push_str(text);
            }
            let event = AgentEvent {
                project_id: request.project_id,
                conversation_id: request.conversation_id,
                turn_id,
                sequence,
                occurred_at: Utc::now(),
                event: agent_event_kind_from_model_event(model_event),
            };
            sequence += 1;
            if let Err(error) = repository.append_agent_event(&event) {
                callback_error.get_or_insert_with(|| error.to_string());
            } else if let Err(error) = app.emit("agent-event", &event) {
                callback_error.get_or_insert_with(|| error.to_string());
            }
        },
    ).await;

    if let Some(error) = callback_error {
        turn.status = TurnStatus::Failed;
        turn.finished_at = Some(Utc::now());
        repository
            .save_agent_turn(&turn)
            .map_err(|save_error| save_error.to_string())?;
        return Err(error);
    }
    if let Err(error) = stream_result {
        let failed = AgentEvent {
            project_id: request.project_id,
            conversation_id: request.conversation_id,
            turn_id,
            sequence,
            occurred_at: Utc::now(),
            event: AgentEventKind::TurnFailed {
                message: error.to_string(),
            },
        };
        repository
            .append_agent_event(&failed)
            .map_err(|save_error| save_error.to_string())?;
        app.emit("agent-event", &failed)
            .map_err(|emit_error| emit_error.to_string())?;
        turn.status = TurnStatus::Failed;
        turn.finished_at = Some(Utc::now());
        repository
            .save_agent_turn(&turn)
            .map_err(|save_error| save_error.to_string())?;
        return Err(error.to_string());
    }

    if !assistant_markdown.is_empty() {
        let assistant = Message::markdown(
            Uuid::new_v4(),
            request.project_id,
            request.conversation_id,
            request.message_sequence + 1,
            MessageRole::Assistant,
            assistant_markdown,
            Utc::now(),
        );
        repository
            .save_message(&assistant)
            .map_err(|error| error.to_string())?;
        app.emit(
            "conversation-event",
            ConversationEvent {
                project_id: request.project_id,
                conversation_id: request.conversation_id,
                message: assistant,
            },
        )
        .map_err(|error| error.to_string())?;
    }
    turn.status = TurnStatus::Succeeded;
    turn.finished_at = Some(Utc::now());
    repository
        .save_agent_turn(&turn)
        .map_err(|error| error.to_string())?;
    Ok(turn_id)
}

#[tauri::command]
pub async fn propose_analysis_plan(
    state: State<'_, AppState>,
    request: ProposePlanRequest,
) -> Result<PlanProposal, String> {
    if request.goal.trim().is_empty() {
        return Err("analysis goal is required".into());
    }
    let repository = state.repository.clone();
    let profile = repository
        .get_model_profile(request.model_profile_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "model profile not found".to_string())?;
    let project = repository
        .get_project(request.project_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "project not found".to_string())?;
    if project.ollama_only && profile.provider != ModelProviderKind::Ollama {
        return Err("this project permits Ollama models only".into());
    }
    let credential = match &profile.credential_reference {
        Some(reference) => state
            .credentials
            .get(reference)
            .map_err(|error| error.to_string())?,
        None => None,
    };
    let protocol = match profile.provider {
        ModelProviderKind::Anthropic => ProviderProtocol::Anthropic,
        ModelProviderKind::OpenAiCompatible => ProviderProtocol::OpenAiCompatible,
        ModelProviderKind::Ollama => ProviderProtocol::Ollama,
    };
    let client = UnifiedModelClient::new(
        profile.id,
        protocol,
        Url::parse(&profile.base_url).map_err(|error| error.to_string())?,
        profile.model,
        credential,
    )
    .map_err(|error| error.to_string())?;
    let catalog = builtin_tool_catalog().map_err(|error| error.to_string())?;
    let tool_summaries =
        serde_json::to_string_pretty(&catalog.summaries()).map_err(|error| error.to_string())?;
    let schema = serde_json::to_value(schemars::schema_for!(AnalysisPlanV2))
        .map_err(|error| error.to_string())?;
    let mut buffer = ToolArgumentBuffer::new("submit_analysis_plan_v2");
    let mut buffer_error: Option<String> = None;
    client.stream_with(ModelRequest {
        system: format!("Create a schema-valid OmicsOps AnalysisPlanV2. Use only the supplied versioned tools, relative project paths, explicit resource limits, verifications for every artifact, and no legacy shell. Available tools:\n{tool_summaries}"),
        messages: vec![ModelMessage { role: "user".into(), content: format!("Project: {}\nGoal: {}\nEnvironment: {}", project.name, request.goal.trim(), request.environment_summary.trim()) }],
        tool_name: Some("submit_analysis_plan_v2".into()),
        tool_schema: Some(schema),
    }, |event| {
        if let Err(error) = buffer.push(event) {
            buffer_error.get_or_insert_with(|| error.to_string());
        }
    }).await.map_err(|error| error.to_string())?;
    if let Some(error) = buffer_error {
        return Err(error);
    }
    let plan: AnalysisPlanV2 =
        serde_json::from_value(buffer.finish().map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let validation = validate_plan_v2(&plan, &catalog);
    let plan_hash = canonical_plan_hash(&plan).map_err(|error| error.to_string())?;
    repository
        .put_json("analysis_plan_v2_draft", &plan.id.to_string(), &plan)
        .map_err(|error| error.to_string())?;
    Ok(PlanProposal {
        plan,
        validation,
        plan_hash,
    })
}
