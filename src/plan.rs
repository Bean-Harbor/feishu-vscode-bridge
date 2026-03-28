use serde::{Deserialize, Serialize};

use crate::Intent;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingApproval {
    step_index: usize,
    run_all_after_approval: bool,
}

#[derive(Debug, Clone)]
pub struct ExecutionOutcome {
    pub success: bool,
    pub reply: String,
}

#[derive(Debug, Clone)]
pub struct StepExecution {
    pub step_number: usize,
    pub intent: Intent,
    pub outcome: ExecutionOutcome,
}

#[derive(Debug, Clone)]
pub struct PlanProgress {
    pub executed: Vec<StepExecution>,
    pub total_steps: usize,
    pub next_step: usize,
    pub completed: bool,
    pub paused_on_failure: bool,
    pub paused_on_approval: bool,
    pub approval_intent: Option<Intent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSession {
    steps: Vec<Intent>,
    next_step: usize,
    pending_approval: Option<PendingApproval>,
}

impl PlanSession {
    pub fn new(steps: Vec<Intent>) -> Self {
        Self {
            steps,
            next_step: 0,
            pending_approval: None,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.next_step >= self.steps.len()
    }

    pub fn next_step_number(&self) -> usize {
        self.next_step + 1
    }

    pub fn total_steps(&self) -> usize {
        self.steps.len()
    }

    pub fn remaining_steps(&self) -> usize {
        self.steps.len().saturating_sub(self.next_step)
    }

    pub fn has_pending_approval(&self) -> bool {
        self.pending_approval.is_some()
    }

    pub fn execute_next<F>(&mut self, mut executor: F) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        self.execute_next_with_policy(&mut executor, |_| false)
    }

    pub fn execute_next_with_policy<F, G>(&mut self, mut executor: F, mut requires_approval: G) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
        G: FnMut(&Intent) -> bool,
    {
        if let Some(progress) = self.pending_approval_progress() {
            return progress;
        }

        if self.is_complete() {
            return PlanProgress {
                executed: Vec::new(),
                total_steps: self.total_steps(),
                next_step: self.next_step,
                completed: true,
                paused_on_failure: false,
                paused_on_approval: false,
                approval_intent: None,
            };
        }

        self.execute_internal(1, false, None, &mut executor, &mut requires_approval)
    }

    pub fn execute_remaining<F>(&mut self, mut executor: F) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        self.execute_remaining_with_policy(&mut executor, |_| false)
    }

    pub fn execute_remaining_with_policy<F, G>(&mut self, mut executor: F, mut requires_approval: G) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
        G: FnMut(&Intent) -> bool,
    {
        if let Some(progress) = self.pending_approval_progress() {
            return progress;
        }

        let max_steps = self.remaining_steps();
        self.execute_internal(max_steps, true, None, &mut executor, &mut requires_approval)
    }

    pub fn approve_pending<F>(&mut self, mut executor: F) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        self.approve_pending_with_policy(&mut executor, |_| false)
    }

    pub fn approve_pending_with_policy<F, G>(&mut self, mut executor: F, mut requires_approval: G) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
        G: FnMut(&Intent) -> bool,
    {
        let Some(pending) = self.pending_approval.take() else {
            return self.pending_approval_progress().unwrap_or_else(|| PlanProgress {
                executed: Vec::new(),
                total_steps: self.total_steps(),
                next_step: self.next_step,
                completed: self.is_complete(),
                paused_on_failure: false,
                paused_on_approval: false,
                approval_intent: None,
            });
        };

        let max_steps = if pending.run_all_after_approval {
            self.remaining_steps()
        } else {
            1
        };

        self.execute_internal(
            max_steps,
            pending.run_all_after_approval,
            Some(pending.step_index),
            &mut executor,
            &mut requires_approval,
        )
    }

    pub fn reject_pending(&mut self) -> bool {
        self.pending_approval.take().is_some()
    }

    fn pending_approval_progress(&self) -> Option<PlanProgress> {
        let pending = self.pending_approval.as_ref()?;
        let approval_intent = self.steps.get(pending.step_index)?.clone();

        Some(PlanProgress {
            executed: Vec::new(),
            total_steps: self.total_steps(),
            next_step: self.next_step,
            completed: false,
            paused_on_failure: false,
            paused_on_approval: true,
            approval_intent: Some(approval_intent),
        })
    }

    fn execute_internal<F>(
        &mut self,
        max_steps: usize,
        run_all_after_approval: bool,
        skip_approval_for_step: Option<usize>,
        executor: &mut F,
        requires_approval: &mut dyn FnMut(&Intent) -> bool,
    ) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        let mut executed = Vec::new();
        let total_steps = self.total_steps();
        let mut paused_on_failure = false;
        let mut paused_on_approval = false;
        let mut approval_intent = None;

        for _ in 0..max_steps {
            if self.is_complete() {
                break;
            }

            let step_number = self.next_step + 1;
            let intent = self.steps[self.next_step].clone();

            if requires_approval(&intent) && Some(self.next_step) != skip_approval_for_step {
                self.pending_approval = Some(PendingApproval {
                    step_index: self.next_step,
                    run_all_after_approval,
                });
                paused_on_approval = true;
                approval_intent = Some(intent);
                break;
            }

            let outcome = executor(&intent);
            let success = outcome.success;

            executed.push(StepExecution {
                step_number,
                intent,
                outcome,
            });

            if success {
                self.next_step += 1;
                continue;
            }

            paused_on_failure = true;
            break;
        }

        PlanProgress {
            executed,
            total_steps,
            next_step: self.next_step,
            completed: self.is_complete() && !paused_on_approval,
            paused_on_failure,
            paused_on_approval,
            approval_intent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Intent;

    #[test]
    fn execute_next_advances_after_success() {
        let mut session = PlanSession::new(vec![
            Intent::OpenFolder {
                path: "/tmp/demo-1".to_string(),
            },
            Intent::OpenFolder {
                path: "/tmp/demo-2".to_string(),
            },
        ]);

        let progress = session.execute_next(|_| ExecutionOutcome {
            success: true,
            reply: "ok".to_string(),
        });

        assert_eq!(progress.executed.len(), 1);
        assert_eq!(progress.next_step, 1);
        assert!(!progress.completed);
        assert!(!progress.paused_on_failure);
        assert!(!progress.paused_on_approval);
    }

    #[test]
    fn execute_remaining_pauses_on_failure() {
        let mut session = PlanSession::new(vec![
            Intent::OpenFolder {
                path: "/tmp/demo-1".to_string(),
            },
            Intent::OpenFolder {
                path: "/tmp/demo-2".to_string(),
            },
        ]);
        let mut calls = 0;

        let progress = session.execute_remaining(|_| {
            calls += 1;
            ExecutionOutcome {
                success: calls == 1,
                reply: "step".to_string(),
            }
        });

        assert_eq!(progress.executed.len(), 2);
        assert_eq!(progress.next_step, 1);
        assert!(progress.paused_on_failure);
        assert!(!progress.completed);
        assert!(!progress.paused_on_approval);
    }

    #[test]
    fn execute_next_pauses_for_approval() {
        let mut session = PlanSession::new(vec![Intent::RunShell {
            cmd: "pwd".to_string(),
        }]);

        let progress = session.execute_next_with_policy(
            |_| ExecutionOutcome {
                success: true,
                reply: "ok".to_string(),
            },
            |intent| matches!(intent, Intent::RunShell { .. }),
        );

        assert!(progress.executed.is_empty());
        assert!(progress.paused_on_approval);
        assert!(!progress.paused_on_failure);
        assert_eq!(progress.approval_intent, Some(Intent::RunShell { cmd: "pwd".to_string() }));
        assert!(session.has_pending_approval());
    }

    #[test]
    fn approve_pending_executes_gated_step() {
        let mut session = PlanSession::new(vec![Intent::RunShell {
            cmd: "pwd".to_string(),
        }]);

        let _ = session.execute_next_with_policy(
            |_| ExecutionOutcome {
                success: true,
                reply: "ok".to_string(),
            },
            |intent| matches!(intent, Intent::RunShell { .. }),
        );

        let progress = session.approve_pending_with_policy(
            |_| ExecutionOutcome {
                success: true,
                reply: "ok".to_string(),
            },
            |intent| matches!(intent, Intent::RunShell { .. }),
        );

        assert_eq!(progress.executed.len(), 1);
        assert!(progress.completed);
        assert!(!progress.paused_on_approval);
        assert!(!session.has_pending_approval());
    }
}