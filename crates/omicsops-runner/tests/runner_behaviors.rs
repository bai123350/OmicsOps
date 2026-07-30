use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use omicsops_core::domain::{
    CompletionCondition, CompletionConditionKind, RepairDecision, StepRisk, StepSpec,
};
use omicsops_runner::{
    AutonomousRunner, ExecutionResult, FailureContext, RemoteExecutor, RepairPlanner, StepOutcome,
    render_cancel_command, render_remote_step_script,
};

#[derive(Clone)]
struct FakeExecutor {
    results: Arc<Mutex<VecDeque<ExecutionResult>>>,
    commands: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl RemoteExecutor for FakeExecutor {
    async fn execute_step(&self, step: &StepSpec, attempt: u8) -> Result<ExecutionResult, String> {
        self.commands.lock().unwrap().push(step.command.clone());
        let mut result = self.results.lock().unwrap().pop_front().unwrap();
        result.attempt = attempt;
        Ok(result)
    }
}

#[derive(Clone)]
struct FakePlanner {
    repairs: Arc<Mutex<VecDeque<RepairDecision>>>,
}

#[async_trait]
impl RepairPlanner for FakePlanner {
    async fn propose_repair(&self, _failure: &FailureContext) -> Result<RepairDecision, String> {
        self.repairs
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| "no repair".into())
    }
}

fn step(command: &str) -> StepSpec {
    StepSpec {
        id: "qc".into(),
        title: "QC".into(),
        rationale: "quality control".into(),
        command: command.into(),
        working_directory: "work".into(),
        timeout_seconds: 300,
        risk: StepRisk::Low,
        completion_conditions: vec![CompletionCondition {
            kind: CompletionConditionKind::ExitCode,
            target: "process".into(),
            expected: Some("0".into()),
        }],
        expected_artifacts: vec![],
    }
}

fn result(status: u32) -> ExecutionResult {
    ExecutionResult {
        attempt: 0,
        exit_status: status,
        stdout: String::new(),
        stderr: if status == 0 {
            String::new()
        } else {
            "failed".into()
        },
        verified_conditions: vec![],
    }
}

fn repair(strategy: &str, command: &str) -> RepairDecision {
    RepairDecision {
        diagnosis: "dependency conflict".into(),
        strategy: strategy.into(),
        replacement_command: command.into(),
        completion_conditions: vec![],
    }
}

#[tokio::test]
async fn runner_repairs_a_failed_step_and_returns_success() {
    let executor = FakeExecutor {
        results: Arc::new(Mutex::new(VecDeque::from([result(1), result(0)]))),
        commands: Arc::new(Mutex::new(Vec::new())),
    };
    let planner = FakePlanner {
        repairs: Arc::new(Mutex::new(VecDeque::from([repair(
            "pin dependency",
            "micromamba install scanpy=1.11",
        )]))),
    };
    let runner = AutonomousRunner::new("/srv/omicsops/pbmc", &[], executor.clone(), planner);

    let outcome = runner.run_step(step("python qc.py")).await.unwrap();

    assert!(matches!(outcome, StepOutcome::Succeeded { attempt: 1, .. }));
    assert_eq!(
        *executor.commands.lock().unwrap(),
        ["python qc.py", "micromamba install scanpy=1.11"]
    );
}

#[tokio::test]
async fn runner_pauses_after_three_distinct_repairs_fail() {
    let executor = FakeExecutor {
        results: Arc::new(Mutex::new(VecDeque::from([
            result(1),
            result(1),
            result(1),
            result(1),
        ]))),
        commands: Arc::new(Mutex::new(Vec::new())),
    };
    let planner = FakePlanner {
        repairs: Arc::new(Mutex::new(VecDeque::from([
            repair("pin", "fix-one"),
            repair("clean cache", "fix-two"),
            repair("fallback tool", "fix-three"),
        ]))),
    };
    let runner = AutonomousRunner::new("/srv/omicsops/pbmc", &[], executor, planner);

    let outcome = runner.run_step(step("initial")).await.unwrap();

    assert!(matches!(
        outcome,
        StepOutcome::NeedsAttention {
            repairs_attempted: 3,
            ..
        }
    ));
}

#[tokio::test]
async fn runner_never_executes_denied_commands() {
    let executor = FakeExecutor {
        results: Arc::new(Mutex::new(VecDeque::from([result(0)]))),
        commands: Arc::new(Mutex::new(Vec::new())),
    };
    let planner = FakePlanner {
        repairs: Arc::new(Mutex::new(VecDeque::new())),
    };
    let runner = AutonomousRunner::new("/srv/omicsops/pbmc", &[], executor.clone(), planner);

    let outcome = runner.run_step(step("sudo apt-get update")).await.unwrap();

    assert!(matches!(outcome, StepOutcome::Denied { .. }));
    assert!(executor.commands.lock().unwrap().is_empty());
}

#[tokio::test]
async fn runner_executes_the_exact_high_risk_command_after_explicit_approval() {
    let executor = FakeExecutor {
        results: Arc::new(Mutex::new(VecDeque::from([result(0)]))),
        commands: Arc::new(Mutex::new(Vec::new())),
    };
    let planner = FakePlanner {
        repairs: Arc::new(Mutex::new(VecDeque::new())),
    };
    let runner = AutonomousRunner::new("/srv/omicsops/pbmc", &[], executor.clone(), planner);
    let mut approved_step = step("python export.py --overwrite results/pbmc.h5ad");
    approved_step.risk = StepRisk::High;
    let approved_command = approved_step.command.clone();

    let outcome = runner
        .run_step_with_approval(approved_step, Some(&approved_command))
        .await
        .unwrap();

    assert!(matches!(outcome, StepOutcome::Succeeded { attempt: 0, .. }));
    assert_eq!(
        *executor.commands.lock().unwrap(),
        ["python export.py --overwrite results/pbmc.h5ad"]
    );
}

#[test]
fn remote_script_persists_pid_exit_status_and_log_contract() {
    let script = render_remote_step_script("/srv/omicsops/pbmc", &step("python qc.py"), 2);

    assert!(script.contains("python qc.py"));
    assert!(script.contains(".omicsops/state/qc.2.exit"));
    assert!(script.contains(".omicsops/state/qc.2.pid"));
    assert!(script.contains("logs/qc.2.log"));
    assert!(script.contains("mv"));
}

#[test]
fn cancellation_targets_only_the_recorded_step_process() {
    let soft = render_cancel_command("/srv/omicsops/pbmc", "qc", 2, false);
    let force = render_cancel_command("/srv/omicsops/pbmc", "qc", 2, true);

    assert_eq!(
        soft,
        "test -f '/srv/omicsops/pbmc/.omicsops/state/qc.2.pid' && kill -TERM -- \"$(cat '/srv/omicsops/pbmc/.omicsops/state/qc.2.pid')\" 2>/dev/null || true"
    );
    assert_eq!(
        force,
        "test -f '/srv/omicsops/pbmc/.omicsops/state/qc.2.pid' && kill -KILL -- \"$(cat '/srv/omicsops/pbmc/.omicsops/state/qc.2.pid')\" 2>/dev/null || true"
    );
}
