use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::plan::{ExecutionOutcome, PlanProgress, PlanSession, StepExecution};
use crate::vscode;
use crate::Intent;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredSession {
    pub plan: Option<PlanSession>,
    pub current_task: Option<String>,
    pub pending_steps: Vec<String>,
    pub last_result: Option<StoredResult>,
    pub last_action: Option<String>,
    pub last_step: Option<StoredStep>,
    pub last_file_path: Option<String>,
    pub recent_file_paths: Vec<String>,
    pub last_diff: Option<StoredDiff>,
    pub last_patch: Option<StoredPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredResult {
    pub status: String,
    pub summary: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredStep {
    pub description: String,
    pub reply: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDiff {
    pub description: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPatch {
    pub content: String,
    pub file_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SessionStoreFile {
    Current(HashMap<String, StoredSession>),
    Legacy(HashMap<String, PlanSession>),
}

pub fn default_session_store_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|dir| dir.join(".feishu-vscode-bridge-session.json"))
}

pub fn load_session_store(path: Option<&PathBuf>) -> HashMap<String, StoredSession> {
    let Some(path) = path else {
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

pub fn save_session_store(
    path: Option<&PathBuf>,
    store: &HashMap<String, StoredSession>,
) -> Result<(), String> {
    let Some(path) = path else {
        return Err("无法定位会话存储目录".to_string());
    };

    let content = serde_json::to_string_pretty(store)
        .map_err(|err| format!("序列化计划会话失败: {err}"))?;
    std::fs::write(path, content).map_err(|err| format!("写入计划会话失败: {err}"))
}

pub fn load_persisted_session(path: Option<&PathBuf>, session_key: &str) -> Option<StoredSession> {
    let store = load_session_store(path);
    store.get(session_key).cloned()
}

pub fn persist_session(
    path: Option<&PathBuf>,
    session_key: &str,
    session: &StoredSession,
) -> Result<(), String> {
    let mut store = load_session_store(path);
    store.insert(session_key.to_string(), session.clone());
    save_session_store(path, &store)
}

pub fn build_stored_session(
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
        last_diff: progress.executed.iter().rev().find_map(stored_diff_from_execution),
        last_patch: progress.executed.iter().rev().find_map(stored_patch_from_execution),
        plan,
    }
}

pub fn progress_from_direct_execution(intent: Intent, outcome: ExecutionOutcome) -> PlanProgress {
    let success = outcome.success;

    PlanProgress {
        executed: vec![StepExecution {
            step_number: 1,
            intent,
            outcome,
        }],
        total_steps: 1,
        next_step: if success { 1 } else { 0 },
        completed: success,
        paused_on_failure: !success,
        paused_on_approval: false,
        approval_request: None,
    }
}

pub fn stored_session_from_agent_result(
    task_text: &str,
    intent: &Intent,
    result: &vscode::AgentAskResult,
    reply: &str,
) -> StoredSession {
    StoredSession {
        plan: None,
        current_task: Some(task_text.trim().to_string()).filter(|value| !value.is_empty()),
        pending_steps: Vec::new(),
        last_result: Some(StoredResult {
            status: format_agent_status(&result.status),
            summary: agent_result_summary(result),
            success: result.success,
        }),
        last_action: Some("直接执行".to_string()),
        last_step: Some(StoredStep {
            description: describe_intent(intent),
            reply: reply.to_string(),
            success: result.success,
        }),
        last_file_path: result.related_files.first().cloned(),
        recent_file_paths: result.related_files.clone(),
        last_diff: None,
        last_patch: None,
    }
}

fn stored_result_from_progress(progress: &PlanProgress) -> StoredResult {
    if progress.completed {
        StoredResult {
            status: "已完成".to_string(),
            summary: format!("计划已完成，共执行 {} 步。", progress.total_steps),
            success: true,
        }
    } else if let Some(request) = progress.approval_request.as_ref() {
        StoredResult {
            status: "待审批".to_string(),
            summary: format!(
                "第 {} / {} 步等待批准：{}。",
                request.step_number, progress.total_steps, request.action_label
            ),
            success: true,
        }
    } else if progress.paused_on_failure {
        let failed = progress
            .executed
            .iter()
            .rev()
            .find(|step| !step.outcome.success)
            .map(|step| {
                format!(
                    "第 {} / {} 步失败：{}",
                    step.step_number,
                    progress.total_steps,
                    describe_intent(&step.intent)
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

fn stored_session_from_legacy(session: PlanSession) -> StoredSession {
    let recent_file_paths = session.current_step().map(paths_for_intent).unwrap_or_default();

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

fn stored_step_from_execution(step: &StepExecution) -> StoredStep {
    StoredStep {
        description: describe_intent(&step.intent),
        reply: step.outcome.reply.clone(),
        success: step.outcome.success,
    }
}

fn stored_diff_from_execution(step: &StepExecution) -> Option<StoredDiff> {
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

fn stored_patch_from_execution(step: &StepExecution) -> Option<StoredPatch> {
    match &step.intent {
        Intent::ApplyPatch { patch } if !patch.trim().is_empty() => Some(StoredPatch {
            content: patch.trim().to_string(),
            file_paths: paths_for_intent(&step.intent),
        }),
        _ => None,
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

fn agent_result_summary(result: &vscode::AgentAskResult) -> String {
    result
        .summary
        .clone()
        .or_else(|| summarize_reply_snippet(&result.message, 3, 220))
        .unwrap_or_else(|| "agent 已返回结果。".to_string())
}

fn format_agent_status(status: &str) -> String {
    match status.trim().to_ascii_lowercase().as_str() {
        "answered" => "已回答".to_string(),
        "working" => "处理中".to_string(),
        "needs_tool" => "需要工具".to_string(),
        "waiting_user" => "等待用户".to_string(),
        "blocked" => "已阻塞".to_string(),
        "completed" => "已完成".to_string(),
        other if other.is_empty() => "已回答".to_string(),
        other => other.to_string(),
    }
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
        Intent::SearchText { query, path, is_regex } => match path {
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
        Intent::SearchSymbol { query, path } => match path {
            Some(path) => format!("搜索符号 {query} 于 {path}"),
            None => format!("搜索符号 {query}"),
        },
        Intent::FindReferences { query, path } => match path {
            Some(path) => format!("查找引用 {query} 于 {path}"),
            None => format!("查找引用 {query}"),
        },
        Intent::FindImplementations { query, path } => match path {
            Some(path) => format!("查找实现 {query} 于 {path}"),
            None => format!("查找实现 {query}"),
        },
        Intent::RunSpecificTest { filter } => format!("运行指定测试 {filter}"),
        Intent::RunTestFile { path } => format!("运行测试文件 {path}"),
        Intent::WriteFile { path, .. } => format!("写入文件 {path}"),
        Intent::AskAgent { prompt } => format!("问 Copilot {prompt}"),
        Intent::ResetAgentSession => "重置 Copilot 会话".to_string(),
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
        Intent::GitLog { count, path } => {
            let n = count.map_or("".to_string(), |n| format!(" {n}"));
            match path {
                Some(path) => format!("查看提交历史{n} {path}"),
                None => format!("查看提交历史{n}"),
            }
        }
        Intent::GitBlame { path } => format!("查看文件追溯 {path}"),
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