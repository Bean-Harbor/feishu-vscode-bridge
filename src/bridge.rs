use std::path::PathBuf;

use crate::bridge_context::BridgeContext;
use crate::direct_command;
use crate::follow_up;
use crate::intent_executor::execute_runnable_intent;
use crate::plan::ExecutionOutcome;
use crate::plan_dispatch;
use crate::reply;
use crate::semantic_planner::{self, SemanticDispatch};
use crate::session;
use crate::{help_text, parse_explicit_intent, ApprovalPolicy, ExecutionMode, Intent};

pub type IntentExecutor = fn(&Intent) -> ExecutionOutcome;
pub type SemanticPlanner = for<'a> fn(&BridgeContext<'a>, &str, &str) -> SemanticDispatch;

#[derive(Debug, Clone)]
pub enum BridgeResponse {
    Text(String),
    Card {
        fallback_text: String,
        card: serde_json::Value,
    },
}

pub struct BridgeApp {
    session_store_path: Option<PathBuf>,
    approval_policy: ApprovalPolicy,
    executor: IntentExecutor,
    semantic_planner: SemanticPlanner,
}

impl Default for BridgeApp {
    fn default() -> Self {
        Self {
            session_store_path: session::default_session_store_path(),
            approval_policy: ApprovalPolicy::from_env(),
            executor: execute_runnable_intent,
            semantic_planner: semantic_planner::plan_freeform_intent,
        }
    }
}

impl BridgeApp {
    pub fn new(session_store_path: Option<PathBuf>, approval_policy: ApprovalPolicy) -> Self {
        Self {
            session_store_path,
            approval_policy,
            executor: execute_runnable_intent,
            semantic_planner: semantic_planner::plan_freeform_intent,
        }
    }

    pub fn with_executor(
        session_store_path: Option<PathBuf>,
        approval_policy: ApprovalPolicy,
        executor: IntentExecutor,
    ) -> Self {
        Self {
            session_store_path,
            approval_policy,
            executor,
            semantic_planner: semantic_planner::plan_freeform_intent,
        }
    }

    pub fn with_executor_and_planner(
        session_store_path: Option<PathBuf>,
        approval_policy: ApprovalPolicy,
        executor: IntentExecutor,
        semantic_planner: SemanticPlanner,
    ) -> Self {
        Self {
            session_store_path,
            approval_policy,
            executor,
            semantic_planner,
        }
    }

    pub fn dispatch(&self, text: &str, session_key: &str) -> BridgeResponse {
        let trimmed_text = text.trim();
        let context = self.context();
        let intent = parse_explicit_intent(trimmed_text);

        if matches!(intent, Intent::Unknown(_)) {
            if let Some(response) =
                follow_up::resolve_contextual_follow_up(&context, session_key, trimmed_text)
            {
                return response;
            }
            return match (self.semantic_planner)(&context, session_key, trimmed_text) {
                SemanticDispatch::Planned(planned) => {
                    self.dispatch_intent(&context, session_key, trimmed_text, planned)
                }
                SemanticDispatch::Response(response) => response,
            };
        }

        self.dispatch_intent(&context, session_key, trimmed_text, intent)
    }

    fn dispatch_intent(
        &self,
        context: &BridgeContext<'_>,
        session_key: &str,
        task_text: &str,
        intent: Intent,
    ) -> BridgeResponse {
        if let Some(response) =
            self.dispatch_required_approval(context, session_key, task_text, &intent)
        {
            return response;
        }

        match intent {
            Intent::RunPlan { steps, mode } => {
                plan_dispatch::start_plan(context, session_key, task_text, steps, mode)
            }
            Intent::ShowPlanPrompt { prompt } => {
                semantic_planner::show_plan_prompt(context, session_key, task_text, &prompt)
            }
            Intent::ContinuePlan
            | Intent::RetryFailedStep
            | Intent::ExecuteAll
            | Intent::ApprovePending
            | Intent::RejectPending => {
                dispatch_plan_action(context, session_key, task_text, intent)
            }
            Intent::StartAgentRun { .. }
            | Intent::ContinueAgentRun { .. }
            | Intent::ShowAgentRunStatus
            | Intent::ApproveAgentRun { .. }
            | Intent::CancelAgentRun => {
                direct_command::execute_direct_command(context, session_key, task_text, intent)
            }
            Intent::ContinueAgent { prompt } => {
                follow_up::continue_agent_task(context, session_key, task_text, prompt.as_deref())
            }
            Intent::ContinueAgentSuggested => {
                follow_up::continue_agent_suggested_action(context, session_key, task_text)
            }
            Intent::ExplainLastFailure
            | Intent::ShowLastResult
            | Intent::ContinueLastFile
            | Intent::ShowLastDiff
            | Intent::ShowRecentFiles
            | Intent::UndoLastPatch => dispatch_follow_up_action(context, session_key, intent),
            Intent::Help => BridgeResponse::Text(help_text().to_string()),
            Intent::Unknown(raw) => BridgeResponse::Text(format!(
                "❓ 无法识别指令: {raw}\n\n发送「帮助」查看可用命令"
            )),
            other => direct_command::execute_direct_command(context, session_key, task_text, other),
        }
    }

    fn dispatch_required_approval(
        &self,
        context: &BridgeContext<'_>,
        session_key: &str,
        task_text: &str,
        intent: &Intent,
    ) -> Option<BridgeResponse> {
        if !self.approval_policy.requires_approval(intent) {
            return None;
        }

        Some(plan_dispatch::start_plan(
            context,
            session_key,
            task_text,
            vec![intent.clone()],
            ExecutionMode::StepByStep,
        ))
    }

    fn context(&self) -> BridgeContext<'_> {
        BridgeContext::new(
            self.session_store_path.as_ref(),
            &self.approval_policy,
            self.executor,
        )
    }

    pub fn approval_policy(&self) -> &ApprovalPolicy {
        &self.approval_policy
    }
}

fn dispatch_plan_action(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    intent: Intent,
) -> BridgeResponse {
    match intent {
        Intent::ContinuePlan => {
            let Some(stored) =
                session::load_persisted_session(context.session_store_path(), session_key)
            else {
                return BridgeResponse::Text("⚠️ 当前没有待继续的计划。\n\n发送「执行计划 <命令1>; <命令2>」创建逐步计划，或先发送「/copilot <问题>」或「/codex <问题>」建立 agent 任务。".to_string());
            };

            if stored.plan.is_some() {
                plan_dispatch::resume_plan(context, session_key, false, "继续")
            } else if session::is_agent_task_session(&stored) {
                follow_up::continue_agent_task(context, session_key, task_text, None)
            } else {
                BridgeResponse::Text(reply::format_stored_session_summary(&stored))
            }
        }
        Intent::RetryFailedStep => {
            plan_dispatch::resume_plan(context, session_key, false, "重新执行失败步骤")
        }
        Intent::ExecuteAll => plan_dispatch::resume_plan(context, session_key, true, "执行全部"),
        Intent::ApprovePending => plan_dispatch::approve_plan(context, session_key),
        Intent::RejectPending => plan_dispatch::reject_plan(context, session_key),
        _ => unreachable!("non-plan action routed to dispatch_plan_action"),
    }
}

fn dispatch_follow_up_action(
    context: &BridgeContext<'_>,
    session_key: &str,
    intent: Intent,
) -> BridgeResponse {
    match intent {
        Intent::ExplainLastFailure => follow_up::explain_last_failure(context, session_key),
        Intent::ShowLastResult => follow_up::show_last_result(context, session_key),
        Intent::ContinueLastFile => follow_up::continue_last_file(context, session_key),
        Intent::ShowLastDiff => follow_up::show_last_diff(context, session_key),
        Intent::ShowRecentFiles => follow_up::show_recent_files(context, session_key),
        Intent::UndoLastPatch => follow_up::undo_last_patch(context, session_key),
        _ => unreachable!("non-follow-up action routed to dispatch_follow_up_action"),
    }
}

pub fn render_bridge_response(response: &BridgeResponse) -> &str {
    match response {
        BridgeResponse::Text(text) => text,
        BridgeResponse::Card { fallback_text, .. } => fallback_text,
    }
}

pub fn response_kind(response: &BridgeResponse) -> &'static str {
    match response {
        BridgeResponse::Text(_) => "文本",
        BridgeResponse::Card { .. } => "卡片",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::semantic_planner::SemanticDispatch;
    use crate::session::{
        self, StoredDiff, StoredResult, StoredSession, StoredSessionKind, StoredStep,
    };
    use crate::test_support::unique_temp_path;

    fn planner_returns_git_sync(
        _context: &BridgeContext<'_>,
        _session_key: &str,
        _task_text: &str,
    ) -> SemanticDispatch {
        SemanticDispatch::Planned(Intent::GitSync { repo: None })
    }

    fn planner_should_not_run(
        _context: &BridgeContext<'_>,
        _session_key: &str,
        _task_text: &str,
    ) -> SemanticDispatch {
        panic!("semantic planner should not run for explicit commands")
    }

    #[test]
    fn continue_plan_without_pending_plan_returns_continuity_summary() {
        let session_path = unique_temp_path("bridge", "continue-summary");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            session_kind: StoredSessionKind::Plan,
            agent_state: None,
            current_project_path: None,
            plan: None,
            current_task: Some("应用补丁后继续检查 bridge 回复".to_string()),
            pending_steps: vec![
                "运行测试命令 cargo test".to_string(),
                "查看当前工作区 diff".to_string(),
            ],
            last_result: Some(StoredResult {
                status: "待继续".to_string(),
                summary: "下一步是第 2 / 3 步。".to_string(),
                success: true,
            }),
            last_action: Some("继续".to_string()),
            last_step: Some(StoredStep {
                description: "应用补丁到当前工作区".to_string(),
                reply: "✅ 应用补丁  (3ms)\n已更新 src/bridge.rs".to_string(),
                success: true,
            }),
            last_file_path: Some("src/bridge.rs".to_string()),
            recent_file_paths: vec!["src/bridge.rs".to_string(), "docs/work_log.md".to_string()],
            last_diff: Some(StoredDiff {
                description: "查看当前工作区 diff".to_string(),
                content: "diff --git a/src/bridge.rs b/src/bridge.rs\n@@ -1 +1 @@\n-old\n+new"
                    .to_string(),
            }),
            last_patch: None,
        };

        session::persist_session(Some(&session_path), session_key, &stored).unwrap();

        match app.dispatch("继续刚才的任务", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 任务连续性回放"));
                assert!(text.contains("🎯 当前任务: 应用补丁后继续检查 bridge 回复"));
                assert!(text.contains("📌 最近状态: 待继续"));
                assert!(text.contains("🧾 最近一步: 应用补丁到当前工作区"));
                assert!(text.contains("📄 当前聚焦文件: src/bridge.rs"));
                assert!(text.contains("🧩 最近 diff: 查看当前工作区 diff"));
                assert!(text.contains("⏭ 下一步: 运行测试命令 cargo test"));
                assert!(text.contains("➡️ 下一步建议:"));
            }
            BridgeResponse::Card { .. } => panic!("expected text continuity summary"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn continue_plan_without_session_returns_warning() {
        let session_path = unique_temp_path("bridge", "continue-missing");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());

        match app.dispatch("继续刚才的任务", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains("当前没有待继续的计划"));
            }
            BridgeResponse::Card { .. } => panic!("expected warning text"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn freeform_text_routes_through_semantic_planner() {
        let session_path = unique_temp_path("bridge", "semantic-planner");
        let app = BridgeApp::with_executor_and_planner(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            execute_runnable_intent,
            planner_returns_git_sync,
        );

        match app.dispatch("把当前项目同步到 github", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains("同步 Git 状态"));
            }
            BridgeResponse::Card { .. } => panic!("expected text reply"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn explicit_commands_bypass_semantic_planner() {
        let session_path = unique_temp_path("bridge", "explicit-fast-path");
        let app = BridgeApp::with_executor_and_planner(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            execute_runnable_intent,
            planner_should_not_run,
        );

        match app.dispatch("帮助", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains("执行计划"));
            }
            BridgeResponse::Card { .. } => panic!("expected text help reply"),
        }

        let _ = fs::remove_file(session_path);
    }
}
