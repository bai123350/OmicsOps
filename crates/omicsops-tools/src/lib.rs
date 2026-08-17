use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use omicsops_agent_core::ToolPortV4;
use omicsops_protocol::{RunModeV4, ToolCallV4, ToolDescriptorV4, ToolEffectV4, ToolOutcomeV4};
use serde_json::{Value, json};
use thiserror::Error;

#[async_trait]
pub trait ToolExecutorV4: Send + Sync {
    async fn execute(&self, call: &ToolCallV4) -> Result<ToolOutcomeV4, String>;
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
        })
    }

    pub fn with_execute_capabilities(mut self, capabilities: BTreeSet<String>) -> Self {
        self.execute_capabilities = Some(capabilities);
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
            "project.list" | "project.read" | "agent.request_input" | "agent.propose_plan"
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
                    "project.list" | "project.read" | "agent.request_input" | "agent.propose_plan"
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

    async fn execute(&self, mode: RunModeV4, call: ToolCallV4) -> Result<ToolOutcomeV4, String> {
        self.authorize(mode, &call)
            .map_err(|error| error.to_string())?;
        self.executor.execute(&call).await
    }
}

fn validate_required(schema: &Value, input: &Value) -> Result<(), String> {
    let object = input
        .as_object()
        .ok_or_else(|| "arguments must be an object".to_string())?;
    for field in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !object.contains_key(field) {
            return Err(format!("missing required field {field}"));
        }
    }
    Ok(())
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
            json!({"type":"object","required":["language","code"],"properties":{"language":{"type":"string"},"code":{"type":"string"},"capture_paths":{"type":"array"}}}),
        ),
        descriptor(
            "artifact.verify",
            "Verify a project-relative artifact",
            ToolEffectV4::ReadOnly,
            json!({"type":"object","required":["path"],"properties":{"path":{"type":"string"}}}),
        ),
        descriptor(
            "agent.complete",
            "Propose completion of the approved plan",
            ToolEffectV4::Mutating,
            json!({"type":"object","properties":{}}),
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
}
