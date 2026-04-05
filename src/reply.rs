use crate::session::StoredSession;
use crate::vscode;
use crate::Intent;

pub fn format_agent_status(status: &str) -> String {
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

pub fn truncate_session_text(text: &str, max_chars: usize) -> String {
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        truncated.push_str("\n… (内容过长已截断)");
    }
    truncated
}

pub fn summarize_reply_snippet(reply: &str, max_lines: usize, max_chars: usize) -> Option<String> {
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

pub fn agent_result_summary(result: &vscode::AgentAskResult) -> String {
    result
        .summary
        .clone()
        .or_else(|| summarize_reply_snippet(&result.message, 3, 220))
        .unwrap_or_else(|| "agent 已返回结果。".to_string())
}

pub fn format_agent_reply(task_text: &str, result: &vscode::AgentAskResult) -> String {
    format_agent_reply_with_action(task_text, "问 Copilot", result)
}

pub fn format_agent_reply_with_action(
    task_text: &str,
    action_label: &str,
    result: &vscode::AgentAskResult,
) -> String {
    let mut blocks = vec!["🧭 Agent 任务更新".to_string()];

    if let Some(session_id) = result.session_id.as_deref().filter(|value| !value.trim().is_empty()) {
        blocks.push(format!("🆔 session: {}", session_id));
    }

    blocks.push(format!("🎯 当前任务: {}", task_text.trim().trim_end_matches('\n')));
    blocks.push(format!("📌 最近状态: {}", format_agent_status(&result.status)));
    blocks.push(format!("🧾 上次动作: {}  ({}ms)", action_label, result.duration_ms));

    if let Some(action) = result.current_action.as_deref().filter(|value| !value.trim().is_empty()) {
        blocks.push(format!("⚙️ 当前动作: {}", action.trim()));
    }

    let summary = agent_result_summary(result);
    if !summary.trim().is_empty() {
        blocks.push(format!("📌 结果摘要: {}", summary));
    }

    if !result.related_files.is_empty() {
        blocks.push(format!("📄 相关文件: {}", result.related_files.join("、")));
    }

    if let Some(tool_call) = result.tool_call.as_deref().filter(|value| !value.trim().is_empty()) {
        blocks.push(format!("🛠 工具动作: {}", tool_call.trim()));
    }

    if let Some(tool_result_summary) = result
        .tool_result_summary
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        blocks.push(format!("🔎 工具结果: {}", tool_result_summary.trim()));
    }

    if let Some(next_action) = result.next_action.as_deref().filter(|value| !value.trim().is_empty()) {
        blocks.push(format!("➡️ 下一步建议: {}", next_action.trim()));
        blocks.push("🤝 采纳这一步可直接发送：按建议继续".to_string());
    }

    if let Some(error) = result.error.as_deref().filter(|value| !value.trim().is_empty()) {
        blocks.push(format!("❌ 错误: {}", error.trim()));
    }

    blocks.push(format!("💬 Agent 回复:\n{}", result.message.trim()));
    blocks.join("\n\n")
}

pub fn format_stored_session_summary(stored: &StoredSession) -> String {
    let Some(result) = stored.last_result.as_ref() else {
        return "⚠️ 当前没有待继续的计划。".to_string();
    };

    let mut blocks = vec![format!("📌 任务结果: {}", result.summary)];

    if let Some(last_step) = stored.last_step.as_ref() {
        blocks.push(format!("🧾 最近一步: {}", last_step.description));
        if let Some(snippet) = summarize_reply_snippet(&last_step.reply, 2, 180) {
            blocks.push(format!("🔎 最近结果摘要: {}", snippet));
        }
    }

    if let Some(path) = stored
        .recent_file_paths
        .first()
        .map(String::as_str)
        .or(stored.last_file_path.as_deref())
    {
        blocks.push(format!("📄 当前聚焦文件: {}", path));
    }

    if stored.recent_file_paths.len() > 1 {
        blocks.push(format!("🗂 最近文件队列: {}", stored.recent_file_paths.join("、")));
    }

    if let Some(last_diff) = stored.last_diff.as_ref() {
        blocks.push(format!("🧩 最近 diff: {}", last_diff.description));
        if let Some(snippet) = summarize_reply_snippet(&last_diff.content, 3, 220) {
            blocks.push(format!("🔎 diff 摘要: {}", snippet));
        }
    }

    if let Some(next_step) = stored.pending_steps.first() {
        blocks.push(format!("⏭ 下一步: {}", next_step));
        if stored.pending_steps.len() > 1 {
            blocks.push(format!(
                "📦 后续步骤: {}",
                stored
                    .pending_steps
                    .iter()
                    .skip(1)
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("；")
            ));
        }
    }

    blocks.push(format!("➡️ 下一步建议: {}", continuation_next_step_hint(stored)));

    format_follow_up_reply("任务连续性回放", stored, blocks)
}

pub fn format_last_failure_reply(stored: &StoredSession) -> String {
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

pub fn format_last_result_reply(stored: &StoredSession) -> String {
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

pub fn format_last_diff_reply(stored: &StoredSession) -> String {
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

pub fn format_recent_files_reply(stored: &StoredSession) -> String {
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

pub fn format_follow_up_reply(title: &str, stored: &StoredSession, detail_blocks: Vec<String>) -> String {
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

    blocks.extend(detail_blocks.into_iter().filter(|block| !block.trim().is_empty()));

    blocks.join("\n\n")
}

pub fn describe_intent(intent: &Intent) -> String {
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
        Intent::ReadFile { path, start_line, end_line } => match (start_line, end_line) {
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
        Intent::ContinueAgent { prompt } => match prompt {
            Some(prompt) if !prompt.trim().is_empty() => format!("继续 Agent 任务：{}", prompt.trim()),
            _ => "继续 Agent 任务".to_string(),
        },
        Intent::ContinueAgentSuggested => "按建议继续 Agent 任务".to_string(),
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

fn continuation_next_step_hint(stored: &StoredSession) -> String {
    if let Some(step) = stored.pending_steps.first() {
        return format!("当前最直接的下一步是「{}」。", step);
    }

    if let Some(path) = stored
        .recent_file_paths
        .first()
        .map(String::as_str)
        .or(stored.last_file_path.as_deref())
    {
        if stored.last_diff.is_some() {
            return format!("可以先回到 {}，结合最近 diff 继续判断是否还要改动。", path);
        }

        return format!("可以先回到 {}，继续围绕这个文件追问或修改。", path);
    }

    if stored.last_step.is_some() {
        return "可以先追问上一步结果，再决定是否继续执行新命令。".to_string();
    }

    "可以直接发下一条开发指令，或先追问最近结果和文件上下文。".to_string()
}