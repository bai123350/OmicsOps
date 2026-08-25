use std::{collections::BTreeMap, io::Write, path::Path, process::Stdio};

use chrono::Utc;
use omicsops_adapters::{credentials::CredentialVault, persistence::Repository};
use omicsops_core::workspace::{
    Artifact, EvidenceReference, MemoryFact, NotebookEntry, SkillCitation,
};
use omicsops_knowledge::schema_digest;
use omicsops_mcp::{McpEnvBinding, McpServerConfig, McpSessionManager};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::State;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};
use uuid::Uuid;

use crate::commands::AppState;

#[derive(Debug, Clone, Deserialize)]
pub struct MemorySearchRequest {
    pub project_id: Uuid,
    pub conversation_id: Option<Uuid>,
    #[serde(default)]
    pub query: String,
    pub dimension: Option<String>,
}

pub fn memory_facts(
    repository: &Repository,
    request: &MemorySearchRequest,
) -> Result<Vec<MemoryFact>, String> {
    let query = request.query.trim().to_lowercase();
    let mut facts = Vec::new();
    for conversation in repository
        .conversations_for_project(request.project_id)
        .map_err(|error| error.to_string())?
    {
        if request
            .conversation_id
            .is_some_and(|id| id != conversation.id)
        {
            continue;
        }
        for message in repository
            .messages_for_conversation(conversation.id)
            .map_err(|error| error.to_string())?
        {
            if !matches!(message.role, omicsops_core::workspace::MessageRole::User) {
                continue;
            }
            facts.push(MemoryFact {
                id: stable_uuid(&format!("message:{}", message.id)),
                project_id: request.project_id,
                conversation_id: Some(conversation.id),
                run_id: None,
                dimension: "task".into(),
                key: format!(
                    "conversation:{}:message:{}",
                    conversation.id, message.sequence
                ),
                value: message.markdown.clone(),
                statement: message.markdown.clone(),
                evidence: vec![EvidenceReference {
                    source_kind: "message".into(),
                    source_id: message.id.to_string(),
                    excerpt: excerpt(&message.markdown, 320),
                }],
                conflicted_with: vec![],
                created_at: message.created_at,
            });
        }
    }
    for artifact in repository
        .artifacts_for_project(request.project_id)
        .map_err(|error| error.to_string())?
    {
        facts.push(MemoryFact {
            id: stable_uuid(&format!("artifact:{}", artifact.id)),
            project_id: request.project_id,
            conversation_id: None,
            run_id: artifact.run_id,
            dimension: "artifact".into(),
            key: artifact.relative_path.clone(),
            value: artifact.sha256.clone(),
            statement: format!(
                "Verified artifact {} ({} bytes)",
                artifact.relative_path, artifact.size_bytes
            ),
            evidence: vec![EvidenceReference {
                source_kind: "artifact".into(),
                source_id: artifact.id.to_string(),
                excerpt: format!(
                    "sha256={} media_type={}",
                    artifact.sha256, artifact.media_type
                ),
            }],
            conflicted_with: vec![],
            created_at: artifact.created_at,
        });
    }
    for entry in repository
        .notebook_for_project(request.project_id)
        .map_err(|error| error.to_string())?
    {
        if request
            .conversation_id
            .is_some_and(|id| entry.conversation_id.is_some_and(|entry_id| entry_id != id))
        {
            continue;
        }
        facts.push(MemoryFact {
            id: stable_uuid(&format!("notebook:{}", entry.id)),
            project_id: request.project_id,
            conversation_id: entry.conversation_id,
            run_id: None,
            dimension: format!("{:?}", entry.kind).to_ascii_lowercase(),
            key: entry.title.clone(),
            value: entry.markdown.clone(),
            statement: entry.markdown.clone(),
            evidence: vec![EvidenceReference {
                source_kind: "notebook".into(),
                source_id: entry.id.to_string(),
                excerpt: excerpt(&entry.markdown, 480),
            }],
            conflicted_with: vec![],
            created_at: entry.updated_at,
        });
    }
    mark_conflicts(&mut facts);
    facts.retain(|fact| {
        request
            .dimension
            .as_deref()
            .is_none_or(|dimension| dimension == fact.dimension)
            && (query.is_empty()
                || format!("{} {} {}", fact.key, fact.value, fact.statement)
                    .to_lowercase()
                    .contains(&query))
    });
    facts.sort_by_key(|fact| std::cmp::Reverse(fact.created_at));
    Ok(facts)
}

#[tauri::command]
pub fn search_agent_memory(
    state: State<'_, AppState>,
    request: MemorySearchRequest,
) -> Result<Vec<MemoryFact>, String> {
    memory_facts(&state.repository, &request)
}

#[tauri::command]
pub fn list_notebook_entries(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<NotebookEntry>, String> {
    state
        .repository
        .notebook_for_project(project_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_project_artifacts(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<Artifact>, String> {
    state
        .repository
        .artifacts_for_project(project_id)
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportNotebookRequest {
    pub project_id: Uuid,
    pub format: String,
    pub local_path: String,
}

#[tauri::command]
pub fn export_project_notebook(
    state: State<'_, AppState>,
    request: ExportNotebookRequest,
) -> Result<(), String> {
    let project = state
        .repository
        .get_project(request.project_id)
        .map_err(|error| error.to_string())?
        .ok_or("project not found")?;
    let notebook = state
        .repository
        .notebook_for_project(request.project_id)
        .map_err(|error| error.to_string())?;
    let artifacts = state
        .repository
        .artifacts_for_project(request.project_id)
        .map_err(|error| error.to_string())?;
    let facts = memory_facts(
        &state.repository,
        &MemorySearchRequest {
            project_id: request.project_id,
            conversation_id: None,
            query: String::new(),
            dimension: None,
        },
    )?;
    let payload = json!({"schema_version":1,"project":project,"notebook":notebook,"artifacts":artifacts,"memory_facts":facts});
    let target = Path::new(&request.local_path);
    match request.format.as_str() {
        "json" => std::fs::write(
            target,
            serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string()),
        "markdown" => std::fs::write(
            target,
            render_markdown(&project.name, &notebook, &artifacts, &facts),
        )
        .map_err(|error| error.to_string()),
        "bundle" => write_project_bundle(
            target,
            &project.name,
            &payload,
            &notebook,
            &artifacts,
            &facts,
        ),
        _ => Err("export format must be markdown, json, or bundle".into()),
    }
}

pub fn enabled_skill_citations(repository: &Repository) -> Result<Vec<SkillCitation>, String> {
    let mut citations = Vec::new();
    for skill in crate::skill_commands::agent_skill_packages(repository)? {
        let markdown = std::fs::read_to_string(Path::new(&skill.source_path).join("SKILL.md"))
            .map_err(|error| format!("cannot read enabled skill {}: {error}", skill.name))?;
        for (section, excerpt) in markdown_sections(&markdown).into_iter().take(12) {
            citations.push(SkillCitation {
                skill_id: skill.id,
                name: skill.name.clone(),
                version: skill.version.clone(),
                package_sha256: skill.sha256.clone(),
                section,
                excerpt_sha256: hex::encode(Sha256::digest(excerpt.as_bytes())),
            });
        }
    }
    Ok(citations)
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpRequest {
    pub project_id: Uuid,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub approved: bool,
    pub tool: Option<String>,
    pub arguments: Option<Value>,
    #[serde(default)]
    pub expected_schema_sha256: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct McpResult {
    pub server_name: String,
    pub capabilities: Value,
    pub tools: Vec<Value>,
    pub result: Option<Value>,
    pub audit_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerProfile {
    pub id: Uuid,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default = "default_mcp_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default, alias = "env")]
    pub env_bindings: Vec<McpEnvBinding>,
    pub enabled: bool,
    #[serde(default)]
    pub launch_approved: bool,
    #[serde(default)]
    pub approved_tools: Vec<String>,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default = "empty_json_object")]
    pub capabilities: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_catalog_sha256: Option<String>,
    #[serde(default)]
    pub catalog_generation: u64,
    #[serde(default)]
    pub config_version: u64,
    #[serde(default = "default_mcp_status")]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
    pub last_inspected_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveMcpServerRequest {
    pub id: Option<Uuid>,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_mcp_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default, alias = "env")]
    pub env_bindings: Vec<McpEnvBinding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddPubMedMcpServerRequest {
    pub api_key: Option<String>,
    pub admin_email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetMcpServerEnabledRequest {
    pub server_id: Uuid,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetMcpLaunchApprovalRequest {
    pub server_id: Uuid,
    pub approved: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InspectConfiguredMcpServerRequest {
    pub project_id: Uuid,
    pub server_id: Uuid,
    pub approved: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetMcpToolApprovalRequest {
    pub server_id: Uuid,
    pub tool: String,
    pub approved: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CallConfiguredMcpToolRequest {
    pub project_id: Uuid,
    pub server_id: Uuid,
    pub tool: String,
    pub arguments: Option<Value>,
    pub approved: bool,
}

fn empty_json_object() -> Value {
    json!({})
}

fn default_mcp_timeout_secs() -> u64 {
    60
}

fn default_mcp_status() -> String {
    "disconnected".into()
}

fn validate_mcp_declaration(name: &str, command: &str, args: &[String]) -> Result<(), String> {
    if name.trim().is_empty()
        || command.trim().is_empty()
        || command.contains(['\n', '\r', '\0'])
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return Err("invalid MCP stdio declaration".into());
    }
    Ok(())
}

fn mcp_server_profile(repository: &Repository, id: Uuid) -> Result<McpServerProfile, String> {
    repository
        .get_json("mcp_server", &id.to_string())
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("MCP server {id} was not found"))
}

/// Resolve a persisted MCP profile only at process-launch time.  Credential
/// references are deliberately converted to an in-memory value here; the
/// returned config is never persisted or included in audit payloads.
fn resolved_mcp_config(
    profile: &McpServerProfile,
    project_id: Uuid,
    credentials: &dyn CredentialVault,
) -> Result<McpServerConfig, String> {
    let mut env = Vec::with_capacity(profile.env_bindings.len());
    for binding in &profile.env_bindings {
        binding.validate().map_err(|error| error.to_string())?;
        let value = match (&binding.value, &binding.credential_reference) {
            (Some(value), None) if !value.is_empty() => value.clone(),
            (None, Some(reference)) if !reference.trim().is_empty() => credentials
                .get(reference)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("MCP credential {reference} is missing"))?,
            _ => {
                return Err(format!(
                    "MCP environment binding {} has no usable value",
                    binding.name
                ));
            }
        };
        env.push((binding.name.clone(), value));
    }
    McpServerConfig {
        project_id,
        server_id: profile.id,
        name: profile.name.clone(),
        command: profile.command.clone(),
        args: profile.args.clone(),
        cwd: profile.cwd.clone(),
        env,
        timeout_secs: profile.timeout_secs,
    }
    .normalized()
    .map_err(|error| error.to_string())
}

fn mcp_profile_from_request(
    request: SaveMcpServerRequest,
    existing: Option<&McpServerProfile>,
    now: chrono::DateTime<Utc>,
) -> Result<McpServerProfile, String> {
    validate_mcp_declaration(&request.name, &request.command, &request.args)?;
    let declaration_changed = existing.is_some_and(|profile| {
        profile.command != request.command.trim()
            || profile.args != request.args
            || profile.cwd != request.cwd
            || profile.timeout_secs != request.timeout_secs.clamp(1, 3_600)
            || profile.env_bindings != request.env_bindings
    });
    Ok(McpServerProfile {
        id: request.id.unwrap_or_else(Uuid::new_v4),
        name: request.name.trim().to_string(),
        command: request.command.trim().to_string(),
        args: request.args,
        cwd: request.cwd,
        timeout_secs: request.timeout_secs.clamp(1, 3_600),
        env_bindings: request.env_bindings,
        enabled: existing.is_some_and(|profile| profile.enabled) && !declaration_changed,
        launch_approved: existing.is_some_and(|profile| profile.launch_approved)
            && !declaration_changed,
        approved_tools: if declaration_changed {
            vec![]
        } else {
            existing
                .map(|profile| profile.approved_tools.clone())
                .unwrap_or_default()
        },
        tools: if declaration_changed {
            vec![]
        } else {
            existing
                .map(|profile| profile.tools.clone())
                .unwrap_or_default()
        },
        capabilities: if declaration_changed {
            json!({})
        } else {
            existing
                .map(|profile| profile.capabilities.clone())
                .unwrap_or_else(|| json!({}))
        },
        tool_catalog_sha256: if declaration_changed {
            None
        } else {
            existing.and_then(|profile| profile.tool_catalog_sha256.clone())
        },
        catalog_generation: if declaration_changed {
            0
        } else {
            existing.map_or(0, |profile| profile.catalog_generation)
        },
        config_version: existing.map_or(1, |profile| profile.config_version.saturating_add(1)),
        status: if declaration_changed {
            "disconnected".into()
        } else {
            existing
                .map(|profile| profile.status.clone())
                .unwrap_or_else(default_mcp_status)
        },
        last_error: if declaration_changed {
            None
        } else {
            existing.and_then(|profile| profile.last_error.clone())
        },
        stderr_tail: if declaration_changed {
            None
        } else {
            existing.and_then(|profile| profile.stderr_tail.clone())
        },
        last_inspected_at: if declaration_changed {
            None
        } else {
            existing.and_then(|profile| profile.last_inspected_at)
        },
        created_at: existing.map(|profile| profile.created_at).unwrap_or(now),
        updated_at: now,
    })
}

#[tauri::command]
pub fn list_mcp_servers(state: State<'_, AppState>) -> Result<Vec<McpServerProfile>, String> {
    let mut profiles = state
        .repository
        .list_json("mcp_server")
        .map_err(|error| error.to_string())?;
    profiles.sort_by(|left: &McpServerProfile, right| right.updated_at.cmp(&left.updated_at));
    Ok(profiles)
}

#[tauri::command]
pub async fn save_mcp_server(
    state: State<'_, AppState>,
    request: SaveMcpServerRequest,
) -> Result<McpServerProfile, String> {
    let now = Utc::now();
    let existing = request
        .id
        .map(|id| mcp_server_profile(&state.repository, id))
        .transpose()?;
    let profile = mcp_profile_from_request(request, existing.as_ref(), now)?;
    if existing.is_some() {
        state.mcp_sessions.invalidate_server(profile.id).await;
    }
    state
        .repository
        .put_json("mcp_server", &profile.id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn add_pubmed_mcp_server(
    state: State<'_, AppState>,
    request: AddPubMedMcpServerRequest,
) -> Result<McpServerProfile, String> {
    let existing = state
        .repository
        .list_json::<McpServerProfile>("mcp_server")
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|profile| {
            profile
                .args
                .iter()
                .any(|arg| arg == "--omicsops-pubmed-mcp")
        });
    let id = existing
        .as_ref()
        .map_or_else(Uuid::new_v4, |profile| profile.id);
    let api_key_account = format!("mcp/{id}/NCBI_API_KEY");
    if let Some(api_key) = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        state
            .credentials
            .set(&api_key_account, api_key)
            .map_err(|error| error.to_string())?;
    }
    let mut env_bindings = existing
        .as_ref()
        .map(|profile| profile.env_bindings.clone())
        .unwrap_or_default();
    env_bindings
        .retain(|binding| binding.name != "NCBI_API_KEY" && binding.name != "NCBI_ADMIN_EMAIL");
    if state
        .credentials
        .get(&api_key_account)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        env_bindings.push(McpEnvBinding {
            name: "NCBI_API_KEY".into(),
            value: None,
            credential_reference: Some(api_key_account),
        });
    }
    if let Some(email) = request
        .admin_email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            existing.as_ref().and_then(|profile| {
                profile
                    .env_bindings
                    .iter()
                    .find(|binding| binding.name == "NCBI_ADMIN_EMAIL")
                    .and_then(|binding| binding.value.clone())
            })
        })
    {
        env_bindings.push(McpEnvBinding {
            name: "NCBI_ADMIN_EMAIL".into(),
            value: Some(email),
            credential_reference: None,
        });
    }
    let command = std::env::current_exe()
        .map_err(|error| format!("could not locate OmicsOps executable: {error}"))?
        .to_string_lossy()
        .into_owned();
    let profile = mcp_profile_from_request(
        SaveMcpServerRequest {
            id: Some(id),
            name: "PubMed".into(),
            command,
            args: vec!["--omicsops-pubmed-mcp".into()],
            cwd: None,
            timeout_secs: 60,
            env_bindings,
        },
        existing.as_ref(),
        Utc::now(),
    )?;
    state.mcp_sessions.invalidate_server(id).await;
    state
        .repository
        .put_json("mcp_server", &id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn set_mcp_server_enabled(
    state: State<'_, AppState>,
    request: SetMcpServerEnabledRequest,
) -> Result<McpServerProfile, String> {
    let mut profile = mcp_server_profile(&state.repository, request.server_id)?;
    if request.enabled && profile.last_inspected_at.is_none() {
        return Err("inspect the MCP server before enabling it".into());
    }
    profile.enabled = request.enabled;
    if !request.enabled {
        state.mcp_sessions.invalidate_server(profile.id).await;
    }
    profile.updated_at = Utc::now();
    state
        .repository
        .put_json("mcp_server", &profile.id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn set_mcp_launch_approval(
    state: State<'_, AppState>,
    request: SetMcpLaunchApprovalRequest,
) -> Result<McpServerProfile, String> {
    let mut profile = mcp_server_profile(&state.repository, request.server_id)?;
    profile.launch_approved = request.approved;
    if !request.approved {
        profile.enabled = false;
        profile.approved_tools.clear();
        state.mcp_sessions.invalidate_server(profile.id).await;
    }
    profile.updated_at = Utc::now();
    state
        .repository
        .put_json("mcp_server", &profile.id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn set_mcp_tool_approval(
    state: State<'_, AppState>,
    request: SetMcpToolApprovalRequest,
) -> Result<McpServerProfile, String> {
    let mut profile = mcp_server_profile(&state.repository, request.server_id)?;
    let tool = request.tool.trim();
    if !profile
        .tools
        .iter()
        .any(|entry| entry.get("name").and_then(Value::as_str) == Some(tool))
    {
        return Err(format!("MCP tool {tool} was not advertised by the server"));
    }
    profile.approved_tools.retain(|entry| entry != tool);
    if !request.approved {
        state.mcp_sessions.invalidate_server(profile.id).await;
    }
    if request.approved {
        profile.approved_tools.push(tool.to_string());
        profile.approved_tools.sort();
        profile.approved_tools.dedup();
    }
    profile.updated_at = Utc::now();
    state
        .repository
        .put_json("mcp_server", &profile.id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn inspect_configured_mcp_server(
    state: State<'_, AppState>,
    request: InspectConfiguredMcpServerRequest,
) -> Result<McpResult, String> {
    let mut profile = mcp_server_profile(&state.repository, request.server_id)?;
    if !request.approved {
        return Err("inspecting an MCP subprocess requires explicit approval".into());
    }
    state.mcp_sessions.invalidate_server(profile.id).await;
    profile.status = "connecting".into();
    profile.last_error = None;
    profile.updated_at = Utc::now();
    state
        .repository
        .put_json("mcp_server", &profile.id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    let config = match resolved_mcp_config(&profile, request.project_id, &state.credentials) {
        Ok(config) => config,
        Err(error) => {
            profile.status = "failed".into();
            profile.last_error = Some(error.clone());
            profile.updated_at = Utc::now();
            let _ = state
                .repository
                .put_json("mcp_server", &profile.id.to_string(), &profile);
            return Err(error);
        }
    };
    let inspection = match state.mcp_sessions.inspect(config).await {
        Ok(inspection) => inspection,
        Err(error) => {
            let error = error.to_string();
            profile.status = "failed".into();
            profile.last_error = Some(error.clone());
            profile.updated_at = Utc::now();
            let _ = state
                .repository
                .put_json("mcp_server", &profile.id.to_string(), &profile);
            return Err(error);
        }
    };
    let audit_id = Uuid::new_v4();
    let result = McpResult {
        server_name: inspection.server_name.clone(),
        capabilities: inspection.capabilities.clone(),
        tools: inspection.tools.clone(),
        result: None,
        audit_id,
    };
    // A fresh discovery can change a tool's schema or behavior without changing its name.
    // Require the user to approve every tool again after each inspection.
    profile.approved_tools.clear();
    profile.launch_approved = true;
    profile.tools = result.tools.clone();
    profile.capabilities = result.capabilities.clone();
    profile.tool_catalog_sha256 = Some(inspection.tool_catalog_sha256);
    profile.catalog_generation = inspection.generation;
    profile.status = "ready".into();
    profile.last_error = None;
    profile.stderr_tail = (!inspection.stderr_tail.is_empty()).then_some(inspection.stderr_tail);
    profile.last_inspected_at = Some(Utc::now());
    profile.updated_at = Utc::now();
    state
        .repository
        .put_json("mcp_server", &profile.id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    let audit = json!({
        "id": audit_id,
        "project_id": request.project_id,
        "server": profile.name,
        "command": profile.command,
        "args": profile.args,
        "operation": "inspect",
        "approved": true,
        "succeeded": true,
        "timestamp": Utc::now(),
    });
    state
        .repository
        .put_json("mcp_audit", &audit_id.to_string(), &audit)
        .map_err(|error| error.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn call_configured_mcp_tool(
    state: State<'_, AppState>,
    request: CallConfiguredMcpToolRequest,
) -> Result<McpResult, String> {
    if !request.approved {
        return Err("calling an MCP subprocess requires explicit approval".into());
    }
    let profile = mcp_server_profile(&state.repository, request.server_id)?;
    if !profile.enabled {
        return Err("MCP server is disabled".into());
    }
    if !profile.launch_approved {
        return Err("MCP server launch has not been approved".into());
    }
    if !profile
        .approved_tools
        .iter()
        .any(|tool| tool == request.tool.trim())
    {
        return Err(format!(
            "MCP tool {} requires explicit tool approval",
            request.tool.trim()
        ));
    }
    let schema = profile
        .tools
        .iter()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some(request.tool.trim()))
        .and_then(|entry| {
            entry
                .get("inputSchema")
                .or_else(|| entry.get("input_schema"))
                .cloned()
        })
        .unwrap_or_else(|| json!({"type":"object"}));
    invoke_configured_mcp_tool_v4(
        &state.repository,
        &state.mcp_sessions,
        &state.credentials,
        request.project_id,
        request.server_id,
        request.tool.trim(),
        request.arguments.unwrap_or_else(|| json!({})),
        schema_digest(&schema),
        false,
    )
    .await
}

#[tauri::command]
pub async fn inspect_mcp_server(
    state: State<'_, AppState>,
    request: McpRequest,
) -> Result<McpResult, String> {
    run_mcp(&state.repository, request, false).await
}
#[tauri::command]
pub async fn call_mcp_tool(
    state: State<'_, AppState>,
    request: McpRequest,
) -> Result<McpResult, String> {
    run_mcp(&state.repository, request, true).await
}

async fn run_mcp(
    repository: &Repository,
    request: McpRequest,
    call_tool: bool,
) -> Result<McpResult, String> {
    if !request.approved {
        return Err("launching an MCP subprocess requires explicit approval".into());
    }
    validate_mcp_declaration(&request.name, &request.command, &request.args)?;
    let audit_id = Uuid::new_v4();
    let mut attempts = 0_u8;
    let outcome = loop {
        attempts += 1;
        let result = run_mcp_session(&request, call_tool, audit_id).await;
        if result.is_ok() || call_tool || attempts >= 2 {
            break result;
        }
    };
    let audit = json!({"id":audit_id,"project_id":request.project_id,"server":request.name,"command":request.command,"args":request.args,"tool":request.tool,"approved":true,"attempts":attempts,"succeeded":outcome.is_ok(),"error":outcome.as_ref().err(),"timestamp":Utc::now()});
    repository
        .put_json("mcp_audit", &audit_id.to_string(), &audit)
        .map_err(|error| error.to_string())?;
    outcome
}

async fn run_mcp_session(
    request: &McpRequest,
    call_tool: bool,
    audit_id: Uuid,
) -> Result<McpResult, String> {
    let mut child = Command::new(&request.command)
        .args(&request.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("could not launch MCP server: {error}"))?;
    let outcome = async {
        let initialize = rpc(&mut child, 1, "initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"OmicsOps","version":env!("CARGO_PKG_VERSION")}})).await?;
        notify(&mut child, "notifications/initialized", json!({})).await?;
        let tools_value = rpc(&mut child, 2, "tools/list", json!({})).await?;
        let tools = tools_value.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
        let result = if call_tool {
            let tool = request.tool.as_deref().ok_or("MCP tool name is required")?;
            let advertised = tools.iter().find(|entry| entry.get("name").and_then(Value::as_str) == Some(tool)).ok_or_else(|| format!("MCP tool {tool} was not advertised by the server"))?;
            if let Some(expected) = &request.expected_schema_sha256 {
                let schema = advertised.get("inputSchema").or_else(|| advertised.get("input_schema")).cloned().unwrap_or_else(|| json!({"type":"object"}));
                let actual = hex::encode(Sha256::digest(serde_json::to_vec(&schema).map_err(|error| error.to_string())?));
                if &actual != expected { return Err("MCP schema changed since search; invocation denied".into()); }
            }
            Some(rpc(&mut child, 3, "tools/call", json!({"name":tool,"arguments":request.arguments.clone().unwrap_or_else(|| json!({}))})).await?)
        } else { None };
        Ok::<_, String>(McpResult { server_name: request.name.clone(), capabilities: initialize.get("capabilities").cloned().unwrap_or_else(|| json!({})), tools, result, audit_id })
    }.await;
    let _ = notify(
        &mut child,
        "notifications/cancelled",
        json!({"reason":"client session complete"}),
    )
    .await;
    let _ = child.kill().await;
    outcome
}

async fn rpc(child: &mut Child, id: u64, method: &str, params: Value) -> Result<Value, String> {
    let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    child
        .stdin
        .as_mut()
        .ok_or("MCP stdin unavailable")?
        .write_all(format!("{}\n", request).as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    child
        .stdin
        .as_mut()
        .unwrap()
        .flush()
        .await
        .map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().ok_or("MCP stdout unavailable")?;
    let mut reader = BufReader::new(stdout);
    let response = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let mut line = String::new();
        loop {
            line.clear();
            if reader
                .read_line(&mut line)
                .await
                .map_err(|error| error.to_string())?
                == 0
            {
                return Err::<Value, String>("MCP server exited before replying".into());
            }
            let value: Value = serde_json::from_str(line.trim())
                .map_err(|error| format!("invalid MCP JSON-RPC response: {error}"))?;
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(value);
            }
        }
    })
    .await
    .map_err(|_| "MCP request timed out".to_string())??;
    child.stdout = Some(reader.into_inner());
    if let Some(error) = response.get("error") {
        return Err(format!("MCP JSON-RPC error: {error}"));
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

async fn notify(child: &mut Child, method: &str, params: Value) -> Result<(), String> {
    let request = json!({"jsonrpc":"2.0","method":method,"params":params});
    child
        .stdin
        .as_mut()
        .ok_or("MCP stdin unavailable")?
        .write_all(format!("{}\n", request).as_bytes())
        .await
        .map_err(|error| error.to_string())
}

fn stable_uuid(value: &str) -> Uuid {
    let digest = Sha256::digest(value.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}
fn excerpt(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
fn mark_conflicts(facts: &mut [MemoryFact]) {
    let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (index, fact) in facts.iter().enumerate() {
        groups
            .entry((fact.dimension.clone(), fact.key.clone()))
            .or_default()
            .push(index);
    }
    for indexes in groups.values() {
        for &left in indexes {
            for &right in indexes {
                if left != right && facts[left].value != facts[right].value {
                    let id = facts[right].id;
                    if !facts[left].conflicted_with.contains(&id) {
                        facts[left].conflicted_with.push(id);
                    }
                }
            }
        }
    }
}
fn markdown_sections(markdown: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let mut title = "Introduction".to_string();
    let mut body = String::new();
    for line in markdown.lines() {
        if line.starts_with('#') {
            if !body.trim().is_empty() {
                result.push((title, excerpt(body.trim(), 1000)));
                body.clear();
            }
            title = line.trim_start_matches('#').trim().to_owned();
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if !body.trim().is_empty() {
        result.push((title, excerpt(body.trim(), 1000)));
    }
    result
}
fn render_markdown(
    name: &str,
    notebook: &[NotebookEntry],
    artifacts: &[Artifact],
    facts: &[MemoryFact],
) -> String {
    let mut out = format!("# {name}\n\n");
    for entry in notebook {
        out.push_str(&format!(
            "## {:?}: {}\n\n{}\n\nEvidence: {}\n\n",
            entry.kind,
            entry.title,
            entry.markdown,
            entry.evidence_ids.join(", ")
        ));
    }
    out.push_str("## Artifacts\n\n");
    for artifact in artifacts {
        out.push_str(&format!(
            "- `{}` — {} bytes — SHA-256 `{}`\n",
            artifact.relative_path, artifact.size_bytes, artifact.sha256
        ));
    }
    out.push_str("\n## Traceable memory facts\n\n");
    for fact in facts {
        out.push_str(&format!(
            "- **{} / {}**: {} (sources: {})\n",
            fact.dimension,
            fact.key,
            fact.statement,
            fact.evidence
                .iter()
                .map(|item| format!("{}:{}", item.source_kind, item.source_id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}
fn write_project_bundle(
    path: &Path,
    name: &str,
    payload: &Value,
    notebook: &[NotebookEntry],
    artifacts: &[Artifact],
    facts: &[MemoryFact],
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("project.json", options)
        .map_err(|error| error.to_string())?;
    zip.write_all(
        serde_json::to_string_pretty(payload)
            .map_err(|error| error.to_string())?
            .as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    zip.start_file("notebook.md", options)
        .map_err(|error| error.to_string())?;
    zip.write_all(render_markdown(name, notebook, artifacts, facts).as_bytes())
        .map_err(|error| error.to_string())?;
    zip.finish().map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicts_keep_both_facts_and_link_sources() {
        let project_id = Uuid::new_v4();
        let mut facts = vec![
            MemoryFact {
                id: Uuid::new_v4(),
                project_id,
                conversation_id: None,
                run_id: None,
                dimension: "environment".into(),
                key: "python".into(),
                value: "3.10".into(),
                statement: "a".into(),
                evidence: vec![],
                conflicted_with: vec![],
                created_at: Utc::now(),
            },
            MemoryFact {
                id: Uuid::new_v4(),
                project_id,
                conversation_id: None,
                run_id: None,
                dimension: "environment".into(),
                key: "python".into(),
                value: "3.11".into(),
                statement: "b".into(),
                evidence: vec![],
                conflicted_with: vec![],
                created_at: Utc::now(),
            },
        ];
        mark_conflicts(&mut facts);
        assert_eq!(facts[0].conflicted_with, vec![facts[1].id]);
    }
    #[test]
    fn skill_sections_are_individually_hashable() {
        let parts = markdown_sections("# A\none\n# B\ntwo");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].0, "B");
    }

    #[tokio::test]
    async fn mcp_subprocess_never_launches_without_explicit_approval() {
        let repository = Repository::open_in_memory().unwrap();
        let result = run_mcp(
            &repository,
            McpRequest {
                project_id: Uuid::new_v4(),
                name: "unapproved".into(),
                command: "this-command-must-not-run".into(),
                args: vec![],
                approved: false,
                tool: None,
                arguments: None,
                expected_schema_sha256: None,
            },
            false,
        )
        .await;
        assert_eq!(
            result.unwrap_err(),
            "launching an MCP subprocess requires explicit approval"
        );
        assert!(
            repository
                .list_json::<Value>("mcp_audit")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn mcp_profiles_start_disabled_and_declaration_changes_revoke_access() {
        let now = Utc::now();
        let created = mcp_profile_from_request(
            SaveMcpServerRequest {
                id: None,
                name: "  papers  ".into(),
                command: "npx".into(),
                args: vec!["server-a".into()],
                cwd: None,
                timeout_secs: 60,
                env_bindings: vec![],
            },
            None,
            now,
        )
        .unwrap();
        assert!(!created.enabled);
        assert!(!created.launch_approved);
        assert!(created.approved_tools.is_empty());
        assert_eq!(created.name, "papers");

        let mut discovered = created.clone();
        discovered.enabled = true;
        discovered.launch_approved = true;
        discovered.approved_tools = vec!["search".into()];
        discovered.tools = vec![json!({"name":"search"})];
        discovered.last_inspected_at = Some(now);
        let changed = mcp_profile_from_request(
            SaveMcpServerRequest {
                id: Some(discovered.id),
                name: "papers".into(),
                command: "uvx".into(),
                args: vec!["server-b".into()],
                cwd: None,
                timeout_secs: 60,
                env_bindings: vec![],
            },
            Some(&discovered),
            now,
        )
        .unwrap();
        assert!(!changed.enabled);
        assert!(!changed.launch_approved);
        assert!(changed.approved_tools.is_empty());
        assert!(changed.tools.is_empty());
        assert!(changed.last_inspected_at.is_none());
    }
}

pub(crate) async fn invoke_configured_mcp_tool_v4(
    repository: &Repository,
    sessions: &McpSessionManager,
    credentials: &dyn CredentialVault,
    project_id: Uuid,
    server_id: Uuid,
    tool: &str,
    arguments: Value,
    expected_schema_sha256: String,
    schema_bound_run_approved: bool,
) -> Result<McpResult, String> {
    let mut profile = mcp_server_profile(repository, server_id)?;
    if !profile.enabled {
        return Err("MCP server is disabled".into());
    }
    if !profile.launch_approved {
        return Err("MCP server launch approval was revoked".into());
    }
    let persistently_approved = profile
        .approved_tools
        .iter()
        .any(|approved| approved == tool);
    if !persistently_approved && !schema_bound_run_approved {
        return Err(format!("MCP tool {tool} approval was revoked"));
    }
    let config = resolved_mcp_config(&profile, project_id, credentials)?;
    let invocation = match sessions
        .call(
            config,
            tool,
            arguments,
            profile.tool_catalog_sha256.as_deref(),
            Some(expected_schema_sha256.as_str()),
        )
        .await
    {
        Ok(invocation) => invocation,
        Err(error) => {
            let error = error.to_string();
            let stale = error.contains("stale") || error.contains("schema changed");
            profile.status = if stale {
                "stale".into()
            } else {
                "failed".into()
            };
            if stale {
                profile.enabled = false;
                profile.approved_tools.clear();
                profile.last_inspected_at = None;
            }
            profile.last_error = Some(error.clone());
            profile.updated_at = Utc::now();
            let _ = repository.put_json("mcp_server", &profile.id.to_string(), &profile);
            return Err(error);
        }
    };
    profile.status = "ready".into();
    profile.last_error = None;
    profile.tool_catalog_sha256 = Some(invocation.tool_catalog_sha256.clone());
    profile.catalog_generation = invocation.generation;
    profile.stderr_tail = (!invocation.stderr_tail.is_empty()).then_some(invocation.stderr_tail);
    profile.updated_at = Utc::now();
    repository
        .put_json("mcp_server", &profile.id.to_string(), &profile)
        .map_err(|error| error.to_string())?;
    let audit_id = Uuid::new_v4();
    let audit = json!({
        "id": audit_id,
        "project_id": project_id,
        "server": profile.name,
        "command": profile.command,
        "args": profile.args,
        "tool": tool,
        "approved": true,
        "approval_scope": if persistently_approved { "persistent" } else { "run_schema_bound" },
        "attempts": 1,
        "succeeded": true,
        "catalog_sha256": invocation.tool_catalog_sha256,
        "schema_sha256": expected_schema_sha256,
        "timestamp": Utc::now(),
    });
    repository
        .put_json("mcp_audit", &audit_id.to_string(), &audit)
        .map_err(|error| error.to_string())?;
    Ok(McpResult {
        server_name: invocation.server_name,
        capabilities: profile.capabilities,
        tools: profile.tools,
        result: Some(invocation.result),
        audit_id,
    })
}
