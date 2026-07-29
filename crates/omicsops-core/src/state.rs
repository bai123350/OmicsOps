use crate::{CoreError, CoreResult, domain::RunState};

#[derive(Debug, Clone)]
pub struct RunStateMachine {
    current: RunState,
}

impl Default for RunStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl RunStateMachine {
    pub fn new() -> Self {
        Self {
            current: RunState::Draft,
        }
    }

    pub fn from_state(state: RunState) -> Self {
        Self { current: state }
    }

    pub fn current(&self) -> RunState {
        self.current
    }

    pub fn transition(&mut self, next: RunState) -> CoreResult<()> {
        if self.current == next {
            return Ok(());
        }

        let allowed = matches!(
            (self.current, next),
            (RunState::Draft, RunState::Inspecting)
                | (RunState::Draft, RunState::Canceled)
                | (RunState::Inspecting, RunState::AwaitingPlanApproval)
                | (RunState::Inspecting, RunState::Failed)
                | (RunState::Inspecting, RunState::Canceled)
                | (RunState::AwaitingPlanApproval, RunState::Preparing)
                | (RunState::AwaitingPlanApproval, RunState::Canceled)
                | (RunState::Preparing, RunState::Running)
                | (RunState::Preparing, RunState::Failed)
                | (RunState::Preparing, RunState::Canceled)
                | (RunState::Running, RunState::PausedForApproval)
                | (RunState::Running, RunState::Succeeded)
                | (RunState::Running, RunState::Failed)
                | (RunState::Running, RunState::Canceled)
                | (RunState::PausedForApproval, RunState::Running)
                | (RunState::PausedForApproval, RunState::Failed)
                | (RunState::PausedForApproval, RunState::Canceled)
        );

        if !allowed {
            return Err(CoreError::InvalidStateTransition {
                from: format!("{:?}", self.current),
                to: format!("{next:?}"),
            });
        }

        self.current = next;
        Ok(())
    }
}
