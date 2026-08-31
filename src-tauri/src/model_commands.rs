use omicsops_adapters::{
    credentials::{CredentialVault, credential_account},
    llm::{ModelProbeResult, ProviderProtocol, UnifiedModelClient},
};
use omicsops_core::workspace::{ModelProfile, ModelProviderKind};
use serde::{Deserialize, Serialize};
use tauri::State;
use url::Url;
use uuid::Uuid;

use crate::commands::AppState;
use crate::model_catalog_shared::exact_model_supports_vision;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveModelProfileRequest {
    pub id: Option<Uuid>,
    pub label: String,
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub credential: Option<String>,
    pub context_window_tokens: Option<u32>,
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
    let model = request.model.trim().to_owned();
    let supports_vision = exact_model_supports_vision(provider, &base_url, &model);
    Ok(ModelProfile {
        id,
        label: request.label.trim().into(),
        provider,
        base_url: base_url.to_string(),
        model,
        credential_reference: (provider != ModelProviderKind::Ollama)
            .then(|| credential_account("model", id)),
        supports_tools: true,
        supports_vision,
        context_window_tokens: request.context_window_tokens,
    })
}

#[tauri::command]
pub async fn list_model_profiles(state: State<'_, AppState>) -> Result<Vec<ModelProfile>, String> {
    state
        .repository
        .list_model_profiles()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_model_profile(
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
        .await
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn probe_model_profile(
    state: State<'_, AppState>,
    profile_id: Uuid,
) -> Result<ModelProbeResult, String> {
    let client = client_for_profile(&state, profile_id).await?;
    match client.probe().await {
        Ok(result) => Ok(result),
        Err(error) => {
            let available = client.list_models().await.unwrap_or_default();
            if available.is_empty() {
                Err(error.to_string())
            } else {
                let shown = available
                    .into_iter()
                    .take(20)
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(format!("{error}; available models: {shown}"))
            }
        }
    }
}

#[tauri::command]
pub async fn list_model_profile_models(
    state: State<'_, AppState>,
    profile_id: Uuid,
) -> Result<Vec<String>, String> {
    client_for_profile(&state, profile_id)
        .await?
        .list_models()
        .await
        .map_err(|error| error.to_string())
}

async fn client_for_profile(
    state: &AppState,
    profile_id: Uuid,
) -> Result<UnifiedModelClient, String> {
    let profile = state
        .repository
        .get_model_profile(profile_id)
        .await
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
    UnifiedModelClient::new(
        profile.id,
        protocol,
        Url::parse(&profile.base_url).map_err(|error| error.to_string())?,
        profile.model,
        credential,
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(provider: &str, base_url: &str, model: &str) -> SaveModelProfileRequest {
        SaveModelProfileRequest {
            id: None,
            label: "vision test".into(),
            provider: provider.into(),
            base_url: base_url.into(),
            model: model.into(),
            credential: None,
            context_window_tokens: None,
        }
    }

    #[test]
    fn vision_capability_requires_exact_provider_host_port_and_model_id() {
        assert!(
            model_profile_from_request(request(
                "open_ai_compatible",
                "https://api.openai.com/v1",
                "gpt-4o"
            ))
            .unwrap()
            .supports_vision
        );
        for candidate in [
            request("open_ai_compatible", "https://gateway.example/v1", "gpt-4o"),
            request(
                "open_ai_compatible",
                "https://api.openai.com/v1",
                "gpt-4o-custom",
            ),
            request("anthropic", "https://api.openai.com/v1", "gpt-4o"),
        ] {
            assert!(
                !model_profile_from_request(candidate)
                    .unwrap()
                    .supports_vision
            );
        }
    }
}
