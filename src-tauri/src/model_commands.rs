use omicsops_adapters::{
    credentials::{CredentialVault, credential_account},
    llm::{
        MODEL_PROBE_OUTPUT_TOKENS, ModelProbeResult, ProviderProtocol, RequestBudget,
        UnifiedModelClient,
    },
};
use omicsops_core::workspace::{ModelProfile, ModelProviderKind};
use tauri::State;
use url::Url;
use uuid::Uuid;

use crate::commands::AppState;
use crate::model_catalog_shared::{exact_model_capabilities, exact_model_supports_vision};

pub use omicsops_dto::SaveModelProfileRequest;

const OPENCODE_GO_CHAT_MODELS: &[&str] = &[
    "glm-5.3-flash",
    "glm-5.3",
    "glm-5.2",
    "glm-5.1",
    "kimi-k3",
    "kimi-k2.7-code",
    "kimi-k2.6",
    "longcat-2.0",
    "deepseek-v4.1-flash",
    "deepseek-v4-pro",
    "deepseek-v4-flash",
    "deepseek-v4-flash-vision-exp",
    "mimo-v2.5",
    "mimo-v2.5-pro",
    "hy4-preview",
    "hy3",
];
const OPENCODE_GO_MESSAGES_MODELS: &[&str] = &[
    "minimax-m3",
    "minimax-m2.7",
    "minimax-m2.5",
    "qwen3.8-max",
    "qwen3.8-flash",
    "qwen3.7-max",
    "qwen3.7-plus",
    "qwen3.6-plus",
];
const OPENCODE_GO_RESPONSES_MODELS: &[&str] = &[
    "grok-4.6",
    "gpt-5.6-luna",
    "muse-spark-1.3-contributor",
    "muse-spark-1.2-contributor",
];

fn is_opencode_go_base_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("opencode.ai")
        && url.port_or_known_default() == Some(443)
        && matches!(url.path(), "/zen/go/v1" | "/zen/go/v1/")
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn validate_opencode_go_protocol(
    provider: ModelProviderKind,
    base_url: &Url,
    model: &str,
) -> Result<(), String> {
    if !is_opencode_go_base_url(base_url) {
        return Ok(());
    }
    if OPENCODE_GO_RESPONSES_MODELS.contains(&model) {
        return Err("this OpenCode Go model requires the unsupported Responses API".into());
    }
    if OPENCODE_GO_CHAT_MODELS.contains(&model) && provider != ModelProviderKind::OpenAiCompatible {
        return Err("this OpenCode Go model requires the Chat Completions protocol".into());
    }
    if OPENCODE_GO_MESSAGES_MODELS.contains(&model) && provider != ModelProviderKind::Anthropic {
        return Err("this OpenCode Go model requires the Anthropic Messages protocol".into());
    }
    Ok(())
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
    if request.delegated_model_profile_id.flatten() == Some(id) {
        return Err("choose another delegated profile or inherit the main model".into());
    }
    let model = request.model.trim().to_owned();
    validate_opencode_go_protocol(provider, &base_url, &model)?;
    let reasoning_effort = request.reasoning_effort.flatten();
    let fast_mode = request.fast_mode.flatten();
    omicsops_core::workspace::validate_reasoning_effort(provider, reasoning_effort.as_deref())?;
    if fast_mode == Some(true)
        && !omicsops_core::workspace::supports_fast_mode(provider, base_url.as_str(), &model)
    {
        return Err(
            "explicit Fast mode requires an exact supported OpenAI endpoint and model".into(),
        );
    }
    let supports_vision = exact_model_supports_vision(provider, &base_url, &model);
    let catalog = exact_model_capabilities(provider, &base_url, &model);
    if request.refresh_catalog && catalog.is_none() {
        return Err("no exact entry in the bundled catalog for this model endpoint".into());
    }
    Ok(ModelProfile {
        id,
        label: request.label.trim().into(),
        provider,
        base_url: base_url.to_string(),
        model,
        credential_reference: (provider != ModelProviderKind::Ollama)
            .then(|| credential_account("model", id)),
        supports_tools: catalog.map_or(true, |row| row.supports_tools),
        supports_vision,
        context_window_tokens: request
            .context_window_tokens
            .or_else(|| catalog.map(|row| row.capabilities.context_limit)),
        catalog_capabilities: catalog.map(|row| row.capabilities.clone()),
        reasoning_effort,
        fast_mode,
        delegated_model_profile_id: request.delegated_model_profile_id.flatten(),
    })
}

/// A catalog update must never mutate the runtime contract of an existing profile.
fn merge_existing_profile(
    profile: &mut ModelProfile,
    existing: Option<&ModelProfile>,
    preserve_binding: bool,
    preserve_window: bool,
    preserve_effort: bool,
    preserve_fast_mode: bool,
    refresh_catalog: bool,
) {
    let Some(existing) = existing else {
        return;
    };
    if preserve_binding {
        profile.delegated_model_profile_id = existing.delegated_model_profile_id;
    }
    let same_identity = profile.provider == existing.provider
        && profile.model == existing.model
        && Url::parse(&profile.base_url).ok() == Url::parse(&existing.base_url).ok();
    if same_identity {
        if !refresh_catalog {
            profile.catalog_capabilities = existing.catalog_capabilities.clone();
            profile.supports_tools = existing.supports_tools;
            profile.supports_vision = existing.supports_vision;
            if preserve_window {
                profile.context_window_tokens = existing.context_window_tokens;
            }
        }
        if preserve_effort {
            profile.reasoning_effort = existing.reasoning_effort.clone();
        }
        if preserve_fast_mode {
            profile.fast_mode = existing.fast_mode;
        }
    }
}

fn validate_profile_capabilities(profile: &ModelProfile) -> Result<(), String> {
    omicsops_core::workspace::validate_reasoning_effort(
        profile.provider,
        profile.reasoning_effort.as_deref(),
    )?;
    if profile.fast_mode == Some(true) && !profile.supports_fast_mode() {
        return Err(
            "explicit Fast mode requires an exact supported OpenAI endpoint and model".into(),
        );
    }
    if profile.context_window_tokens == Some(0) {
        return Err("context window must be positive".into());
    }
    if let Some(caps) = &profile.catalog_capabilities {
        if profile
            .context_window_tokens
            .is_some_and(|window| window > caps.context_limit)
        {
            return Err("context window exceeds the model catalog limit".into());
        }
        if let Some(effort) = &profile.reasoning_effort {
            if !caps.reasoning
                || caps
                    .reasoning_efforts
                    .as_ref()
                    .is_some_and(|values| !values.contains(effort))
            {
                return Err("reasoning effort is not supported by this model catalog entry".into());
            }
        }
    }
    Ok(())
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
    credential_mutations: State<'_, crate::credential_settings::CredentialMutationState>,
    request: SaveModelProfileRequest,
) -> Result<ModelProfile, String> {
    let _credential_guard = credential_mutations.lock.lock().await;
    let refresh_catalog = request.refresh_catalog;
    let credential = request.credential.clone();
    let preserve_binding = request.delegated_model_profile_id.is_none();
    let preserve_window = request.context_window_tokens.is_none();
    let preserve_effort = request.reasoning_effort.is_none();
    let preserve_fast_mode = request.fast_mode.is_none();
    let mut profile = model_profile_from_request(request)?;
    let existing = state
        .repository
        .get_model_profile(profile.id)
        .await
        .map_err(|error| error.to_string())?;
    merge_existing_profile(
        &mut profile,
        existing.as_ref(),
        preserve_binding,
        preserve_window,
        preserve_effort,
        preserve_fast_mode,
        refresh_catalog,
    );
    validate_profile_capabilities(&profile)?;
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

fn catalog_probe_budget(profile: &ModelProfile) -> Option<RequestBudget> {
    profile
        .catalog_capabilities
        .as_ref()
        .map(|caps| RequestBudget {
            context_window_tokens: profile.effective_context_window_tokens(),
            reserved_output_tokens: caps.output_limit.min(MODEL_PROBE_OUTPUT_TOKENS),
            safety_margin_tokens: 1024,
        })
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
    if profile.fast_mode == Some(true) && !profile.supports_fast_mode() {
        return Err(
            "explicit Fast mode requires an exact supported OpenAI endpoint and model".into(),
        );
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
    let budget = catalog_probe_budget(&profile);
    UnifiedModelClient::new(
        profile.id,
        protocol,
        Url::parse(&profile.base_url).map_err(|error| error.to_string())?,
        profile.model,
        credential,
    )
    .and_then(|client| client.with_reasoning_effort(profile.reasoning_effort))
    .map(|client| client.with_fast_mode(profile.fast_mode))
    .map(|client| match budget {
        Some(budget) => client.with_request_budget(budget),
        None => client,
    })
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
            refresh_catalog: false,
            reasoning_effort: None,
            fast_mode: None,
            delegated_model_profile_id: None,
        }
    }

    #[test]
    fn opencode_go_requires_the_exact_wire_protocol_and_rejects_responses_models() {
        for model in ["glm-5.3-flash", "deepseek-v4.1-flash", "hy3"] {
            assert!(
                model_profile_from_request(request(
                    "open_ai_compatible",
                    "https://opencode.ai/zen/go/v1",
                    model,
                ))
                .is_ok()
            );
            assert!(
                model_profile_from_request(request(
                    "anthropic",
                    "https://opencode.ai/zen/go/v1",
                    model,
                ))
                .is_err()
            );
        }
        for model in ["minimax-m3", "qwen3.8-flash", "qwen3.6-plus"] {
            assert!(
                model_profile_from_request(request(
                    "anthropic",
                    "https://opencode.ai/zen/go/v1/",
                    model,
                ))
                .is_ok()
            );
            assert!(
                model_profile_from_request(request(
                    "open_ai_compatible",
                    "https://opencode.ai/zen/go/v1",
                    model,
                ))
                .is_err()
            );
        }
        for model in [
            "grok-4.6",
            "gpt-5.6-luna",
            "muse-spark-1.3-contributor",
            "muse-spark-1.2-contributor",
        ] {
            assert!(
                model_profile_from_request(request(
                    "open_ai_compatible",
                    "https://opencode.ai/zen/go/v1",
                    model,
                ))
                .is_err()
            );
        }

        assert!(
            model_profile_from_request(request(
                "anthropic",
                "https://opencode.ai/zen/go/v1",
                "future-custom",
            ))
            .is_ok()
        );
        assert!(
            model_profile_from_request(request(
                "open_ai_compatible",
                "https://proxy.example/zen/go/v1",
                "minimax-m3",
            ))
            .is_ok()
        );
    }

    #[test]
    fn explicit_refresh_adopts_snapshot_and_preserves_effort_and_binding() {
        let mut input = request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-5.6-luna",
        );
        let mut saved = model_profile_from_request(input.clone()).unwrap();
        saved.catalog_capabilities = None;
        saved.context_window_tokens = Some(32000);
        saved.supports_vision = false;
        saved.reasoning_effort = Some("max".into());
        saved.delegated_model_profile_id = Some(Uuid::new_v4());
        let old_hash = saved.execution_configuration_hash();
        input.id = Some(saved.id);
        input.refresh_catalog = true;
        let mut refreshed = model_profile_from_request(input.clone()).unwrap();
        merge_existing_profile(&mut refreshed, Some(&saved), true, true, true, true, true);
        validate_profile_capabilities(&refreshed).unwrap();
        assert!(refreshed.catalog_capabilities.is_some());
        assert_eq!(refreshed.context_window_tokens, Some(1050000));
        assert_eq!(refreshed.reasoning_effort, saved.reasoning_effort);
        assert_eq!(
            refreshed.delegated_model_profile_id,
            saved.delegated_model_profile_id
        );
        assert_ne!(refreshed.execution_configuration_hash(), old_hash);
        let mut repeated = model_profile_from_request(input.clone()).unwrap();
        merge_existing_profile(
            &mut repeated,
            Some(&refreshed),
            true,
            true,
            true,
            true,
            true,
        );
        assert_eq!(
            repeated.execution_configuration_hash(),
            refreshed.execution_configuration_hash()
        );
        input.context_window_tokens = Some(64000);
        let mut custom = model_profile_from_request(input).unwrap();
        merge_existing_profile(&mut custom, Some(&saved), true, false, true, true, true);
        assert_eq!(custom.context_window_tokens, Some(64000));
    }

    #[test]
    fn opencode_go_refresh_repairs_a_legacy_profile_without_overwriting_a_user_limit() {
        let mut input = request(
            "open_ai_compatible",
            "https://opencode.ai/zen/go/v1",
            "glm-5.3",
        );
        let mut legacy = model_profile_from_request(input.clone()).unwrap();
        assert_eq!(legacy.context_window_tokens, Some(1_000_000));
        assert_eq!(
            legacy.catalog_capabilities.as_ref().unwrap().output_limit,
            131_072
        );
        legacy.catalog_capabilities = None;
        legacy.context_window_tokens = None;

        input.id = Some(legacy.id);
        input.refresh_catalog = true;
        let mut refreshed = model_profile_from_request(input.clone()).unwrap();
        merge_existing_profile(&mut refreshed, Some(&legacy), true, true, true, true, true);
        assert_eq!(refreshed.context_window_tokens, Some(1_000_000));
        assert_eq!(refreshed.effective_context_window_tokens(), 1_000_000);

        input.context_window_tokens = Some(64_000);
        let mut bounded = model_profile_from_request(input).unwrap();
        merge_existing_profile(&mut bounded, Some(&legacy), true, false, true, true, true);
        assert_eq!(bounded.context_window_tokens, Some(64_000));
        assert_eq!(bounded.effective_context_window_tokens(), 64_000);
    }

    #[test]
    fn refresh_rejects_unknown_endpoints_and_revalidates_saved_effort() {
        let mut unknown = request("open_ai_compatible", "https://gateway.example/v1", "gpt-4o");
        unknown.refresh_catalog = true;
        assert!(model_profile_from_request(unknown).is_err());
        let mut input = request("open_ai_compatible", "https://api.openai.com/v1", "gpt-4o");
        let mut legacy = model_profile_from_request(input.clone()).unwrap();
        legacy.catalog_capabilities = None;
        legacy.reasoning_effort = Some("max".into());
        input.refresh_catalog = true;
        let mut refreshed = model_profile_from_request(input).unwrap();
        merge_existing_profile(&mut refreshed, Some(&legacy), true, true, true, true, true);
        assert!(validate_profile_capabilities(&refreshed).is_err());
        refreshed.reasoning_effort = None;
        assert!(validate_profile_capabilities(&refreshed).is_ok());
        assert!(legacy.catalog_capabilities.is_none());
        assert_eq!(legacy.reasoning_effort.as_deref(), Some("max"));
    }

    #[test]
    fn probes_cap_reasoning_safe_allowance_to_the_catalog_output_limit() {
        let mut profile = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-5.6-luna",
        ))
        .unwrap();
        assert_eq!(
            catalog_probe_budget(&profile)
                .unwrap()
                .reserved_output_tokens,
            4096
        );
        profile.reasoning_effort = Some("max".into());
        assert_eq!(
            catalog_probe_budget(&profile)
                .unwrap()
                .reserved_output_tokens,
            4096
        );
        profile.catalog_capabilities.as_mut().unwrap().output_limit = 2048;
        assert_eq!(
            catalog_probe_budget(&profile)
                .unwrap()
                .reserved_output_tokens,
            2048
        );
        profile.catalog_capabilities = None;
        assert!(catalog_probe_budget(&profile).is_none());
    }

    #[test]
    fn catalog_snapshot_defaults_and_explicit_constraints() {
        let mut profile = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-4o",
        ))
        .unwrap();
        assert_eq!(profile.context_window_tokens, Some(128000));
        assert_eq!(
            profile.catalog_capabilities.as_ref().unwrap().output_limit,
            16384
        );
        assert!(profile.supports_tools && profile.supports_vision);
        assert!(validate_profile_capabilities(&profile).is_ok());
        profile.context_window_tokens = Some(128001);
        assert!(validate_profile_capabilities(&profile).is_err());
        profile.context_window_tokens = Some(0);
        assert!(validate_profile_capabilities(&profile).is_err());
        profile.context_window_tokens = Some(64000);
        profile.reasoning_effort = Some("max".into());
        assert!(validate_profile_capabilities(&profile).is_err());
        let mut luna = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-5.6-luna",
        ))
        .unwrap();
        luna.reasoning_effort = Some("max".into());
        assert!(validate_profile_capabilities(&luna).is_ok());
        luna.reasoning_effort = Some("ultra".into());
        assert!(validate_profile_capabilities(&luna).is_err());
    }

    #[test]
    fn editing_preserves_legacy_contract_but_changing_identity_gets_new_snapshot() {
        let mut old = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-4o",
        ))
        .unwrap();
        old.catalog_capabilities = None;
        old.context_window_tokens = None;
        old.supports_tools = false;
        old.supports_vision = false;
        let hash = old.execution_configuration_hash();
        let mut edited = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-4o",
        ))
        .unwrap();
        edited.id = old.id;
        edited.label = "Renamed".into();
        merge_existing_profile(&mut edited, Some(&old), true, true, true, true, false);
        assert_eq!(edited.execution_configuration_hash(), hash);
        assert!(edited.catalog_capabilities.is_none());
        assert_eq!(edited.effective_context_window_tokens(), 32768);
        assert_eq!(edited.effective_output_tokens(), 4096);
        let mut replacement = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-5.6-luna",
        ))
        .unwrap();
        replacement.id = old.id;
        merge_existing_profile(&mut replacement, Some(&old), true, true, true, true, false);
        assert!(replacement.catalog_capabilities.is_some());
        assert_ne!(replacement.execution_configuration_hash(), hash);
    }

    #[test]
    fn catalog_refresh_cannot_replace_saved_snapshot_and_budget_changes_are_hashed() {
        let mut saved = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-4o",
        ))
        .unwrap();
        let original_hash = saved.execution_configuration_hash();
        saved.catalog_capabilities.as_mut().unwrap().source_sha256 = "f".repeat(64);
        assert_eq!(saved.execution_configuration_hash(), original_hash);
        saved.catalog_capabilities.as_mut().unwrap().output_limit = 2048;
        assert_eq!(saved.effective_output_tokens(), 2048);
        assert_ne!(saved.execution_configuration_hash(), original_hash);
        saved.catalog_capabilities.as_mut().unwrap().input_limit = Some(32000);
        assert_eq!(saved.effective_context_window_tokens(), 32000);
        let mut edited = model_profile_from_request(request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-4o",
        ))
        .unwrap();
        edited.id = saved.id;
        merge_existing_profile(&mut edited, Some(&saved), true, true, true, true, false);
        assert_eq!(edited.catalog_capabilities, saved.catalog_capabilities);
        assert_eq!(
            edited.execution_configuration_hash(),
            saved.execution_configuration_hash()
        );
        let decoded: ModelProfile =
            serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
        assert_eq!(decoded, saved);
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

    #[test]
    fn fast_mode_conversion_round_trips_supported_profile_values() {
        for value in [false, true] {
            let mut input = request(
                "open_ai_compatible",
                "https://api.openai.com/v1",
                "gpt-5.6-luna",
            );
            input.fast_mode = Some(Some(value));
            let profile = model_profile_from_request(input).unwrap();
            assert_eq!(profile.fast_mode, Some(value));
            assert!(profile.supports_fast_mode());
        }
    }

    #[test]
    fn explicit_fast_mode_rejects_unknown_endpoint_model_and_provider() {
        for (provider, base_url, model) in [
            (
                "open_ai_compatible",
                "https://gateway.example/v1",
                "gpt-5.6-luna",
            ),
            (
                "open_ai_compatible",
                "https://api.openai.com/v1",
                "gpt-5.6-luna-preview",
            ),
            ("anthropic", "https://api.openai.com/v1", "gpt-5.6-luna"),
            ("ollama", "http://127.0.0.1:11434", "gpt-5.6-luna"),
        ] {
            let mut input = request(provider, base_url, model);
            input.fast_mode = Some(Some(true));
            assert!(model_profile_from_request(input).is_err());
        }
    }

    #[test]
    fn fast_mode_edit_omission_preserves_and_null_clears_existing_value() {
        let mut saved_request = request(
            "open_ai_compatible",
            "https://api.openai.com/v1",
            "gpt-5.6-luna",
        );
        saved_request.fast_mode = Some(Some(true));
        let mut saved = model_profile_from_request(saved_request.clone()).unwrap();

        saved_request.id = Some(saved.id);
        saved_request.fast_mode = None;
        let mut omitted = model_profile_from_request(saved_request.clone()).unwrap();
        merge_existing_profile(&mut omitted, Some(&saved), true, true, true, true, false);
        assert_eq!(omitted.fast_mode, Some(true));

        saved_request.fast_mode = Some(None);
        let mut cleared = model_profile_from_request(saved_request).unwrap();
        merge_existing_profile(&mut cleared, Some(&saved), true, true, true, false, false);
        assert_eq!(cleared.fast_mode, None);

        saved.fast_mode = Some(false);
        assert_eq!(saved.fast_mode, Some(false));
    }
}
