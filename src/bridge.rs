use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::plan::{ExecutionOutcome, PlanProgress, PlanSession};
use crate::vscode;
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StoredSession {
    plan: Option<PlanSession>,
    current_task: Option<String>,
    pending_steps: Vec<String>,
    last_result: Option<StoredResult>,
    last_action: Option<String>,
    last_step: Option<StoredStep>,
    last_file_path: Option<String>,
    recent_file_paths: Vec<String>,
    last_diff: Option<StoredDiff>,
    last_patch: Option<StoredPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredResult {
    status: String,
    summary: String,
    success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredStep {
    description: String,
    reply: String,
    success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredDiff {
    description: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPatch {
    content: String,
    file_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SessionStoreFile {
    Current(HashMap<String, StoredSession>),
    Legacy(HashMap<String, PlanSession>),
}

impl Default for BridgeApp {
    fn default() -> Self {
        Self {
            session_store_path: default_session_store_path(),
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
            Intent::ContinuePlan => self.resume_plan(session_key, false),
            Intent::RetryFailedStep => self.resume_plan(session_key, false),
            Intent::ExecuteAll => self.resume_plan(session_key, true),
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
                |intent| self.approval_policy.requires_approval(intent),
            ),
            ExecutionMode::ContinueAll => session.execute_remaining_with_policy(
                self.executor,
                |intent| self.approval_policy.requires_approval(intent),
            ),
        };
        let stored = self.build_stored_session(
            if progress.completed { None } else { Some(session.clone()) },
            task_text,
            action_label_for_mode(&mode),
            &progress,
        );
        let reply = format_plan_reply(
            &progress,
            matches!(mode, ExecutionMode::ContinueAll),
            &self.approval_policy,
            &stored,
        );
        let _ = self.persist_session(session_key, &stored);

        reply
    }

    fn resume_plan(&self, session_key: &str, run_all: bool) -> BridgeResponse {
        let Some(mut stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待继续的计划。\n\n发送「执行计划 <命令1>; <命令2>」创建逐步计划，或发送「执行全部 <命令1>; <命令2>」连续执行。".to_string());
        };

        let Some(mut session) = stored.plan.take() else {
            return BridgeResponse::Text(format_stored_session_summary(&stored));
        };

        let progress = if run_all {
            session.execute_remaining_with_policy(self.executor, |intent| {
                self.approval_policy.requires_approval(intent)
            })
        } else {
            session.execute_next_with_policy(self.executor, |intent| {
                self.approval_policy.requires_approval(intent)
            })
        };
        stored = self.build_stored_session(
            if progress.completed { None } else { Some(session.clone()) },
            stored.current_task.as_deref().unwrap_or("继续当前计划"),
            if run_all { "执行全部" } else { "继续" },
            &progress,
        );
        let reply = format_plan_reply(&progress, run_all, &self.approval_policy, &stored);
        let _ = self.persist_session(session_key, &stored);

        reply
    }

    fn approve_plan(&self, session_key: &str) -> BridgeResponse {
        let Some(mut stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
        };

        let Some(mut session) = stored.plan.take() else {
            return BridgeResponse::Text(format_stored_session_summary(&stored));
        };

        if !session.has_pending_approval() {
            return BridgeResponse::Text("⚠️ 当前没有待审批步骤。可以发送「继续」或「执行全部」推进计划。".to_string());
        }

        let progress = session.approve_pending_with_policy(self.executor, |intent| {
            self.approval_policy.requires_approval(intent)
        });
        stored = self.build_stored_session(
            if progress.completed { None } else { Some(session.clone()) },
            stored.current_task.as_deref().unwrap_or("批准当前计划"),
            "批准",
            &progress,
        );
        let reply = format_plan_reply(&progress, false, &self.approval_policy, &stored);
        let _ = self.persist_session(session_key, &stored);

        reply
    }

    fn reject_plan(&self, session_key: &str) -> BridgeResponse {
        let Some(mut stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
        };

        let Some(mut session) = stored.plan.take() else {
            return BridgeResponse::Text(format_stored_session_summary(&stored));
        };

        if !session.reject_pending() {
            return BridgeResponse::Text("⚠️ 当前没有待审批步骤。".to_string());
        }

        stored.plan = None;
        stored.pending_steps = Vec::new();
        stored.last_action = Some("拒绝".to_string());
        stored.last_result = Some(StoredResult {
            status: "已取消".to_string(),
            summary: "当前待审批任务已被拒绝并取消。".to_string(),
            success: false,
        });
        stored.last_step = None;
        let _ = self.persist_session(session_key, &stored);
        BridgeResponse::Text("🛑 已拒绝当前待审批步骤，当前计划已取消。".to_string())
    }

    fn explain_last_failure(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(format_last_failure_reply(&stored))
    }

    fn show_last_result(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(format_last_result_reply(&stored))
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
        BridgeResponse::Text(format_follow_up_reply("继续文件上下文", &stored, blocks))
    }

    fn show_last_diff(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(format_last_diff_reply(&stored))
    }

    fn show_recent_files(&self, session_key: &str) -> BridgeResponse {
        let Some(stored) = self.load_persisted_session(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有可回看的任务记录。".to_string());
        };

        BridgeResponse::Text(format_recent_files_reply(&stored))
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
        stored.last_result = Some(StoredResult {
            status: if result.success { "已完成".to_string() } else { "失败暂停".to_string() },
            summary: if result.success {
                format!("最近一次补丁已撤回，共涉及 {} 个文件。", last_patch.file_paths.len())
            } else {
                "撤回最近一次补丁失败。".to_string()
            },
            success: result.success,
        });
        stored.last_step = Some(StoredStep {
            description: "撤回刚才的补丁".to_string(),
            reply: reply.clone(),
            success: result.success,
        });
        stored.last_file_path = last_patch.file_paths.first().cloned();
        stored.recent_file_paths = last_patch.file_paths.clone();
        stored.last_diff = Some(StoredDiff {
            description: "撤回刚才的补丁".to_string(),
            content: truncate_session_text(&last_patch.content, 4000),
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
        let outcome = (self.executor)(&intent);
        let reply = outcome.reply.clone();
        let progress = progress_from_direct_execution(intent, outcome);
        let stored = self.build_stored_session(None, task_text, "直接执行", &progress);
        let _ = self.persist_session(session_key, &stored);
        BridgeResponse::Text(reply)
    }

    fn load_session_store(&self) -> HashMap<String, StoredSession> {
        let Some(path) = self.session_store_path.as_ref() else {
            return HashMap::new();
        };

        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<SessionStoreFile>(&content) {
                Ok(SessionStoreFile::Current(store)) => store,
                Ok(SessionStoreFile::Legacy(store)) => store
                    .into_iter()
                    .map(|(key, session)| (key, stored_session_from_legacy(session)))
                    .collect(),
                Err(_) => HashMap::new(),
            },
            Err(_) => HashMap::new(),
        }
    }

    fn save_session_store(&self, store: &HashMap<String, StoredSession>) -> Result<(), String> {
        let Some(path) = self.session_store_path.as_ref() else {
            return Err("无法定位会话存储目录".to_string());
        };

        let content = serde_json::to_string_pretty(store)
            .map_err(|err| format!("序列化计划会话失败: {err}"))?;
        std::fs::write(path, content).map_err(|err| format!("写入计划会话失败: {err}"))
    }

    fn load_persisted_session(&self, session_key: &str) -> Option<StoredSession> {
        let store = self.load_session_store();
        store.get(session_key).cloned()
    }

    fn persist_session(&self, session_key: &str, session: &StoredSession) -> Result<(), String> {
        let mut store = self.load_session_store();
        store.insert(session_key.to_string(), session.clone());
        self.save_session_store(&store)
    }

    fn build_stored_session(
        &self,
        plan: Option<PlanSession>,
        task_text: &str,
        action: &str,
        progress: &PlanProgress,
    ) -> StoredSession {
        let recent_file_paths = collect_recent_file_paths(progress);

        StoredSession {
            pending_steps: plan
                .as_ref()
                .map(|session| {
                    session
                        .pending_steps()
                        .iter()
                        .map(describe_intent)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            current_task: Some(task_text.trim().to_string()).filter(|value| !value.is_empty()),
            last_action: Some(action.to_string()),
            last_result: Some(stored_result_from_progress(progress)),
            last_step: progress.executed.last().map(stored_step_from_execution),
            last_file_path: recent_file_paths.first().cloned(),
            recent_file_paths,
            last_diff: progress
                .executed
                .iter()
                .rev()
                .find_map(stored_diff_from_execution),
            last_patch: progress
                .executed
                .iter()
                .rev()
                .find_map(stored_patch_from_execution),
            plan,
        }
    }
}

fn progress_from_direct_execution(intent: Intent, outcome: ExecutionOutcome) -> PlanProgress {
    let success = outcome.success;

    PlanProgress {
        executed: vec![crate::plan::StepExecution {
            step_number: 1,
            intent,
            outcome,
        }],
        total_steps: 1,
        next_step: if success { 1 } else { 0 },
        completed: success,
        paused_on_failure: !success,
        paused_on_approval: false,
        approval_intent: None,
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
            .map(|step| format!("第 {} / {} 步失败：{}", step.step_number, progress.total_steps, describe_intent(&step.intent)))
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

fn stored_session_from_legacy(session: PlanSession) -> StoredSession {
    let recent_file_paths = session
        .current_step()
        .map(paths_for_intent)
        .unwrap_or_default();

    StoredSession {
        pending_steps: session
            .pending_steps()
            .iter()
            .map(describe_intent)
            .collect(),
        current_task: None,
        last_result: None,
        last_action: None,
        last_step: None,
        last_file_path: recent_file_paths.first().cloned(),
        recent_file_paths,
        last_diff: None,
        last_patch: None,
        plan: Some(session),
    }
}

fn stored_step_from_execution(step: &crate::plan::StepExecution) -> StoredStep {
    StoredStep {
        description: describe_intent(&step.intent),
        reply: step.outcome.reply.clone(),
        success: step.outcome.success,
    }
}

fn stored_diff_from_execution(step: &crate::plan::StepExecution) -> Option<StoredDiff> {
    match &step.intent {
        Intent::GitDiff { .. } | Intent::DiffFiles { .. } => Some(StoredDiff {
            description: describe_intent(&step.intent),
            content: step.outcome.reply.clone(),
        }),
        Intent::ApplyPatch { patch } if !patch.trim().is_empty() => Some(StoredDiff {
            description: describe_intent(&step.intent),
            content: truncate_session_text(patch.trim(), 4000),
        }),
        _ => None,
    }
}

fn stored_patch_from_execution(step: &crate::plan::StepExecution) -> Option<StoredPatch> {
    match &step.intent {
        Intent::ApplyPatch { patch } if !patch.trim().is_empty() => Some(StoredPatch {
            content: patch.trim().to_string(),
            file_paths: paths_for_intent(&step.intent),
        }),
        _ => None,
    }
}

fn paths_for_intent(intent: &Intent) -> Vec<String> {
    match intent {
        Intent::OpenFile { path, .. } => vec![path.clone()],
        Intent::ReadFile { path, .. } => vec![path.clone()],
        Intent::GitDiff { path } => path.clone().into_iter().collect(),
        Intent::DiffFiles { file1, file2 } => {
            let mut paths = vec![file1.clone()];
            if file2 != file1 {
                paths.push(file2.clone());
            }
            paths
        }
        Intent::ApplyPatch { patch } => vscode::extract_patch_paths(patch),
        _ => Vec::new(),
    }
}

fn collect_recent_file_paths(progress: &PlanProgress) -> Vec<String> {
    let mut paths = Vec::new();

    for step in progress.executed.iter().rev() {
        for path in paths_for_intent(&step.intent) {
            if !paths.iter().any(|existing| existing == &path) {
                paths.push(path);
            }
        }
    }

    paths
}

fn truncate_session_text(text: &str, max_chars: usize) -> String {
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        truncated.push_str("\n… (内容过长已截断)");
    }
    truncated
}

fn summarize_reply_snippet(reply: &str, max_lines: usize, max_chars: usize) -> Option<String> {
    let lines = reply
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(max_lines)
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return None;
    }

    let mut summary = lines.join(" / ");
    if summary.chars().count() > max_chars {
        summary = summary.chars().take(max_chars).collect::<String>();
        summary.push_str("...");
    }

    Some(summary)
}

fn failure_next_step_hint(stored: &StoredSession) -> String {
    if let Some(step) = stored.pending_steps.first() {
        return format!("建议先处理失败点，再继续后面的步骤，例如先回到「{}」。", step);
    }

    if let Some(path) = stored
        .recent_file_paths
        .first()
        .map(String::as_str)
        .or(stored.last_file_path.as_deref())
    {
        return format!("建议先检查相关文件 {}，确认后再决定重试还是继续追问。", path);
    }

    "建议先看原始结果里的退出码或报错正文，再决定是重试、改文件还是调整命令。".to_string()
}

fn result_next_step_hint(stored: &StoredSession) -> String {
    if let Some(path) = stored
        .recent_file_paths
        .first()
        .map(String::as_str)
        .or(stored.last_file_path.as_deref())
    {
        return format!("如果要继续这个上下文，可以直接继续处理 {}，或再追问最近 diff。", path);
    }

    if let Some(step) = stored.pending_steps.first() {
        return format!("如果要继续推进任务，下一步可以先执行「{}」。", step);
    }

    "如果这就是你要的结果，可以继续追问 diff、文件上下文，或直接发下一条开发指令。".to_string()
}

fn format_stored_session_summary(stored: &StoredSession) -> String {
    let task = stored
        .current_task
        .as_deref()
        .filter(|task| !task.is_empty())
        .unwrap_or("(未记录任务描述)");
    let last_action = stored
        .last_action
        .as_deref()
        .unwrap_or("(未记录)");

    match stored.last_result.as_ref() {
        Some(result) => {
            let mut lines = vec![format!("ℹ️ 上次任务状态: {}", result.status)];
            lines.push(format!("🎯 当前任务: {}", task));
            lines.push(format!("🧾 上次动作: {}", last_action));
            lines.push(format!("📌 最近结果: {}", result.summary));

            if !stored.recent_file_paths.is_empty() {
                lines.push(format!("📄 最近文件: {}", stored.recent_file_paths.join("、")));
            }

            if !stored.pending_steps.is_empty() {
                lines.push("📦 剩余步骤: ".to_string());
                lines.extend(
                    stored
                        .pending_steps
                        .iter()
                        .map(|step| format!("- {}", step)),
                );
            }

            lines.join("\n")
        }
        None => "⚠️ 当前没有待继续的计划。".to_string(),
    }
}

fn format_last_failure_reply(stored: &StoredSession) -> String {
    let Some(last_result) = stored.last_result.as_ref() else {
        return "⚠️ 当前没有可回看的失败记录。".to_string();
    };

    if last_result.success {
        return format_follow_up_reply(
            "失败原因回放",
            stored,
            vec![
                "✅ 上一次任务没有失败。".to_string(),
                format!("📌 最近结果: {}", last_result.summary),
            ],
        );
    }

    let mut blocks = vec![
        format!("❌ 上次失败状态: {}", last_result.status),
        format!("📌 失败摘要: {}", last_result.summary),
    ];

    if let Some(last_step) = stored.last_step.as_ref() {
        blocks.push(format!("📍 卡住的位置: {}", last_step.description));
        if let Some(snippet) = summarize_reply_snippet(&last_step.reply, 3, 220) {
            blocks.push(format!("🔎 关键报错: {}", snippet));
        }
        blocks.push(format!("🧾 失败步骤: {}", last_step.description));
        blocks.push(format!("➡️ 下一步建议: {}", failure_next_step_hint(stored)));
        blocks.push(format!("📤 原始结果:\n{}", last_step.reply));
    } else {
        blocks.push(format!("➡️ 下一步建议: {}", failure_next_step_hint(stored)));
    }

    format_follow_up_reply("失败原因回放", stored, blocks)
}

fn format_last_result_reply(stored: &StoredSession) -> String {
    let Some(last_step) = stored.last_step.as_ref() else {
        return format_stored_session_summary(stored);
    };

    let mut blocks = vec![format!(
        "🧾 上一步结果: {}",
        if last_step.success { "成功" } else { "失败" }
    )];
    blocks.push(format!(
        "📎 导语: 上一步已经{}，这里先给你摘要，再附上原始结果。",
        if last_step.success { "完成" } else { "返回失败结果" }
    ));
    blocks.push(format!("📌 上一步: {}", last_step.description));
    if let Some(snippet) = summarize_reply_snippet(&last_step.reply, 3, 220) {
        blocks.push(format!("🔎 结果摘要: {}", snippet));
    }
    blocks.push(format!("➡️ 下一步建议: {}", result_next_step_hint(stored)));
    blocks.push(format!("📤 原始结果:\n{}", last_step.reply));

    if let Some(path) = stored.last_file_path.as_deref() {
        blocks.push(format!("📄 相关文件: {}", path));
    }
    if stored.recent_file_paths.len() > 1 {
        blocks.push(format!("🗂 最近文件列表: {}", stored.recent_file_paths.join("、")));
    }

    format_follow_up_reply("上一步结果回放", stored, blocks)
}

fn format_last_diff_reply(stored: &StoredSession) -> String {
    let Some(last_diff) = stored.last_diff.as_ref() else {
        return "⚠️ 最近一次任务里没有记录到 diff 或补丁内容。可以先发送「查看 diff」或「应用补丁 ...」。".to_string();
    };

    let mut blocks = vec![format!("🧩 最近一次 diff: {}", last_diff.description)];
    if !stored.recent_file_paths.is_empty() {
        blocks.push(format!("📄 相关文件: {}", stored.recent_file_paths.join("、")));
    }
    blocks.push(format!("📤 diff 内容:\n{}", last_diff.content));

    format_follow_up_reply("最近 diff 回放", stored, blocks)
}

fn format_recent_files_reply(stored: &StoredSession) -> String {
    if stored.recent_file_paths.is_empty() {
        return "⚠️ 最近一次任务里没有记录到文件列表。可以先发送「读取 <文件>」、「查看 diff」或「应用补丁 ...」。".to_string();
    }

    let mut blocks = vec![format!("📚 最近改动文件列表（{}）", stored.recent_file_paths.len())];
    blocks.extend(
        stored
            .recent_file_paths
            .iter()
            .enumerate()
            .map(|(index, path)| format!("{}. {}", index + 1, path)),
    );

    format_follow_up_reply("最近文件回放", stored, blocks)
}

fn format_follow_up_reply(title: &str, stored: &StoredSession, detail_blocks: Vec<String>) -> String {
    let mut blocks = vec![format!("🧭 {}", title)];
    blocks.push(format!(
        "🎯 当前任务: {}",
        stored
            .current_task
            .as_deref()
            .filter(|task| !task.is_empty())
            .unwrap_or("(未记录任务描述)")
    ));

    if let Some(last_result) = stored.last_result.as_ref() {
        blocks.push(format!("📌 最近状态: {}", last_result.status));
    }

    if let Some(last_action) = stored.last_action.as_deref().filter(|action| !action.is_empty()) {
        blocks.push(format!("🧾 上次动作: {}", last_action));
    }

    blocks.extend(
        detail_blocks
            .into_iter()
            .filter(|block| !block.trim().is_empty()),
    );

    blocks.join("\n\n")
}

fn default_session_store_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|dir| dir.join(".feishu-vscode-bridge-session.json"))
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

pub fn feishu_session_key(receive_id_type: &str, receive_id: &str) -> String {
    format!("feishu:{receive_id_type}:{receive_id}")
}

fn format_plan_reply(
    progress: &PlanProgress,
    auto_run: bool,
    approval_policy: &ApprovalPolicy,
    stored: &StoredSession,
) -> BridgeResponse {
    if progress.executed.is_empty() && progress.completed {
        return BridgeResponse::Card {
            fallback_text: "✅ 当前计划已经执行完成，没有剩余步骤。".to_string(),
            card: build_plan_card(
                progress,
                auto_run,
                approval_policy,
                "✅ 当前计划已经执行完成，没有剩余步骤。",
                stored,
            ),
        };
    }

    let mut lines = vec![format!("🧭 状态: {}", plan_status_label(progress))];
    lines.push(format!("✅ 已完成: {} / {} 步", progress.next_step, progress.total_steps));

    if progress.completed {
        lines.push("⏭ 当前步骤: 无，计划已完成。".to_string());
    } else {
        lines.push(format!(
            "⏭ 当前步骤: 第 {} / {} 步",
            progress.next_step + 1,
            progress.total_steps
        ));
        lines.push(format!(
            "📦 剩余步骤: {} 步",
            progress.total_steps.saturating_sub(progress.next_step)
        ));
    }

    if let Some(failed_step) = plan_failed_step(progress) {
        lines.push(format!(
            "❌ 失败步骤: 第 {} / {} 步 - {}",
            failed_step.step_number,
            progress.total_steps,
            describe_intent(&failed_step.intent)
        ));
    }

    if let Some(approval_intent) = progress.approval_intent.as_ref() {
        lines.push(format!(
            "🔐 待审批步骤: 第 {} / {} 步 - {}",
            progress.next_step + 1,
            progress.total_steps,
            describe_intent(approval_intent)
        ));
    }

    lines.push("📝 本次执行: ".to_string());

    for step in &progress.executed {
        lines.push(format!(
            "{} 第 {}/{} 步: {}",
            if step.outcome.success { "✅" } else { "❌" },
            step.step_number,
            progress.total_steps,
            describe_intent(&step.intent)
        ));
        lines.push(step.outcome.reply.clone());
    }

    if progress.completed {
        lines.push(format!("✅ 计划执行完成，共 {} 步。", progress.total_steps));
        let text = lines.join("\n\n");
        return BridgeResponse::Card {
            fallback_text: text.clone(),
            card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
        };
    }

    let next_step = progress.next_step + 1;
    let remaining = progress.total_steps.saturating_sub(progress.next_step);

    if progress.paused_on_approval {
        lines.push(format!(
            "🔐 第 {} 步需要批准后才能继续。发送「批准」执行该步骤，或发送「拒绝」取消当前计划。",
            next_step
        ));
        let text = lines.join("\n\n");
        return BridgeResponse::Card {
            fallback_text: text.clone(),
            card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
        };
    }

    if progress.paused_on_failure {
        lines.push(format!(
            "⏸ 已在第 {} 步失败后暂停。发送「重新执行失败步骤」重试该步骤，或发送「执行全部」连续执行剩余 {} 步。",
            next_step, remaining
        ));
        let text = lines.join("\n\n");
        return BridgeResponse::Card {
            fallback_text: text.clone(),
            card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
        };
    }

    if auto_run {
        lines.push(format!(
            "⏭ 已暂停，下一步是第 {} 步。发送「继续」执行下一步，或发送「执行全部」连续执行剩余 {} 步。",
            next_step, remaining
        ));
    } else {
        lines.push(format!(
            "⏭ 下一步是第 {} 步。发送「继续」执行下一步，或发送「执行全部」连续执行剩余 {} 步。",
            next_step, remaining
        ));
    }

    let text = lines.join("\n\n");
    BridgeResponse::Card {
        fallback_text: text.clone(),
        card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
    }
}

fn plan_status_label(progress: &PlanProgress) -> &'static str {
    if progress.completed {
        "已完成"
    } else if progress.paused_on_approval {
        "待审批"
    } else if progress.paused_on_failure {
        "失败暂停"
    } else {
        "待继续"
    }
}

fn plan_failed_step(progress: &PlanProgress) -> Option<&crate::plan::StepExecution> {
    progress.executed.iter().rev().find(|step| !step.outcome.success)
}

fn build_plan_card(
    progress: &PlanProgress,
    auto_run: bool,
    approval_policy: &ApprovalPolicy,
    summary: &str,
    stored: &StoredSession,
) -> serde_json::Value {
    let status_label = plan_status_label(progress);
    let (template, title, subtitle) = if progress.completed {
        ("green", "已完成", "这轮任务已经跑完。")
    } else if progress.paused_on_approval {
        ("orange", "等你确认", "当前步骤确认后才会继续。")
    } else if progress.paused_on_failure {
        ("orange", "已暂停", "这一步失败了，可以重试或继续往后跑。")
    } else if auto_run {
        ("blue", "待继续", "连续执行已暂停，还有后续步骤。")
    } else {
        ("blue", "待继续", "已经停在安全点，可以继续下一步。")
    };

    let current_step = if progress.completed {
        "无".to_string()
    } else {
        format!("第 {} / {} 步", progress.next_step + 1, progress.total_steps)
    };
    let remaining_steps = progress.total_steps.saturating_sub(progress.next_step);
    let current_task = stored
        .current_task
        .as_deref()
        .filter(|task| !task.is_empty())
        .unwrap_or("(未记录任务描述)");
    let last_result = stored
        .last_result
        .as_ref()
        .map(|result| format!("{}: {}", result.status, result.summary))
        .unwrap_or_else(|| "(暂无结果摘要)".to_string());

    let mut elements = vec![
        json!({
            "tag": "div",
            "fields": [
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**状态**\n{}", status_label)
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**已完成**\n{} / {} 步", progress.next_step, progress.total_steps)
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**当前步骤**\n{}", current_step)
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**剩余步骤**\n{} 步", remaining_steps)
                    }
                }
            ]
        }),
        json!({
            "tag": "div",
            "fields": [
                {
                    "is_short": false,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**当前任务**\n{}", current_task)
                    }
                },
                {
                    "is_short": false,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**最近结果**\n{}", last_result)
                    }
                }
            ]
        }),
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**摘要**\n{}", summary)
            }
        }),
    ];

    if let Some(failed_step) = plan_failed_step(progress) {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "**失败步骤**\n第 {} / {} 步：{}",
                    failed_step.step_number,
                    progress.total_steps,
                    describe_intent(&failed_step.intent)
                )
            }
        }));
    }

    if let Some(approval_intent) = progress.approval_intent.as_ref() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "**待审批步骤**\n第 {} / {} 步：{}",
                    progress.next_step + 1,
                    progress.total_steps,
                    describe_intent(approval_intent)
                )
            }
        }));
    }

    if !progress.executed.is_empty() {
        let execution_lines = progress
            .executed
            .iter()
            .map(|step| {
                format!(
                    "- {} 第 {}/{} 步：{}",
                    if step.outcome.success { "成功" } else { "失败" },
                    step.step_number,
                    progress.total_steps,
                    describe_intent(&step.intent)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**本次执行**\n{}", execution_lines)
            }
        }));
    }

    elements.push(json!({
        "tag": "note",
        "elements": [
            {
                "tag": "plain_text",
                "content": subtitle
            }
        ]
    }));

    let approval_summary = approval_policy.summary();
    let approval_summary = if approval_summary.is_empty() {
        "当前无需审批的命令类型。".to_string()
    } else {
        format!("当前审批策略：{}", approval_summary.join(", "))
    };

    elements.push(json!({
        "tag": "note",
        "elements": [
            {
                "tag": "plain_text",
                "content": approval_summary
            }
        ]
    }));

    if !stored.recent_file_paths.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**最近文件**\n{}", stored.recent_file_paths.join("、"))
            }
        }));
    }

    let main_actions = if !progress.completed {
        if progress.paused_on_approval {
            vec![
                json!({
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "确认继续"
                    },
                    "value": {
                        "command": "批准"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "取消这步"
                    },
                    "value": {
                        "command": "拒绝"
                    }
                }),
            ]
        } else if progress.paused_on_failure {
            vec![
                json!({
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "重试这步"
                    },
                    "value": {
                        "command": "重新执行失败步骤"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "继续全部"
                    },
                    "value": {
                        "command": "执行全部"
                    }
                }),
            ]
        } else {
            vec![
                json!({
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "继续下一步"
                    },
                    "value": {
                        "command": "继续"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "继续全部"
                    },
                    "value": {
                        "command": "执行全部"
                    }
                }),
            ]
        }
    } else {
        Vec::new()
    };

    if !main_actions.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**下一步**"
            }
        }));
        elements.push(json!({
            "tag": "action",
            "actions": main_actions
        }));
    }

    let follow_up_actions = build_follow_up_actions(stored);
    if !follow_up_actions.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**继续问**"
            }
        }));
        elements.push(json!({
            "tag": "action",
            "actions": follow_up_actions
        }));
    }

    json!({
        "config": {
            "wide_screen_mode": true
        },
        "header": {
            "template": template,
            "title": {
                "tag": "plain_text",
                "content": title
            }
        },
        "elements": elements
    })
}

fn describe_intent(intent: &Intent) -> String {
    match intent {
        Intent::OpenFile { path, line } => match line {
            Some(line) => format!("打开文件 {path}:{line}"),
            None => format!("打开文件 {path}"),
        },
        Intent::OpenFolder { path } => format!("打开目录 {path}"),
        Intent::InstallExtension { ext_id } => format!("安装扩展 {ext_id}"),
        Intent::UninstallExtension { ext_id } => format!("卸载扩展 {ext_id}"),
        Intent::ListExtensions => "列出扩展".to_string(),
        Intent::DiffFiles { file1, file2 } => format!("对比 {file1} 和 {file2}"),
        Intent::ReadFile {
            path,
            start_line,
            end_line,
        } => match (start_line, end_line) {
            (Some(start_line), Some(end_line)) => format!("读取文件 {path}:{start_line}-{end_line}"),
            _ => format!("读取文件 {path}"),
        },
        Intent::ListDirectory { path } => match path {
            Some(path) => format!("列出目录 {path}"),
            None => "列出当前目录".to_string(),
        },
        Intent::SearchText {
            query,
            path,
            is_regex,
        } => match path {
            Some(path) => format!("{}搜索 {query} 于 {path}", if *is_regex { "正则" } else { "文本" }),
            None => format!("{}搜索 {query}", if *is_regex { "正则" } else { "文本" }),
        },
        Intent::RunTests { command } => match command {
            Some(command) => format!("运行测试命令 {command}"),
            None => "运行默认测试命令".to_string(),
        },
        Intent::GitDiff { path } => match path {
            Some(path) => format!("查看工作区 diff {path}"),
            None => "查看当前工作区 diff".to_string(),
        },
        Intent::ApplyPatch { .. } => "应用补丁到当前工作区".to_string(),
        Intent::GitStatus { repo } => match repo {
            Some(repo) => format!("查看仓库状态 {repo}"),
            None => "查看当前仓库状态".to_string(),
        },
        Intent::GitPull { repo } => match repo {
            Some(repo) => format!("拉取仓库 {repo}"),
            None => "拉取当前仓库".to_string(),
        },
        Intent::GitPushAll { repo, message } => match repo {
            Some(repo) => format!("提交并推送 {repo}: {message}"),
            None => format!("提交并推送当前仓库: {message}"),
        },
        Intent::RunShell { cmd } => format!("执行命令 {cmd}"),
        Intent::RunPlan { .. } => "执行计划".to_string(),
        Intent::ContinuePlan => "继续计划".to_string(),
        Intent::RetryFailedStep => "重新执行失败步骤".to_string(),
        Intent::ExecuteAll => "执行全部".to_string(),
        Intent::ApprovePending => "批准待审批步骤".to_string(),
        Intent::RejectPending => "拒绝待审批步骤".to_string(),
        Intent::ExplainLastFailure => "解释最近一次失败原因".to_string(),
        Intent::ShowLastResult => "查看上一步结果".to_string(),
        Intent::ContinueLastFile => "继续处理刚才那个文件".to_string(),
        Intent::ShowLastDiff => "查看最近一次 diff".to_string(),
        Intent::ShowRecentFiles => "查看最近改动文件列表".to_string(),
        Intent::UndoLastPatch => "撤回刚才的补丁".to_string(),
        Intent::Help => "查看帮助".to_string(),
        Intent::Unknown(raw) => format!("未识别命令 {raw}"),
    }
}

fn build_follow_up_actions(stored: &StoredSession) -> Vec<serde_json::Value> {
    let mut actions = Vec::new();

    if stored.last_result.as_ref().is_some_and(|result| !result.success) {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "为什么失败了"
            },
            "value": {
                "command": "刚才为什么失败"
            }
        }));
    }

    if stored.last_step.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "看上一步"
            },
            "value": {
                "command": "把上一步结果发我"
            }
        }));
    }

    if !stored.recent_file_paths.is_empty() || stored.last_file_path.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "继续这个文件"
            },
            "value": {
                "command": "继续改刚才那个文件"
            }
        }));
    }

    if stored.last_diff.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "看 diff"
            },
            "value": {
                "command": "把刚才的 diff 发我"
            }
        }));
    }

    if !stored.recent_file_paths.is_empty() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "看文件列表"
            },
            "value": {
                "command": "把刚才改动的文件列表发我"
            }
        }));
    }

    if stored.last_patch.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "撤回补丁"
            },
            "value": {
                "command": "撤回刚才的补丁"
            }
        }));
    }

    actions
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
        Intent::RunShell { cmd } => {
            let result = vscode::run_shell(cmd);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("$ {cmd}")),
            }
        }
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
            approval_intent: None,
        };

        let stored = stored_task("执行计划 $ pwd", "已完成", "计划已完成，共执行 1 步。");

        match format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
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
            approval_intent: None,
        };

        let stored = stored_task("执行全部 $ false; $ pwd", "失败暂停", "第 2 / 3 步失败：执行命令 false");

        match format_plan_reply(&progress, true, &ApprovalPolicy::default(), &stored) {
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
            approval_intent: Some(shell_intent("pwd")),
        };

        let stored = stored_task("执行计划 git pull", "待审批", "第 1 / 1 步等待批准。");

        match format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
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
            approval_intent: None,
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
            approval_intent: None,
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
            approval_intent: None,
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

        match format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
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
                reply: format!("ok: {}", describe_intent(intent)),
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
}