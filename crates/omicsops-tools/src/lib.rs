use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use omicsops_agent_core::{PlanToolAuthorizationV4, ToolPortV4};
use omicsops_protocol::{RunModeV4, ToolCallV4, ToolDescriptorV4, ToolEffectV4, ToolOutcomeV4};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};

#[async_trait]
pub trait ToolExecutorV4: Send + Sync {
    async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String>;
    async fn recover_result(&self, _call: &ToolCallV4) -> Result<Option<ToolOutcomeV4>, String> { Ok(None) }
    fn has_persistent_authorization(&self, _call: &ToolCallV4) -> bool {
        false
    }
    /// Dynamic authority for the generic MCP wrapper in Plan mode. Ordinary
    /// executors do not expose Network tools to planning; the desktop
    /// executor overrides this only after checking the concrete target.
    async fn authorize_plan_call(
        &self,
        _call: &ToolCallV4,
    ) -> Result<PlanToolAuthorizationV4, String> {
        Err("dynamic Plan tool authorization is unavailable".into())
    }
    /// Execute a call already authorized in Plan mode. This separate seam
    /// lets a host re-run dynamic checks with an explicit Plan context while
    /// preserving ordinary Execute authorization for the same wrapper.
    async fn execute_plan(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
        self.execute(call).await
    }
    async fn interrupt(&self, _run_id: uuid::Uuid) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ToolRegistryErrorV4 {
    #[error("unknown V4 tool {0}")]
    Unknown(String),
    #[error("tool {0} is forbidden in plan mode")]
    PlanModeDenied(String),
    #[error("invalid arguments for {0}: {1}")]
    InvalidArguments(String, String),
    #[error("tool {0} was not approved in the frozen V4 plan")]
    CapabilityDenied(String),
}

pub struct ToolRegistryV4 {
    definitions: BTreeMap<String, ToolDescriptorV4>,
    executor: Arc<dyn ToolExecutorV4>,
    execute_capabilities: Option<BTreeSet<String>>,
    read_slots: Semaphore,
    side_effect_lock: Arc<Mutex<()>>,
}

impl ToolRegistryV4 {
    pub fn new(
        definitions: Vec<ToolDescriptorV4>,
        executor: Arc<dyn ToolExecutorV4>,
    ) -> Result<Self, ToolRegistryErrorV4> {
        let mut mapped = BTreeMap::new();
        for definition in definitions {
            if mapped.insert(definition.id.clone(), definition).is_some() {
                return Err(ToolRegistryErrorV4::Unknown("duplicate tool".into()));
            }
        }
        Ok(Self {
            definitions: mapped,
            executor,
            execute_capabilities: None,
            read_slots: Semaphore::new(4),
            side_effect_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn with_execute_capabilities(mut self, capabilities: BTreeSet<String>) -> Self {
        self.execute_capabilities = Some(capabilities);
        self
    }

    pub fn with_side_effect_lock(mut self, lock: Arc<Mutex<()>>) -> Self {
        self.side_effect_lock = lock;
        self
    }

    fn authorize(
        &self,
        mode: RunModeV4,
        call: &ToolCallV4,
    ) -> Result<&ToolDescriptorV4, ToolRegistryErrorV4> {
        let definition = self
            .definitions
            .get(&call.tool_id)
            .ok_or_else(|| ToolRegistryErrorV4::Unknown(call.tool_id.clone()))?;
        let direct_only = matches!(
            call.tool_id.as_str(),
            "agent.route_request"
                | "agent.record_mcp_unavailable"
                | "agent.update_tasks"
                | "agent.read_tool_result"
        );
        let planning_allowed = !direct_only
            && (definition.effect == ToolEffectV4::ReadOnly
                || matches!(
                    call.tool_id.as_str(),
                    "agent.request_input" | "agent.propose_plan" | "use_mcp_tool"
                ));
        if mode == RunModeV4::Plan && !planning_allowed {
            return Err(ToolRegistryErrorV4::PlanModeDenied(call.tool_id.clone()));
        }
        let coordinator = matches!(
            call.tool_id.as_str(),
            "agent.request_input" | "agent.complete"
        );
        if mode == RunModeV4::Execute
            && !coordinator
            && self
                .execute_capabilities
                .as_ref()
                .is_some_and(|allowed| !allowed.contains(&call.tool_id))
        {
            return Err(ToolRegistryErrorV4::CapabilityDenied(call.tool_id.clone()));
        }
        validate_required(&definition.input_schema, &call.arguments)
            .map_err(|error| ToolRegistryErrorV4::InvalidArguments(call.tool_id.clone(), error))?;
        validate_browser_call(call)
            .map_err(|error| ToolRegistryErrorV4::InvalidArguments(call.tool_id.clone(), error))?;
        Ok(definition)
    }

    async fn authorize_plan_call(
        &self,
        call: &ToolCallV4,
    ) -> Result<PlanToolAuthorizationV4, String> {
        let definition = self
            .authorize(RunModeV4::Plan, call)
            .map_err(|error| error.to_string())?;
        if call.tool_id == "use_mcp_tool" {
            let authorization = self.executor.authorize_plan_call(call).await?;
            return match &authorization {
                PlanToolAuthorizationV4::Allowed {
                    effect: ToolEffectV4::ReadOnly,
                }
                | PlanToolAuthorizationV4::RequiresApproval {
                    effect: ToolEffectV4::ReadOnly,
                    ..
                } => Ok(authorization),
                PlanToolAuthorizationV4::Allowed { .. }
                | PlanToolAuthorizationV4::RequiresApproval { .. } => {
                    Err(ToolRegistryErrorV4::PlanModeDenied(call.tool_id.clone()).to_string())
                }
            };
        }
        if definition.effect == ToolEffectV4::ReadOnly
            || matches!(
                call.tool_id.as_str(),
                "agent.request_input" | "agent.propose_plan"
            )
        {
            return Ok(PlanToolAuthorizationV4::Allowed {
                effect: definition.effect,
            });
        }
        Err(ToolRegistryErrorV4::PlanModeDenied(call.tool_id.clone()).to_string())
    }
}

#[async_trait]
impl ToolPortV4 for ToolRegistryV4 {
    async fn recover_result(&self, call: &ToolCallV4) -> Result<Option<ToolOutcomeV4>, String> {
        self.authorize(RunModeV4::Execute, call).map_err(|error| error.to_string())?;
        self.executor.recover_result(call).await
    }
    fn descriptors(&self, mode: RunModeV4) -> Vec<ToolDescriptorV4> {
        self.definitions
            .values()
            .filter(|definition| match mode {
                RunModeV4::Plan => {
                    !matches!(
                        definition.id.as_str(),
                        "agent.route_request"
                            | "agent.record_mcp_unavailable"
                            | "agent.update_tasks"
                            | "agent.read_tool_result"
                    ) && (definition.effect == ToolEffectV4::ReadOnly
                        || matches!(
                            definition.id.as_str(),
                            "agent.request_input" | "agent.propose_plan" | "use_mcp_tool"
                        ))
                }
                RunModeV4::Execute => {
                    definition.id != "agent.propose_plan"
                        && self.execute_capabilities.as_ref().is_none_or(|allowed| {
                            matches!(
                                definition.id.as_str(),
                                "agent.request_input" | "agent.complete"
                            ) || allowed.contains(&definition.id)
                        })
                }
            })
            .cloned()
            .collect()
    }

    fn effect(&self, tool_id: &str) -> Option<ToolEffectV4> {
        self.definitions
            .get(tool_id)
            .map(|definition| definition.effect)
    }

    fn validate(&self, mode: RunModeV4, call: &ToolCallV4) -> Result<(), String> {
        self.authorize(mode, call)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn has_persistent_authorization(&self, call: &ToolCallV4) -> bool {
        self.executor.has_persistent_authorization(call)
    }

    async fn authorize_plan_call(
        &self,
        call: &ToolCallV4,
    ) -> Result<PlanToolAuthorizationV4, String> {
        self.authorize_plan_call(call).await
    }

    async fn execute(&self, mode: RunModeV4, call: ToolCallV4) -> Result<ToolOutcomeV4, String> {
        let plan_effect = if mode == RunModeV4::Plan {
            match ToolPortV4::authorize_plan_call(self, &call).await? {
                PlanToolAuthorizationV4::Allowed { effect } => Some(effect),
                PlanToolAuthorizationV4::RequiresApproval { .. } => {
                    return Err(format!(
                        "tool {} requires explicit approval in plan mode",
                        call.tool_id
                    ));
                }
            }
        } else {
            None
        };
        let effect = self
            .authorize(mode, &call)
            .map_err(|error| error.to_string())?
            .effect;
        let effect = plan_effect.unwrap_or(effect);
        if effect == ToolEffectV4::ReadOnly {
            let _permit = self
                .read_slots
                .acquire()
                .await
                .map_err(|_| "V4 read-only tool scheduler is closed".to_string())?;
            if mode == RunModeV4::Plan {
                self.executor.execute_plan(&call).await
            } else {
                self.executor.execute(&call).await
            }
        } else if mode == RunModeV4::Plan {
            // The only non-read-only descriptor visible in Plan is the
            // generic MCP wrapper; its concrete target was authorized above.
            self.executor.execute_plan(&call).await
        } else {
            let _guard = self.side_effect_lock.lock().await;
            self.executor.execute(&call).await
        }
    }

    async fn interrupt(&self, run_id: uuid::Uuid) -> Result<(), String> {
        self.executor.interrupt(run_id).await
    }
}

fn validate_required(schema: &Value, input: &Value) -> Result<(), String> {
    validate_required_at(schema, input, "")
}

fn validate_required_at(schema: &Value, input: &Value, path: &str) -> Result<(), String> {
    let object = input
        .as_object()
        .ok_or_else(|| format!("{} must be an object", display_path(path)))?;
    for field in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !object.contains_key(field) {
            return Err(format!("missing required field {}", join_path(path, field)));
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (field, field_schema) in properties {
            let Some(value) = object.get(field) else {
                continue;
            };
            let field_path = join_path(path, field);
            if let Some(min_length) = field_schema.get("minLength").and_then(Value::as_u64)
                && value
                    .as_str()
                    .is_some_and(|text| text.chars().count() < min_length as usize)
            {
                return Err(format!(
                    "field {field_path} must contain at least {min_length} character(s)"
                ));
            }
            if field_schema.get("type").and_then(Value::as_str) == Some("object") {
                validate_required_at(field_schema, value, &field_path)?;
            }
            if let (Some(items), Some(values)) = (field_schema.get("items"), value.as_array()) {
                for (index, item) in values.iter().enumerate() {
                    if items.get("type").and_then(Value::as_str) == Some("object") {
                        validate_required_at(items, item, &format!("{field_path}[{index}]"))?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn join_path(prefix: &str, field: &str) -> String {
    if prefix.is_empty() {
        field.into()
    } else {
        format!("{prefix}.{field}")
    }
}

fn display_path(path: &str) -> &str {
    if path.is_empty() { "arguments" } else { path }
}

pub fn builtin_tool_definitions_v4() -> Vec<ToolDescriptorV4> {
    vec![
        descriptor(
            "project.list",
            "List project-relative files",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","properties":{"path":{"type":"string"}}}),
        ),
        descriptor(
            "project.read",
            "Read a project-relative text file",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        ),
        descriptor(
            "search_skills",
            "Search enabled Skill metadata and matching section names without loading instructions",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"},"limit":{"type":"integer"}}}),
        ),
        descriptor(
            "use_skill",
            "Load and freeze selected sections from one enabled Skill package",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["skill_id"],"properties":{"skill_id":{"type":"string"},"sections":{"type":"array"}}}),
        ),
        descriptor(
            "search_memory",
            "Search project Memory with Unicode lexical matching, Chinese n-grams, recency, and RRF",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"},"dimension":{"type":"string"},"limit":{"type":"integer"}}}),
        ),
        descriptor(
            "search_mcp_tools",
            "Search stored MCP tool descriptions without launching a server",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"},"limit":{"type":"integer"}}}),
        ),
        descriptor(
            "use_mcp_tool",
            "Call one configured, enabled, launch-approved MCP stdio tool. A persistently approved tool is callable immediately; otherwise the Host can require explicit schema-bound approval for this run",
            ToolEffectV4::Network,
            // `catalog_sha256` is required by the dynamic Plan gate, but it
            // remains optional at the shared descriptor boundary so legacy
            // Execute calls (which predate catalog binding) stay readable.
            json!({"type":"object","required":["server_id","tool","arguments","schema_sha256"],"properties":{"server_id":{"type":"string"},"tool":{"type":"string"},"arguments":{"type":"object"},"catalog_sha256":{"type":"string","minLength":1},"schema_sha256":{"type":"string","minLength":1}}}),
        ),
        descriptor(
            "agent.route_request",
            "Classify the current ordinary Agent request and its task shape before any task tool is used. Research retrieval includes papers, external databases, current web evidence, and cross-source verification",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["route","task_shape","reason"],"properties":{"route":{"type":"string","enum":["research_retrieval","adaptive"]},"task_shape":{"type":"string","enum":["fast","multi_step"]},"reason":{"type":"string","minLength":1}}}),
        ),
        descriptor(
            "agent.read_tool_result",
            "Read a bounded page of an original tool outcome or delegation trace from this run using its result_reference. Delegation traces use field data. Offsets and limits are UTF-8 bytes; use next_offset for the next page. Cannot read other runs or arbitrary files.",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["sequence","event_hash","field","offset","limit"],"properties":{
                "sequence":{"type":"integer","minimum":0},"event_hash":{"type":"string","minLength":1},
                "field":{"type":"string","enum":["model_content","data"]},
                "offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":4,"maximum":8192}
            }}),
        ),
        descriptor(
            "agent.record_mcp_unavailable",
            "Record the structured reason that no discovered professional MCP can be called for this research retrieval",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["reason","searched_query","candidate_count"],"properties":{"reason":{"type":"string","minLength":1},"searched_query":{"type":"string","minLength":1},"candidate_count":{"type":"integer","minimum":0}}}),
        ),
        descriptor(
            "browser_setup",
            "Connect or launch the authorized OmicsOps real-browser bridge. This is never available in Plan mode and Full Access cannot bypass browser authorization",
            ToolEffectV4::Network,
            json!({"type":"object","required":["session"],"properties":{"session":{"type":"string","enum":["shared","workspace"]},"launch_if_needed":{"type":"boolean"}}}),
        ),
        descriptor(
            "web_search",
            "Search through the user's real browser and return the results tab identity. Never sends prompts to web AI services",
            ToolEffectV4::Network,
            json!({"type":"object","required":["session","query","provider"],"properties":{"session":{"type":"string","enum":["shared","workspace"]},"query":{"type":"string","minLength":1},"provider":{"type":"string","enum":["google","bing","duckduckgo"]}}}),
        ),
        descriptor(
            "web_open_tab",
            "Open one HTTP(S) URL in the authorized real browser and ledger the tab to the current run",
            ToolEffectV4::Network,
            json!({"type":"object","required":["session","url"],"properties":{"session":{"type":"string","enum":["shared","workspace"]},"url":{"type":"string","minLength":1}}}),
        ),
        descriptor(
            "web_scan",
            "Wait for page stability and return a structured scan. Re-scan after every navigation or material page change",
            ToolEffectV4::Network,
            json!({"type":"object","required":["session","tab_id","target_host","page_kind"],"properties":{"session":{"type":"string","enum":["shared","workspace"]},"tab_id":{"type":"integer"},"target_host":{"type":"string","minLength":1},"page_kind":{"type":"string","enum":["search_results","source"]}}}),
        ),
        descriptor(
            "web_execute_js",
            "Execute a bounded browser operation through tabs/CDP. Script source is size-limited and web AI prompting is forbidden",
            ToolEffectV4::Network,
            json!({"type":"object","required":["session","tab_id","target_host","script"],"properties":{"session":{"type":"string","enum":["shared","workspace"]},"tab_id":{"type":"integer"},"target_host":{"type":"string","minLength":1},"script":{"type":"string","maxLength":16000}}}),
        ),
        descriptor(
            "web_screenshot",
            "Capture a browser tab screenshot to the project without storing image bytes in SQLite",
            ToolEffectV4::Network,
            json!({"type":"object","required":["session","tab_id","target_host","relative_path"],"properties":{"session":{"type":"string","enum":["shared","workspace"]},"tab_id":{"type":"integer"},"target_host":{"type":"string","minLength":1},"relative_path":{"type":"string","minLength":1}}}),
        ),
        descriptor(
            "web_save_assets",
            "Copy browser-staged downloads to project-relative regular files and record size plus SHA-256",
            ToolEffectV4::Network,
            json!({"type":"object","required":["session","target_host","assets"],"properties":{"session":{"type":"string","enum":["shared","workspace"]},"target_host":{"type":"string","minLength":1},"assets":{"type":"array","minItems":1,"items":{"type":"object","required":["relative_path","source_url"],"properties":{"relative_path":{"type":"string","minLength":1},"source_url":{"type":"string","minLength":1}}}}}}),
        ),
        descriptor(
            "agent.update_tasks",
            "Create or revise the Host-persisted read-only task list for an ordinary multi-step Agent run. This is a coordinator, not an approved Plan",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["schema_version","expected_revision","change_summary","tasks"],"properties":{"schema_version":{"type":"integer","const":4},"expected_revision":{"type":"integer","minimum":0},"change_summary":{"type":"string","minLength":1},"tasks":{"type":"array","minItems":2,"maxItems":12,"items":{"type":"object","required":["id","title","status"],"properties":{"id":{"type":"string","minLength":1,"maxLength":64},"title":{"type":"string","minLength":1,"maxLength":240},"status":{"type":"string","enum":["pending","in_progress","completed","blocked"]},"blocked_reason":{"type":"string","minLength":1,"maxLength":500}}}}}}),
        ),
        descriptor(
            "agent.request_input",
            "Request user input",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["question"],"properties":{"question":{"type":"string"},"reason":{"type":"string","enum":["scope","decision","missing_data","blocker"],"default":"decision"}}}),
        ),
        descriptor(
            "agent.propose_plan",
            "Submit a complete ExecutionPlanV4",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["schema_version","objective","steps","completion_criteria","requested_capabilities"],"properties":{}}),
        ),
        descriptor(
            "runtime.execute",
            "Execute code in a persistent run-scoped kernel",
            ToolEffectV4::Runtime,
            json!({"type":"object","required":["language","code"],"properties":{"language":{"type":"string"},"environment":{"type":"string","default":"system"},"code":{"type":"string"},"capture_paths":{"type":"array"},"analysis":{"type":"object","required":["analysis_type","input_dataset_ids","sample_ids","method","parameters"],"properties":{"analysis_type":{"type":"string"},"input_dataset_ids":{"type":"array"},"sample_ids":{"type":"array"},"method":{"type":"string"},"parameters":{"type":"object"},"software_requirements":{"type":"array"},"database_versions":{"type":"object"},"random_seed":{"type":["integer","null"]}}}}}),
        ),
        descriptor(
            "science.register_dataset",
            "Register a dataset after Host verification of its path, size, and SHA-256",
            ToolEffectV4::Mutating,
            json!({"type":"object","required":["modality","species","sample_ids","matrix_shape","stage","path"],"properties":{"modality":{"type":"string"},"species":{"type":"string"},"sample_ids":{"type":"array"},"matrix_shape":{"type":"array"},"stage":{"type":"string","enum":["raw","processed"]},"path":{"type":"string"}}}),
        ),
        descriptor(
            "science.record_evidence",
            "Record a claim linked to verified artifacts or literature sources",
            ToolEffectV4::Mutating,
            json!({"type":"object","required":["claim","sources","strength"],"properties":{"claim":{"type":"string"},"sources":{"type":"array"},"strength":{"type":"string","enum":["exploratory","supporting","strong"]},"conflicts_with":{"type":"array"}}}),
        ),
        descriptor(
            "runtime.environment.ensure",
            "Verify an immutable system environment, or create/reuse the exact frozen project-scoped SSH Micromamba environment",
            ToolEffectV4::Runtime,
            json!({"type":"object","required":["environment","language"],"properties":{"environment":{"type":"string"},"language":{"type":"string"}}}),
        ),
        descriptor(
            "runtime.rebuild",
            "Stop and rebuild a persistent kernel explicitly",
            ToolEffectV4::Runtime,
            json!({"type":"object","required":["language"],"properties":{"language":{"type":"string"},"environment":{"type":"string"}}}),
        ),
        descriptor(
            "runtime.interrupt",
            "Interrupt and discard the selected persistent kernel",
            ToolEffectV4::Runtime,
            json!({"type":"object","required":["language"],"properties":{"language":{"type":"string"},"environment":{"type":"string"}}}),
        ),
        descriptor(
            "artifact.verify",
            "Verify a project-relative artifact",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        ),
        descriptor(
            "agent.delegate",
            "Run a bounded read-only DAG of temporary tasks. Nodes cannot expand the frozen run capabilities, write, or delegate recursively",
            ToolEffectV4::Delegation,
            json!({"type":"object","required":["schema_version","nodes"],"properties":{"schema_version":{"type":"integer","const":4},"nodes":{"type":"array","maxItems":8,"items":{"type":"object","required":["id","objective","budget","capabilities","output_schema","isolation"],"properties":{"id":{"type":"string"},"objective":{"type":"string"},"dependencies":{"type":"array","items":{"type":"string"}},"budget":{"type":"object","required":["max_turns","max_tool_calls"],"properties":{"max_turns":{"type":"integer","maximum":4},"max_tool_calls":{"type":"integer","maximum":8}}},"capabilities":{"type":"array","items":{"type":"string"}},"output_schema":{"type":"object"},"isolation":{"type":"string","enum":["read_only_project","evidence_only"]}}}}}}),
        ),
        descriptor(
            "agent.complete",
            "Propose completion with a user-visible final Markdown answer and evidence for every frozen completion criterion; the Host verifier and read-only Reviewer decide whether the run can finish",
            ToolEffectV4::Mutating,
            json!({"type":"object","required":["schema_version","summary","answer_markdown","criteria"],"properties":{"schema_version":{"type":"integer","const":4},"summary":{"type":"string"},"answer_markdown":{"type":"string","minLength":1,"description":"User-visible final response rendered as Markdown. Include the actual result, key evidence or artifacts, and limitations or follow-up actions."},"criteria":{"type":"array","items":{"type":"object","required":["criterion","evidence"],"properties":{"criterion":{"type":"string"},"evidence":{"type":"array","items":{"type":"object","required":["kind"],"properties":{"kind":{"type":"string","enum":["event","artifact","evidence"]},"sequence":{"type":"integer"},"artifact_id":{"type":"string"},"evidence_id":{"type":"string"}}}}}}}}}),
        ),
    ]
}

fn descriptor(
    id: &str,
    description: &str,
    effect: ToolEffectV4,
    input_schema: Value,
) -> ToolDescriptorV4 {
    ToolDescriptorV4 {
        id: id.into(),
        description: description.into(),
        input_schema,
        effect,
    }
}

fn validate_browser_call(call: &ToolCallV4) -> Result<(), String> {
    fn concrete_host(value: &str) -> Result<String, String> {
        let host = value.trim_end_matches('.').to_ascii_lowercase();
        if value.trim() != value
            || host.is_empty()
            || host.len() > 253
            || host.contains('*')
            || !host.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
            })
        {
            return Err("target_host must be a concrete host".into());
        }
        Ok(host)
    }

    fn safe_url(value: &str) -> Result<url::Url, String> {
        let parsed = url::Url::parse(value).map_err(|_| "browser URL is invalid")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("browser URL must be credential-free HTTP(S)".into());
        }
        if parsed
            .query_pairs()
            .any(|(key, _)| sensitive_browser_query_key(&key))
        {
            return Err("browser URL contains a sensitive query parameter".into());
        }
        Ok(parsed)
    }

    match call.tool_id.as_str() {
        "web_open_tab" => {
            safe_url(
                call.arguments
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or("url is required")?,
            )?;
        }
        "web_scan" | "web_screenshot" => {
            concrete_host(
                call.arguments
                    .get("target_host")
                    .and_then(Value::as_str)
                    .ok_or("target_host is required")?,
            )?;
        }
        "web_execute_js" => {
            concrete_host(
                call.arguments
                    .get("target_host")
                    .and_then(Value::as_str)
                    .ok_or("target_host is required")?,
            )?;
            let script = call
                .arguments
                .get("script")
                .and_then(Value::as_str)
                .ok_or("script is required")?
                .to_ascii_lowercase();
            if [
                "document.cookie",
                "localstorage",
                "sessionstorage",
                "indexeddb",
                "navigator.credentials",
                "navigator.clipboard",
                "fetch(",
                "xmlhttprequest",
                "sendbeacon",
                "websocket",
                "eventsource",
            ]
            .iter()
            .any(|forbidden| script.contains(forbidden))
            {
                return Err("script requests a forbidden sensitive browser API".into());
            }
        }
        "web_save_assets" => {
            let target_host = concrete_host(
                call.arguments
                    .get("target_host")
                    .and_then(Value::as_str)
                    .ok_or("target_host is required")?,
            )?;
            for asset in call
                .arguments
                .get("assets")
                .and_then(Value::as_array)
                .ok_or("assets are required")?
            {
                let parsed = safe_url(
                    asset
                        .get("source_url")
                        .and_then(Value::as_str)
                        .ok_or("asset source_url is required")?,
                )?;
                if parsed.host_str().map(str::to_ascii_lowercase).as_deref()
                    != Some(target_host.as_str())
                {
                    return Err("asset source_url does not match target_host".into());
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn sensitive_browser_query_key(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace('-', "_");
    matches!(normalized.as_str(), "access_token" | "api_key")
        || normalized.split('_').any(|part| {
            matches!(
                part,
                "auth"
                    | "authorization"
                    | "code"
                    | "key"
                    | "password"
                    | "secret"
                    | "signature"
                    | "token"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Noop;
    #[async_trait]
    impl ToolExecutorV4 for Noop {
        async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
            Ok(ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: true,
                model_content: "ok".into(),
                data: json!({}),
                provenance: vec![],
            })
        }
    }

    struct DynamicPlanExecutor {
        authorization: PlanToolAuthorizationV4,
        dispatches: AtomicUsize,
    }

    #[async_trait]
    impl ToolExecutorV4 for DynamicPlanExecutor {
        async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
            self.dispatches.fetch_add(1, Ordering::SeqCst);
            Ok(ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: true,
                model_content: "unexpected dispatch".into(),
                data: json!({}),
                provenance: vec![],
            })
        }

        async fn authorize_plan_call(
            &self,
            _call: &ToolCallV4,
        ) -> Result<PlanToolAuthorizationV4, String> {
            Ok(self.authorization.clone())
        }
    }

    #[tokio::test]
    async fn dynamic_plan_mcp_authorization_must_be_read_only_before_dispatch() {
        for authorization in [
            PlanToolAuthorizationV4::Allowed {
                effect: ToolEffectV4::Network,
            },
            PlanToolAuthorizationV4::RequiresApproval {
                effect: ToolEffectV4::Network,
                reason: "untrusted dynamic effect".into(),
            },
        ] {
            let executor = Arc::new(DynamicPlanExecutor {
                authorization,
                dispatches: AtomicUsize::new(0),
            });
            let registry =
                ToolRegistryV4::new(builtin_tool_definitions_v4(), executor.clone()).unwrap();
            let error = registry
                .execute(
                    RunModeV4::Plan,
                    ToolCallV4 {
                        call_id: "dynamic-mcp".into(),
                        tool_id: "use_mcp_tool".into(),
                        arguments: json!({
                            "server_id": "server",
                            "tool": "read",
                            "arguments": {},
                            "catalog_sha256": "catalog",
                            "schema_sha256": "schema"
                        }),
                    },
                )
                .await
                .unwrap_err();
            assert!(error.contains("forbidden in plan mode"), "{error}");
            assert_eq!(executor.dispatches.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn plan_mode_hard_denies_runtime() {
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop)).unwrap();
        let planning = registry
            .descriptors(RunModeV4::Plan)
            .into_iter()
            .map(|tool| tool.id)
            .collect::<BTreeSet<_>>();
        assert!(planning.contains("search_skills"));
        assert!(planning.contains("use_skill"));
        assert!(planning.contains("search_memory"));
        assert!(planning.contains("search_mcp_tools"));
        assert!(!planning.contains("agent.route_request"));
        assert!(!planning.contains("agent.record_mcp_unavailable"));
        assert!(!planning.contains("agent.update_tasks"));
        assert!(!planning.contains("agent.read_tool_result"));
        // The generic MCP wrapper remains visible so the planner can request
        // a concrete target; its Network effect is dynamically gated by the
        // host rather than being treated as a permanently read-only tool.
        assert!(planning.contains("use_mcp_tool"));
        let error = registry
            .execute(
                RunModeV4::Plan,
                ToolCallV4 {
                    call_id: "1".into(),
                    tool_id: "runtime.execute".into(),
                    arguments: json!({"language":"python","code":"x=1"}),
                },
            )
            .await
            .unwrap_err();
        assert!(error.contains("forbidden in plan mode"));
        let route_error = registry
            .validate(
                RunModeV4::Plan,
                &ToolCallV4 {
                    call_id: "route".into(),
                    tool_id: "agent.route_request".into(),
                    arguments: json!({"route":"adaptive","task_shape":"fast","reason":"not a direct run"}),
                },
            )
            .unwrap_err();
        assert!(route_error.contains("forbidden in plan mode"));
        let result = registry
            .execute(
                RunModeV4::Plan,
                ToolCallV4 {
                    call_id: "3".into(),
                    tool_id: "artifact.verify".into(),
                    arguments: json!({"path":"a"}),
                },
            )
            .await
            .unwrap();
        assert!(result.succeeded);
    }

    #[test]
    fn ordinary_task_coordinator_is_schema_bounded_and_direct_run_scoped() {
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop))
            .unwrap()
            .with_execute_capabilities(BTreeSet::from(["agent.update_tasks".into()]));
        let valid = ToolCallV4 {
            call_id: "tasks-1".into(),
            tool_id: "agent.update_tasks".into(),
            arguments: json!({
                "schema_version":4,
                "expected_revision":0,
                "change_summary":"initial breakdown",
                "tasks":[
                    {"id":"discover","title":"Discover context","status":"completed"},
                    {"id":"execute","title":"Execute work","status":"in_progress"}
                ]
            }),
        };
        assert!(registry.validate(RunModeV4::Execute, &valid).is_ok());
        let mut invalid = valid;
        invalid.arguments["tasks"] = json!([
            {"id":"discover","title":"Discover context"},
            {"id":"execute","title":"Execute work","status":"pending"}
        ]);
        assert!(registry.validate(RunModeV4::Execute, &invalid).is_err());
    }

    #[test]
    fn ordinary_route_requires_an_explicit_task_shape() {
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop)).unwrap();
        let mut call = ToolCallV4 {
            call_id: "route".into(),
            tool_id: "agent.route_request".into(),
            arguments: json!({"route":"adaptive","reason":"bounded request"}),
        };
        assert!(registry.validate(RunModeV4::Execute, &call).is_err());
        call.arguments["task_shape"] = json!("fast");
        assert!(registry.validate(RunModeV4::Execute, &call).is_ok());
    }

    #[test]
    fn browser_validation_rejects_credentials_sensitive_queries_and_browser_secrets() {
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop)).unwrap();
        for url in [
            "https://user:secret@example.org/paper",
            "https://example.org/paper?token=secret",
            "https://example.org/paper?session_token=secret",
        ] {
            assert!(
                registry
                    .validate(
                        RunModeV4::Execute,
                        &ToolCallV4 {
                            call_id: "unsafe-url".into(),
                            tool_id: "web_open_tab".into(),
                            arguments: json!({"session":"shared","url":url}),
                        },
                    )
                    .is_err()
            );
        }
        assert!(
            registry
                .validate(
                    RunModeV4::Execute,
                    &ToolCallV4 {
                        call_id: "unsafe-js".into(),
                        tool_id: "web_execute_js".into(),
                        arguments: json!({
                            "session":"shared",
                            "tab_id":1,
                            "target_host":"example.org",
                            "script":"return document.cookie"
                        }),
                    },
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn any_custom_read_only_descriptor_is_visible_and_executable_in_plan() {
        let registry = ToolRegistryV4::new(
            vec![descriptor(
                "custom.inspect",
                "custom read-only inspection",
                ToolEffectV4::ReadOnly,
                json!({"type":"object"}),
            )],
            Arc::new(Noop),
        )
        .unwrap();
        assert!(
            registry
                .descriptors(RunModeV4::Plan)
                .iter()
                .any(|tool| tool.id == "custom.inspect")
        );
        registry
            .validate(
                RunModeV4::Plan,
                &ToolCallV4 {
                    call_id: "custom".into(),
                    tool_id: "custom.inspect".into(),
                    arguments: json!({}),
                },
            )
            .unwrap();
        assert!(
            registry
                .execute(
                    RunModeV4::Plan,
                    ToolCallV4 {
                        call_id: "custom".into(),
                        tool_id: "custom.inspect".into(),
                        arguments: json!({}),
                    },
                )
                .await
                .unwrap()
                .succeeded
        );
    }

    #[test]
    fn all_non_read_only_effects_are_denied_in_plan_by_default() {
        for (id, effect) in [
            ("custom.runtime", ToolEffectV4::Runtime),
            ("custom.mutating", ToolEffectV4::Mutating),
            ("custom.network", ToolEffectV4::Network),
            ("custom.delegation", ToolEffectV4::Delegation),
        ] {
            let registry = ToolRegistryV4::new(
                vec![descriptor(id, id, effect, json!({"type":"object"}))],
                Arc::new(Noop),
            )
            .unwrap();
            let error = registry
                .validate(
                    RunModeV4::Plan,
                    &ToolCallV4 {
                        call_id: id.into(),
                        tool_id: id.into(),
                        arguments: json!({}),
                    },
                )
                .unwrap_err();
            assert!(error.contains("forbidden in plan mode"), "{id}: {error}");
            assert!(
                registry
                    .descriptors(RunModeV4::Plan)
                    .iter()
                    .all(|tool| tool.id != id)
            );
        }
    }

    #[test]
    fn delegation_is_denied_in_plan_even_when_execute_capability_exists() {
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop))
            .unwrap()
            .with_execute_capabilities(BTreeSet::from(["agent.delegate".into()]));
        let error = registry
            .validate(
                RunModeV4::Plan,
                &ToolCallV4 {
                    call_id: "delegate-plan".into(),
                    tool_id: "agent.delegate".into(),
                    arguments: json!({"schema_version":4,"nodes":[]}),
                },
            )
            .unwrap_err();
        assert!(error.contains("forbidden in plan mode"));
        assert!(
            !registry
                .descriptors(RunModeV4::Plan)
                .iter()
                .any(|tool| tool.id == "agent.delegate")
        );
    }

    #[test]
    fn runtime_analysis_schema_rejects_missing_nested_sample_ids_before_approval() {
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop)).unwrap();
        let call = ToolCallV4 {
            call_id: "literature".into(),
            tool_id: "runtime.execute".into(),
            arguments: json!({
                "language":"python",
                "code":"print('search')",
                "analysis":{
                    "analysis_type":"literature_search",
                    "input_dataset_ids":[],
                    "method":"PubMed",
                    "parameters":{}
                }
            }),
        };

        let error = registry.validate(RunModeV4::Execute, &call).unwrap_err();
        assert!(error.contains("analysis.sample_ids"));
    }

    #[test]
    fn delegation_requires_frozen_execute_capability_and_is_never_visible_in_plan_mode() {
        let graph = ToolCallV4 {
            call_id: "delegate".into(),
            tool_id: "agent.delegate".into(),
            arguments: json!({"schema_version":4,"nodes":[]}),
        };
        let denied = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop))
            .unwrap()
            .with_execute_capabilities(BTreeSet::new());
        assert!(
            denied
                .validate(RunModeV4::Execute, &graph)
                .unwrap_err()
                .contains("not approved")
        );
        assert!(
            denied
                .validate(RunModeV4::Plan, &graph)
                .unwrap_err()
                .contains("forbidden")
        );
        let approved = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop))
            .unwrap()
            .with_execute_capabilities(BTreeSet::from(["agent.delegate".into()]));
        approved.validate(RunModeV4::Execute, &graph).unwrap();
        assert_eq!(
            approved.effect("agent.delegate"),
            Some(ToolEffectV4::Delegation)
        );
        assert!(
            approved
                .descriptors(RunModeV4::Plan)
                .iter()
                .all(|tool| tool.id != "agent.delegate")
        );
    }

    #[test]
    fn completion_schema_requires_non_empty_user_visible_answer() {
        let registry = ToolRegistryV4::new(builtin_tool_definitions_v4(), Arc::new(Noop)).unwrap();
        let base = json!({
            "schema_version": 4,
            "summary": "done",
            "criteria": []
        });
        let missing = ToolCallV4 {
            call_id: "missing-answer".into(),
            tool_id: "agent.complete".into(),
            arguments: base.clone(),
        };
        assert!(registry.validate(RunModeV4::Execute, &missing).is_err());

        let mut empty = base;
        empty["answer_markdown"] = json!("");
        let empty = ToolCallV4 {
            call_id: "empty-answer".into(),
            tool_id: "agent.complete".into(),
            arguments: empty,
        };
        assert!(registry.validate(RunModeV4::Execute, &empty).is_err());
    }

    struct ConcurrencyProbe {
        active: AtomicUsize,
        maximum: AtomicUsize,
    }
    #[async_trait]
    impl ToolExecutorV4 for ConcurrencyProbe {
        async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(ToolOutcomeV4 {
                call_id: call.call_id.clone(),
                tool_id: call.tool_id.clone(),
                succeeded: true,
                model_content: "ok".into(),
                data: json!({}),
                provenance: vec![],
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn read_only_calls_are_bounded_to_four_and_side_effects_are_serial() {
        let probe = Arc::new(ConcurrencyProbe {
            active: AtomicUsize::new(0),
            maximum: AtomicUsize::new(0),
        });
        let registry = Arc::new(
            ToolRegistryV4::new(
                vec![
                    descriptor(
                        "read",
                        "read",
                        ToolEffectV4::ReadOnly,
                        json!({"type":"object"}),
                    ),
                    descriptor(
                        "write",
                        "write",
                        ToolEffectV4::Mutating,
                        json!({"type":"object"}),
                    ),
                ],
                probe.clone(),
            )
            .unwrap(),
        );
        let reads = (0..8)
            .map(|index| {
                let registry = registry.clone();
                tokio::spawn(async move {
                    registry
                        .execute(
                            RunModeV4::Execute,
                            ToolCallV4 {
                                call_id: index.to_string(),
                                tool_id: "read".into(),
                                arguments: json!({}),
                            },
                        )
                        .await
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        for task in reads {
            task.await.unwrap();
        }
        assert!((2..=4).contains(&probe.maximum.load(Ordering::SeqCst)));

        probe.maximum.store(0, Ordering::SeqCst);
        let writes = (0..4)
            .map(|index| {
                let registry = registry.clone();
                tokio::spawn(async move {
                    registry
                        .execute(
                            RunModeV4::Execute,
                            ToolCallV4 {
                                call_id: index.to_string(),
                                tool_id: "write".into(),
                                arguments: json!({}),
                            },
                        )
                        .await
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        for task in writes {
            task.await.unwrap();
        }
        assert_eq!(probe.maximum.load(Ordering::SeqCst), 1);
    }
}
