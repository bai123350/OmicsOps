use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    plan_v2::{
        ActionDiff, AnalysisPlanV2, StepAction, StepSpecV2, VerificationSpec,
        require_supported_schema, risk_rank,
    },
    project::validate_relative_remote_path,
    tools::ToolCatalog,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValidationIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanValidation {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RepairAssessment {
    pub within_envelope: bool,
    pub diffs: Vec<ActionDiff>,
    pub validation: PlanValidation,
}

pub fn assess_repair(
    plan: &AnalysisPlanV2,
    approved: &StepSpecV2,
    proposed: &StepSpecV2,
    catalog: &ToolCatalog,
) -> RepairAssessment {
    let mut diffs = Vec::new();
    macro_rules! record_diff {
        ($field:literal, $left:expr, $right:expr, $reason:literal) => {
            if $left != $right {
                diffs.push(ActionDiff {
                    field: $field.into(),
                    approved: serde_json::to_string($left).unwrap_or_default(),
                    proposed: serde_json::to_string($right).unwrap_or_default(),
                    reason: $reason.into(),
                });
            }
        };
    }
    record_diff!(
        "action",
        &approved.action,
        &proposed.action,
        "repair changes the compiled action"
    );
    record_diff!(
        "resources",
        &approved.resources,
        &proposed.resources,
        "resource changes must stay within the approved envelope"
    );
    record_diff!(
        "risk",
        &approved.risk,
        &proposed.risk,
        "risk changes must stay within the approved envelope"
    );
    record_diff!(
        "expected_artifacts",
        &approved.expected_artifacts,
        &proposed.expected_artifacts,
        "output paths are frozen by approval"
    );
    record_diff!(
        "verifications",
        &approved.verifications,
        &proposed.verifications,
        "verification changes require review"
    );
    let mut candidate = plan.clone();
    for step in candidate
        .stages
        .iter_mut()
        .flat_map(|stage| stage.steps.iter_mut())
    {
        if step.id == approved.id {
            *step = proposed.clone();
        }
    }
    let validation = validate_plan_v2(&candidate, catalog);
    let approved_output_arguments = action_output_arguments(&approved.action);
    let proposed_output_arguments = action_output_arguments(&proposed.action);
    if approved_output_arguments != proposed_output_arguments {
        diffs.push(ActionDiff {
            field: "action.output_paths".into(),
            approved: serde_json::to_string(&approved_output_arguments).unwrap_or_default(),
            proposed: serde_json::to_string(&proposed_output_arguments).unwrap_or_default(),
            reason: "tool output arguments are frozen by approval".into(),
        });
    }
    let frozen_boundary_changed = approved.expected_artifacts != proposed.expected_artifacts
        || approved.working_directory != proposed.working_directory
        || approved_output_arguments != proposed_output_arguments
        || risk_rank(proposed.risk) > risk_rank(plan.policy.max_risk);
    RepairAssessment {
        within_envelope: validation.valid && !frozen_boundary_changed,
        diffs,
        validation,
    }
}

fn action_output_arguments(action: &StepAction) -> BTreeMap<String, serde_json::Value> {
    match action {
        StepAction::Tool { arguments, .. } => arguments
            .as_object()
            .into_iter()
            .flat_map(|arguments| arguments.iter())
            .filter(|(name, _)| matches!(name.as_str(), "output" | "outdir"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        StepAction::LegacyShell { command } => BTreeMap::from([(
            "legacy_command".into(),
            serde_json::Value::String(command.clone()),
        )]),
    }
}

pub fn validate_plan_v2(plan: &AnalysisPlanV2, catalog: &ToolCatalog) -> PlanValidation {
    let mut issues = Vec::new();
    if let Err(error) = require_supported_schema(plan) {
        issue(
            &mut issues,
            "unsupported_schema",
            "schema_version",
            error.to_string(),
        );
    }

    let mut stage_ids = BTreeSet::new();
    for (stage_index, stage) in plan.stages.iter().enumerate() {
        if !stage_ids.insert(stage.id.clone()) {
            issue(
                &mut issues,
                "duplicate_stage",
                format!("stages[{stage_index}].id"),
                "stage ids must be unique",
            );
        }
    }
    validate_graph(
        plan.stages
            .iter()
            .map(|stage| (stage.id.as_str(), stage.dependencies.as_slice())),
        "stages",
        &mut issues,
    );

    let mut step_ids = BTreeSet::new();
    let mut step_dependencies = Vec::new();
    for (stage_index, stage) in plan.stages.iter().enumerate() {
        for (step_index, step) in stage.steps.iter().enumerate() {
            let base = format!("stages[{stage_index}].steps[{step_index}]");
            if !step_ids.insert(step.id.clone()) {
                issue(
                    &mut issues,
                    "duplicate_step",
                    format!("{base}.id"),
                    "step ids must be globally unique",
                );
            }
            step_dependencies.push((step.id.as_str(), step.dependencies.as_slice()));
            validate_path(
                &step.working_directory,
                &format!("{base}.working_directory"),
                &mut issues,
            );
            for (index, path) in step.expected_artifacts.iter().enumerate() {
                validate_path(
                    path,
                    &format!("{base}.expected_artifacts[{index}]"),
                    &mut issues,
                );
                if !step
                    .verifications
                    .iter()
                    .filter_map(verification_path)
                    .any(|verified| verified == path)
                {
                    issue(
                        &mut issues,
                        "artifact_unverified",
                        format!("{base}.expected_artifacts[{index}]"),
                        "every expected artifact must have a path-specific verification",
                    );
                }
            }
            for (index, verification) in step.verifications.iter().enumerate() {
                if let Some(path) = verification_path(verification) {
                    validate_path(path, &format!("{base}.verifications[{index}]"), &mut issues);
                }
            }
            if step.verifications.is_empty() {
                issue(
                    &mut issues,
                    "missing_verification",
                    format!("{base}.verifications"),
                    "every step requires a machine-checkable verification",
                );
            }
            validate_resources(plan, step, &base, &mut issues);
            if risk_rank(step.risk) > risk_rank(plan.policy.max_risk) {
                issue(
                    &mut issues,
                    "risk_not_allowed",
                    format!("{base}.risk"),
                    "step risk exceeds the approved policy envelope",
                );
            }
            validate_action(plan, catalog, step, &base, &mut issues);
        }
    }
    validate_graph(step_dependencies.into_iter(), "steps", &mut issues);

    PlanValidation {
        valid: issues.is_empty(),
        issues,
    }
}

fn validate_action(
    plan: &AnalysisPlanV2,
    catalog: &ToolCatalog,
    step: &crate::plan_v2::StepSpecV2,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match &step.action {
        StepAction::LegacyShell { command } => {
            if !plan.policy.allow_legacy_shell {
                issue(
                    issues,
                    "legacy_shell_not_allowed",
                    format!("{base}.action"),
                    "legacy shell requires an explicit policy envelope",
                );
            }
            if command.trim().is_empty() {
                issue(
                    issues,
                    "empty_command",
                    format!("{base}.action.command"),
                    "legacy command must not be empty",
                );
            }
        }
        StepAction::Tool {
            tool_id,
            version,
            arguments,
        } => {
            let Some(manifest) = catalog.get(tool_id, version) else {
                issue(
                    issues,
                    "unknown_tool",
                    format!("{base}.action"),
                    format!("unknown tool {tool_id}@{version}"),
                );
                return;
            };
            if !plan
                .policy
                .allowed_tools
                .iter()
                .any(|allowed| allowed == tool_id)
            {
                issue(
                    issues,
                    "tool_not_allowed",
                    format!("{base}.action.tool_id"),
                    format!("tool {tool_id} is outside the policy envelope"),
                );
            }
            validate_argument_schema(arguments, &manifest.argument_schema, base, issues);
            let maximum = &manifest.max_resources;
            if step.resources.max_cpu_cores > maximum.max_cpu_cores
                || step.resources.max_memory_gib > maximum.max_memory_gib
                || step.resources.max_disk_gib > maximum.max_disk_gib
                || step.resources.max_step_seconds > maximum.max_step_seconds
            {
                issue(
                    issues,
                    "tool_resource_limit_exceeded",
                    format!("{base}.resources"),
                    "step resources exceed the tool manifest limit",
                );
            }
            if let Some(url) = arguments.get("url").and_then(|value| value.as_str()) {
                match Url::parse(url)
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
                {
                    Some(host) if !domain_allowed(&host, &plan.policy.allowed_domains) => issue(
                        issues,
                        "domain_not_allowed",
                        format!("{base}.action.arguments.url"),
                        format!("domain {host} is outside the policy envelope"),
                    ),
                    None => issue(
                        issues,
                        "invalid_url",
                        format!("{base}.action.arguments.url"),
                        "tool URL is invalid",
                    ),
                    _ => {}
                }
            }
            for domain in &manifest.allowed_domains {
                if !domain_allowed(domain, &plan.policy.allowed_domains) {
                    issue(
                        issues,
                        "domain_not_allowed",
                        format!("{base}.action"),
                        format!("tool requires undeclared domain {domain}"),
                    );
                }
            }
        }
    }
}

fn validate_argument_schema(
    arguments: &serde_json::Value,
    schema: &serde_json::Value,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(arguments) = arguments.as_object() else {
        issue(
            issues,
            "invalid_arguments",
            format!("{base}.action.arguments"),
            "tool arguments must be an object",
        );
        return;
    };
    let properties = schema
        .get("properties")
        .and_then(serde_json::Value::as_object);
    for required in schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
    {
        if !arguments.contains_key(required) {
            issue(
                issues,
                "missing_argument",
                format!("{base}.action.arguments.{required}"),
                format!("required argument {required} is missing"),
            );
        }
    }
    for (name, value) in arguments {
        let Some(property) = properties.and_then(|properties| properties.get(name)) else {
            if schema
                .get("additionalProperties")
                .and_then(serde_json::Value::as_bool)
                == Some(false)
            {
                issue(
                    issues,
                    "unknown_argument",
                    format!("{base}.action.arguments.{name}"),
                    format!("argument {name} is not declared by the tool"),
                );
            }
            continue;
        };
        let expected = property
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let type_matches = match expected {
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => true,
        };
        if !type_matches {
            issue(
                issues,
                "invalid_argument_type",
                format!("{base}.action.arguments.{name}"),
                format!("argument {name} must be {expected}"),
            );
        }
        if matches!(
            name.as_str(),
            "output" | "outdir" | "input" | "index" | "read1" | "read2" | "script" | "file"
        ) {
            if let Some(path) = value.as_str() {
                if validate_relative_remote_path(Path::new(path)).is_err() {
                    issue(
                        issues,
                        "invalid_argument_path",
                        format!("{base}.action.arguments.{name}"),
                        format!("argument {name} must be a project-relative path"),
                    );
                }
            }
        }
    }
}

fn validate_resources(
    plan: &AnalysisPlanV2,
    step: &crate::plan_v2::StepSpecV2,
    base: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let requested = &step.resources;
    let budget = &plan.resource_budget;
    if requested.max_cpu_cores > budget.max_cpu_cores
        || requested.max_memory_gib > budget.max_memory_gib
        || requested.max_disk_gib > budget.max_disk_gib
        || requested.max_step_seconds > budget.max_step_seconds
    {
        issue(
            issues,
            "resource_budget_exceeded",
            format!("{base}.resources"),
            "step resources exceed the plan budget",
        );
    }
}

fn validate_path(path: &str, location: &str, issues: &mut Vec<ValidationIssue>) {
    if validate_relative_remote_path(Path::new(path)).is_err() {
        issue(
            issues,
            "invalid_path",
            location,
            format!("{path} is not a project-relative path"),
        );
    }
}

fn verification_path(spec: &VerificationSpec) -> Option<&str> {
    match spec {
        VerificationSpec::ExitCode { .. } => None,
        VerificationSpec::File { path, .. }
        | VerificationSpec::JsonField { path, .. }
        | VerificationSpec::Table { path, .. }
        | VerificationSpec::DomainReport { path, .. } => Some(path),
    }
}

fn validate_graph<'a>(
    nodes: impl Iterator<Item = (&'a str, &'a [String])>,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let graph = nodes
        .map(|(id, dependencies)| (id.to_owned(), dependencies.to_vec()))
        .collect::<BTreeMap<_, _>>();
    for (id, dependencies) in &graph {
        for dependency in dependencies {
            if !graph.contains_key(dependency) {
                issue(
                    issues,
                    "unknown_dependency",
                    path,
                    format!("{id} depends on unknown node {dependency}"),
                );
            }
        }
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in graph.keys() {
        if has_cycle(id, &graph, &mut visiting, &mut visited) {
            issue(
                issues,
                "dependency_cycle",
                path,
                "dependency graph contains a cycle",
            );
            break;
        }
    }
}

fn has_cycle(
    id: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if visited.contains(id) {
        return false;
    }
    if !visiting.insert(id.to_owned()) {
        return true;
    }
    if graph.get(id).is_some_and(|dependencies| {
        dependencies
            .iter()
            .any(|dependency| has_cycle(dependency, graph, visiting, visited))
    }) {
        return true;
    }
    visiting.remove(id);
    visited.insert(id.to_owned());
    false
}

fn domain_allowed(host: &str, allowed: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    allowed.iter().any(|domain| {
        let domain = domain.to_ascii_lowercase();
        host == domain || host.ends_with(&format!(".{domain}"))
    })
}

fn issue(
    issues: &mut Vec<ValidationIssue>,
    code: &str,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    issues.push(ValidationIssue {
        code: code.into(),
        path: path.into(),
        message: message.into(),
    });
}
