use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use omicsops_adapters::{AdapterError, llm::OpenAiCompatibleClient, ssh::SshSession};
use omicsops_core::{
    domain::{AnalysisPlan, CompletionConditionKind, RepairDecision, StepSpec},
    policy::{CommandPolicy, PolicyDecision},
    project::shell_quote,
    redaction::redact_secrets,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub mod scrna;
mod v2;

pub use v2::{
    ExecutionResultV2, SshV2Executor, compile_step_action, reconcile_manifest,
    render_remote_step_script_v2, render_verification_command,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub attempt: u8,
    pub exit_status: u32,
    pub stdout: String,
    pub stderr: String,
    pub verified_conditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureContext {
    pub step: StepSpec,
    pub attempt: u8,
    pub exit_status: u32,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub previous_strategies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Succeeded {
        attempt: u8,
        result: ExecutionResult,
    },
    AwaitingApproval {
        reason: String,
        command: String,
    },
    Denied {
        reason: String,
    },
    NeedsAttention {
        repairs_attempted: u8,
        last_error: String,
    },
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("remote executor failed: {0}")]
    Executor(String),
    #[error("repair planner failed: {0}")]
    Planner(String),
    #[error("analysis planner failed: {0}")]
    AnalysisPlanner(String),
}

#[async_trait]
pub trait RemoteExecutor: Send + Sync {
    async fn execute_step(&self, step: &StepSpec, attempt: u8) -> Result<ExecutionResult, String>;
}

#[async_trait]
pub trait RepairPlanner: Send + Sync {
    async fn propose_repair(&self, failure: &FailureContext) -> Result<RepairDecision, String>;
}

pub struct AutonomousRunner<E, P> {
    policy: CommandPolicy,
    executor: E,
    planner: P,
}

impl<E, P> AutonomousRunner<E, P>
where
    E: RemoteExecutor,
    P: RepairPlanner,
{
    pub fn new(
        project_root: impl Into<String>,
        allowed_domains: &[&str],
        executor: E,
        planner: P,
    ) -> Self {
        Self {
            policy: CommandPolicy::new(project_root, allowed_domains),
            executor,
            planner,
        }
    }

    pub async fn run_step(&self, step: StepSpec) -> Result<StepOutcome, RunnerError> {
        self.run_step_with_approval(step, None).await
    }

    pub async fn run_step_with_approval(
        &self,
        mut step: StepSpec,
        approved_command: Option<&str>,
    ) -> Result<StepOutcome, RunnerError> {
        let mut attempt = 0_u8;
        let mut strategies = Vec::new();
        let mut command_hashes = HashSet::new();
        let mut approved_command = approved_command.map(str::to_owned);
        command_hashes.insert(command_hash(&step.command));

        loop {
            match self.policy.evaluate(&step.command, step.risk) {
                PolicyDecision::Denied { reason } => {
                    return Ok(StepOutcome::Denied { reason });
                }
                PolicyDecision::RequiresApproval { reason } => {
                    if approved_command.as_deref() == Some(step.command.as_str()) {
                        approved_command = None;
                    } else {
                        return Ok(StepOutcome::AwaitingApproval {
                            reason,
                            command: step.command,
                        });
                    }
                }
                PolicyDecision::Allowed => {}
            }

            let result = self
                .executor
                .execute_step(&step, attempt)
                .await
                .map_err(RunnerError::Executor)?;
            if execution_satisfies(&step, &result) {
                return Ok(StepOutcome::Succeeded { attempt, result });
            }
            if strategies.len() == 3 {
                return Ok(StepOutcome::NeedsAttention {
                    repairs_attempted: 3,
                    last_error: failure_summary(&result),
                });
            }

            let failure = FailureContext {
                step: step.clone(),
                attempt,
                exit_status: result.exit_status,
                stdout_tail: tail(&result.stdout, 8_000),
                stderr_tail: tail(&result.stderr, 8_000),
                previous_strategies: strategies.clone(),
            };
            let mut duplicate_proposals = 0;
            let repair = loop {
                let candidate = self
                    .planner
                    .propose_repair(&failure)
                    .await
                    .map_err(RunnerError::Planner)?;
                let hash = command_hash(&candidate.replacement_command);
                if command_hashes.insert(hash) {
                    break candidate;
                }
                duplicate_proposals += 1;
                if duplicate_proposals == 3 {
                    return Ok(StepOutcome::NeedsAttention {
                        repairs_attempted: strategies.len() as u8,
                        last_error: "model repeated an already attempted repair".into(),
                    });
                }
            };

            strategies.push(repair.strategy.clone());
            step.command = repair.replacement_command;
            step.rationale = format!("{} Repair: {}", step.rationale, repair.diagnosis);
            if !repair.completion_conditions.is_empty() {
                step.completion_conditions = repair.completion_conditions;
            }
            attempt += 1;
        }
    }
}

fn execution_satisfies(step: &StepSpec, result: &ExecutionResult) -> bool {
    step.completion_conditions
        .iter()
        .all(|condition| match condition.kind {
            CompletionConditionKind::ExitCode => {
                let expected = condition
                    .expected
                    .as_deref()
                    .unwrap_or("0")
                    .parse::<u32>()
                    .unwrap_or(0);
                result.exit_status == expected
            }
            _ => result
                .verified_conditions
                .contains(&condition_key(condition)),
        })
}

fn condition_key(condition: &omicsops_core::domain::CompletionCondition) -> String {
    format!(
        "{:?}:{}:{}",
        condition.kind,
        condition.target,
        condition.expected.as_deref().unwrap_or_default()
    )
}

fn failure_summary(result: &ExecutionResult) -> String {
    if result.stderr.trim().is_empty() {
        format!("exit status {}", result.exit_status)
    } else {
        tail(&result.stderr, 2_000)
    }
}

fn tail(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    value
        .chars()
        .skip(count.saturating_sub(max_chars))
        .collect()
}

fn command_hash(command: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn render_remote_step_script(root: &str, step: &StepSpec, attempt: u8) -> String {
    let root = root.trim_end_matches('/');
    let id = safe_identifier(&step.id);
    let working = format!("{root}/{}", step.working_directory.trim_matches('/'));
    let state_prefix = format!("{root}/.omicsops/state/{id}.{attempt}");
    let log = format!("{root}/logs/{id}.{attempt}.log");
    format!(
        "#!/usr/bin/env bash\n\
         set -u -o pipefail\n\
         umask 077\n\
         printf '%s\\n' \"$$\" > {pid_tmp}\n\
         mv {pid_tmp} {pid}\n\
         cd {working}\n\
         set +e\n\
         {{\n{command}\n\
         }} > {log} 2>&1\n\
         status=$?\n\
         printf '%s\\n' \"$status\" > {exit_tmp}\n\
         mv {exit_tmp} {exit}\n\
         exit \"$status\"\n",
        pid_tmp = shell_quote(&format!("{state_prefix}.pgid.tmp")),
        pid = shell_quote(&format!("{state_prefix}.pgid")),
        working = shell_quote(&working),
        command = step.command,
        log = shell_quote(&log),
        exit_tmp = shell_quote(&format!("{state_prefix}.exit.tmp")),
        exit = shell_quote(&format!("{state_prefix}.exit")),
    )
}

pub fn render_start_command(root: &str, step_id: &str, attempt: u8) -> String {
    let root = root.trim_end_matches('/');
    let id = safe_identifier(step_id);
    format!(
        "nohup setsid bash {} >/dev/null 2>&1 < /dev/null &",
        shell_quote(&format!("{root}/scripts/{id}.{attempt}.sh"))
    )
}

pub fn render_status_command(root: &str, step_id: &str, attempt: u8) -> String {
    let root = root.trim_end_matches('/');
    let id = safe_identifier(step_id);
    let prefix = format!("{root}/.omicsops/state/{id}.{attempt}");
    format!(
        "if test -f {exit}; then printf 'EXIT:'; cat {exit}; \
         elif test -f {pid} && kill -0 \"$(cat {pid})\" 2>/dev/null; then printf RUNNING; \
         else printf LOST; fi",
        exit = shell_quote(&format!("{prefix}.exit")),
        pid = shell_quote(&format!("{prefix}.pgid")),
    )
}

pub fn render_cancel_command(root: &str, step_id: &str, attempt: u8, force: bool) -> String {
    let root = root.trim_end_matches('/');
    let id = safe_identifier(step_id);
    let pgid = shell_quote(&format!("{root}/.omicsops/state/{id}.{attempt}.pgid"));
    let signal = if force { "KILL" } else { "TERM" };
    format!("test -f {pgid} && kill -{signal} -- -\"$(cat {pgid})\" 2>/dev/null || true")
}

fn safe_identifier(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

pub struct SshRemoteExecutor {
    session: Arc<SshSession>,
    project_root: String,
    poll_interval: Duration,
    cancel_requested: Option<Arc<AtomicBool>>,
}

impl SshRemoteExecutor {
    pub fn new(session: Arc<SshSession>, project_root: impl Into<String>) -> Self {
        Self {
            session,
            project_root: project_root.into().trim_end_matches('/').to_owned(),
            poll_interval: Duration::from_secs(2),
            cancel_requested: None,
        }
    }

    pub fn with_cancellation(mut self, cancel_requested: Arc<AtomicBool>) -> Self {
        self.cancel_requested = Some(cancel_requested);
        self
    }

    async fn cancel_step(&self, step: &StepSpec, attempt: u8) -> Result<(), String> {
        self.session
            .execute(&render_cancel_command(
                &self.project_root,
                &step.id,
                attempt,
                false,
            ))
            .await
            .map_err(|error| error.to_string())?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let output = self
                .session
                .execute(&render_status_command(
                    &self.project_root,
                    &step.id,
                    attempt,
                ))
                .await
                .map_err(|error| error.to_string())?;
            if output.stdout.trim() != "RUNNING" {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                self.session
                    .execute(&render_cancel_command(
                        &self.project_root,
                        &step.id,
                        attempt,
                        true,
                    ))
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

#[async_trait]
impl RemoteExecutor for SshRemoteExecutor {
    async fn execute_step(&self, step: &StepSpec, attempt: u8) -> Result<ExecutionResult, String> {
        let id = safe_identifier(&step.id);
        let script_path = format!("{}/scripts/{id}.{attempt}.sh", self.project_root);
        let script = render_remote_step_script(&self.project_root, step, attempt);
        self.session
            .upload_text(&script_path, &script)
            .await
            .map_err(|error| error.to_string())?;
        self.session
            .execute_checked(&format!(
                "chmod 700 {} && {}",
                shell_quote(&script_path),
                render_start_command(&self.project_root, &step.id, attempt)
            ))
            .await
            .map_err(|error| error.to_string())?;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(step.timeout_seconds);
        let status = loop {
            if self
                .cancel_requested
                .as_ref()
                .is_some_and(|requested| requested.load(Ordering::SeqCst))
            {
                self.cancel_step(step, attempt).await?;
                return Err("execution canceled".into());
            }
            if tokio::time::Instant::now() >= deadline {
                break 124;
            }
            let output = self
                .session
                .execute(&render_status_command(
                    &self.project_root,
                    &step.id,
                    attempt,
                ))
                .await
                .map_err(|error| error.to_string())?;
            if let Some(value) = output.stdout.trim().strip_prefix("EXIT:") {
                break value.trim().parse::<u32>().unwrap_or(255);
            }
            if output.stdout.trim() == "LOST" {
                break 255;
            }
            tokio::time::sleep(self.poll_interval).await;
        };
        let log_path = format!("{}/logs/{id}.{attempt}.log", self.project_root);
        let log = self
            .session
            .execute(&format!("test -f {0} && cat {0}", shell_quote(&log_path)))
            .await
            .map_err(|error| error.to_string())?;
        Ok(ExecutionResult {
            attempt,
            exit_status: status,
            stdout: log.stdout,
            stderr: log.stderr,
            verified_conditions: Vec::new(),
        })
    }
}

#[derive(Clone)]
pub struct LlmRepairPlanner {
    client: OpenAiCompatibleClient,
}

impl LlmRepairPlanner {
    pub fn new(client: OpenAiCompatibleClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl RepairPlanner for LlmRepairPlanner {
    async fn propose_repair(&self, failure: &FailureContext) -> Result<RepairDecision, String> {
        let context = serde_json::to_string_pretty(failure).map_err(|error| error.to_string())?;
        let context = redact_secrets(&context, &[] as &[&str]);
        self.client
            .call_tool(
                "Diagnose the failed bioinformatics step. Return one distinct, project-local repair. Never use sudo or access paths outside the project.",
                &context,
                "submit_repair_decision",
            )
            .await
            .map_err(|error| error.to_string())
    }
}

pub async fn generate_analysis_plan(
    client: &OpenAiCompatibleClient,
    extracted_plan: &str,
    server_inspection: &str,
) -> Result<AnalysisPlan, RunnerError> {
    let prompt = format!(
        "USER PLAN:\n{extracted_plan}\n\nREAD-ONLY SERVER INSPECTION:\n{server_inspection}"
    );
    client
        .call_tool(
            "Compile the user plan into a reproducible stage-level OmicsOps analysis plan. Commands must be non-interactive, project-relative, resumable, and include machine-checkable completion conditions.",
            &prompt,
            "submit_analysis_plan",
        )
        .await
        .map_err(|error: AdapterError| RunnerError::AnalysisPlanner(error.to_string()))
}
