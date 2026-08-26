use std::path::PathBuf;

use omicsops_adapters::{
    credentials::{CredentialVault, SystemCredentialVault},
    llm::{ProviderProtocol, UnifiedModelClient},
};
use omicsops_core::workspace::ModelProviderKind;
use omicsops_store::Store;
use uuid::Uuid;

#[tokio::test]
#[ignore = "reads an explicitly selected desktop model profile and performs a real API request"]
async fn configured_desktop_model_accepts_a_minimal_chat_probe() {
    let profile_id: Uuid = std::env::var("OMICSOPS_LIVE_MODEL_PROFILE_ID")
        .expect("model profile id must be explicitly selected")
        .parse()
        .expect("model profile id must be a UUID");
    let data_dir = PathBuf::from(std::env::var("APPDATA").expect("APPDATA is required"))
        .join("io.omicsops.desktop");
    // Work from a disposable snapshot so opening the repository can never run
    // migrations or otherwise mutate the user's live desktop database.
    let snapshot_dir = tempfile::tempdir().unwrap();
    let snapshot_path = snapshot_dir.path().join("omicsops.db");
    std::fs::copy(data_dir.join("omicsops.db"), &snapshot_path).unwrap();
    let store = Store::open(snapshot_path).await.unwrap();
    let profile = store
        .get_model_profile(profile_id)
        .await
        .unwrap()
        .expect("selected model profile was not found");
    let credential = profile
        .credential_reference
        .as_deref()
        .map(|reference| SystemCredentialVault.get(reference).unwrap())
        .flatten();
    let protocol = match profile.provider {
        ModelProviderKind::Anthropic => ProviderProtocol::Anthropic,
        ModelProviderKind::OpenAiCompatible => ProviderProtocol::OpenAiCompatible,
        ModelProviderKind::Ollama => ProviderProtocol::Ollama,
    };
    let model = std::env::var("OMICSOPS_LIVE_MODEL_OVERRIDE").unwrap_or(profile.model);
    let client = UnifiedModelClient::new(
        profile.id,
        protocol,
        profile.base_url.parse().unwrap(),
        model,
        credential,
    )
    .unwrap();
    let result = match client.probe().await {
        Ok(result) => result,
        Err(error) => {
            let available = client.list_models().await.unwrap_or_default();
            panic!("{error}; available models: {}", available.join(", "));
        }
    };
    assert!(!result.response_preview.is_empty());
}
