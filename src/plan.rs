use serde::{Deserialize, Serialize};

use crate::Intent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub step_index: usize,
    pub step_number: usize,
    pub intent: Intent,
    pub action_label: String,
    pub reason: String,
    pub risk_summary: String,
    pub run_all_after_approval: bool,
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
    pub approval_request: Option<ApprovalRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSession {
    steps: Vec<Intent>,
    next_step: usize,
    pending_approval: Option<ApprovalRequest>,
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

    pub fn pending_steps(&self) -> &[Intent] {
        if self.next_step >= self.steps.len() {
            &[]
        } else {
            &self.steps[self.next_step..]
        }
    }

    pub fn current_step(&self) -> Option<&Intent> {
        self.steps.get(self.next_step)
    }

    pub fn execute_next<F>(&mut self, mut executor: F) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        self.execute_next_with_policy(&mut executor, |_, _, _, _| None)
    }

    pub fn execute_next_with_policy<F, G>(
        &mut self,
        mut executor: F,
        mut approval_request: G,
    ) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
        G: FnMut(usize, usize, &Intent, bool) -> Option<ApprovalRequest>,
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
                approval_request: None,
            };
        }

        self.execute_internal(1, false, None, &mut executor, &mut approval_request)
    }

    pub fn execute_remaining<F>(&mut self, mut executor: F) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        self.execute_remaining_with_policy(&mut executor, |_, _, _, _| None)
    }

    pub fn execute_remaining_with_policy<F, G>(
        &mut self,
        mut executor: F,
        mut approval_request: G,
    ) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
        G: FnMut(usize, usize, &Intent, bool) -> Option<ApprovalRequest>,
    {
        if let Some(progress) = self.pending_approval_progress() {
            return progress;
        }

        let max_steps = self.remaining_steps();
        self.execute_internal(max_steps, true, None, &mut executor, &mut approval_request)
    }

    pub fn approve_pending<F>(&mut self, mut executor: F) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        self.approve_pending_with_policy(&mut executor, |_, _, _, _| None)
    }

    pub fn approve_pending_with_policy<F, G>(
        &mut self,
        mut executor: F,
        mut approval_request: G,
    ) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
        G: FnMut(usize, usize, &Intent, bool) -> Option<ApprovalRequest>,
    {
        let Some(pending) = self.pending_approval.take() else {
            return self.pending_approval_progress().unwrap_or_else(|| PlanProgress {
                executed: Vec::new(),
                total_steps: self.total_steps(),
                next_step: self.next_step,
                completed: self.is_complete(),
                paused_on_failure: false,
                paused_on_approval: false,
                approval_request: None,
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
            &mut approval_request,
        )
    }

    pub fn reject_pending(&mut self) -> bool {
        self.pending_approval.take().is_some()
    }

    fn pending_approval_progress(&self) -> Option<PlanProgress> {
        let pending = self.pending_approval.as_ref()?.clone();

        Some(PlanProgress {
            executed: Vec::new(),
            total_steps: self.total_steps(),
            next_step: self.next_step,
            completed: false,
            paused_on_failure: false,
            paused_on_approval: true,
            approval_request: Some(pending),
        })
    }

    fn execute_internal<F>(
        &mut self,
        max_steps: usize,
        run_all_after_approval: bool,
        skip_approval_for_step: Option<usize>,
        executor: &mut F,
        approval_request: &mut dyn FnMut(usize, usize, &Intent, bool) -> Option<ApprovalRequest>,
    ) -> PlanProgress
    where
        F: FnMut(&Intent) -> ExecutionOutcome,
    {
        let mut executed = Vec::new();
        let total_steps = self.total_steps();
        let mut paused_on_failure = false;
        let mut paused_on_approval = false;
        let mut pending_request = None;

        for _ in 0..max_steps {
            if self.is_complete() {
                break;
            }

            let step_number = self.next_step + 1;
            let intent = self.steps[self.next_step].clone();

            if Some(self.next_step) != skip_approval_for_step {
                if let Some(request) = approval_request(
                    self.next_step,
                    step_number,
                    &intent,
                    run_all_after_approval,
                ) {
                    self.pending_approval = Some(request.clone());
                    paused_on_approval = true;
                    pending_request = Some(request);
                    break;
                }
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
            approval_request: pending_request,
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
        assert!(progress.approval_request.is_none());
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
        assert!(progress.approval_request.is_none());
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
            |step_index, step_number, intent, run_all_after_approval| match intent {
                Intent::RunShell { .. } => Some(ApprovalRequest {
                    step_index,
                    step_number,
                    intent: intent.clone(),
                    action_label: "执行命令 pwd".to_string(),
                    reason: "shell 命令默认需要人工确认。".to_string(),
                    risk_summary: "会在本地 shell 中执行命令。".to_string(),
                    run_all_after_approval,
                }),
                _ => None,
            },
        );

        assert!(progress.executed.is_empty());
        assert!(progress.paused_on_approval);
        assert!(!progress.paused_on_failure);
        assert_eq!(progress.approval_request.as_ref().map(|request| request.step_number), Some(1));
        assert_eq!(
            progress
                .approval_request
                .as_ref()
                .map(|request| request.intent.clone()),
            Some(Intent::RunShell {
                cmd: "pwd".to_string()
            })
        );
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
            |step_index, step_number, intent, run_all_after_approval| match intent {
                Intent::RunShell { .. } => Some(ApprovalRequest {
                    step_index,
                    step_number,
                    intent: intent.clone(),
                    action_label: "执行命令 pwd".to_string(),
                    reason: "shell 命令默认需要人工确认。".to_string(),
                    risk_summary: "会在本地 shell 中执行命令。".to_string(),
                    run_all_after_approval,
                }),
                _ => None,
            },
        );

        let progress = session.approve_pending_with_policy(
            |_| ExecutionOutcome {
                success: true,
                reply: "ok".to_string(),
            },
            |_, _, _, _| None,
        );

        assert_eq!(progress.executed.len(), 1);
        assert!(progress.completed);
        assert!(!progress.paused_on_approval);
        assert!(progress.approval_request.is_none());
        assert!(!session.has_pending_approval());
    }
}