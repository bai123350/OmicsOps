use omicsops_adapters::{
    credentials::{CredentialVault, credential_account},
    llm::{ProviderProtocol, UnifiedModelClient},
};
use omicsops_agent::{ModelMessage, ModelRequest};
use omicsops_core::workspace::{ModelProfile, ModelProviderKind};
use serde::{Deserialize, Serialize};
use tauri::State;
use url::Url;
use uuid::Uuid;

use crate::commands::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveModelProfileRequest {
    pub id: Option<Uuid>,
    pub label: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub credential: Option<String>,
}

pub fn model_profile_from_request(
    request: SaveModelProfileRequest,
) -> Result<ModelProfile, String> {
    if request.label.trim().is_empty() || request.model.trim().is_empty() {
        return Err("model label and model name are required".into());
    }
    let base_url = Url::parse(request.base_url.trim()).map_err(|error| error.to_string())?;
    if !matches!(base_url.scheme(), "http" | "https") {
        return Err("model base URL must use http or https".into());
    }
    let provider = match request.provider.as_str() {
        "anthropic" => ModelProviderKind::Anthropic,
        "open_ai_compatible" => ModelProviderKind::OpenAiCompatible,
        "ollama" => ModelProviderKind::Ollama,
        other => return Err(format!("unsupported model provider: {other}")),
    };
    let id = request.id.unwrap_or_else(Uuid::new_v4);
    Ok(ModelProfile {
        id,
        label: request.label.trim().into(),
        provider,
        base_url: base_url.to_string(),
        model: request.model.trim().into(),
        credential_reference: (provider != ModelProviderKind::Ollama)
            .then(|| credential_account("model", id)),
        supports_tools: true,
        supports_vision: false,
    })
}

#[tauri::command]
pub fn list_model_profiles(state: State<'_, AppState>) -> Result<Vec<ModelProfile>, String> {
    state
        .repository
        .list_model_profiles()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_model_profile(
    state: State<'_, AppState>,
    request: SaveModelProfileRequest,
) -> Result<ModelProfile, String> {
    let credential = request.credential.clone();
    let profile = model_profile_from_request(request)?;
    if let (Some(reference), Some(secret)) = (&profile.credential_reference, credential) {
        if !secret.trim().is_empty() {
            state
                .credentials
                .set(reference, &secret)
                .map_err(|error| error.to_string())?;
        }
    }
    state
        .repository
        .save_model_profile(&profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn probe_model_profile(
    state: State<'_, AppState>,
    profile_id: Uuid,
) -> Result<(), String> {
    let profile = state
        .repository
        .get_model_profile(profile_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "model profile not found".to_string())?;
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
    let mut completed = false;
    client
        .stream_with(
            ModelRequest {
                system: "Reply briefly to confirm the model is available.".into(),
                messages: vec![ModelMessage {
                    role: "user".into(),
                    content: "Reply with OK.".into(),
                }],
                tool_name: None,
                tool_schema: None,
            },
            |event| completed |= matches!(event, omicsops_agent::ModelStreamEvent::Completed),
        )
        .await
        .map_err(|error| error.to_string())?;
    if completed {
        Ok(())
    } else {
        Err("model stream ended without a completion event".into())
    }
}
