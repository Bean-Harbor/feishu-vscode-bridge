use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent_runtime::AgentRunState;
use crate::plan::{ExecutionOutcome, PlanProgress, PlanSession, StepExecution};
use crate::reply;
use crate::vscode;
use crate::Intent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StoredSessionKind {
    Agent,
    Plan,
    #[default]
    Direct,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredSession {
    #[serde(default)]
    pub session_kind: StoredSessionKind,
    #[serde(default)]
    pub agent_state: Option<StoredAgentState>,
    #[serde(default)]
    pub current_project_path: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredAgentState {
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub current_action: Option<String>,
    pub next_action: Option<String>,
    pub tool_call: Option<String>,
    pub tool_result_summary: Option<String>,
    #[serde(default)]
    pub run: Option<AgentRunState>,
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

pub fn stored_session_from_semantic_plan_result(
    task_text: &str,
    intent: &Intent,
    result: &vscode::SemanticPlanResult,
    reply_text: &str,
    current_project_path: Option<String>,
    pending_steps: Vec<String>,
) -> StoredSession {
    let status = match result.decision.trim().to_ascii_lowercase().as_str() {
        "confirm" => "待确认".to_string(),
        "clarify" => "待澄清".to_string(),
        "execute" => "已规划".to_string(),
        other if other.is_empty() => "已规划".to_string(),
        other => format!("已规划 ({other})"),
    };

    let summary = result
        .summary_for_user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            result
                .summary
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            let message = result.message.trim();
            (!message.is_empty()).then(|| message.to_string())
        })
        .unwrap_or_else(|| task_text.trim().to_string());

    StoredSession {
        session_kind: StoredSessionKind::Plan,
        agent_state: None,
        current_project_path,
        plan: None,
        current_task: Some(task_text.trim().to_string()).filter(|value| !value.is_empty()),
        pending_steps,
        last_result: Some(StoredResult {
            status,
            summary,
            success: result.success,
        }),
        last_action: Some(reply::describe_intent(intent)),
        last_step: Some(StoredStep {
            description: reply::describe_intent(intent),
            reply: reply_text.to_string(),
            success: result.success,
        }),
        last_file_path: None,
        recent_file_paths: Vec::new(),
        last_diff: None,
        last_patch: None,
    }
}
pub fn save_session_store(
    path: Option<&PathBuf>,
    store: &HashMap<String, StoredSession>,
) -> Result<(), String> {
    let Some(path) = path else {
        return Err("无法定位会话存储目录".to_string());
    };

    let content =
        serde_json::to_string_pretty(store).map_err(|err| format!("序列化计划会话失败: {err}"))?;
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
    let mut merged = session.clone();

    if merged
        .current_project_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        merged.current_project_path = store.get(session_key).and_then(selected_project_path);
    }

    store.insert(session_key.to_string(), merged);
    save_session_store(path, &store)
}

pub fn build_stored_session(
    session_kind: StoredSessionKind,
    plan: Option<PlanSession>,
    task_text: &str,
    action: &str,
    progress: &PlanProgress,
) -> StoredSession {
    let recent_file_paths = collect_recent_file_paths(progress);

    StoredSession {
        session_kind,
        agent_state: None,
        current_project_path: None,
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
    current_project_path: Option<String>,
) -> StoredSession {
    StoredSession {
        session_kind: StoredSessionKind::Agent,
        agent_state: Some(StoredAgentState {
            session_id: result.session_id.clone(),
            status: Some(format_agent_status(&result.status)),
            current_action: result.current_action.clone(),
            next_action: result.next_action.clone(),
            tool_call: result.tool_call.clone(),
            tool_result_summary: result.tool_result_summary.clone(),
            run: result.run.clone(),
        }),
        current_project_path,
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

pub fn stored_session_from_agent_run_result(
    task_text: &str,
    intent: &Intent,
    result: &vscode::AgentRunResult,
    reply_text: &str,
    current_project_path: Option<String>,
) -> StoredSession {
    let run = result.run.clone();
    let (status, current_action, next_action, recent_file_paths) = run
        .as_ref()
        .map(|run| {
            let recent_files = run
                .reversible_artifacts
                .iter()
                .flat_map(|artifact| artifact.file_paths.iter().cloned())
                .collect::<Vec<_>>();
            (
                reply::format_agent_run_status(run.status.as_str()),
                Some(run.current_action.clone()).filter(|value| !value.trim().is_empty()),
                Some(run.next_action.clone()).filter(|value| !value.trim().is_empty()),
                recent_files,
            )
        })
        .unwrap_or_else(|| {
            (
                if result.success {
                    "已初始化".to_string()
                } else {
                    "已阻塞".to_string()
                },
                None,
                None,
                Vec::new(),
            )
        });

    let summary = run
        .as_ref()
        .map(|value| value.summary.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| result.message.clone());

    StoredSession {
        session_kind: StoredSessionKind::Agent,
        agent_state: Some(StoredAgentState {
            session_id: Some(result.session_id.clone()).filter(|value| !value.trim().is_empty()),
            status: Some(status.clone()),
            current_action,
            next_action,
            tool_call: None,
            tool_result_summary: Some(summary.clone()).filter(|value| !value.trim().is_empty()),
            run,
        }),
        current_project_path,
        plan: None,
        current_task: Some(task_text.trim().to_string()).filter(|value| !value.is_empty()),
        pending_steps: Vec::new(),
        last_result: Some(StoredResult {
            status,
            summary,
            success: result.success,
        }),
        last_action: Some(reply::describe_intent(intent)),
        last_step: Some(StoredStep {
            description: reply::describe_intent(intent),
            reply: reply_text.to_string(),
            success: result.success,
        }),
        last_file_path: recent_file_paths.first().cloned(),
        recent_file_paths,
        last_diff: None,
        last_patch: None,
    }
}

pub fn is_agent_task_session(stored: &StoredSession) -> bool {
    if stored.session_kind == StoredSessionKind::Agent {
        return true;
    }

    stored
        .last_step
        .as_ref()
        .map(|step| is_agent_step_description(&step.description))
        .unwrap_or(false)
        || stored
            .current_task
            .as_deref()
            .map(is_agent_task_text)
            .unwrap_or(false)
}

pub fn suggested_agent_next_action(stored: &StoredSession) -> Option<String> {
    stored
        .agent_state
        .as_ref()
        .and_then(|state| {
            state
                .run
                .as_ref()
                .map(|run| run.next_action.as_str())
                .or_else(|| state.next_action.as_deref())
        })
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub fn current_agent_run(stored: &StoredSession) -> Option<&AgentRunState> {
    stored
        .agent_state
        .as_ref()
        .and_then(|state| state.run.as_ref())
}

pub fn current_agent_run_id(stored: &StoredSession) -> Option<String> {
    current_agent_run(stored).map(|run| run.run_id.clone())
}

pub fn current_agent_decision(
    stored: &StoredSession,
) -> Option<(&AgentRunState, &crate::agent_runtime::PendingUserDecision)> {
    let run = current_agent_run(stored)?;
    let decision = run.pending_user_decision.as_ref()?;
    Some((run, decision))
}

pub fn suggested_agent_decision_option(stored: &StoredSession) -> Option<String> {
    let (_, decision) = current_agent_decision(stored)?;
    decision
        .recommended_option_id
        .as_ref()
        .cloned()
        .or_else(|| {
            decision
                .options
                .iter()
                .find(|option| option.primary)
                .map(|option| option.option_id.clone())
        })
        .or_else(|| {
            decision
                .options
                .first()
                .map(|option| option.option_id.clone())
        })
}

pub fn selected_project_path(stored: &StoredSession) -> Option<String> {
    stored
        .current_project_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn is_agent_step_description(description: &str) -> bool {
    let trimmed = description.trim_start();
    trimmed.starts_with("问 Copilot")
        || trimmed.starts_with("问 Codex")
        || trimmed.starts_with("继续 Agent 任务")
        || trimmed.starts_with("启动 Agent Runtime")
        || trimmed.starts_with("继续 Agent Runtime")
}

fn is_agent_task_text(task: &str) -> bool {
    let trimmed = task.trim_start();
    trimmed.starts_with("问 Copilot")
        || trimmed.starts_with("问 Codex")
        || trimmed.starts_with("/copilot")
        || trimmed.starts_with("/codex")
        || trimmed.to_ascii_lowercase().starts_with("ask copilot")
        || trimmed.to_ascii_lowercase().starts_with("ask codex")
}

pub(crate) fn stored_result_from_progress(progress: &PlanProgress) -> StoredResult {
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
            summary: format!(
                "下一步是第 {} / {} 步。",
                progress.next_step + 1,
                progress.total_steps
            ),
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
        session_kind: StoredSessionKind::Plan,
        agent_state: None,
        current_project_path: None,
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
        Intent::ShowPlanPrompt { prompt } => format!("Plan 模式：{prompt}"),
        Intent::InstallExtension { ext_id } => format!("安装扩展 {ext_id}"),
        Intent::UninstallExtension { ext_id } => format!("卸载扩展 {ext_id}"),
        Intent::ListExtensions => "列出扩展".to_string(),
        Intent::DiffFiles { file1, file2 } => format!("对比 {file1} 和 {file2}"),
        Intent::ReadFile {
            path,
            start_line,
            end_line,
        } => match (start_line, end_line) {
            (Some(start_line), Some(end_line)) => {
                format!("读取文件 {path}:{start_line}-{end_line}")
            }
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
            Some(path) => format!(
                "{}搜索 {query} 于 {path}",
                if *is_regex { "正则" } else { "文本" }
            ),
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
        Intent::AskCodex { prompt } => format!("问 Codex {prompt}"),
        Intent::StartAgentRun { prompt } => format!("启动 Agent Runtime：{prompt}"),
        Intent::ContinueAgentRun { prompt } => match prompt {
            Some(prompt) if !prompt.trim().is_empty() => {
                format!("继续 Agent Runtime：{}", prompt.trim())
            }
            _ => "继续 Agent Runtime".to_string(),
        },
        Intent::ShowAgentRunStatus => "查看 Agent Runtime 状态".to_string(),
        Intent::ApproveAgentRun { option_id } => match option_id {
            Some(option_id) if !option_id.trim().is_empty() => {
                format!("批准 Agent Runtime 决策 {}", option_id.trim())
            }
            _ => "批准当前 Agent Runtime 决策".to_string(),
        },
        Intent::CancelAgentRun => "取消 Agent Runtime".to_string(),
        Intent::ContinueAgent { prompt } => match prompt {
            Some(prompt) if !prompt.trim().is_empty() => {
                format!("继续当前任务：{}", prompt.trim())
            }
            _ => "继续当前任务".to_string(),
        },
        Intent::ContinueAgentSuggested => "按建议继续当前任务".to_string(),
        Intent::ResetAgentSession => "重置 Copilot 会话".to_string(),
        Intent::ShowProjectPicker => "打开项目选择卡片".to_string(),
        Intent::ShowProjectBrowser { path } => match path {
            Some(path) => format!("浏览项目目录 {path}"),
            None => "浏览项目目录".to_string(),
        },
        Intent::ShowCurrentProject => "查看当前项目".to_string(),
        Intent::GitStatus { repo } => match repo {
            Some(repo) => format!("查看仓库状态 {repo}"),
            None => "查看当前仓库状态".to_string(),
        },
        Intent::GitSync { repo } => match repo {
            Some(repo) => format!("同步 Git 状态 {repo}"),
            None => "同步当前项目的 Git 状态".to_string(),
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::agent_runtime::{
        AgentRunMode, AgentRunState, AgentRunStatus, ResultDisposition, RunBudget, RunCheckpoint,
    };
    use crate::test_support::unique_temp_path;

    #[test]
    fn build_stored_session_remembers_file_from_apply_patch() {
        let progress = PlanProgress {
            executed: vec![StepExecution {
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

        let stored = build_stored_session(
            StoredSessionKind::Plan,
            None,
            "应用补丁",
            "执行计划",
            &progress,
        );

        assert_eq!(stored.last_file_path, Some("src/demo.rs".to_string()));
        assert_eq!(stored.recent_file_paths, vec!["src/demo.rs".to_string()]);
        assert!(stored.last_patch.is_some());
    }

    #[test]
    fn build_stored_session_remembers_all_files_from_apply_patch() {
        let progress = PlanProgress {
            executed: vec![StepExecution {
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

        let stored = build_stored_session(
            StoredSessionKind::Plan,
            None,
            "应用补丁",
            "执行计划",
            &progress,
        );

        assert_eq!(
            stored.recent_file_paths,
            vec!["src/bridge.rs".to_string(), "src/lib.rs".to_string()]
        );
        assert!(stored
            .last_diff
            .as_ref()
            .is_some_and(|diff| diff.content.contains("diff --git a/src/lib.rs")));
        assert!(stored
            .last_patch
            .as_ref()
            .is_some_and(|patch| patch.file_paths
                == vec!["src/bridge.rs".to_string(), "src/lib.rs".to_string()]));
    }

    #[test]
    fn detects_agent_task_session_from_last_step() {
        let stored = StoredSession {
            session_kind: StoredSessionKind::Agent,
            agent_state: Some(StoredAgentState {
                session_id: Some("session-1".to_string()),
                status: Some("已回答".to_string()),
                current_action: Some("继续分析".to_string()),
                next_action: Some("读取 src/lib.rs 350-420".to_string()),
                tool_call: None,
                tool_result_summary: None,
                run: None,
            }),
            current_project_path: None,
            plan: None,
            current_task: Some("问 Copilot parse_intent 这个函数是干什么的".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已回答".to_string(),
                summary: "已返回 grounded answer。".to_string(),
                success: true,
            }),
            last_action: Some("直接执行".to_string()),
            last_step: Some(StoredStep {
                description: "继续 Agent 任务：给我最小修复建议".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some("src/lib.rs".to_string()),
            recent_file_paths: vec!["src/lib.rs".to_string()],
            last_diff: None,
            last_patch: None,
        };

        assert!(is_agent_task_session(&stored));
        assert_eq!(
            suggested_agent_next_action(&stored).as_deref(),
            Some("读取 src/lib.rs 350-420")
        );
    }

    #[test]
    fn detects_agent_task_session_from_persisted_runtime_run() {
        let stored = StoredSession {
            session_kind: StoredSessionKind::Agent,
            agent_state: Some(StoredAgentState {
                session_id: Some("session-ask-runtime".to_string()),
                status: Some("等待用户".to_string()),
                current_action: Some("Waiting for follow-up".to_string()),
                next_action: Some("继续检查 parser 边界情况".to_string()),
                tool_call: None,
                tool_result_summary: Some("已完成 ask-mode runtime 第一轮分析。".to_string()),
                run: Some(AgentRunState {
                    run_id: "run-ask-1".to_string(),
                    mode: AgentRunMode::Ask,
                    status: AgentRunStatus::WaitingUser,
                    summary: "Ask runtime paused with a follow-up suggestion.".to_string(),
                    current_action: "Waiting for follow-up".to_string(),
                    next_action: "继续检查 parser 边界情况".to_string(),
                    current_step: Some("waiting_user".to_string()),
                    waiting_reason: None,
                    authorization_policy: None,
                    result_disposition: ResultDisposition::Pending,
                    pending_user_decision: None,
                    budget: RunBudget {
                        max_iterations: 3,
                        max_tool_calls: 2,
                        max_write_operations: 0,
                    },
                    checkpoints: vec![RunCheckpoint {
                        checkpoint_id: "cp-1".to_string(),
                        label: "waiting-user".to_string(),
                        status_summary: "Ask runtime paused".to_string(),
                        timestamp_ms: 1,
                    }],
                    reversible_artifacts: Vec::new(),
                }),
            }),
            current_project_path: None,
            plan: None,
            current_task: Some("问 Copilot parser 的自然语言入口还缺什么".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "等待用户".to_string(),
                summary: "Ask runtime paused with a follow-up suggestion.".to_string(),
                success: true,
            }),
            last_action: Some("直接执行".to_string()),
            last_step: Some(StoredStep {
                description: "问 Copilot parser 的自然语言入口还缺什么".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some("src/lib.rs".to_string()),
            recent_file_paths: vec!["src/lib.rs".to_string()],
            last_diff: None,
            last_patch: None,
        };

        assert!(is_agent_task_session(&stored));
        assert!(current_agent_run(&stored).is_some());
        assert_eq!(current_agent_run_id(&stored).as_deref(), Some("run-ask-1"));
        assert_eq!(
            suggested_agent_next_action(&stored).as_deref(),
            Some("继续检查 parser 边界情况")
        );
    }

    #[test]
    fn detects_codex_task_session_from_current_task() {
        let stored = StoredSession {
            session_kind: StoredSessionKind::Direct,
            agent_state: Some(StoredAgentState {
                session_id: Some("codex-session".to_string()),
                status: Some("已回答".to_string()),
                current_action: Some("Executed Codex CLI prompt".to_string()),
                next_action: Some("继续检查 src/bridge.rs".to_string()),
                tool_call: None,
                tool_result_summary: None,
                run: None,
            }),
            current_project_path: Some("C:/work/demo".to_string()),
            plan: None,
            current_task: Some("/codex 修复当前桥接".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已回答".to_string(),
                summary: "已定位 bridge 入口".to_string(),
                success: true,
            }),
            last_action: Some("直接执行".to_string()),
            last_step: Some(StoredStep {
                description: "问 Codex 修复当前桥接".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some("src/bridge.rs".to_string()),
            recent_file_paths: vec!["src/bridge.rs".to_string()],
            last_diff: None,
            last_patch: None,
        };

        assert!(is_agent_task_session(&stored));
        assert_eq!(
            suggested_agent_next_action(&stored).as_deref(),
            Some("继续检查 src/bridge.rs")
        );
    }

    #[test]
    fn stored_session_from_agent_result_preserves_runtime_run() {
        let result = crate::vscode::AgentAskResult {
            success: true,
            session_id: Some("session-1".to_string()),
            status: "waiting_user".to_string(),
            message: "Ask runtime paused.".to_string(),
            summary: Some("Ask runtime paused.".to_string()),
            current_action: Some("Waiting for follow-up".to_string()),
            next_action: Some("继续查看 src/lib.rs 120-180".to_string()),
            related_files: vec!["src/lib.rs".to_string()],
            tool_call: None,
            tool_result_summary: None,
            run: Some(AgentRunState {
                run_id: "run-ask-2".to_string(),
                mode: AgentRunMode::Ask,
                status: AgentRunStatus::WaitingUser,
                summary: "Ask runtime paused.".to_string(),
                current_action: "Waiting for follow-up".to_string(),
                next_action: "继续查看 src/lib.rs 120-180".to_string(),
                current_step: Some("waiting_user".to_string()),
                waiting_reason: None,
                authorization_policy: None,
                result_disposition: ResultDisposition::Pending,
                pending_user_decision: None,
                budget: RunBudget {
                    max_iterations: 3,
                    max_tool_calls: 2,
                    max_write_operations: 0,
                },
                checkpoints: Vec::new(),
                reversible_artifacts: Vec::new(),
            }),
            duration_ms: 42,
            error: None,
        };

        let stored = stored_session_from_agent_result(
            "问 Copilot parser 的自然语言入口还缺什么",
            &Intent::AskAgent {
                prompt: "parser 的自然语言入口还缺什么".to_string(),
            },
            &result,
            "reply",
            None,
        );

        assert_eq!(stored.session_kind, StoredSessionKind::Agent);
        assert_eq!(current_agent_run_id(&stored).as_deref(), Some("run-ask-2"));
        assert_eq!(
            suggested_agent_next_action(&stored).as_deref(),
            Some("继续查看 src/lib.rs 120-180")
        );
    }

    #[test]
    fn stored_session_from_semantic_plan_result_keeps_planner_context() {
        let result = vscode::SemanticPlanResult {
            success: true,
            decision: "confirm".to_string(),
            message: "当前请求可能影响多个失败点，建议先确认范围。".to_string(),
            summary: Some("建议先确认修复范围。".to_string()),
            summary_for_user: Some("先确认修复范围，再开始执行。".to_string()),
            confidence: Some(0.64),
            risk: Some("medium".to_string()),
            actions: Vec::new(),
            options: vec![vscode::SemanticPlanOption {
                label: "只修 parser".to_string(),
                command: "先只修 parser 相关测试".to_string(),
                note: None,
                primary: true,
            }],
            error: None,
        };

        let stored = stored_session_from_semantic_plan_result(
            "/plan 修复当前测试失败",
            &Intent::ShowPlanPrompt {
                prompt: "修复当前测试失败".to_string(),
            },
            &result,
            "🧭 Plan 模式\n\n任务: 修复当前测试失败",
            Some("C:\\work\\demo".to_string()),
            vec!["只修 parser -> 先只修 parser 相关测试".to_string()],
        );

        assert_eq!(stored.session_kind, StoredSessionKind::Plan);
        assert_eq!(
            stored.current_project_path.as_deref(),
            Some("C:\\work\\demo")
        );
        assert_eq!(
            stored.current_task.as_deref(),
            Some("/plan 修复当前测试失败")
        );
        assert_eq!(
            stored
                .last_result
                .as_ref()
                .map(|value| value.status.as_str()),
            Some("待确认")
        );
        assert_eq!(
            stored
                .last_result
                .as_ref()
                .map(|value| value.summary.as_str()),
            Some("先确认修复范围，再开始执行。")
        );
        assert_eq!(
            stored.pending_steps,
            vec!["只修 parser -> 先只修 parser 相关测试".to_string()]
        );
        assert_eq!(
            stored
                .last_step
                .as_ref()
                .map(|step| step.description.as_str()),
            Some("Plan 模式：修复当前测试失败")
        );
    }

    #[test]
    fn persist_session_preserves_current_project_path_when_new_write_omits_it() {
        let session_path = unique_temp_path("session-store", "current-project-merge");

        let selected_project = StoredSession {
            session_kind: StoredSessionKind::Direct,
            agent_state: None,
            current_project_path: Some("C:/work/demo".to_string()),
            plan: None,
            current_task: Some("选择项目 C:/work/demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "已选择项目。".to_string(),
                success: true,
            }),
            last_action: Some("直接执行".to_string()),
            last_step: Some(StoredStep {
                description: "选择项目 C:/work/demo".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: None,
            recent_file_paths: Vec::new(),
            last_diff: None,
            last_patch: None,
        };
        persist_session(Some(&session_path), "cli", &selected_project).unwrap();

        let later_agent_write = StoredSession {
            session_kind: StoredSessionKind::Agent,
            agent_state: Some(StoredAgentState {
                session_id: Some("session-1".to_string()),
                status: Some("已回答".to_string()),
                current_action: Some("继续分析".to_string()),
                next_action: Some("给我最小修复建议".to_string()),
                tool_call: None,
                tool_result_summary: None,
                run: None,
            }),
            current_project_path: None,
            plan: None,
            current_task: Some("问 Copilot 继续当前项目".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已回答".to_string(),
                summary: "已返回建议。".to_string(),
                success: true,
            }),
            last_action: Some("直接执行".to_string()),
            last_step: Some(StoredStep {
                description: "问 Copilot 继续当前项目".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: None,
            recent_file_paths: Vec::new(),
            last_diff: None,
            last_patch: None,
        };
        persist_session(Some(&session_path), "cli", &later_agent_write).unwrap();

        let merged = load_persisted_session(Some(&session_path), "cli").unwrap();
        assert_eq!(
            selected_project_path(&merged).as_deref(),
            Some("C:/work/demo")
        );
        assert_eq!(merged.session_kind, StoredSessionKind::Agent);

        let _ = fs::remove_file(session_path);
    }
}
