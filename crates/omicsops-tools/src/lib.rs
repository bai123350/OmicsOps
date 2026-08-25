use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use omicsops_agent_core::ToolPortV4;
use omicsops_protocol::{RunModeV4, ToolCallV4, ToolDescriptorV4, ToolEffectV4, ToolOutcomeV4};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{Mutex, Semaphore};

#[async_trait]
pub trait ToolExecutorV4: Send + Sync {
    async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String>;
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
        let planning_allowed = matches!(
            call.tool_id.as_str(),
            "project.list"
                | "project.read"
                | "search_skills"
                | "use_skill"
                | "search_memory"
                | "search_mcp_tools"
                | "agent.request_input"
                | "agent.propose_plan"
        );
        if mode == RunModeV4::Plan
            && (!planning_allowed || definition.effect != ToolEffectV4::ReadOnly)
        {
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
        Ok(definition)
    }
}

#[async_trait]
impl ToolPortV4 for ToolRegistryV4 {
    fn descriptors(&self, mode: RunModeV4) -> Vec<ToolDescriptorV4> {
        self.definitions
            .values()
            .filter(|definition| match mode {
                RunModeV4::Plan => matches!(
                    definition.id.as_str(),
                    "project.list"
                        | "project.read"
                        | "search_skills"
                        | "use_skill"
                        | "search_memory"
                        | "search_mcp_tools"
                        | "agent.request_input"
                        | "agent.propose_plan"
                ),
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

    async fn execute(&self, mode: RunModeV4, call: ToolCallV4) -> Result<ToolOutcomeV4, String> {
        let effect = self
            .authorize(mode, &call)
            .map_err(|error| error.to_string())?
            .effect;
        if effect == ToolEffectV4::ReadOnly {
            let _permit = self
                .read_slots
                .acquire()
                .await
                .map_err(|_| "V4 read-only tool scheduler is closed".to_string())?;
            self.executor.execute(&call).await
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
            json!({"type":"object","required":["server_id","tool","arguments","schema_sha256"],"properties":{"server_id":{"type":"string"},"tool":{"type":"string"},"arguments":{"type":"object"},"schema_sha256":{"type":"string"}}}),
        ),
        descriptor(
            "agent.request_input",
            "Request user input",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["question"],"properties":{"question":{"type":"string"}}}),
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
        assert!(!planning.contains("use_mcp_tool"));
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
        let error = registry
            .execute(
                RunModeV4::Plan,
                ToolCallV4 {
                    call_id: "2".into(),
                    tool_id: "artifact.verify".into(),
                    arguments: json!({"path":"a"}),
                },
            )
            .await
            .unwrap_err();
        assert!(error.contains("forbidden in plan mode"));
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
