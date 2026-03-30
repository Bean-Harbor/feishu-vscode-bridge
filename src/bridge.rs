use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::card;
use crate::plan::{ApprovalRequest, ExecutionOutcome, PlanProgress, PlanSession};
use crate::reply;
use crate::session::{self, StoredResult, StoredSession};
use crate::vscode;
use crate::{ApprovalPolicy, ExecutionMode, Intent, help_text, parse_intent};

#[cfg(test)]
use crate::session::{StoredDiff, StoredPatch, StoredStep};

pub type IntentExecutor = fn(&Intent) -> ExecutionOutcome;

#[derive(Debug, Clone)]
pub enum BridgeResponse {
    Text(String),
    Card {
        fallback_text: String,
        card: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub timestamp_ms: u128,
    pub source: String,
    pub session_key: String,
    pub chat_id: String,
    pub chat_type: Option<String>,
    pub sender_id: String,
    pub event_id: String,
    pub command: String,
    pub action_name: Option<String>,
    pub response_kind: String,
    pub response_preview: String,
    pub result_status: Option<String>,
    pub result_summary: Option<String>,
    pub success: bool,
    pub error: Option<String>,
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

        if self.approval_policy.requires_approval(&intent) {
            return self.start_plan(
                session_key,
                trimmed_text,
                vec![intent],
                ExecutionMode::StepByStep,
            );
        }

        match intent {
            Intent::RunPlan { steps, mode } => self.start_plan(session_key, trimmed_text, steps, mode),
            Intent::ContinuePlan => self.resume_plan(session_key, false, "继续"),
            Intent::RetryFailedStep => self.resume_plan(session_key, false, "重新执行失败步骤"),
            Intent::ExecuteAll => self.resume_plan(session_key, true, "执行全部"),
            Intent::ApprovePending => self.approve_plan(session_key),
            Intent::RejectPending => self.reject_plan(session_key),
            Intent::ExplainLastFailure => self.explain_last_failure(session_key),
            Intent::ShowLastResult => self.show_last_result(session_key),
            Intent::ContinueLastFile => self.continue_last_file(session_key),
            Intent::ShowLastDiff => self.show_last_diff(session_key),
            Intent::ShowRecentFiles => self.show_recent_files(session_key),
            Intent::UndoLastPatch => self.undo_last_patch(session_key),
            Intent::Help => BridgeResponse::Text(help_text().to_string()),
            Intent::Unknown(raw) => {
                BridgeResponse::Text(format!("❓ 无法识别指令: {raw}\n\n发送「帮助」查看可用命令"))
            }
            other => self.execute_direct_command(session_key, trimmed_text, other),
        }
    }

    pub fn approval_policy(&self) -> &ApprovalPolicy {
        &self.approval_policy
    }

    fn start_plan(
        &self,
        session_key: &str,
        task_text: &str,
        steps: Vec<Intent>,
        mode: ExecutionMode,
    ) -> BridgeResponse {
        let mut session = PlanSession::new(steps);
        let progress = match mode {
            ExecutionMode::StepByStep => session.execute_next_with_policy(
                self.executor,
                |step_index, step_number, intent, run_all_after_approval| {
                    self.build_approval_request(step_index, step_number, intent, run_all_after_approval)
                },
            ),
            ExecutionMode::ContinueAll => session.execute_remaining_with_policy(
                self.executor,
                |step_index, step_number, intent, run_all_after_approval| {
                    self.build_approval_request(step_index, step_number, intent, run_all_after_approval)
                },
            ),
        };
        let stored = self.build_stored_session(
            if progress.completed { None } else { Some(session.clone()) },
            task_text,
            action_label_for_mode(&mode),
            &progress,
        );
        let reply = card::format_plan_reply(
            &progress,
            matches!(mode, ExecutionMode::ContinueAll),
            &self.approval_policy,
            &stored,
        );
        let _ = self.persist_session(session_key, &stored);

        reply
    }

    fn resume_plan(&self, session_key: &str, run_all: bool, action_name: &str) -> BridgeResponse {
        let Some(mut stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待继续的计划。\n\n发送「执行计划 <命令1>; <命令2>」创建逐步计划，或发送「执行全部 <命令1>; <命令2>」连续执行。".to_string());
        };

        let Some(mut session) = stored.plan.take() else {
            return BridgeResponse::Text(reply::format_stored_session_summary(&stored));
        };

        let progress = if run_all {
            session.execute_remaining_with_policy(self.executor, |step_index, step_number, intent, run_all_after_approval| {
                self.build_approval_request(step_index, step_number, intent, run_all_after_approval)
            })
        } else {
            session.execute_next_with_policy(self.executor, |step_index, step_number, intent, run_all_after_approval| {
                self.build_approval_request(step_index, step_number, intent, run_all_after_approval)
            })
        };
        stored = self.build_stored_session(
            if progress.completed { None } else { Some(session.clone()) },
            stored.current_task.as_deref().unwrap_or("继续当前计划"),
            action_name,
            &progress,
        );
        let reply = card::format_plan_reply(&progress, run_all, &self.approval_policy, &stored);
        let _ = self.persist_session(session_key, &stored);
        append_plan_action_audit(session_key, action_name, &reply, &stored, Some(&progress));

        reply
    }

    fn approve_plan(&self, session_key: &str) -> BridgeResponse {
        let Some(mut stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
        };

        let Some(mut session) = stored.plan.take() else {
            return BridgeResponse::Text(reply::format_stored_session_summary(&stored));
        };

        if !session.has_pending_approval() {
            return BridgeResponse::Text("⚠️ 当前没有待审批步骤。可以发送「继续」或「执行全部」推进计划。".to_string());
        }

        let progress = session.approve_pending_with_policy(self.executor, |step_index, step_number, intent, run_all_after_approval| {
            self.build_approval_request(step_index, step_number, intent, run_all_after_approval)
        });
        stored = self.build_stored_session(
            if progress.completed { None } else { Some(session.clone()) },
            stored.current_task.as_deref().unwrap_or("批准当前计划"),
            "批准",
            &progress,
        );
        let reply = card::format_plan_reply(&progress, false, &self.approval_policy, &stored);
        let _ = self.persist_session(session_key, &stored);
        append_plan_action_audit(session_key, "批准", &reply, &stored, Some(&progress));

        reply
    }

    fn reject_plan(&self, session_key: &str) -> BridgeResponse {
        let Some(mut stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
        };

        let Some(mut session) = stored.plan.take() else {
            return BridgeResponse::Text(reply::format_stored_session_summary(&stored));
        };

        if !session.reject_pending() {
            return BridgeResponse::Text("⚠️ 当前没有待审批步骤。".to_string());
        }

        stored.plan = None;
        stored.pending_steps = Vec::new();
        stored.last_action = Some("拒绝".to_string());
        stored.last_result = Some(session::StoredResult {
            status: "已取消".to_string(),
            summary: "当前待审批任务已被拒绝并取消。".to_string(),
            success: false,
        });
        stored.last_step = None;
        let _ = self.persist_session(session_key, &stored);
        let reply = BridgeResponse::Text("🛑 已拒绝当前待审批步骤，当前计划已取消。".to_string());
        append_plan_action_audit(session_key, "拒绝", &reply, &stored, None);
        reply
    }

    fn explain_last_failure(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(reply::format_last_failure_reply(&stored))
    }

    fn show_last_result(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(reply::format_last_result_reply(&stored))
    }

    fn continue_last_file(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        let Some(path) = stored
            .recent_file_paths
            .first()
            .map(String::as_str)
            .or(stored.last_file_path.as_deref())
        else {
            return BridgeResponse::Text("⚠️ 最近一次任务里没有记录到明确的文件路径。可以先发送「读取 <文件>」或「打开 <文件>」。".to_string());
        };

        let result = vscode::read_file(path, None, None);
        let mut blocks = vec![format!("📄 继续处理刚才的文件: {}", path)];

        if let Some(last_step) = stored.last_step.as_ref() {
            blocks.push(format!("🧾 最近一步: {}", last_step.description));
        }
        if stored.recent_file_paths.len() > 1 {
            blocks.push(format!(
                "🗂 其他最近文件: {}",
                stored.recent_file_paths[1..].join("、")
            ));
        }

        blocks.push(result.to_reply(&format!("读取文件 {path}")));
        BridgeResponse::Text(reply::format_follow_up_reply("继续文件上下文", &stored, blocks))
    }

    fn show_last_diff(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(reply::format_last_diff_reply(&stored))
    }

    fn show_recent_files(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(reply::format_recent_files_reply(&stored))
    }

    fn undo_last_patch(&self, session_key: &str) -> BridgeResponse {
        let Some(mut stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        let Some(last_patch) = stored.last_patch.clone() else {
            return BridgeResponse::Text("⚠️ 最近一次任务里没有可撤回的补丁记录。请先发送「应用补丁 ...」。".to_string());
        };

        let result = vscode::reverse_patch(&last_patch.content);
        let reply = result.to_reply("撤回补丁");
        stored.plan = None;
        stored.current_task = Some("撤回刚才的补丁".to_string());
        stored.pending_steps.clear();
        stored.last_action = Some("撤回补丁".to_string());
        stored.last_result = Some(session::StoredResult {
            status: if result.success { "已完成".to_string() } else { "失败暂停".to_string() },
            summary: if result.success {
                format!("最近一次补丁已撤回，共涉及 {} 个文件。", last_patch.file_paths.len())
            } else {
                "撤回最近一次补丁失败。".to_string()
            },
            success: result.success,
        });
        stored.last_step = Some(session::StoredStep {
            description: "撤回刚才的补丁".to_string(),
            reply: reply.clone(),
            success: result.success,
        });
        stored.last_file_path = last_patch.file_paths.first().cloned();
        stored.recent_file_paths = last_patch.file_paths.clone();
        stored.last_diff = Some(session::StoredDiff {
            description: "撤回刚才的补丁".to_string(),
            content: reply::truncate_session_text(&last_patch.content, 4000),
        });
        if result.success {
            stored.last_patch = None;
        }

        let _ = self.persist_session(session_key, &stored);
        BridgeResponse::Text(reply)
    }

    fn execute_direct_command(
        &self,
        session_key: &str,
        task_text: &str,
        intent: Intent,
    ) -> BridgeResponse {
        if let Intent::AskAgent { prompt } = &intent {
            let result = vscode::ask_agent(session_key, prompt);
            let reply = reply::format_agent_reply(task_text, &result);
            let stored = session::stored_session_from_agent_result(task_text, &intent, &result, &reply);
            let _ = self.persist_session(session_key, &stored);
            return BridgeResponse::Text(reply);
        }

        if let Intent::ResetAgentSession = &intent {
            let result = vscode::reset_agent_session(session_key);
            let outcome = ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("重置 Copilot 会话"),
            };
            let progress = session::progress_from_direct_execution(intent, outcome.clone());
            let stored = self.build_stored_session(None, task_text, "直接执行", &progress);
            let _ = self.persist_session(session_key, &stored);
            return BridgeResponse::Text(outcome.reply);
        }

        let outcome = (self.executor)(&intent);
        let reply = outcome.reply.clone();
        let progress = session::progress_from_direct_execution(intent, outcome);
        let stored = self.build_stored_session(None, task_text, "直接执行", &progress);
        let _ = self.persist_session(session_key, &stored);
        BridgeResponse::Text(reply)
    }

    fn build_approval_request(
        &self,
        step_index: usize,
        step_number: usize,
        intent: &Intent,
        run_all_after_approval: bool,
    ) -> Option<ApprovalRequest> {
        if !self.approval_policy.requires_approval(intent) {
            return None;
        }

        let (reason, risk_summary) = match intent {
            Intent::RunShell { .. } => (
                "shell 命令默认需要人工确认。".to_string(),
                "会在本地 shell 中执行命令，并可能修改工作区或系统状态。".to_string(),
            ),
            Intent::ApplyPatch { .. } => (
                "补丁会直接修改工作区文件。".to_string(),
                "会把补丁写入当前仓库中的一个或多个文件。".to_string(),
            ),
            Intent::WriteFile { path, .. } => (
                format!("写入文件 {path} 前需要人工确认。"),
                format!("会创建或覆盖文件 {path}。"),
            ),
            Intent::GitPushAll { .. } => (
                "推送到远端仓库前需要人工确认。".to_string(),
                "会提交当前改动并把提交推送到远端。".to_string(),
            ),
            Intent::GitPull { .. } => (
                "拉取远端仓库前需要人工确认。".to_string(),
                "会把远端变更合入本地工作区。".to_string(),
            ),
            Intent::InstallExtension { ext_id } => (
                format!("安装扩展 {ext_id} 前需要人工确认。"),
                format!("会在当前 VS Code 环境里安装扩展 {ext_id}。"),
            ),
            Intent::UninstallExtension { ext_id } => (
                format!("卸载扩展 {ext_id} 前需要人工确认。"),
                format!("会从当前 VS Code 环境里移除扩展 {ext_id}。"),
            ),
            _ => (
                "该步骤已命中当前审批策略。".to_string(),
                "执行前需要人工确认。".to_string(),
            ),
        };

        Some(ApprovalRequest {
            step_index,
            step_number,
            intent: intent.clone(),
            action_label: reply::describe_intent(intent),
            reason,
            risk_summary,
            run_all_after_approval,
        })
    }

    fn load_persisted_session(&self, session_key: &str) -> Option<StoredSession> {
        session::load_persisted_session(self.session_store_path.as_ref(), session_key)
    }

    fn persist_session(&self, session_key: &str, session: &StoredSession) -> Result<(), String> {
        session::persist_session(self.session_store_path.as_ref(), session_key, session)
    }

    fn build_stored_session(
        &self,
        plan: Option<PlanSession>,
        task_text: &str,
        action: &str,
        progress: &PlanProgress,
    ) -> StoredSession {
        session::build_stored_session(plan, task_text, action, progress)
    }
}

fn action_label_for_mode(mode: &ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::StepByStep => "执行计划",
        ExecutionMode::ContinueAll => "执行全部",
    }
}

fn stored_result_from_progress(progress: &PlanProgress) -> StoredResult {
    if progress.completed {
        StoredResult {
            status: "已完成".to_string(),
            summary: format!("计划已完成，共执行 {} 步。", progress.total_steps),
            success: true,
        }
    } else if progress.paused_on_approval {
        StoredResult {
            status: "待审批".to_string(),
            summary: format!("第 {} / {} 步等待批准。", progress.next_step + 1, progress.total_steps),
            success: true,
        }
    } else if progress.paused_on_failure {
        let failed = plan_failed_step(progress)
            .map(|step| {
                format!(
                    "第 {} / {} 步失败：{}",
                    step.step_number,
                    progress.total_steps,
                    reply::describe_intent(&step.intent)
                )
            })
            .unwrap_or_else(|| "计划执行失败并已暂停。".to_string());
        StoredResult {
            status: "失败暂停".to_string(),
            summary: failed,
            success: false,
        }
    } else {
        StoredResult {
            status: "待继续".to_string(),
            summary: format!("下一步是第 {} / {} 步。", progress.next_step + 1, progress.total_steps),
            success: true,
        }
    }
}

fn default_audit_log_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("BRIDGE_AUDIT_LOG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    std::env::current_dir()
        .ok()
        .map(|dir| dir.join(".feishu-vscode-bridge-audit.jsonl"))
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

pub fn feishu_session_key(chat_id: &str, sender_id: &str) -> String {
    format!("feishu:chat:{chat_id}:sender:{sender_id}")
}

pub fn new_audit_entry(
    source: &str,
    session_key: &str,
    chat_id: &str,
    chat_type: Option<&str>,
    sender_id: &str,
    event_id: &str,
    command: &str,
    response: &BridgeResponse,
    error: Option<&str>,
) -> AuditEntry {
    AuditEntry {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        source: source.to_string(),
        session_key: session_key.to_string(),
        chat_id: chat_id.to_string(),
        chat_type: chat_type.map(str::to_string),
        sender_id: sender_id.to_string(),
        event_id: event_id.to_string(),
        command: command.to_string(),
        action_name: None,
        response_kind: response_kind(response).to_string(),
        response_preview: reply::truncate_session_text(render_bridge_response(response), 300),
        result_status: None,
        result_summary: None,
        success: error.is_none(),
        error: error.map(str::to_string),
    }
}

fn new_plan_action_audit_entry(
    session_key: &str,
    action_name: &str,
    response: &BridgeResponse,
    stored: &StoredSession,
    progress: Option<&PlanProgress>,
) -> Option<AuditEntry> {
    let (chat_id, sender_id) = parse_feishu_session_key(session_key)?;
    let result = stored.last_result.as_ref();

    Some(AuditEntry {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        source: "plan_action".to_string(),
        session_key: session_key.to_string(),
        chat_id,
        chat_type: None,
        sender_id,
        event_id: format!("plan_action:{action_name}"),
        command: action_name.to_string(),
        action_name: Some(action_name.to_string()),
        response_kind: response_kind(response).to_string(),
        response_preview: reply::truncate_session_text(render_bridge_response(response), 300),
        result_status: result.map(|item| item.status.clone()),
        result_summary: result.map(|item| item.summary.clone()).or_else(|| {
            progress.map(|item| stored_result_from_progress(item).summary)
        }),
        success: result.map(|item| item.success).unwrap_or(true),
        error: None,
    })
}

fn append_plan_action_audit(
    session_key: &str,
    action_name: &str,
    response: &BridgeResponse,
    stored: &StoredSession,
    progress: Option<&PlanProgress>,
) {
    let Some(entry) = new_plan_action_audit_entry(session_key, action_name, response, stored, progress) else {
        return;
    };

    if let Err(err) = append_audit_entry(&entry) {
        eprintln!("❌ 审计写入失败: {err}");
    }
}

fn parse_feishu_session_key(session_key: &str) -> Option<(String, String)> {
    let rest = session_key.strip_prefix("feishu:chat:")?;
    let (chat_id, sender_id) = rest.split_once(":sender:")?;
    Some((chat_id.to_string(), sender_id.to_string()))
}

pub fn append_audit_entry(entry: &AuditEntry) -> Result<(), String> {
    let Some(path) = default_audit_log_path() else {
        return Err("无法定位审计日志路径".to_string());
    };

    append_audit_entry_to_path(&path, entry)
}

fn append_audit_entry_to_path(path: &Path, entry: &AuditEntry) -> Result<(), String> {
    let line = serde_json::to_string(entry)
        .map_err(|err| format!("序列化审计日志失败: {err}"))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("打开审计日志失败: {err}"))?;

    writeln!(file, "{line}").map_err(|err| format!("写入审计日志失败: {err}"))
}

fn plan_failed_step(progress: &PlanProgress) -> Option<&crate::plan::StepExecution> {
    progress.executed.iter().rev().find(|step| !step.outcome.success)
}

fn execute_runnable_intent(intent: &Intent) -> ExecutionOutcome {
    match intent {
        Intent::OpenFile { path, line } => {
            let result = vscode::open_file(path, *line);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("打开 {path}")),
            }
        }
        Intent::OpenFolder { path } => {
            let result = vscode::open_folder(path);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("打开目录 {path}")),
            }
        }
        Intent::InstallExtension { ext_id } => {
            let result = vscode::install_extension(ext_id);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("安装扩展 {ext_id}")),
            }
        }
        Intent::UninstallExtension { ext_id } => {
            let result = vscode::uninstall_extension(ext_id);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("卸载扩展 {ext_id}")),
            }
        }
        Intent::ListExtensions => {
            let result = vscode::list_extensions();
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("已安装扩展"),
            }
        }
        Intent::DiffFiles { file1, file2 } => {
            let result = vscode::diff_files(file1, file2);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("diff {file1} {file2}")),
            }
        }
        Intent::ReadFile {
            path,
            start_line,
            end_line,
        } => {
            let result = vscode::read_file(path, *start_line, *end_line);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("读取文件 {path}")),
            }
        }
        Intent::ListDirectory { path } => {
            let result = vscode::list_directory(path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("列出目录"),
            }
        }
        Intent::SearchText {
            query,
            path,
            is_regex,
        } => {
            let result = vscode::search_text(query, path.as_deref(), *is_regex);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(if *is_regex { "搜索正则" } else { "搜索文本" }),
            }
        }
        Intent::RunTests { command } => {
            let result = vscode::run_tests(command.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("运行测试"),
            }
        }
        Intent::GitDiff { path } => {
            let result = vscode::git_diff(path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("查看 diff"),
            }
        }
        Intent::ApplyPatch { patch } => {
            let result = vscode::apply_patch(patch);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("应用补丁"),
            }
        }
        Intent::GitStatus { repo } => {
            let result = vscode::git_status(repo.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git 状态"),
            }
        }
        Intent::GitPull { repo } => {
            let result = vscode::git_pull(repo.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git Pull"),
            }
        }
        Intent::GitPushAll { repo, message } => {
            let result = vscode::git_push_all(repo.as_deref(), message);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git Push"),
            }
        }
        Intent::GitLog { count, path } => {
            let result = vscode::git_log(*count, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git Log"),
            }
        }
        Intent::GitBlame { path } => {
            let result = vscode::git_blame(path);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("Git Blame {path}")),
            }
        }
        Intent::SearchSymbol { query, path } => {
            let result = vscode::search_symbol(query, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("搜索符号"),
            }
        }
        Intent::FindReferences { query, path } => {
            let result = vscode::find_references(query, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("查找引用"),
            }
        }
        Intent::FindImplementations { query, path } => {
            let result = vscode::find_implementations(query, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("查找实现"),
            }
        }
        Intent::RunSpecificTest { filter } => {
            let result = vscode::run_specific_test(filter);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("运行测试 {filter}")),
            }
        }
        Intent::RunTestFile { path } => {
            let result = vscode::run_test_file(path);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("运行测试文件 {path}")),
            }
        }
        Intent::WriteFile { path, content } => {
            let result = vscode::write_file(path, content);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("写入 {path}")),
            }
        }
        Intent::RunShell { cmd } => {
            let result = vscode::run_shell(cmd);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("$ {cmd}")),
            }
        }
        Intent::AskAgent { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ 问 Copilot 目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::ResetAgentSession => ExecutionOutcome {
            success: false,
            reply: "⚠️ 重置 Copilot 会话目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::Help => ExecutionOutcome {
            success: true,
            reply: help_text().to_string(),
        },
        Intent::Unknown(raw) => ExecutionOutcome {
            success: false,
            reply: format!("❓ 无法识别指令: {raw}"),
        },
        Intent::RunPlan { .. }
        | Intent::ContinuePlan
        | Intent::RetryFailedStep
        | Intent::ExecuteAll
        | Intent::ApprovePending
        | Intent::RejectPending
        | Intent::ExplainLastFailure
        | Intent::ShowLastResult
        | Intent::ContinueLastFile
        | Intent::ShowLastDiff
        | Intent::ShowRecentFiles
        | Intent::UndoLastPatch => ExecutionOutcome {
            success: false,
            reply: "⚠️ 当前步骤不是可直接执行的底层命令。".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn stored_task(task: &str, status: &str, summary: &str) -> StoredSession {
        StoredSession {
            plan: None,
            current_task: Some(task.to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: status.to_string(),
                summary: summary.to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: None,
            last_file_path: None,
            recent_file_paths: Vec::new(),
            last_diff: None,
            last_patch: None,
        }
    }

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

    fn shell_intent(cmd: &str) -> Intent {
        Intent::RunShell {
            cmd: cmd.to_string(),
        }
    }

    #[test]
    fn completion_reply_returns_completion_card() {
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 1,
                intent: shell_intent("pwd"),
                outcome: ExecutionOutcome {
                    success: true,
                    reply: "ok".to_string(),
                },
            }],
            total_steps: 1,
            next_step: 1,
            completed: true,
            paused_on_failure: false,
            paused_on_approval: false,
            approval_request: None,
        };

        let stored = stored_task("执行计划 $ pwd", "已完成", "计划已完成，共执行 1 步。");

        match crate::card::format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card { fallback_text, card } => {
                assert!(fallback_text.contains("状态: 已完成"));
                assert_eq!(card["header"]["title"]["content"], "已完成");
                assert!(card.to_string().contains("当前任务"));
                assert!(card.to_string().contains("执行计划 $ pwd"));
                assert!(card.to_string().contains("最近结果"));
                assert!(card["elements"].as_array().unwrap().iter().all(|element| element["tag"] != "action"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn paused_reply_contains_failed_step_details() {
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 2,
                intent: shell_intent("false"),
                outcome: ExecutionOutcome {
                    success: false,
                    reply: "failed".to_string(),
                },
            }],
            total_steps: 3,
            next_step: 1,
            completed: false,
            paused_on_failure: true,
            paused_on_approval: false,
            approval_request: None,
        };

        let stored = stored_task("执行全部 $ false; $ pwd", "失败暂停", "第 2 / 3 步失败：执行命令 false");

        match crate::card::format_plan_reply(&progress, true, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card { fallback_text, card } => {
                assert!(fallback_text.contains("失败步骤: 第 2 / 3 步"));
                assert_eq!(card["header"]["title"]["content"], "已暂停");
                assert!(card.to_string().contains("执行全部 $ false; $ pwd"));
                assert!(card.to_string().contains("失败暂停: 第 2 / 3 步失败"));
                assert!(card.to_string().contains("失败步骤"));
                assert!(card.to_string().contains("重试这步"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn approval_reply_contains_approve_actions() {
        let progress = PlanProgress {
            executed: Vec::new(),
            total_steps: 1,
            next_step: 0,
            completed: false,
            paused_on_failure: false,
            paused_on_approval: true,
            approval_request: Some(ApprovalRequest {
                step_index: 0,
                step_number: 1,
                intent: shell_intent("pwd"),
                action_label: "执行命令 pwd".to_string(),
                reason: "shell 命令默认需要人工确认。".to_string(),
                risk_summary: "会在本地 shell 中执行命令，并可能修改工作区或系统状态。".to_string(),
                run_all_after_approval: false,
            }),
        };

        let stored = stored_task("执行计划 git pull", "待审批", "第 1 / 1 步等待批准。");

        match crate::card::format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card { fallback_text, card } => {
                assert!(fallback_text.contains("待审批步骤"));
                assert_eq!(card["header"]["title"]["content"], "等你确认");
                assert!(card.to_string().contains("执行计划 git pull"));
                assert!(card.to_string().contains("待审批: 第 1 / 1 步等待批准。"));
                assert!(card.to_string().contains("确认继续"));
                assert!(card.to_string().contains("取消这步"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn explain_last_failure_returns_last_step_detail() {
        let session_path = unique_temp_path("failure");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("执行计划 $ false; $ pwd".to_string()),
            pending_steps: vec!["执行命令 pwd".to_string()],
            last_result: Some(StoredResult {
                status: "失败暂停".to_string(),
                summary: "第 1 / 2 步失败：执行命令 false".to_string(),
                success: false,
            }),
            last_action: Some("继续".to_string()),
            last_step: Some(StoredStep {
                description: "执行命令 false".to_string(),
                reply: "❌ $ false  (1ms)\n(exit code 1)".to_string(),
                success: false,
            }),
            last_file_path: None,
            recent_file_paths: Vec::new(),
            last_diff: None,
            last_patch: None,
        };

        app.persist_session(session_key, &stored).unwrap();

        match app.dispatch("刚才为什么失败", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 失败原因回放"));
                assert!(text.contains("🎯 当前任务: 执行计划 $ false; $ pwd"));
                assert!(text.contains("上次失败状态: 失败暂停"));
                assert!(text.contains("卡住的位置: 执行命令 false"));
                assert!(text.contains("关键报错:"));
                assert!(text.contains("下一步建议:"));
                assert!(text.contains("执行命令 false"));
                assert!(text.contains("$ false"));
            }
            BridgeResponse::Card { .. } => panic!("expected text failure explanation"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn show_last_result_returns_last_step_and_file() {
        let session_path = unique_temp_path("last-result");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("读取 src/lib.rs 1-20".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "计划已完成，共执行 1 步。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(StoredStep {
                description: "读取文件 src/lib.rs:1-20".to_string(),
                reply: "✅ 读取文件 src/lib.rs  (1ms)".to_string(),
                success: true,
            }),
            last_file_path: Some("src/lib.rs".to_string()),
            recent_file_paths: vec!["src/lib.rs".to_string()],
            last_diff: None,
            last_patch: None,
        };

        app.persist_session(session_key, &stored).unwrap();

        match app.dispatch("把上一步结果发我", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 上一步结果回放"));
                assert!(text.contains("📌 最近状态: 已完成"));
                assert!(text.contains("上一步结果: 成功"));
                assert!(text.contains("导语: 上一步已经完成"));
                assert!(text.contains("结果摘要:"));
                assert!(text.contains("下一步建议:"));
                assert!(text.contains("读取文件 src/lib.rs:1-20"));
                assert!(text.contains("相关文件: src/lib.rs"));
            }
            BridgeResponse::Card { .. } => panic!("expected text last-result reply"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn continue_last_file_reads_the_file() {
        let session_path = unique_temp_path("last-file-session");
        let file_path = unique_temp_path("last-file-target");
        fs::write(&file_path, "alpha\nbeta\n").unwrap();

        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("继续修改 demo 文件".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "最近一次读取成功。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(StoredStep {
                description: "读取文件 demo.txt".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some(file_path.to_string_lossy().to_string()),
            recent_file_paths: vec![file_path.to_string_lossy().to_string()],
            last_diff: None,
            last_patch: None,
        };

        app.persist_session(session_key, &stored).unwrap();

        match app.dispatch("继续改刚才那个文件", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 继续文件上下文"));
                assert!(text.contains("继续处理刚才的文件"));
                assert!(text.contains(file_path.to_string_lossy().as_ref()));
                assert!(text.contains("alpha"));
            }
            BridgeResponse::Card { .. } => panic!("expected text file continuation reply"),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_file(file_path);
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

        app.persist_session(session_key, &stored).unwrap();

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

    #[test]
    fn build_stored_session_remembers_file_from_apply_patch() {
        let app = BridgeApp::new(None, ApprovalPolicy::default());
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 1,
                intent: Intent::ApplyPatch {
                    patch: "diff --git a/src/demo.rs b/src/demo.rs\n--- a/src/demo.rs\n+++ b/src/demo.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
                },
                outcome: ExecutionOutcome {
                    success: true,
                    reply: "patched".to_string(),
                },
            }],
            total_steps: 1,
            next_step: 1,
            completed: true,
            paused_on_failure: false,
            paused_on_approval: false,
            approval_request: None,
        };

        let stored = app.build_stored_session(None, "应用补丁", "执行计划", &progress);

        assert_eq!(stored.last_file_path, Some("src/demo.rs".to_string()));
        assert_eq!(stored.recent_file_paths, vec!["src/demo.rs".to_string()]);
        assert!(stored.last_patch.is_some());
    }

    #[test]
    fn build_stored_session_remembers_all_files_from_apply_patch() {
        let app = BridgeApp::new(None, ApprovalPolicy::default());
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 1,
                intent: Intent::ApplyPatch {
                    patch: "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/src/bridge.rs b/src/bridge.rs\n--- a/src/bridge.rs\n+++ b/src/bridge.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
                },
                outcome: ExecutionOutcome {
                    success: true,
                    reply: "patched".to_string(),
                },
            }],
            total_steps: 1,
            next_step: 1,
            completed: true,
            paused_on_failure: false,
            paused_on_approval: false,
            approval_request: None,
        };

        let stored = app.build_stored_session(None, "应用补丁", "执行计划", &progress);

        assert_eq!(
            stored.recent_file_paths,
            vec!["src/bridge.rs".to_string(), "src/lib.rs".to_string()]
        );
        assert!(stored.last_diff.as_ref().is_some_and(|diff| diff.content.contains("diff --git a/src/lib.rs")));
        assert!(stored.last_patch.as_ref().is_some_and(|patch| patch.file_paths == vec!["src/bridge.rs".to_string(), "src/lib.rs".to_string()]));
    }

    #[test]
    fn show_last_diff_returns_patch_content() {
        let session_path = unique_temp_path("last-diff");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("应用补丁 demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "补丁已应用。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(StoredStep {
                description: "应用补丁到当前工作区".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some("src/demo.rs".to_string()),
            recent_file_paths: vec!["src/demo.rs".to_string()],
            last_diff: Some(StoredDiff {
                description: "应用补丁到当前工作区".to_string(),
                content: "diff --git a/src/demo.rs b/src/demo.rs\n--- a/src/demo.rs\n+++ b/src/demo.rs".to_string(),
            }),
            last_patch: Some(StoredPatch {
                content: "diff --git a/src/demo.rs b/src/demo.rs\n--- a/src/demo.rs\n+++ b/src/demo.rs".to_string(),
                file_paths: vec!["src/demo.rs".to_string()],
            }),
        };

        app.persist_session(session_key, &stored).unwrap();

        match app.dispatch("把刚才的 diff 发我", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 最近 diff 回放"));
                assert!(text.contains("最近一次 diff"));
                assert!(text.contains("src/demo.rs"));
                assert!(text.contains("diff --git a/src/demo.rs b/src/demo.rs"));
            }
            BridgeResponse::Card { .. } => panic!("expected text last-diff reply"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn completion_card_includes_follow_up_actions_when_context_exists() {
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 1,
                intent: Intent::ApplyPatch {
                    patch: "diff --git a/src/demo.rs b/src/demo.rs\n--- a/src/demo.rs\n+++ b/src/demo.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
                },
                outcome: ExecutionOutcome {
                    success: true,
                    reply: "ok".to_string(),
                },
            }],
            total_steps: 1,
            next_step: 1,
            completed: true,
            paused_on_failure: false,
            paused_on_approval: false,
            approval_request: None,
        };
        let stored = StoredSession {
            plan: None,
            current_task: Some("应用补丁 demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "补丁已应用。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(StoredStep {
                description: "应用补丁到当前工作区".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some("src/demo.rs".to_string()),
            recent_file_paths: vec!["src/demo.rs".to_string()],
            last_diff: Some(StoredDiff {
                description: "应用补丁到当前工作区".to_string(),
                content: "diff --git a/src/demo.rs b/src/demo.rs".to_string(),
            }),
            last_patch: Some(StoredPatch {
                content: "diff --git a/src/demo.rs b/src/demo.rs".to_string(),
                file_paths: vec!["src/demo.rs".to_string()],
            }),
        };

        match crate::card::format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card { card, .. } => {
                let card_text = card.to_string();
                assert!(card_text.contains("看上一步"));
                assert!(card_text.contains("继续这个文件"));
                assert!(card_text.contains("看 diff"));
                assert!(card_text.contains("看文件列表"));
                assert!(card_text.contains("撤回补丁"));
                assert!(card_text.contains("继续问"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn show_recent_files_returns_recent_file_list() {
        let session_path = unique_temp_path("recent-files");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("应用补丁 demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "补丁已应用。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: None,
            last_file_path: Some("src/a.rs".to_string()),
            recent_file_paths: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
            last_diff: None,
            last_patch: None,
        };

        app.persist_session(session_key, &stored).unwrap();

        match app.dispatch("把刚才改动的文件列表发我", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 最近文件回放"));
                assert!(text.contains("最近改动文件列表"));
                assert!(text.contains("1. src/a.rs"));
                assert!(text.contains("2. src/b.rs"));
            }
            BridgeResponse::Card { .. } => panic!("expected text recent-files reply"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn direct_command_persists_session_context() {
        let session_path = unique_temp_path("direct-session");
        let file_path = unique_temp_path("direct-file");
        fs::write(&file_path, "alpha\nbeta\n").unwrap();

        fn fake_executor(intent: &Intent) -> ExecutionOutcome {
            ExecutionOutcome {
                success: true,
                reply: format!("ok: {}", reply::describe_intent(intent)),
            }
        }

        let app = BridgeApp::with_executor(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            fake_executor,
        );

        match app.dispatch(&format!("读取 {} 1-1", file_path.to_string_lossy()), "cli") {
            BridgeResponse::Text(text) => assert!(text.contains("ok: 读取文件")),
            BridgeResponse::Card { .. } => panic!("expected direct text reply"),
        }

        match app.dispatch("继续改刚才那个文件", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains(file_path.to_string_lossy().as_ref()));
                assert!(text.contains("alpha"));
            }
            BridgeResponse::Card { .. } => panic!("expected file continuation reply"),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_file(file_path);
    }

    #[test]
    fn feishu_session_key_isolates_senders_in_same_chat() {
        let alice = feishu_session_key("oc_chat_demo", "ou_alice");
        let bob = feishu_session_key("oc_chat_demo", "ou_bob");

        assert_ne!(alice, bob);
        assert_eq!(alice, "feishu:chat:oc_chat_demo:sender:ou_alice");
    }

    #[test]
    fn parse_feishu_session_key_extracts_chat_and_sender() {
        let parsed = parse_feishu_session_key("feishu:chat:oc_chat_demo:sender:ou_alice").unwrap();

        assert_eq!(parsed.0, "oc_chat_demo");
        assert_eq!(parsed.1, "ou_alice");
    }

    #[test]
    fn new_plan_action_audit_entry_captures_result_status() {
        let stored = StoredSession {
            last_result: Some(StoredResult {
                status: "已取消".to_string(),
                summary: "当前待审批任务已被拒绝并取消。".to_string(),
                success: false,
            }),
            ..StoredSession::default()
        };

        let entry = new_plan_action_audit_entry(
            "feishu:chat:oc_chat_demo:sender:ou_alice",
            "拒绝",
            &BridgeResponse::Text("🛑 已拒绝当前待审批步骤，当前计划已取消。".to_string()),
            &stored,
            None,
        )
        .unwrap();

        assert_eq!(entry.source, "plan_action");
        assert_eq!(entry.command, "拒绝");
        assert_eq!(entry.action_name.as_deref(), Some("拒绝"));
        assert_eq!(entry.result_status.as_deref(), Some("已取消"));
        assert_eq!(entry.result_summary.as_deref(), Some("当前待审批任务已被拒绝并取消。"));
        assert!(!entry.success);
    }

    #[test]
    fn append_audit_entry_writes_jsonl_record() {
        let audit_path = unique_temp_path("audit-log");
        let entry = AuditEntry {
            timestamp_ms: 123,
            source: "message".to_string(),
            session_key: "feishu:chat:oc_chat_demo:sender:ou_alice".to_string(),
            chat_id: "oc_chat_demo".to_string(),
            chat_type: Some("group".to_string()),
            sender_id: "ou_alice".to_string(),
            event_id: "om_123".to_string(),
            command: "查看 diff".to_string(),
            action_name: None,
            response_kind: "文本".to_string(),
            response_preview: "ok".to_string(),
            result_status: None,
            result_summary: None,
            success: true,
            error: None,
        };

        append_audit_entry_to_path(&audit_path, &entry).unwrap();

        let content = fs::read_to_string(&audit_path).unwrap();
        assert!(content.contains("\"source\":\"message\""));
        assert!(content.contains("\"chat_type\":\"group\""));
        assert!(content.contains("\"command\":\"查看 diff\""));

        let _ = fs::remove_file(audit_path);
    }
}