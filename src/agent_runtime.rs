use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunMode {
    Ask,
    Plan,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Initialized,
    Running,
    WaitingUser,
    Completed,
    Cancelled,
    Failed,
}

impl AgentRunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Initialized => "initialized",
            Self::Running => "running",
            Self::WaitingUser => "waiting_user",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlPointKind {
    Authorization,
    ResultDisposition,
    GoalRevision,
    Pacing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultDisposition {
    Pending,
    Kept,
    Reverted,
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReversibleArtifactKind {
    Patch,
    FileWrite,
    CommandSideEffect,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuthorizationPolicy {
    pub require_write_approval: bool,
    pub require_shell_approval: bool,
    pub require_destructive_approval: bool,
    pub allow_bypass_for_session: bool,
}

impl Default for AgentAuthorizationPolicy {
    fn default() -> Self {
        Self {
            require_write_approval: true,
            require_shell_approval: true,
            require_destructive_approval: true,
            allow_bypass_for_session: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDecisionOption {
    pub option_id: String,
    pub label: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUserDecision {
    pub decision_id: String,
    pub control_kind: ControlPointKind,
    pub summary: String,
    #[serde(default)]
    pub options: Vec<AgentDecisionOption>,
    #[serde(default)]
    pub recommended_option_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReversibleArtifact {
    pub artifact_id: String,
    pub kind: ReversibleArtifactKind,
    pub summary: String,
    #[serde(default)]
    pub file_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBudget {
    pub max_iterations: u32,
    pub max_tool_calls: u32,
    pub max_write_operations: u32,
}

impl Default for RunBudget {
    fn default() -> Self {
        Self {
            max_iterations: 12,
            max_tool_calls: 24,
            max_write_operations: 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCheckpoint {
    pub checkpoint_id: String,
    pub label: String,
    pub status_summary: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunState {
    pub run_id: String,
    pub mode: AgentRunMode,
    pub status: AgentRunStatus,
    pub summary: String,
    pub current_action: String,
    pub next_action: String,
    #[serde(default)]
    pub current_step: Option<String>,
    #[serde(default)]
    pub authorization_policy: Option<AgentAuthorizationPolicy>,
    pub result_disposition: ResultDisposition,
    #[serde(default)]
    pub pending_user_decision: Option<PendingUserDecision>,
    pub budget: RunBudget,
    #[serde(default)]
    pub checkpoints: Vec<RunCheckpoint>,
    #[serde(default)]
    pub reversible_artifacts: Vec<ReversibleArtifact>,
}