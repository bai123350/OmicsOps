use omicsops_adapters::{
    credentials::{CredentialVault, credential_account},
    llm::{ModelProbeResult, ProviderProtocol, UnifiedModelClient},
};
use omicsops_core::workspace::{ModelProfile, ModelProviderKind};
use tauri::State;
use url::Url;
use uuid::Uuid;

use crate::commands::AppState;
use crate::model_catalog_shared::exact_model_supports_vision;

pub use omicsops_dto::SaveModelProfileRequest;

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
    if request.delegated_model_profile_id.flatten() == Some(id) {
        return Err("choose another delegated profile or inherit the main model".into());
    }
    let model = request.model.trim().to_owned();
    let reasoning_effort = request.reasoning_effort.flatten();
    omicsops_core::workspace::validate_reasoning_effort(provider, reasoning_effort.as_deref())?;
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
        reasoning_effort,
        delegated_model_profile_id: request.delegated_model_profile_id.flatten(),
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
    let preserve_binding = request.delegated_model_profile_id.is_none();
    let preserve_window = request.context_window_tokens.is_none();
    let preserve_effort = request.reasoning_effort.is_none();
    let mut profile = model_profile_from_request(request)?;
    if preserve_binding || preserve_window || preserve_effort {
        if let Some(existing) = state
            .repository
            .get_model_profile(profile.id)
            .await
            .map_err(|error| error.to_string())?
        {
            if preserve_binding {
                profile.delegated_model_profile_id = existing.delegated_model_profile_id;
            }
            if preserve_window {
                profile.context_window_tokens = existing.context_window_tokens;
            }
            if preserve_effort {
                profile.reasoning_effort = existing.reasoning_effort;
            }
        }
    }
    omicsops_core::workspace::validate_reasoning_effort(
        profile.provider,
        profile.reasoning_effort.as_deref(),
    )?;
    if let Some(child_id) = profile.delegated_model_profile_id {
        let child = state
            .repository
            .get_model_profile(child_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or("delegated model profile not found")?;
        if !child.supports_tools {
            return Err("delegated model must support tools".into());
        }
    }
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
    .and_then(|client| client.with_reasoning_effort(profile.reasoning_effort))
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
            reasoning_effort: None,
            delegated_model_profile_id: None,
        }
    }

    #[test]
    fn effort_validation_rejects_protocol_mismatch_and_invalid_values() {
        for (provider, effort) in [
            ("anthropic", "max"),
            ("ollama", "max"),
            ("open_ai_compatible", "MAX"),
        ] {
            let mut input = request(provider, "https://gateway.example/v1", "exact-model");
            input.reasoning_effort = Some(Some(effort.into()));
            assert!(model_profile_from_request(input).is_err());
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
