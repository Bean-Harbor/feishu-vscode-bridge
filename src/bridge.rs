use std::path::PathBuf;

use crate::direct_command;
use crate::follow_up;
use crate::bridge_context::BridgeContext;
use crate::intent_executor::execute_runnable_intent;
use crate::plan_dispatch;
use crate::plan::ExecutionOutcome;
use crate::session;
use crate::{ApprovalPolicy, ExecutionMode, Intent, help_text, parse_intent};

pub type IntentExecutor = fn(&Intent) -> ExecutionOutcome;

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
}

impl Default for BridgeApp {
    fn default() -> Self {
        Self {
            session_store_path: session::default_session_store_path(),
            approval_policy: ApprovalPolicy::from_env(),
            executor: execute_runnable_intent,
        }
    }
}

impl BridgeApp {
    pub fn new(session_store_path: Option<PathBuf>, approval_policy: ApprovalPolicy) -> Self {
        Self {
            session_store_path,
            approval_policy,
            executor: execute_runnable_intent,
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
        }
    }

    pub fn dispatch(&self, text: &str, session_key: &str) -> BridgeResponse {
        let intent = parse_intent(text);
        let trimmed_text = text.trim();
        let context = self.context();

        if self.approval_policy.requires_approval(&intent) {
            return plan_dispatch::start_plan(
                &context,
                session_key,
                trimmed_text,
                vec![intent],
                ExecutionMode::StepByStep,
            );
        }

        match intent {
            Intent::RunPlan { steps, mode } => plan_dispatch::start_plan(
                &context,
                session_key,
                trimmed_text,
                steps,
                mode,
            ),
            Intent::ContinuePlan => plan_dispatch::resume_plan(
                &context,
                session_key,
                false,
                "继续",
            ),
            Intent::RetryFailedStep => plan_dispatch::resume_plan(
                &context,
                session_key,
                false,
                "重新执行失败步骤",
            ),
            Intent::ExecuteAll => plan_dispatch::resume_plan(
                &context,
                session_key,
                true,
                "执行全部",
            ),
            Intent::ApprovePending => plan_dispatch::approve_plan(&context, session_key),
            Intent::RejectPending => plan_dispatch::reject_plan(&context, session_key),
            Intent::ExplainLastFailure => follow_up::explain_last_failure(&context, session_key),
            Intent::ShowLastResult => follow_up::show_last_result(&context, session_key),
            Intent::ContinueLastFile => follow_up::continue_last_file(&context, session_key),
            Intent::ShowLastDiff => follow_up::show_last_diff(&context, session_key),
            Intent::ShowRecentFiles => follow_up::show_recent_files(&context, session_key),
            Intent::UndoLastPatch => follow_up::undo_last_patch(&context, session_key),
            Intent::Help => BridgeResponse::Text(help_text().to_string()),
            Intent::Unknown(raw) => {
                BridgeResponse::Text(format!("❓ 无法识别指令: {raw}\n\n发送「帮助」查看可用命令"))
            }
            other => direct_command::execute_direct_command(
                &context,
                session_key,
                trimmed_text,
                other,
            ),
        }
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::session::{self, StoredDiff, StoredResult, StoredSession, StoredStep};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "feishu-vscode-bridge-bridge-tests-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn continue_plan_without_pending_plan_returns_continuity_summary() {
        let session_path = unique_temp_path("continue-summary");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("应用补丁后继续检查 bridge 回复".to_string()),
            pending_steps: vec!["运行测试命令 cargo test".to_string(), "查看当前工作区 diff".to_string()],
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
                content: "diff --git a/src/bridge.rs b/src/bridge.rs\n@@ -1 +1 @@\n-old\n+new".to_string(),
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
        let session_path = unique_temp_path("continue-missing");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());

        match app.dispatch("继续刚才的任务", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains("当前没有待继续的计划"));
            }
            BridgeResponse::Card { .. } => panic!("expected warning text"),
        }

        let _ = fs::remove_file(session_path);
    }

}