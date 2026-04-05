//! VS Code CLI 操作：打开文件、安装/列出扩展、运行 shell 等

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::path::Component;
use std::process::Command;
use std::time::Instant;

use crate::executor::{run_cmd, CmdResult};

pub const WORKSPACE_PATH_ENV: &str = "BRIDGE_WORKSPACE_PATH";
pub const TEST_COMMAND_ENV: &str = "BRIDGE_TEST_COMMAND";
pub const AGENT_BRIDGE_URL_ENV: &str = "BRIDGE_AGENT_BRIDGE_URL";
pub const AGENT_BRIDGE_PORT_ENV: &str = "BRIDGE_AGENT_BRIDGE_PORT";
const DEFAULT_AGENT_BRIDGE_PORT: u16 = 8765;
const AGENT_TOOL_RESULT_PATH: &str = "/v1/chat/tool-result";
const MAX_AGENT_TOOL_SUMMARY_CHARS: usize = 240;

#[derive(Deserialize)]
struct AgentTaskStateResponse {
    status: Option<String>,
    #[serde(rename = "currentAction")]
    current_action: Option<String>,
    #[serde(rename = "resultSummary")]
    result_summary: Option<String>,
    #[serde(rename = "nextAction")]
    next_action: Option<String>,
    #[serde(rename = "relatedFiles", default)]
    related_files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentToolCall {
    pub name: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Deserialize)]
struct AgentAskResponse {
    #[serde(rename = "sessionId")]
    session_id: String,
    reply: Option<String>,
    message: Option<String>,
    summary: Option<String>,
    status: Option<String>,
    #[serde(rename = "currentAction")]
    current_action: Option<String>,
    #[serde(rename = "nextAction")]
    next_action: Option<String>,
    #[serde(rename = "relatedFiles", default)]
    related_files: Vec<String>,
    #[serde(rename = "toolCall")]
    tool_call: Option<String>,
    #[serde(rename = "toolResultSummary")]
    tool_result_summary: Option<String>,
    #[serde(rename = "toolRequest")]
    tool_request: Option<AgentToolCall>,
    #[serde(rename = "taskState")]
    task_state: Option<AgentTaskStateResponse>,
}

#[derive(Debug, Clone, Serialize)]
struct AgentToolResultPayload {
    success: bool,
    output: String,
    summary: String,
    #[serde(rename = "relatedFiles")]
    related_files: Vec<String>,
}

#[derive(Serialize)]
struct AgentToolResultRequest<'a> {
    #[serde(rename = "sessionId")]
    session_id: &'a str,
    #[serde(rename = "toolRequest")]
    tool_request: &'a AgentToolCall,
    #[serde(rename = "toolResult")]
    tool_result: AgentToolResultPayload,
}

#[derive(Deserialize)]
struct AgentResetResponse {
    #[serde(rename = "sessionId")]
    session_id: String,
    reset: bool,
    #[serde(rename = "remainingSessions")]
    remaining_sessions: usize,
}

#[derive(Debug, Clone)]
pub struct AgentAskResult {
    pub success: bool,
    pub session_id: Option<String>,
    pub status: String,
    pub message: String,
    pub summary: Option<String>,
    pub current_action: Option<String>,
    pub next_action: Option<String>,
    pub related_files: Vec<String>,
    pub tool_call: Option<String>,
    pub tool_result_summary: Option<String>,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// 打开文件（可指定行号）
pub fn open_file(path: &str, line: Option<u32>) -> CmdResult {
    match line {
        Some(line) => {
            let target = format!("{path}:{line}");
            run_cmd("code", &["--goto", &target], 10)
        }
        None => run_cmd("code", &["--reuse-window", path], 10),
    }
}

/// 安装扩展
pub fn install_extension(ext_id: &str) -> CmdResult {
    run_cmd("code", &["--install-extension", ext_id], 60)
}

/// 卸载扩展
pub fn uninstall_extension(ext_id: &str) -> CmdResult {
    run_cmd("code", &["--uninstall-extension", ext_id], 30)
}

/// 列出已安装扩展
pub fn list_extensions() -> CmdResult {
    run_cmd("code", &["--list-extensions"], 10)
}

/// 在当前工作区打开一个文件夹
pub fn open_folder(path: &str) -> CmdResult {
    run_cmd("code", &["--add", path], 10)
}

/// 用 VS Code 执行 diff
pub fn diff_files(file1: &str, file2: &str) -> CmdResult {
    run_cmd("code", &["--diff", file1, file2], 10)
}

pub fn ask_agent(session_id: &str, prompt: &str) -> AgentAskResult {
    let start = Instant::now();
    let result = (|| -> Result<AgentAskResult, String> {
        let trimmed_prompt = prompt.trim();
        if trimmed_prompt.is_empty() {
            return Err("提问内容不能为空。".to_string());
        }

        let base_url = agent_bridge_base_url()?;
        let endpoint = format!("{}/v1/chat/ask", base_url);
        let response = ureq::post(&endpoint)
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "sessionId": session_id,
                "prompt": trimmed_prompt,
            }))
            .map_err(|err| format_agent_bridge_error(err, &endpoint))?;

        let mut payload: AgentAskResponse = response
            .into_json()
            .map_err(|err| format!("解析 agent bridge 响应失败: {err}"))?;

        let mut carried_tool_call = None;
        let mut carried_tool_result_summary = None;

        if agent_status_from_response(&payload)
            .eq_ignore_ascii_case("needs_tool")
        {
            let tool_request = payload
                .tool_request
                .clone()
                .ok_or_else(|| "agent bridge 请求了工具，但没有返回可执行的 toolRequest。".to_string())?;

            let executed = execute_agent_tool_call(&tool_request);
            carried_tool_call = Some(format_agent_tool_call(&tool_request));
            carried_tool_result_summary = Some(executed.summary.clone());

            if !executed.success {
                return Ok(AgentAskResult {
                    success: false,
                    session_id: Some(payload.session_id.clone()),
                    status: "blocked".to_string(),
                    message: executed.output.clone(),
                    summary: Some(executed.summary.clone()),
                    current_action: Some(format!(
                        "执行只读工具失败: {}",
                        format_agent_tool_call(&tool_request)
                    )),
                    next_action: Some("请调整问题描述、检查路径参数，或改为更明确的文件/符号后重试。".to_string()),
                    related_files: executed.related_files,
                    tool_call: carried_tool_call,
                    tool_result_summary: carried_tool_result_summary,
                    duration_ms: start.elapsed().as_millis() as u64,
                    error: Some(executed.output),
                });
            }

            let tool_endpoint = format!("{}{}", base_url, AGENT_TOOL_RESULT_PATH);
            let response = ureq::post(&tool_endpoint)
                .set("Content-Type", "application/json")
                .send_json(ureq::json!(AgentToolResultRequest {
                    session_id,
                    tool_request: &tool_request,
                    tool_result: AgentToolResultPayload {
                        success: executed.success,
                        output: executed.output.clone(),
                        summary: executed.summary.clone(),
                        related_files: executed.related_files.clone(),
                    },
                }))
                .map_err(|err| format_agent_bridge_error(err, &tool_endpoint))?;

            payload = response
                .into_json()
                .map_err(|err| format!("解析 agent tool result 响应失败: {err}"))?;
        }

        let task_state = payload.task_state;
        let message = payload
            .message
            .or(payload.reply)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "agent bridge 没有返回 message。".to_string())?;

        let status = task_state
            .as_ref()
            .and_then(|state| state.status.clone())
            .or(payload.status)
            .unwrap_or_else(|| "answered".to_string());

        let summary = task_state
            .as_ref()
            .and_then(|state| state.result_summary.clone())
            .or(payload.summary)
            .filter(|value| !value.trim().is_empty());

        let current_action = task_state
            .as_ref()
            .and_then(|state| state.current_action.clone())
            .or(payload.current_action)
            .filter(|value| !value.trim().is_empty());

        let next_action = task_state
            .as_ref()
            .and_then(|state| state.next_action.clone())
            .or(payload.next_action)
            .filter(|value| !value.trim().is_empty());

        let related_files = task_state
            .as_ref()
            .map(|state| state.related_files.clone())
            .filter(|files| !files.is_empty())
            .unwrap_or_else(|| {
                if payload.related_files.is_empty() {
                    payload
                        .tool_request
                        .as_ref()
                        .map(agent_tool_related_files)
                        .unwrap_or_default()
                } else {
                    payload.related_files
                }
            });

        let tool_call = carried_tool_call
            .or(payload.tool_call)
            .or_else(|| payload.tool_request.as_ref().map(format_agent_tool_call));

        let tool_result_summary = carried_tool_result_summary
            .or(payload.tool_result_summary)
            .filter(|value| !value.trim().is_empty());

        Ok(AgentAskResult {
            success: true,
            session_id: Some(payload.session_id),
            status,
            message,
            summary,
            current_action,
            next_action,
            related_files,
            tool_call,
            tool_result_summary,
            duration_ms: start.elapsed().as_millis() as u64,
            error: None,
        })
    })();

    match result {
        Ok(reply) => reply,
        Err(err) => AgentAskResult {
            success: false,
            session_id: Some(session_id.trim().to_string()).filter(|value| !value.is_empty()),
            status: "blocked".to_string(),
            message: err.clone(),
            summary: Some(err.clone()),
            current_action: Some("向本地 agent bridge 发起请求失败".to_string()),
            next_action: Some("确认 VS Code companion extension 已启动，并检查本地 bridge 健康状态。".to_string()),
            related_files: Vec::new(),
            tool_call: None,
            tool_result_summary: None,
            duration_ms: start.elapsed().as_millis() as u64,
            error: Some(err),
        },
    }
}

#[derive(Debug, Clone)]
struct ExecutedAgentTool {
    success: bool,
    output: String,
    summary: String,
    related_files: Vec<String>,
}

impl Serialize for ExecutedAgentTool {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AgentToolResultPayload {
            success: self.success,
            output: self.output.clone(),
            summary: self.summary.clone(),
            related_files: self.related_files.clone(),
        }
        .serialize(serializer)
    }
}

fn agent_status_from_response(payload: &AgentAskResponse) -> String {
    payload
        .task_state
        .as_ref()
        .and_then(|state| state.status.clone())
        .or_else(|| payload.status.clone())
        .unwrap_or_else(|| "answered".to_string())
}

fn execute_agent_tool_call(tool_call: &AgentToolCall) -> ExecutedAgentTool {
    match tool_call.name.trim() {
        "read_file" => execute_agent_read_file(tool_call),
        "search_text" => execute_agent_search_text(tool_call),
        other => ExecutedAgentTool {
            success: false,
            output: format!("不支持的 agent 工具: {other}"),
            summary: format!("不支持的 agent 工具: {other}"),
            related_files: Vec::new(),
        },
    }
}

fn execute_agent_read_file(tool_call: &AgentToolCall) -> ExecutedAgentTool {
    let path = tool_call
        .args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(path) = path else {
        return ExecutedAgentTool {
            success: false,
            output: "read_file 缺少 path 参数。".to_string(),
            summary: "read_file 缺少 path 参数。".to_string(),
            related_files: Vec::new(),
        };
    };

    let start_line = tool_call
        .args
        .get("startLine")
        .and_then(value_to_usize);
    let end_line = tool_call
        .args
        .get("endLine")
        .and_then(value_to_usize);
    let result = read_file(path, start_line, end_line);
    build_executed_agent_tool(result, format_agent_tool_call(tool_call), vec![path.to_string()])
}

fn execute_agent_search_text(tool_call: &AgentToolCall) -> ExecutedAgentTool {
    let query = tool_call
        .args
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(query) = query else {
        return ExecutedAgentTool {
            success: false,
            output: "search_text 缺少 query 参数。".to_string(),
            summary: "search_text 缺少 query 参数。".to_string(),
            related_files: Vec::new(),
        };
    };

    let path = tool_call.args.get("path").and_then(Value::as_str);
    let is_regex = tool_call
        .args
        .get("isRegex")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let related_files = path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default();
    let result = search_text(query, path, is_regex);
    build_executed_agent_tool(result, format_agent_tool_call(tool_call), related_files)
}

fn build_executed_agent_tool(
    result: CmdResult,
    label: String,
    related_files: Vec<String>,
) -> ExecutedAgentTool {
    let output = combine_cmd_output(&result);
    let summary = summarize_agent_tool_output(&label, &output, result.success);

    ExecutedAgentTool {
        success: result.success,
        output,
        summary,
        related_files,
    }
}

fn combine_cmd_output(result: &CmdResult) -> String {
    if !result.stdout.trim().is_empty() && !result.stderr.trim().is_empty() {
        format!("{}\n{}", result.stdout.trim(), result.stderr.trim())
    } else if !result.stdout.trim().is_empty() {
        result.stdout.trim().to_string()
    } else if !result.stderr.trim().is_empty() {
        result.stderr.trim().to_string()
    } else {
        "(无输出)".to_string()
    }
}

fn summarize_agent_tool_output(label: &str, output: &str, success: bool) -> String {
    let first_lines = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" / ");
    let body = if first_lines.is_empty() {
        if success {
            "工具执行成功。".to_string()
        } else {
            "工具执行失败。".to_string()
        }
    } else if first_lines.chars().count() > MAX_AGENT_TOOL_SUMMARY_CHARS {
        let mut truncated = first_lines
            .chars()
            .take(MAX_AGENT_TOOL_SUMMARY_CHARS)
            .collect::<String>();
        truncated.push('…');
        truncated
    } else {
        first_lines
    };

    format!("{}: {}", label, body)
}

fn format_agent_tool_call(tool_call: &AgentToolCall) -> String {
    match tool_call.name.trim() {
        "read_file" => {
            let path = tool_call
                .args
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("(unknown path)");
            let start_line = tool_call.args.get("startLine").and_then(value_to_usize);
            let end_line = tool_call.args.get("endLine").and_then(value_to_usize);
            match (start_line, end_line) {
                (Some(start_line), Some(end_line)) => {
                    format!("read_file({path}:{start_line}-{end_line})")
                }
                _ => format!("read_file({path})"),
            }
        }
        "search_text" => {
            let query = tool_call
                .args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("");
            let path = tool_call
                .args
                .get("path")
                .and_then(Value::as_str)
                .map(|value| format!(", path={value}"))
                .unwrap_or_default();
            let regex = if tool_call
                .args
                .get("isRegex")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                ", regex=true"
            } else {
                ""
            };
            format!("search_text({query}{path}{regex})")
        }
        other => other.to_string(),
    }
}

fn agent_tool_related_files(tool_call: &AgentToolCall) -> Vec<String> {
    tool_call
        .args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}

fn value_to_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .or_else(|| value.as_str().and_then(|value| value.trim().parse::<usize>().ok()))
}

pub fn reset_agent_session(session_id: &str) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let trimmed_session_id = session_id.trim();
        if trimmed_session_id.is_empty() {
            return Err("sessionId 不能为空。".to_string());
        }

        let endpoint = format!("{}/v1/chat/reset", agent_bridge_base_url()?);
        let response = ureq::post(&endpoint)
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
                "sessionId": trimmed_session_id,
            }))
            .map_err(|err| format_agent_bridge_error(err, &endpoint))?;

        let payload: AgentResetResponse = response
            .into_json()
            .map_err(|err| format!("解析 agent bridge reset 响应失败: {err}"))?;

        let status = if payload.reset {
            "已重置当前 Copilot 会话历史。"
        } else {
            "当前没有可重置的 Copilot 会话历史。"
        };

        Ok(format!(
            "session: {}\n{}\n剩余本地会话数: {}",
            payload.session_id, status, payload.remaining_sessions
        ))
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

/// 读取工作区文件内容
pub fn read_file(path: &str, start_line: Option<usize>, end_line: Option<usize>) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let resolved = resolve_workspace_target(path)?;
        let content = fs::read_to_string(&resolved)
            .map_err(|err| format!("读取文件失败: {err}"))?;
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            return Ok(format!("文件: {}\n(文件为空)", resolved.display()));
        }

        let total_lines = lines.len();
        let default_end = total_lines.min(200);
        let start_line = start_line.unwrap_or(1);
        let requested_end_line = end_line;
        let end_line = requested_end_line.unwrap_or(default_end);

        if start_line == 0 || end_line == 0 || end_line < start_line {
            return Err("行号范围无效，请使用如 1-120 的格式。".to_string());
        }
        if start_line > total_lines {
            return Err(format!(
                "起始行超出文件范围: 文件共 {} 行，但请求从第 {} 行开始。",
                total_lines, start_line
            ));
        }

        let actual_end = end_line.min(total_lines);
        let body = lines[start_line - 1..actual_end]
            .iter()
            .enumerate()
            .map(|(offset, line)| format!("{:>4} | {}", start_line + offset, line))
            .collect::<Vec<_>>()
            .join("\n");

        let mut output = format!(
            "文件: {}\n行: {}-{} / {}\n\n{}",
            resolved.display(),
            start_line,
            actual_end,
            total_lines,
            body
        );

        if requested_end_line.is_none() && total_lines > actual_end {
            output.push_str(&format!(
                "\n\n… 已默认截断，仅显示前 {} 行；可用 `读取 <文件> {}-{}` 查看更多。",
                actual_end,
                actual_end + 1,
                (actual_end + 200).min(total_lines)
            ));
        }

        Ok(output)
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

/// 列出目录内容
pub fn list_directory(path: Option<&str>) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let target = match path.map(str::trim).filter(|path| !path.is_empty()) {
            Some(path) => resolve_workspace_target(path)?,
            None => workspace_root()?,
        };

        let entries = fs::read_dir(&target).map_err(|err| format!("读取目录失败: {err}"))?;
        let mut items = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| {
                let file_type = entry.file_type().ok();
                let mut name = entry.file_name().to_string_lossy().to_string();
                if file_type.map(|kind| kind.is_dir()).unwrap_or(false) {
                    name.push('/');
                }
                name
            })
            .collect::<Vec<_>>();

        items.sort();

        let total = items.len();
        let shown = items.iter().take(200).cloned().collect::<Vec<_>>();
        let mut output = format!("目录: {}\n项目数: {}\n\n{}", target.display(), total, shown.join("\n"));
        if total > shown.len() {
            output.push_str(&format!("\n\n… 仅显示前 {} 项。", shown.len()));
        }
        if total == 0 {
            output.push_str("\n\n(目录为空)");
        }

        Ok(output)
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

/// 在工作区内搜索文本
pub fn search_text(query: &str, path: Option<&str>, is_regex: bool) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Err("搜索内容不能为空。".to_string());
        }

        let target = match path.map(str::trim).filter(|path| !path.is_empty()) {
            Some(path) => resolve_workspace_target(path)?,
            None => workspace_root()?,
        };

        let mut command = Command::new("rg");
        command.args(["-n", "--no-heading", "--color", "never"]);
        if !is_regex {
            command.arg("-F");
        }
        command.arg(trimmed_query);
        command.arg(&target);

        let output = command.output().map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                String::new()
            } else {
                format!("执行搜索失败: {err}")
            }
        });

        let output = match output {
            Ok(output) => output,
            Err(message) if message.is_empty() => {
                return fallback_text_search(trimmed_query, &target, is_regex);
            }
            Err(message) => return Err(message),
        };

        match output.status.code() {
            Some(0) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let lines = stdout.lines().take(200).collect::<Vec<_>>();
                let truncated = stdout.lines().count() > lines.len();
                let mut result = format!(
                    "搜索范围: {}\n模式: {}\n关键词: {}\n\n{}",
                    target.display(),
                    if is_regex { "正则" } else { "文本" },
                    trimmed_query,
                    lines.join("\n")
                );
                if truncated {
                    result.push_str("\n\n… 搜索结果过多，仅显示前 200 行。");
                }
                Ok(result)
            }
            Some(1) => Ok(format!(
                "搜索范围: {}\n模式: {}\n关键词: {}\n\n未找到匹配结果。",
                target.display(),
                if is_regex { "正则" } else { "文本" },
                trimmed_query
            )),
            _ => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        }
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

/// 运行工作区测试
pub fn run_tests(command: Option<&str>) -> CmdResult {
    let start = Instant::now();
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => {
            return CmdResult {
                success: false,
                stdout: String::new(),
                stderr: err,
                exit_code: Some(1),
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }
    };

    let test_command = command
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(default_test_command)
        .unwrap_or_else(|| "cargo test".to_string());

    let output = run_test_command(&workspace, &test_command);

    let duration_ms = start.elapsed().as_millis() as u64;

    match output {
        Ok(output) => {
            let success = output.status.success();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            CmdResult {
                success,
                stdout: summarize_test_output(&test_command, &stdout, &stderr, success),
                stderr: String::new(),
                exit_code: output.status.code(),
                duration_ms,
            }
        }
        Err(error) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr: format!("执行测试失败: {error}"),
            exit_code: None,
            duration_ms,
        },
    }
}

/// 查看当前工作区 diff
pub fn git_diff(path: Option<&str>) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let pathspec = match path {
        Some(path) if !path.trim().is_empty() => match normalize_pathspec(&workspace, path) {
            Ok(pathspec) => pathspec,
            Err(err) => return error_cmd_result(err),
        },
        _ => ".".to_string(),
    };

    let pathspec_ref = pathspec.as_str();
    run_cmd(
        "git",
        &["-C", workspace.to_string_lossy().as_ref(), "diff", "--", pathspec_ref],
        30,
    )
}

/// 在当前工作区应用 unified diff 补丁
pub fn apply_patch(patch: &str) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let normalized_patch = normalize_patch_text(patch);
    if normalized_patch.trim().is_empty() {
        return error_cmd_result("补丁内容不能为空。请使用 unified diff 格式。".to_string());
    }

    if let Err(err) = validate_patch_paths(&normalized_patch) {
        return error_cmd_result(err);
    }

    let check = run_git_apply(&workspace, &normalized_patch, true, false);
    if !check.success {
        return with_step_label("git apply --check", check);
    }

    let apply = run_git_apply(&workspace, &normalized_patch, false, false);
    if !apply.success {
        return with_step_label("git apply", apply);
    }

    combine_results(&[("git apply --check", check), ("git apply", apply)])
}

pub fn reverse_patch(patch: &str) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let normalized_patch = normalize_patch_text(patch);
    if normalized_patch.trim().is_empty() {
        return error_cmd_result("补丁内容不能为空。请使用 unified diff 格式。".to_string());
    }

    if let Err(err) = validate_patch_paths(&normalized_patch) {
        return error_cmd_result(err);
    }

    let check = run_git_apply(&workspace, &normalized_patch, true, true);
    if !check.success {
        return with_step_label("git apply --check --reverse", check);
    }

    let apply = run_git_apply(&workspace, &normalized_patch, false, true);
    if !apply.success {
        return with_step_label("git apply --reverse", apply);
    }

    combine_results(&[("git apply --check --reverse", check), ("git apply --reverse", apply)])
}

/// 执行任意 shell 命令（用户通过飞书发送时需要谨慎）
pub fn run_shell(cmd: &str) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let start = Instant::now();

    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", cmd])
        .current_dir(&workspace)
        .output();

    #[cfg(not(target_os = "windows"))]
    let result = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(&workspace)
        .output();

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(output) => CmdResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
            duration_ms,
        },
        Err(e) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr: format!("执行失败: {e}"),
            exit_code: None,
            duration_ms,
        },
    }
}

/// 查看 Git 仓库状态
pub fn git_status(repo_path: Option<&str>) -> CmdResult {
    let resolved_repo_path = resolve_repo_path(repo_path);

    match resolved_repo_path.as_deref() {
        Some(p) => run_cmd("git", &["-C", p, "status", "--short"], 10),
        None => run_cmd("git", &["status", "--short"], 10),
    }
}

/// Git pull
pub fn git_pull(repo_path: Option<&str>) -> CmdResult {
    let resolved_repo_path = resolve_repo_path(repo_path);

    match resolved_repo_path.as_deref() {
        Some(p) => run_cmd("git", &["-C", p, "pull"], 30),
        None => run_cmd("git", &["pull"], 30),
    }
}

/// Git add + commit + push
pub fn git_push_all(repo_path: Option<&str>, message: &str) -> CmdResult {
    let resolved_repo_path = resolve_repo_path(repo_path);

    let add = run_git(resolved_repo_path.as_deref(), &["add", "-A"], 30);
    if !add.success {
        return with_step_label("git add -A", add);
    }

    let commit = run_git(resolved_repo_path.as_deref(), &["commit", "-m", message], 30);
    if !commit.success {
        if is_nothing_to_commit(&commit) {
            return CmdResult {
                success: true,
                stdout: "[git commit]\nnothing to commit, working tree clean\n\n[git push]\n无需推送：当前工作区没有可提交的变更".to_string(),
                stderr: String::new(),
                exit_code: Some(0),
                duration_ms: add.duration_ms + commit.duration_ms,
            };
        }
        return with_step_label("git commit", commit);
    }

    let push = run_git(resolved_repo_path.as_deref(), &["push"], 30);
    if !push.success {
        return with_step_label("git push", push);
    }

    combine_results(&[
        ("git add -A", add),
        ("git commit", commit),
        ("git push", push),
    ])
}

/// 查看 Git 提交历史
pub fn git_log(count: Option<usize>, path: Option<&str>) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let ws = workspace.to_string_lossy();
    let n = count.unwrap_or(20).min(100).to_string();
    let mut args = vec!["-C", &ws, "log", "--oneline", "--no-decorate", "-n", &n];

    let pathspec;
    if let Some(p) = path.map(str::trim).filter(|p| !p.is_empty()) {
        args.push("--");
        pathspec = match normalize_pathspec(&workspace, p) {
            Ok(s) => s,
            Err(err) => return error_cmd_result(err),
        };
        args.push(&pathspec);
    }

    run_cmd("git", &args, 30)
}

/// 查看文件逐行 blame
pub fn git_blame(path: &str) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let pathspec = match normalize_pathspec(&workspace, path) {
        Ok(s) => s,
        Err(err) => return error_cmd_result(err),
    };

    let ws = workspace.to_string_lossy();
    run_cmd("git", &["-C", &ws, "blame", "--", &pathspec], 30)
}

/// 搜索符号（函数、结构体、类型定义）
pub fn search_symbol(query: &str, path: Option<&str>) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Err("符号名称不能为空。".to_string());
        }

        let target = match path.map(str::trim).filter(|path| !path.is_empty()) {
            Some(path) => resolve_workspace_target(path)?,
            None => workspace_root()?,
        };
        let options = SearchOptions::from_explicit_path(path);

        let pattern = symbol_definition_pattern(trimmed_query);

        match run_rg_regex_search(&pattern, &target, options) {
            Ok(SearchBackendResult::Matches(matches, truncated)) => {
                Ok(format_grouped_search_reply(
                    "符号",
                    trimmed_query,
                    &target,
                    &matches,
                    truncated,
                    None,
                ))
            }
            Ok(SearchBackendResult::NoMatches) => Ok(format!(
                "搜索范围: {}\n符号: {}\n\n未找到匹配的符号定义。",
                target.display(),
                trimmed_query
            )),
            Ok(SearchBackendResult::FallbackRequired) => {
                fallback_symbol_search(trimmed_query, &target, options)
            }
            Err(err) => Err(err),
        }
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

/// 搜索符号引用位置
pub fn find_references(query: &str, path: Option<&str>) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Err("引用名称不能为空。".to_string());
        }

        let target = match path.map(str::trim).filter(|path| !path.is_empty()) {
            Some(path) => resolve_workspace_target(path)?,
            None => workspace_root()?,
        };

        let options = SearchOptions::from_explicit_path(path);
        let pattern = format!(r"\b{}\b", regex_escape(trimmed_query));
        match run_rg_regex_search(&pattern, &target, options) {
            Ok(SearchBackendResult::Matches(matches, truncated)) => {
                let filtered = matches
                    .into_iter()
                    .filter(|item| !is_definition_line_for_symbol(&item.line_text, trimmed_query))
                    .collect::<Vec<_>>();

                if filtered.is_empty() {
                    Ok(format!(
                        "搜索范围: {}\n引用: {}\n\n未找到匹配引用。",
                        target.display(),
                        trimmed_query
                    ))
                } else {
                    Ok(format_grouped_search_reply(
                        "引用",
                        trimmed_query,
                        &target,
                        &filtered,
                        truncated,
                        None,
                    ))
                }
            }
            Ok(SearchBackendResult::NoMatches) => Ok(format!(
                "搜索范围: {}\n引用: {}\n\n未找到匹配引用。",
                target.display(),
                trimmed_query
            )),
            Ok(SearchBackendResult::FallbackRequired) => {
                fallback_reference_search(trimmed_query, &target, options)
            }
            Err(err) => Err(err),
        }
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

/// 搜索符号实现位置
pub fn find_implementations(query: &str, path: Option<&str>) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Err("实现名称不能为空。".to_string());
        }

        let target = match path.map(str::trim).filter(|path| !path.is_empty()) {
            Some(path) => resolve_workspace_target(path)?,
            None => workspace_root()?,
        };

        let options = SearchOptions::from_explicit_path(path);
        let pattern = implementation_pattern(trimmed_query);
        match run_rg_regex_search(&pattern, &target, options) {
            Ok(SearchBackendResult::Matches(matches, truncated)) => {
                Ok(format_grouped_search_reply(
                    "实现",
                    trimmed_query,
                    &target,
                    &matches,
                    truncated,
                    None,
                ))
            }
            Ok(SearchBackendResult::NoMatches) => Ok(format!(
                "搜索范围: {}\n实现: {}\n\n未找到匹配实现。",
                target.display(),
                trimmed_query
            )),
            Ok(SearchBackendResult::FallbackRequired) => {
                fallback_implementation_search(trimmed_query, &target, options)
            }
            Err(err) => Err(err),
        }
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

/// 运行指定名称的测试
pub fn run_specific_test(filter: &str) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let start = Instant::now();
    let trimmed = filter.trim();
    if trimmed.is_empty() {
        return error_cmd_result("测试过滤词不能为空。".to_string());
    }

    // Detect project type and build the right test command
    let test_command = if workspace.join("Cargo.toml").exists() {
        format!("cargo test {trimmed}")
    } else if workspace.join("package.json").exists() {
        format!("npx jest --testNamePattern {trimmed}")
    } else if workspace.join("pyproject.toml").exists() || workspace.join("setup.py").exists() {
        format!("python -m pytest -k {trimmed}")
    } else {
        format!("cargo test {trimmed}")
    };

    let output = run_test_command(&workspace, &test_command);

    let duration_ms = start.elapsed().as_millis() as u64;

    match output {
        Ok(output) => {
            let success = output.status.success();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            CmdResult {
                success,
                stdout: summarize_test_output(&test_command, &stdout, &stderr, success),
                stderr: String::new(),
                exit_code: output.status.code(),
                duration_ms,
            }
        }
        Err(error) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr: format!("执行测试失败: {error}"),
            exit_code: None,
            duration_ms,
        },
    }
}

/// 按测试文件执行测试
pub fn run_test_file(path: &str) -> CmdResult {
    let workspace = match workspace_root() {
        Ok(path) => path,
        Err(err) => return error_cmd_result(err),
    };

    let start = Instant::now();
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return error_cmd_result("测试文件路径不能为空。".to_string());
    }

    let target = match resolve_workspace_target(trimmed) {
        Ok(target) => target,
        Err(err) => return error_cmd_result(err),
    };

    let command = match build_test_file_command(&workspace, &target) {
        Ok(command) => command,
        Err(err) => return error_cmd_result(err),
    };

    let output = run_test_command(&workspace, &command);

    let duration_ms = start.elapsed().as_millis() as u64;

    match output {
        Ok(output) => {
            let success = output.status.success();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            CmdResult {
                success,
                stdout: summarize_test_output(&command, &stdout, &stderr, success),
                stderr: String::new(),
                exit_code: output.status.code(),
                duration_ms,
            }
        }
        Err(error) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr: format!("执行测试失败: {error}"),
            exit_code: None,
            duration_ms,
        },
    }
}

/// 写入文件（创建或覆盖）
pub fn write_file(path: &str, content: &str) -> CmdResult {
    let start = Instant::now();
    let result = (|| -> Result<String, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err("文件路径不能为空。".to_string());
        }
        let target = resolve_workspace_target(trimmed)?;

        // Ensure parent directory exists
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("创建目录失败: {err}"))?;
        }

        fs::write(&target, content).map_err(|err| format!("写入文件失败: {err}"))?;

        Ok(format!(
            "已写入: {}\n大小: {} 字节",
            target.display(),
            content.len()
        ))
    })();

    into_cmd_result(result, start.elapsed().as_millis() as u64)
}

fn resolve_repo_path(repo_path: Option<&str>) -> Option<String> {
    repo_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .or_else(configured_workspace_path)
}

fn configured_workspace_path() -> Option<String> {
    let value = std::env::var(WORKSPACE_PATH_ENV).ok()?;
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed)
}

fn default_test_command() -> Option<String> {
    let value = std::env::var(TEST_COMMAND_ENV).ok()?;
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed)
}

fn build_test_file_command(workspace: &Path, target: &Path) -> Result<String, String> {
    if !target.exists() {
        return Err(format!("测试文件不存在: {}", target.display()));
    }

    let relative = target
        .strip_prefix(workspace)
        .unwrap_or(target)
        .to_string_lossy()
        .replace('\\', "/");

    if workspace.join("Cargo.toml").exists() {
        if relative.starts_with("tests/") && relative.ends_with(".rs") {
            let stem = Path::new(&relative)
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "无法解析 Rust 测试文件名。".to_string())?;
            return Ok(format!("cargo test --test {stem}"));
        }

        let stem = Path::new(&relative)
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "无法解析测试文件名。".to_string())?;
        return Ok(format!("cargo test {stem}"));
    }

    if workspace.join("package.json").exists() {
        return Ok(format!("npx jest {relative}"));
    }

    if workspace.join("pyproject.toml").exists() || workspace.join("setup.py").exists() {
        return Ok(format!("python -m pytest {relative}"));
    }

    Err("当前工作区缺少已知测试运行器配置。".to_string())
}

fn run_test_command(workspace: &Path, test_command: &str) -> std::io::Result<std::process::Output> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", test_command]);
        command
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut command = Command::new("sh");
        command.args(["-c", test_command]);
        command
    };

    command.current_dir(workspace);

    if should_isolate_rust_test_target_dir(workspace, test_command) {
        command.env("CARGO_TARGET_DIR", isolated_rust_test_target_dir(workspace));
    }

    command.output()
}

fn should_isolate_rust_test_target_dir(workspace: &Path, test_command: &str) -> bool {
    workspace.join("Cargo.toml").exists() && test_command.trim_start().starts_with("cargo test")
}

fn isolated_rust_test_target_dir(workspace: &Path) -> PathBuf {
    workspace.join("target").join("bridge-test-runner")
}

enum SearchBackendResult {
    Matches(Vec<SearchMatch>, bool),
    NoMatches,
    FallbackRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchMatch {
    path: PathBuf,
    line_number: usize,
    line_text: String,
}

#[derive(Clone, Copy)]
struct SearchOptions {
    exclude_test_paths: bool,
    exclude_inline_rust_tests: bool,
    exclude_runtime_artifacts: bool,
}

impl SearchOptions {
    fn from_explicit_path(path: Option<&str>) -> Self {
        let explicit_path = path.map(str::trim).filter(|path| !path.is_empty());
        let exclude_test_paths = explicit_path
            .map(|path| !path_targets_test_scope(Path::new(path)))
            .unwrap_or(true);
        let exclude_runtime_artifacts = explicit_path
            .map(|path| !path_targets_runtime_artifact(Path::new(path)))
            .unwrap_or(true);
        Self {
            exclude_test_paths,
            exclude_inline_rust_tests: exclude_test_paths,
            exclude_runtime_artifacts,
        }
    }
}

fn run_rg_regex_search(
    pattern: &str,
    target: &Path,
    options: SearchOptions,
) -> Result<SearchBackendResult, String> {
    let mut command = Command::new("rg");
    command.args(["-n", "--no-heading", "--color", "never"]);
    if options.exclude_test_paths {
        for glob in excluded_test_globs() {
            command.arg("-g");
            command.arg(glob);
        }
    }
    if options.exclude_runtime_artifacts {
        for glob in excluded_runtime_artifact_globs() {
            command.arg("-g");
            command.arg(glob);
        }
    }
    command.arg(pattern);
    command.arg(target);

    let output = command.output().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            String::new()
        } else {
            format!("执行搜索失败: {err}")
        }
    });

    let output = match output {
        Ok(output) => output,
        Err(message) if message.is_empty() => return Ok(SearchBackendResult::FallbackRequired),
        Err(message) => return Err(message),
    };

    match output.status.code() {
        Some(0) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let matches = collect_filtered_rg_matches(&stdout, options);
            let truncated = matches.len() > 200;
            let matches = matches.into_iter().take(200).collect::<Vec<_>>();
            if matches.is_empty() {
                Ok(SearchBackendResult::NoMatches)
            } else {
                Ok(SearchBackendResult::Matches(matches, truncated))
            }
        }
        Some(1) => Ok(SearchBackendResult::NoMatches),
        _ => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
    }
}

fn parse_rg_match_line(line: &str) -> Option<SearchMatch> {
    let captures = Regex::new(r"^(.*?):(\d+):(.*)$").ok()?.captures(line)?;
    Some(SearchMatch {
        path: PathBuf::from(captures.get(1)?.as_str()),
        line_number: captures.get(2)?.as_str().parse().ok()?,
        line_text: captures.get(3)?.as_str().to_string(),
    })
}

fn collect_filtered_rg_matches(output: &str, options: SearchOptions) -> Vec<SearchMatch> {
    let mut filtered = Vec::new();
    let mut inline_test_ranges = BTreeMap::<PathBuf, Vec<(usize, usize)>>::new();

    for line in output.lines() {
        let Some(item) = parse_rg_match_line(line) else {
            continue;
        };

        if should_exclude_search_match(&item, options, &mut inline_test_ranges) {
            continue;
        }

        filtered.push(item);
    }

    filtered
}

const SEARCH_MATCH_LIMIT_PER_FILE: usize = 10;

fn format_grouped_search_reply(
    label: &str,
    query: &str,
    target: &Path,
    matches: &[SearchMatch],
    truncated: bool,
    fallback_note: Option<&str>,
) -> String {
    let mut groups = BTreeMap::<String, Vec<&SearchMatch>>::new();
    for item in matches {
        groups
            .entry(display_search_path(&item.path))
            .or_default()
            .push(item);
    }

    let file_count = groups.len();
    let mut rendered = Vec::new();
    let mut truncated_per_file = false;
    for (path, items) in groups {
        rendered.push(path);
        for item in items.iter().take(SEARCH_MATCH_LIMIT_PER_FILE) {
            rendered.push(format!("  {}: {}", item.line_number, item.line_text.trim_end()));
        }
        if items.len() > SEARCH_MATCH_LIMIT_PER_FILE {
            truncated_per_file = true;
            rendered.push(format!(
                "  … 另有 {} 处匹配未显示",
                items.len() - SEARCH_MATCH_LIMIT_PER_FILE
            ));
        }
        rendered.push(String::new());
    }
    while matches!(rendered.last(), Some(line) if line.is_empty()) {
        rendered.pop();
    }

    let mut result = format!(
        "搜索范围: {}\n{}: {}\n命中: {} 个文件，{} 处匹配\n\n{}",
        target.display(),
        label,
        query,
        file_count,
        matches.len(),
        rendered.join("\n")
    );

    if truncated {
        result.push_str("\n\n… 结果过多，仅显示前 200 处匹配。");
    }
    if truncated_per_file {
        result.push_str(&format!(
            "\n\n… 为避免单文件结果刷屏，每个文件最多显示前 {} 处匹配。",
            SEARCH_MATCH_LIMIT_PER_FILE
        ));
    }
    if let Some(note) = fallback_note {
        result.push_str("\n\n");
        result.push_str(note);
    }

    result
}

fn display_search_path(path: &Path) -> String {
    if let Ok(workspace) = workspace_root() {
        if let Ok(relative) = path.strip_prefix(&workspace) {
            let display = relative.to_string_lossy().replace('\\', "/");
            if !display.is_empty() {
                return display;
            }
        }
    }

    path.display().to_string().replace('\\', "/")
}

fn fallback_text_search(query: &str, target: &Path, is_regex: bool) -> Result<String, String> {
    let regex = if is_regex {
        Some(Regex::new(query).map_err(|err| format!("正则无效: {err}"))?)
    } else {
        None
    };

    let matches = search_text_in_files(target, SearchOptions { exclude_test_paths: false, exclude_inline_rust_tests: false, exclude_runtime_artifacts: false }, |_, line| {
        match &regex {
            Some(regex) => regex.is_match(line),
            None => line.contains(query),
        }
    })?;

    let mut result = format!(
        "搜索范围: {}\n模式: {}\n关键词: {}\n",
        target.display(),
        if is_regex { "正则" } else { "文本" },
        query
    );

    if matches.is_empty() {
        result.push_str("\n未找到匹配结果。\n\n(当前环境未安装 rg，已自动使用内置搜索。)");
        return Ok(result);
    }

    result.push('\n');
    result.push_str(&format_search_match_lines(&matches).join("\n"));
    if matches.len() >= 200 {
        result.push_str("\n\n… 搜索结果过多，仅显示前 200 行。");
    }
    result.push_str("\n\n(当前环境未安装 rg，已自动使用内置搜索。)");
    Ok(result)
}

fn fallback_symbol_search(
    query: &str,
    target: &Path,
    options: SearchOptions,
) -> Result<String, String> {
    let pattern = symbol_definition_pattern(query);
    let regex = Regex::new(&pattern).map_err(|err| format!("符号搜索模式无效: {err}"))?;

    let matches = search_text_in_files(target, options, |_, line| {
        regex.is_match(line)
    })?;

    let mut result = format!("搜索范围: {}\n符号: {}\n", target.display(), query);

    if matches.is_empty() {
        result.push_str("\n未找到匹配的符号定义。\n\n(当前环境未安装 rg，已自动使用内置搜索。)");
        return Ok(result);
    }

    Ok(format_grouped_search_reply(
        "符号",
        query,
        target,
        &matches,
        matches.len() >= 200,
        Some("(当前环境未安装 rg，已自动使用内置搜索。)"),
    ))
}

fn fallback_reference_search(
    query: &str,
    target: &Path,
    options: SearchOptions,
) -> Result<String, String> {
    let pattern = format!(r"\b{}\b", regex_escape(query));
    let regex = Regex::new(&pattern).map_err(|err| format!("引用搜索模式无效: {err}"))?;
    let matches = search_text_in_files(target, options, |_, line| {
        regex.is_match(line) && !is_definition_line_for_symbol(line, query)
    })?;

    if matches.is_empty() {
        return Ok(format!(
            "搜索范围: {}\n引用: {}\n\n未找到匹配引用。\n\n(当前环境未安装 rg，已自动使用内置搜索。)",
            target.display(),
            query
        ));
    }

    Ok(format_grouped_search_reply(
        "引用",
        query,
        target,
        &matches,
        matches.len() >= 200,
        Some("(当前环境未安装 rg，已自动使用内置搜索。)"),
    ))
}

fn is_definition_line_for_symbol(line: &str, query: &str) -> bool {
    let pattern = symbol_definition_pattern(query);

    Regex::new(&pattern)
        .map(|regex| regex.is_match(line))
        .unwrap_or(false)
}

fn symbol_definition_pattern(query: &str) -> String {
    let escaped = regex_escape(query);
    format!(
        r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:fn|struct|enum|type|trait|impl|class|def|function|const|let|interface|mod)\s+{escaped}\b"
    )
}

fn implementation_pattern(query: &str) -> String {
    let escaped = regex_escape(query);
    format!(
        r"(?:^\s*impl\b[^\n]*\b{escaped}\b|^\s*class\s+{escaped}\b|^\s*interface\s+{escaped}\b|^\s*\w[^\n]*\bimplements\b[^\n]*\b{escaped}\b|^\s*\w[^\n]*\bextends\b[^\n]*\b{escaped}\b)"
    )
}

fn fallback_implementation_search(
    query: &str,
    target: &Path,
    options: SearchOptions,
) -> Result<String, String> {
    let regex = Regex::new(&implementation_pattern(query))
        .map_err(|err| format!("实现搜索模式无效: {err}"))?;
    let matches = search_text_in_files(target, options, |_, line| regex.is_match(line))?;

    if matches.is_empty() {
        return Ok(format!(
            "搜索范围: {}\n实现: {}\n\n未找到匹配实现。\n\n(当前环境未安装 rg，已自动使用内置搜索。)",
            target.display(),
            query
        ));
    }

    Ok(format_grouped_search_reply(
        "实现",
        query,
        target,
        &matches,
        matches.len() >= 200,
        Some("(当前环境未安装 rg，已自动使用内置搜索。)"),
    ))
}

fn format_search_match_lines(matches: &[SearchMatch]) -> Vec<String> {
    matches
        .iter()
        .map(|item| format!("{}:{}:{}", item.path.display(), item.line_number, item.line_text))
        .collect()
}

fn should_exclude_search_match(
    item: &SearchMatch,
    options: SearchOptions,
    inline_test_ranges: &mut BTreeMap<PathBuf, Vec<(usize, usize)>>,
) -> bool {
    if options.exclude_runtime_artifacts && is_runtime_artifact_path(&item.path) {
        return true;
    }

    if options.exclude_test_paths && is_test_path(&item.path) {
        return true;
    }

    if options.exclude_inline_rust_tests && is_rust_inline_test_line(item, inline_test_ranges) {
        return true;
    }

    false
}

fn is_rust_inline_test_line(
    item: &SearchMatch,
    inline_test_ranges: &mut BTreeMap<PathBuf, Vec<(usize, usize)>>,
) -> bool {
    if item.path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
        return false;
    }

    let ranges = inline_test_ranges.entry(item.path.clone()).or_insert_with(|| {
        fs::read_to_string(&item.path)
            .map(|content| rust_inline_test_module_ranges(&content))
            .unwrap_or_default()
    });

    ranges
        .iter()
        .any(|(start, end)| item.line_number >= *start && item.line_number <= *end)
}

fn rust_inline_test_module_ranges(content: &str) -> Vec<(usize, usize)> {
    let lines = content.lines().collect::<Vec<_>>();
    let mut ranges = Vec::new();
    let mut pending_cfg_test = false;
    let mut index = 0;

    while index < lines.len() {
        let trimmed = lines[index].trim();

        if trimmed.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
            index += 1;
            continue;
        }

        if pending_cfg_test && (trimmed.is_empty() || trimmed.starts_with("#[")) {
            index += 1;
            continue;
        }

        if pending_cfg_test && is_rust_test_module_declaration(trimmed) {
            if let Some(end_line) = find_rust_module_end_line(&lines, index) {
                ranges.push((index + 1, end_line));
                index = end_line;
                pending_cfg_test = false;
                continue;
            }
        }

        pending_cfg_test = false;
        index += 1;
    }

    ranges
}

fn is_rust_test_module_declaration(line: &str) -> bool {
    line.starts_with("mod tests")
        || line.starts_with("pub mod tests")
        || line.starts_with("pub(crate) mod tests")
        || line.starts_with("pub(super) mod tests")
        || line.starts_with("pub(self) mod tests")
}

fn find_rust_module_end_line(lines: &[&str], start_index: usize) -> Option<usize> {
    let mut brace_depth = 0isize;
    let mut opened = false;

    for (index, line) in lines.iter().enumerate().skip(start_index) {
        if line.contains('{') {
            opened = true;
        }

        if opened {
            brace_depth += line.chars().filter(|ch| *ch == '{').count() as isize;
            brace_depth -= line.chars().filter(|ch| *ch == '}').count() as isize;
            if brace_depth == 0 {
                return Some(index + 1);
            }
        }
    }

    None
}

fn search_text_in_files<F>(
    target: &Path,
    options: SearchOptions,
    mut matches_line: F,
) -> Result<Vec<SearchMatch>, String>
where
    F: FnMut(&Path, &str) -> bool,
{
    let mut results = Vec::new();
    visit_searchable_files(target, options, &mut |path| {
        if results.len() >= 200 {
            return Ok(());
        }

        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(()),
        };

        if bytes.contains(&0) {
            return Ok(());
        }

        let content = String::from_utf8_lossy(&bytes);
        let inline_test_ranges = if options.exclude_inline_rust_tests
            && path.extension().and_then(|ext| ext.to_str()) == Some("rs")
        {
            rust_inline_test_module_ranges(&content)
        } else {
            Vec::new()
        };
        for (index, line) in content.lines().enumerate() {
            let line_number = index + 1;
            if inline_test_ranges
                .iter()
                .any(|(start, end)| line_number >= *start && line_number <= *end)
            {
                continue;
            }
            if matches_line(path, line) {
                results.push(SearchMatch {
                    path: path.to_path_buf(),
                    line_number,
                    line_text: line.to_string(),
                });
                if results.len() >= 200 {
                    break;
                }
            }
        }

        Ok(())
    })?;

    Ok(results)
}

fn visit_searchable_files<F>(
    target: &Path,
    options: SearchOptions,
    visit: &mut F,
) -> Result<(), String>
where
    F: FnMut(&Path) -> Result<(), String>,
{
    if target.is_file() {
        return visit(target);
    }

    let entries = fs::read_dir(target).map_err(|err| format!("读取目录失败: {err}"))?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), ".git" | "target" | "node_modules" | ".next" | "dist" | "build") {
                continue;
            }
            if options.exclude_test_paths && is_test_path(&path) {
                continue;
            }
            visit_searchable_files(&path, options, visit)?;
        } else if file_type.is_file() {
            if options.exclude_runtime_artifacts && is_runtime_artifact_path(&path) {
                continue;
            }
            if options.exclude_test_paths && is_test_path(&path) {
                continue;
            }
            visit(&path)?;
        }
    }

    Ok(())
}

fn excluded_test_globs() -> &'static [&'static str] {
    &["!tests/**", "!test/**", "!__tests__/**", "!spec/**", "!specs/**"]
}

fn excluded_runtime_artifact_globs() -> &'static [&'static str] {
    &[
        "!.feishu-vscode-bridge-audit.jsonl",
        "!.feishu-vscode-bridge-session.json",
    ]
}

fn path_targets_test_scope(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => is_test_component(&name.to_string_lossy()),
        _ => false,
    })
}

fn path_targets_runtime_artifact(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => is_runtime_artifact_name(&name.to_string_lossy()),
        _ => false,
    })
}

fn is_test_path(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => is_test_component(&name.to_string_lossy()),
        _ => false,
    })
}

fn is_runtime_artifact_path(path: &Path) -> bool {
    path.file_name()
        .map(|name| is_runtime_artifact_name(&name.to_string_lossy()))
        .unwrap_or(false)
}

fn is_test_component(name: &str) -> bool {
    matches!(name, "tests" | "test" | "__tests__" | "spec" | "specs")
}

fn is_runtime_artifact_name(name: &str) -> bool {
    matches!(name, ".feishu-vscode-bridge-audit.jsonl" | ".feishu-vscode-bridge-session.json")
}

fn error_cmd_result(message: String) -> CmdResult {
    CmdResult {
        success: false,
        stdout: String::new(),
        stderr: message,
        exit_code: Some(1),
        duration_ms: 0,
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    configured_workspace_path()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "无法定位当前工作区路径。".to_string())
}

fn agent_bridge_base_url() -> Result<String, String> {
    dotenvy::dotenv().ok();

    if let Ok(url) = std::env::var(AGENT_BRIDGE_URL_ENV) {
        let trimmed = url.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let port = std::env::var(AGENT_BRIDGE_PORT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_AGENT_BRIDGE_PORT);

    Ok(format!("http://127.0.0.1:{port}"))
}

fn format_agent_bridge_error(error: ureq::Error, endpoint: &str) -> String {
    match error {
        ureq::Error::Status(status, response) => {
            let body = response.into_string().unwrap_or_else(|_| String::new());
            if body.trim().is_empty() {
                format!("agent bridge 请求失败: HTTP {status} ({endpoint})")
            } else {
                format!("agent bridge 请求失败: HTTP {status} ({endpoint})\n{body}")
            }
        }
        ureq::Error::Transport(err) => format!(
            "无法连接到本地 agent bridge ({endpoint}): {err}. 请先在 VS Code 中启动 companion extension。"
        ),
    }
}

fn resolve_workspace_target(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("路径不能为空。".to_string());
    }

    let raw = Path::new(trimmed);
    Ok(if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        workspace_root()?.join(raw)
    })
}

fn normalize_pathspec(workspace: &Path, path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(".".to_string());
    }

    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        let relative = raw
            .strip_prefix(workspace)
            .map_err(|_| "diff 路径必须位于当前工作区内。".to_string())?;
        return sanitize_relative_path(relative);
    }

    sanitize_relative_path(raw)
}

fn sanitize_relative_path(path: &Path) -> Result<String, String> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.contains(':') {
                    return Err("路径不能包含冒号。".to_string());
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("路径不能跳出当前工作区。".to_string())
            }
        }
    }

    if parts.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(parts.join("/"))
    }
}

fn validate_patch_paths(patch: &str) -> Result<(), String> {
    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("--- ") {
            validate_patch_path_entry(path.trim())?;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            validate_patch_path_entry(path.trim())?;
        }
    }

    Ok(())
}

fn validate_patch_path_entry(path: &str) -> Result<(), String> {
    normalize_patch_path_entry(path).map(|_| ())
}

#[cfg(test)]
pub(crate) fn extract_primary_patch_path(patch: &str) -> Option<String> {
    extract_patch_paths(patch).into_iter().next()
}

pub(crate) fn extract_patch_paths(patch: &str) -> Vec<String> {
    let mut last_old_path = None;
    let mut paths = Vec::new();

    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("--- ") {
            last_old_path = normalize_patch_path_entry(path.trim()).ok().flatten();
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ ") {
            let new_path = normalize_patch_path_entry(path.trim()).ok().flatten();
            if let Some(path) = new_path.or_else(|| last_old_path.clone()) {
                paths.retain(|existing| existing != &path);
                paths.insert(0, path);
            }
        }
    }

    paths
}

fn normalize_patch_path_entry(path: &str) -> Result<Option<String>, String> {
    let path_only = path.split_whitespace().next().unwrap_or("");
    if path_only == "/dev/null" {
        return Ok(None);
    }

    let trimmed = path_only
        .strip_prefix("a/")
        .or_else(|| path_only.strip_prefix("b/"))
        .unwrap_or(path_only)
        .trim();

    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err("补丁不能修改工作区外的绝对路径。".to_string());
    }

    let sanitized = sanitize_relative_path(Path::new(trimmed))?;
    if sanitized == "." {
        return Err("补丁路径无效。".to_string());
    }

    Ok(Some(sanitized))
}

fn normalize_patch_text(patch: &str) -> String {
    let normalized = patch.trim();
    if normalized.is_empty() {
        return String::new();
    }

    let mut normalized = normalized.to_string();
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }

    normalized
}

fn run_git_apply(workspace: &Path, patch: &str, check_only: bool, reverse: bool) -> CmdResult {
    let start = Instant::now();
    let mut command = Command::new("git");
    command.arg("-C");
    command.arg(workspace);
    command.arg("apply");
    if check_only {
        command.arg("--check");
    }
    if reverse {
        command.arg("--reverse");
    }
    command.arg("--whitespace=nowarn");
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    match command.spawn() {
        Ok(mut child) => {
            if let Some(stdin) = child.stdin.as_mut() {
                if let Err(err) = stdin.write_all(patch.as_bytes()) {
                    return CmdResult {
                        success: false,
                        stdout: String::new(),
                        stderr: format!("写入补丁失败: {err}"),
                        exit_code: None,
                        duration_ms: start.elapsed().as_millis() as u64,
                    };
                }
            }

            match child.wait_with_output() {
                Ok(output) => CmdResult {
                    success: output.status.success(),
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code(),
                    duration_ms: start.elapsed().as_millis() as u64,
                },
                Err(err) => CmdResult {
                    success: false,
                    stdout: String::new(),
                    stderr: format!("执行 git apply 失败: {err}"),
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                },
            }
        }
        Err(err) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr: format!("启动 git apply 失败: {err}"),
            exit_code: None,
            duration_ms: start.elapsed().as_millis() as u64,
        },
    }
}

fn into_cmd_result(result: Result<String, String>, duration_ms: u64) -> CmdResult {
    match result {
        Ok(stdout) => CmdResult {
            success: true,
            stdout,
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms,
        },
        Err(stderr) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr,
            exit_code: Some(1),
            duration_ms,
        },
    }
}

/// Escape special regex characters for use in rg patterns.
fn regex_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(c, '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

fn summarize_test_output(command: &str, stdout: &str, stderr: &str, success: bool) -> String {
    let mut lines = Vec::new();
    lines.push(format!("命令: {}", command));
    lines.push(format!("状态: {}", if success { "通过" } else { "失败" }));

    let combined = format!("{}\n{}", stdout, stderr);
    let summary = if success {
        collect_matching_lines(&combined, &["test result:", "running "])
    } else {
        let important = collect_matching_lines(
            &combined,
            &[
                "error:",
                "failures:",
                "test result:",
                "failed",
                "panicked at",
                "---- ",
            ],
        );
        if important.is_empty() {
            tail_lines(&combined, 40)
        } else {
            important
        }
    };

    if !summary.is_empty() {
        lines.push(String::new());
        lines.extend(summary);
    }

    lines.join("\n")
}

fn collect_matching_lines(text: &str, needles: &[&str]) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .filter(|line| needles.iter().any(|needle| line.contains(needle)))
        .take(40)
        .map(ToOwned::to_owned)
        .collect()
}

fn tail_lines(text: &str, count: usize) -> Vec<String> {
    let lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let start = lines.len().saturating_sub(count);
    lines[start..].iter().map(|line| (*line).to_string()).collect()
}

fn run_git(repo_path: Option<&str>, git_args: &[&str], timeout_secs: u64) -> CmdResult {
    let start = Instant::now();
    let mut command = Command::new("git");

    if let Some(path) = repo_path {
        command.args(["-C", path]);
    }
    command.args(git_args);

    let result = command.output();
    let duration_ms = start.elapsed().as_millis() as u64;
    let _ = timeout_secs;

    match result {
        Ok(output) => CmdResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
            duration_ms,
        },
        Err(error) => CmdResult {
            success: false,
            stdout: String::new(),
            stderr: format!("执行失败: {error}"),
            exit_code: None,
            duration_ms,
        },
    }
}

fn with_step_label(step: &str, result: CmdResult) -> CmdResult {
    CmdResult {
        success: result.success,
        stdout: prefix_output(step, &result.stdout),
        stderr: prefix_output(step, &result.stderr),
        exit_code: result.exit_code,
        duration_ms: result.duration_ms,
    }
}

fn combine_results(results: &[(&str, CmdResult)]) -> CmdResult {
    let success = results.iter().all(|(_, result)| result.success);
    let stdout = results
        .iter()
        .map(|(step, result)| prefix_output(step, &result.stdout))
        .filter(|output| !output.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let stderr = results
        .iter()
        .map(|(step, result)| prefix_output(step, &result.stderr))
        .filter(|output| !output.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let exit_code = results.last().and_then(|(_, result)| result.exit_code);
    let duration_ms = results.iter().map(|(_, result)| result.duration_ms).sum();

    CmdResult {
        success,
        stdout,
        stderr,
        exit_code,
        duration_ms,
    }
}

fn prefix_output(step: &str, output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("[{step}]\n{trimmed}")
    }
}

fn is_nothing_to_commit(result: &CmdResult) -> bool {
    let combined = format!("{}\n{}", result.stdout, result.stderr).to_lowercase();
    combined.contains("nothing to commit")
        || combined.contains("working tree clean")
        || combined.contains("nothing added to commit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "feishu-vscode-bridge-vscode-tests-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn resolve_repo_path_prefers_explicit_value() {
        let _guard = env_lock().lock().unwrap();

        unsafe {
            std::env::set_var(WORKSPACE_PATH_ENV, "/tmp/workspace-from-env");
        }

        assert_eq!(
            resolve_repo_path(Some("/tmp/explicit-repo")),
            Some("/tmp/explicit-repo".to_string())
        );

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }
    }

    #[test]
    fn resolve_repo_path_uses_workspace_env_when_repo_missing() {
        let _guard = env_lock().lock().unwrap();

        unsafe {
            std::env::set_var(WORKSPACE_PATH_ENV, "/tmp/workspace-from-env");
        }

        assert_eq!(
            resolve_repo_path(None),
            Some("/tmp/workspace-from-env".to_string())
        );

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }
    }

    #[test]
    fn resolve_repo_path_ignores_blank_values() {
        let _guard = env_lock().lock().unwrap();

        unsafe {
            std::env::set_var(WORKSPACE_PATH_ENV, "   ");
        }

        assert_eq!(resolve_repo_path(None), None);

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }
    }

    #[test]
    fn detect_nothing_to_commit_from_stdout() {
        let result = CmdResult {
            success: false,
            stdout: "On branch main\nnothing to commit, working tree clean\n".to_string(),
            stderr: String::new(),
            exit_code: Some(1),
            duration_ms: 10,
        };

        assert!(is_nothing_to_commit(&result));
    }

    #[test]
    fn ignore_unrelated_git_failures() {
        let result = CmdResult {
            success: false,
            stdout: String::new(),
            stderr: "fatal: not a git repository".to_string(),
            exit_code: Some(128),
            duration_ms: 10,
        };

        assert!(!is_nothing_to_commit(&result));
    }

    #[test]
    fn resolve_workspace_target_uses_workspace_root_for_relative_path() {
        let _guard = env_lock().lock().unwrap();

        unsafe {
            std::env::set_var(WORKSPACE_PATH_ENV, "/tmp/workspace-root");
        }

        let resolved = resolve_workspace_target("src/lib.rs").unwrap();
        assert_eq!(resolved, PathBuf::from("/tmp/workspace-root").join("src/lib.rs"));

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }
    }

    #[test]
    fn read_file_rejects_invalid_range() {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/vscode.rs");
        let result = read_file(file.to_string_lossy().as_ref(), Some(10), Some(1));
        assert!(!result.success);
        assert!(result.stderr.contains("行号范围无效"));
    }

    #[test]
    fn fallback_text_search_finds_plain_matches_without_rg() {
        let workspace = unique_temp_dir("fallback-search-text");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "fn parse_intent() {}\nfn other() {}\n",
        )
        .unwrap();

        let result = fallback_text_search("parse_intent", &workspace, false).unwrap();

        assert!(result.contains("parse_intent"));
        assert!(result.contains("内置搜索"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_symbol_search_finds_function_definition_without_rg() {
        let workspace = unique_temp_dir("fallback-search-symbol");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "pub fn parse_intent() {}\nstruct Bridge {}\n",
        )
        .unwrap();

        let result = fallback_symbol_search(
            "parse_intent",
            &workspace,
            SearchOptions {
                exclude_test_paths: false,
                exclude_inline_rust_tests: false,
                exclude_runtime_artifacts: false,
            },
        )
        .unwrap();

        assert!(result.contains("parse_intent"));
        assert!(result.contains("pub fn parse_intent"));
        assert!(result.contains("命中: 1 个文件，1 处匹配"));
        assert!(result.contains("src/lib.rs"));
        assert!(result.contains("内置搜索"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_symbol_search_ignores_string_literal_false_positive() {
        let workspace = unique_temp_dir("fallback-search-symbol-literal");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "fn real_symbol() {}\nassert!(text.contains(\"fn fake_symbol() {}\"));\n",
        )
        .unwrap();

        let result = fallback_symbol_search(
            "fake_symbol",
            &workspace,
            SearchOptions {
                exclude_test_paths: false,
                exclude_inline_rust_tests: false,
                exclude_runtime_artifacts: false,
            },
        )
        .unwrap();

        assert!(result.contains("未找到匹配的符号定义"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_reference_search_finds_symbol_occurrences_without_rg() {
        let workspace = unique_temp_dir("fallback-reference-search");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "pub fn parse_intent() {}\nfn call() { parse_intent(); }\n",
        )
        .unwrap();

        let result = fallback_reference_search(
            "parse_intent",
            &workspace,
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("parse_intent"));
        assert!(result.contains("fn call() { parse_intent(); }"));
        assert!(!result.contains("pub fn parse_intent() {}"));
        assert!(result.contains("命中: 1 个文件，1 处匹配"));
        assert!(result.contains("内置搜索"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_implementation_search_finds_impl_blocks_without_rg() {
        let workspace = unique_temp_dir("fallback-implementation-search");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "struct Bridge;\nimpl Bridge { fn new() -> Self { Self } }\n",
        )
        .unwrap();

        let result = fallback_implementation_search(
            "Bridge",
            &workspace,
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("impl Bridge"));
        assert!(result.contains("命中: 1 个文件，1 处匹配"));
        assert!(result.contains("内置搜索"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_implementation_search_ignores_string_literal_false_positive() {
        let workspace = unique_temp_dir("fallback-implementation-literal");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "assert!(result.contains(\"impl Bridge\"));\n",
        )
        .unwrap();

        let result = fallback_implementation_search(
            "Bridge",
            &workspace,
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("未找到匹配实现"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_reference_search_excludes_test_directories_by_default() {
        let workspace = unique_temp_dir("fallback-reference-excludes-tests");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::create_dir_all(workspace.join("tests")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "fn call() { parse_intent(); }\n",
        )
        .unwrap();
        fs::write(
            workspace.join("tests/reference_test.rs"),
            "#[test]\nfn check() { parse_intent(); }\n",
        )
        .unwrap();

        let result = fallback_reference_search(
            "parse_intent",
            &workspace,
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("src/lib.rs"));
        assert!(!result.contains("tests/reference_test.rs"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_reference_search_keeps_explicit_test_scope() {
        let workspace = unique_temp_dir("fallback-reference-keep-tests");
        fs::create_dir_all(workspace.join("tests")).unwrap();
        fs::write(
            workspace.join("tests/reference_test.rs"),
            "#[test]\nfn check() { parse_intent(); }\n",
        )
        .unwrap();

        let result = fallback_reference_search(
            "parse_intent",
            &workspace.join("tests"),
            SearchOptions {
                exclude_test_paths: false,
                exclude_inline_rust_tests: false,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("reference_test.rs"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn search_options_detect_explicit_test_scope() {
        assert!(SearchOptions::from_explicit_path(None).exclude_test_paths);
        assert!(SearchOptions::from_explicit_path(None).exclude_inline_rust_tests);
        assert!(SearchOptions::from_explicit_path(None).exclude_runtime_artifacts);
        assert!(SearchOptions::from_explicit_path(Some("src")).exclude_test_paths);
        assert!(SearchOptions::from_explicit_path(Some("src")).exclude_inline_rust_tests);
        assert!(SearchOptions::from_explicit_path(Some("src")).exclude_runtime_artifacts);
        assert!(!SearchOptions::from_explicit_path(Some("tests")).exclude_test_paths);
        assert!(!SearchOptions::from_explicit_path(Some("tests")).exclude_inline_rust_tests);
        assert!(SearchOptions::from_explicit_path(Some("tests")).exclude_runtime_artifacts);
        assert!(!SearchOptions::from_explicit_path(Some("src/__tests__")).exclude_test_paths);
        assert!(!SearchOptions::from_explicit_path(Some("src/__tests__")).exclude_inline_rust_tests);
        assert!(SearchOptions::from_explicit_path(Some("src/__tests__")).exclude_runtime_artifacts);
        assert!(!SearchOptions::from_explicit_path(Some(".feishu-vscode-bridge-audit.jsonl")).exclude_runtime_artifacts);
    }

    #[test]
    fn fallback_reference_search_excludes_runtime_artifacts_by_default() {
        let workspace = unique_temp_dir("fallback-reference-runtime-artifacts");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "fn call() { parse_intent(); }\n",
        )
        .unwrap();
        fs::write(
            workspace.join(".feishu-vscode-bridge-audit.jsonl"),
            "{\"command\":\"搜索符号 parse_intent 在 src\"}\n",
        )
        .unwrap();

        let result = fallback_reference_search(
            "parse_intent",
            &workspace,
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("src/lib.rs"));
        assert!(!result.contains(".feishu-vscode-bridge-audit.jsonl"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_reference_search_keeps_explicit_runtime_artifact_scope() {
        let workspace = unique_temp_dir("fallback-reference-runtime-explicit");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            workspace.join(".feishu-vscode-bridge-audit.jsonl"),
            "{\"command\":\"搜索符号 parse_intent 在 src\"}\n",
        )
        .unwrap();

        let result = fallback_reference_search(
            "parse_intent",
            &workspace.join(".feishu-vscode-bridge-audit.jsonl"),
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: false,
            },
        )
        .unwrap();

        assert!(result.contains(".feishu-vscode-bridge-audit.jsonl"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_reference_search_excludes_inline_rust_test_module_by_default() {
        let workspace = unique_temp_dir("fallback-reference-inline-tests");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "fn parse_intent() {}\nfn call() { parse_intent(); }\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn it_works() { parse_intent(); }\n}\n",
        )
        .unwrap();

        let result = fallback_reference_search(
            "parse_intent",
            &workspace,
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("fn call() { parse_intent(); }"));
        assert!(!result.contains("fn it_works() { parse_intent(); }"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn fallback_implementation_search_excludes_inline_rust_test_module_by_default() {
        let workspace = unique_temp_dir("fallback-implementation-inline-tests");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("src/lib.rs"),
            "struct Bridge;\n#[cfg(test)]\nmod tests {\n    use super::*;\n    impl Bridge { fn test_only() -> Self { Bridge } }\n}\n",
        )
        .unwrap();

        let result = fallback_implementation_search(
            "Bridge",
            &workspace,
            SearchOptions {
                exclude_test_paths: true,
                exclude_inline_rust_tests: true,
                exclude_runtime_artifacts: true,
            },
        )
        .unwrap();

        assert!(result.contains("未找到匹配实现"));

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn format_grouped_search_reply_limits_matches_per_file() {
        let matches = (1..=12)
            .map(|line_number| SearchMatch {
                path: PathBuf::from("src/lib.rs"),
                line_number,
                line_text: format!("match_{line_number}"),
            })
            .collect::<Vec<_>>();

        let result = format_grouped_search_reply(
            "引用",
            "parse_intent",
            Path::new("src"),
            &matches,
            false,
            None,
        );

        assert!(result.contains("命中: 1 个文件，12 处匹配"));
        assert!(result.contains("  10: match_10"));
        assert!(!result.contains("  11: match_11"));
        assert!(result.contains("  … 另有 2 处匹配未显示"));
        assert!(result.contains("每个文件最多显示前 10 处匹配"));
    }

    #[test]
    fn parse_rg_match_line_supports_windows_paths() {
        let parsed = parse_rg_match_line(r"C:\repo\src\lib.rs:42:fn parse_intent() {}").unwrap();

        assert_eq!(parsed.path, PathBuf::from(r"C:\repo\src\lib.rs"));
        assert_eq!(parsed.line_number, 42);
        assert_eq!(parsed.line_text, "fn parse_intent() {}");
    }

    #[test]
    fn build_test_file_command_maps_rust_integration_test() {
        let workspace = unique_temp_dir("test-file-command");
        fs::create_dir_all(workspace.join("tests")).unwrap();
        fs::write(workspace.join("Cargo.toml"), "[package]\nname='demo'\nversion='0.1.0'\n").unwrap();
        let test_file = workspace.join("tests/approval_card_flow.rs");
        fs::write(&test_file, "#[test]\nfn demo() {}\n").unwrap();

        let command = build_test_file_command(&workspace, &test_file).unwrap();
        assert_eq!(command, "cargo test --test approval_card_flow");

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn should_isolate_rust_test_target_dir_for_cargo_test_commands() {
        let workspace = unique_temp_dir("test-target-dir");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("Cargo.toml"), "[package]\nname='demo'\nversion='0.1.0'\n").unwrap();

        assert!(should_isolate_rust_test_target_dir(&workspace, "cargo test --test approval_card_flow"));
        assert_eq!(
            isolated_rust_test_target_dir(&workspace),
            workspace.join("target").join("bridge-test-runner")
        );

        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn run_shell_uses_workspace_env_as_cwd() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let workspace = unique_temp_dir("run-shell-cwd");
        fs::create_dir_all(&workspace).unwrap();

        unsafe {
            std::env::set_var(WORKSPACE_PATH_ENV, &workspace);
        }

        #[cfg(target_os = "windows")]
        let result = run_shell("cd");

        #[cfg(not(target_os = "windows"))]
        let result = run_shell("pwd");

        assert!(result.success, "{}", result.to_reply("run_shell"));
        let reported = fs::canonicalize(result.stdout.trim()).unwrap();
        let expected = fs::canonicalize(&workspace).unwrap();
        assert_eq!(reported, expected);

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }

        let _ = fs::remove_dir_all(&workspace);
    }

    #[test]
    fn default_test_command_reads_env() {
        let _guard = env_lock().lock().unwrap();

        unsafe {
            std::env::set_var(TEST_COMMAND_ENV, "cargo test --lib");
        }

        assert_eq!(default_test_command(), Some("cargo test --lib".to_string()));

        unsafe {
            std::env::remove_var(TEST_COMMAND_ENV);
        }
    }

    #[test]
    fn summarize_test_output_includes_status_and_summary() {
        let summary = summarize_test_output(
            "cargo test",
            "running 2 tests\ntest result: ok. 2 passed; 0 failed;\n",
            "",
            true,
        );

        assert!(summary.contains("命令: cargo test"));
        assert!(summary.contains("状态: 通过"));
        assert!(summary.contains("test result: ok"));
    }

    #[test]
    fn normalize_pathspec_rejects_parent_dir() {
        let workspace = PathBuf::from("/tmp/workspace-root");
        let result = normalize_pathspec(&workspace, "../secret.txt");
        assert!(result.is_err());
    }

    #[test]
    fn validate_patch_paths_rejects_absolute_path() {
        let patch = "--- /tmp/x\n+++ /tmp/x\n@@ -1 +1 @@\n-a\n+b\n";
        let result = validate_patch_paths(patch);
        assert!(result.is_err());
    }

    #[test]
    fn validate_patch_paths_accepts_git_style_paths() {
        let patch = "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(validate_patch_paths(patch).is_ok());
    }

    #[test]
    fn validate_patch_paths_accepts_timestamped_headers() {
        let patch = "--- a/src/lib.rs  2026-03-29 11:45:16\n+++ b/src/lib.rs  2026-03-29 11:45:51\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(validate_patch_paths(patch).is_ok());
    }

    #[test]
    fn extract_primary_patch_path_returns_last_modified_file() {
        let patch = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/src/bridge.rs b/src/bridge.rs\n--- a/src/bridge.rs\n+++ b/src/bridge.rs\n@@ -1 +1 @@\n-a\n+b\n";

        assert_eq!(extract_primary_patch_path(patch), Some("src/bridge.rs".to_string()));
    }

    #[test]
    fn extract_patch_paths_returns_recent_files_in_order() {
        let patch = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/src/bridge.rs b/src/bridge.rs\n--- a/src/bridge.rs\n+++ b/src/bridge.rs\n@@ -1 +1 @@\n-a\n+b\n";

        assert_eq!(
            extract_patch_paths(patch),
            vec!["src/bridge.rs".to_string(), "src/lib.rs".to_string()]
        );
    }

    #[test]
    fn extract_primary_patch_path_handles_deleted_file() {
        let patch = "diff --git a/src/old.rs b/src/old.rs\n--- a/src/old.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-old\n";

        assert_eq!(extract_primary_patch_path(patch), Some("src/old.rs".to_string()));
    }

    #[test]
    fn normalize_patch_text_appends_trailing_newline() {
        let patch = "diff --git a/demo.txt b/demo.txt\n--- a/demo.txt\n+++ b/demo.txt\n@@ -1 +1,2 @@\n a\n+b";
        let normalized = normalize_patch_text(patch);

        assert!(normalized.ends_with('\n'));
        assert_eq!(normalized.lines().last(), Some("+b"));
    }

    #[test]
    fn reverse_patch_reverts_previous_apply_patch() {
        let _guard = env_lock().lock().unwrap();
        let workspace = unique_temp_dir("reverse-patch");
        fs::create_dir_all(&workspace).unwrap();
        let file_path = workspace.join("demo.txt");
        fs::write(&file_path, "old\n").unwrap();

        let init = Command::new("git")
            .arg("init")
            .current_dir(&workspace)
            .output()
            .unwrap();
        assert!(init.status.success());

        let patch = "diff --git a/demo.txt b/demo.txt\n--- a/demo.txt\n+++ b/demo.txt\n@@ -1 +1 @@\n-old\n+new\n";

        unsafe {
            std::env::set_var(WORKSPACE_PATH_ENV, &workspace);
        }

        let apply = apply_patch(patch);
        assert!(apply.success, "{}", apply.to_reply("apply patch"));
        assert_eq!(fs::read_to_string(&file_path).unwrap().replace("\r\n", "\n"), "new\n");

        let reverse = reverse_patch(patch);
        assert!(reverse.success, "{}", reverse.to_reply("reverse patch"));
        assert_eq!(fs::read_to_string(&file_path).unwrap().replace("\r\n", "\n"), "old\n");

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }
        let _ = fs::remove_dir_all(workspace);
    }
}
