use std::collections::HashMap;
use std::path::PathBuf;

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

        if self.approval_policy.requires_approval(&intent) {
            return self.start_plan(session_key, vec![intent], ExecutionMode::StepByStep);
        }

        match intent {
            Intent::RunPlan { steps, mode } => self.start_plan(session_key, steps, mode),
            Intent::ContinuePlan => self.resume_plan(session_key, false),
            Intent::RetryFailedStep => self.resume_plan(session_key, false),
            Intent::ExecuteAll => self.resume_plan(session_key, true),
            Intent::ApprovePending => self.approve_plan(session_key),
            Intent::RejectPending => self.reject_plan(session_key),

            Intent::OpenFile { path, line } => {
                let r = vscode::open_file(&path, line);
                BridgeResponse::Text(r.to_reply(&format!("打开 {path}")))
            }
            Intent::OpenFolder { path } => {
                let r = vscode::open_folder(&path);
                BridgeResponse::Text(r.to_reply(&format!("打开目录 {path}")))
            }
            Intent::InstallExtension { ext_id } => {
                let r = vscode::install_extension(&ext_id);
                BridgeResponse::Text(r.to_reply(&format!("安装扩展 {ext_id}")))
            }
            Intent::UninstallExtension { ext_id } => {
                let r = vscode::uninstall_extension(&ext_id);
                BridgeResponse::Text(r.to_reply(&format!("卸载扩展 {ext_id}")))
            }
            Intent::ListExtensions => {
                let r = vscode::list_extensions();
                BridgeResponse::Text(r.to_reply("已安装扩展"))
            }
            Intent::DiffFiles { file1, file2 } => {
                let r = vscode::diff_files(&file1, &file2);
                BridgeResponse::Text(r.to_reply(&format!("diff {file1} {file2}")))
            }
            Intent::GitStatus { repo } => {
                let r = vscode::git_status(repo.as_deref());
                BridgeResponse::Text(r.to_reply("Git 状态"))
            }
            Intent::GitPull { repo } => {
                let r = vscode::git_pull(repo.as_deref());
                BridgeResponse::Text(r.to_reply("Git Pull"))
            }
            Intent::GitPushAll { repo, message } => {
                let r = vscode::git_push_all(repo.as_deref(), &message);
                BridgeResponse::Text(r.to_reply("Git Push"))
            }
            Intent::RunShell { cmd } => {
                let r = vscode::run_shell(&cmd);
                BridgeResponse::Text(r.to_reply(&format!("$ {cmd}")))
            }
            Intent::Help => BridgeResponse::Text(help_text().to_string()),
            Intent::Unknown(raw) => {
                BridgeResponse::Text(format!("❓ 无法识别指令: {raw}\n\n发送「帮助」查看可用命令"))
            }
        }
    }

    pub fn approval_policy(&self) -> &ApprovalPolicy {
        &self.approval_policy
    }

    fn start_plan(&self, session_key: &str, steps: Vec<Intent>, mode: ExecutionMode) -> BridgeResponse {
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
        let reply = format_plan_reply(
            &progress,
            matches!(mode, ExecutionMode::ContinueAll),
            &self.approval_policy,
        );

        if progress.completed {
            let _ = self.clear_persisted_plan(session_key);
        } else {
            let _ = self.persist_plan(session_key, &session);
        }

        reply
    }

    fn resume_plan(&self, session_key: &str, run_all: bool) -> BridgeResponse {
        let Some(mut session) = self.load_persisted_plan(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待继续的计划。\n\n发送「执行计划 <命令1>; <命令2>」创建逐步计划，或发送「执行全部 <命令1>; <命令2>」连续执行。".to_string());
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
        let reply = format_plan_reply(&progress, run_all, &self.approval_policy);

        if progress.completed {
            let _ = self.clear_persisted_plan(session_key);
        } else {
            let _ = self.persist_plan(session_key, &session);
        }

        reply
    }

    fn approve_plan(&self, session_key: &str) -> BridgeResponse {
        let Some(mut session) = self.load_persisted_plan(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
        };

        if !session.has_pending_approval() {
            return BridgeResponse::Text("⚠️ 当前没有待审批步骤。可以发送「继续」或「执行全部」推进计划。".to_string());
        }

        let progress = session.approve_pending_with_policy(self.executor, |intent| {
            self.approval_policy.requires_approval(intent)
        });
        let reply = format_plan_reply(&progress, false, &self.approval_policy);

        if progress.completed {
            let _ = self.clear_persisted_plan(session_key);
        } else {
            let _ = self.persist_plan(session_key, &session);
        }

        reply
    }

    fn reject_plan(&self, session_key: &str) -> BridgeResponse {
        let Some(mut session) = self.load_persisted_plan(session_key) else {
            return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
        };

        if !session.reject_pending() {
            return BridgeResponse::Text("⚠️ 当前没有待审批步骤。".to_string());
        }

        let _ = self.clear_persisted_plan(session_key);
        BridgeResponse::Text("🛑 已拒绝当前待审批步骤，当前计划已取消。".to_string())
    }

    fn load_session_store(&self) -> HashMap<String, PlanSession> {
        let Some(path) = self.session_store_path.as_ref() else {
            return HashMap::new();
        };

        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    fn save_session_store(&self, store: &HashMap<String, PlanSession>) -> Result<(), String> {
        let Some(path) = self.session_store_path.as_ref() else {
            return Err("无法定位会话存储目录".to_string());
        };

        let content = serde_json::to_string_pretty(store)
            .map_err(|err| format!("序列化计划会话失败: {err}"))?;
        std::fs::write(path, content).map_err(|err| format!("写入计划会话失败: {err}"))
    }

    fn load_persisted_plan(&self, session_key: &str) -> Option<PlanSession> {
        let store = self.load_session_store();
        store.get(session_key).cloned()
    }

    fn persist_plan(&self, session_key: &str, session: &PlanSession) -> Result<(), String> {
        let mut store = self.load_session_store();
        store.insert(session_key.to_string(), session.clone());
        self.save_session_store(&store)
    }

    fn clear_persisted_plan(&self, session_key: &str) -> Result<(), String> {
        let mut store = self.load_session_store();
        store.remove(session_key);

        if store.is_empty() {
            if let Some(path) = self.session_store_path.as_ref() {
                match std::fs::remove_file(path) {
                    Ok(()) => return Ok(()),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                    Err(err) => return Err(format!("删除计划会话失败: {err}")),
                }
            }
            return Ok(());
        }

        self.save_session_store(&store)
    }
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
) -> BridgeResponse {
    if progress.executed.is_empty() && progress.completed {
        return BridgeResponse::Card {
            fallback_text: "✅ 当前计划已经执行完成，没有剩余步骤。".to_string(),
            card: build_plan_card(
                progress,
                auto_run,
                approval_policy,
                "✅ 当前计划已经执行完成，没有剩余步骤。",
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
            card: build_plan_card(progress, auto_run, approval_policy, &text),
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
            card: build_plan_card(progress, auto_run, approval_policy, &text),
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
            card: build_plan_card(progress, auto_run, approval_policy, &text),
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
        card: build_plan_card(progress, auto_run, approval_policy, &text),
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
) -> serde_json::Value {
    let status_label = plan_status_label(progress);
    let (template, title, subtitle) = if progress.completed {
        ("green", "计划已完成", "所有步骤已执行完成。")
    } else if progress.paused_on_approval {
        ("orange", "等待批准", "当前步骤需要明确批准后才会执行。")
    } else if progress.paused_on_failure {
        ("orange", "计划已暂停", "当前步骤执行失败，可重试当前步骤或继续连续执行。")
    } else if auto_run {
        ("blue", "计划待继续", "已按连续执行模式运行，当前仍有剩余步骤。")
    } else {
        ("blue", "计划待继续", "当前已执行到一个安全暂停点，可继续下一步。")
    };

    let current_step = if progress.completed {
        "无".to_string()
    } else {
        format!("第 {} / {} 步", progress.next_step + 1, progress.total_steps)
    };
    let remaining_steps = progress.total_steps.saturating_sub(progress.next_step);

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

    if !progress.completed {
        let actions = if progress.paused_on_approval {
            vec![
                json!({
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "批准"
                    },
                    "value": {
                        "command": "批准"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "拒绝"
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
                        "content": "重新执行失败步骤"
                    },
                    "value": {
                        "command": "重新执行失败步骤"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "执行全部"
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
                        "content": "继续"
                    },
                    "value": {
                        "command": "继续"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "执行全部"
                    },
                    "value": {
                        "command": "执行全部"
                    }
                }),
            ]
        };

        elements.push(json!({
            "tag": "action",
            "actions": actions
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
        Intent::Help => "查看帮助".to_string(),
        Intent::Unknown(raw) => format!("未识别命令 {raw}"),
    }
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
        | Intent::RejectPending => ExecutionOutcome {
            success: false,
            reply: "⚠️ 当前步骤不是可直接执行的底层命令。".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        match format_plan_reply(&progress, false, &ApprovalPolicy::default()) {
            BridgeResponse::Card { fallback_text, card } => {
                assert!(fallback_text.contains("状态: 已完成"));
                assert_eq!(card["header"]["title"]["content"], "计划已完成");
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

        match format_plan_reply(&progress, true, &ApprovalPolicy::default()) {
            BridgeResponse::Card { fallback_text, card } => {
                assert!(fallback_text.contains("失败步骤: 第 2 / 3 步"));
                assert_eq!(card["header"]["title"]["content"], "计划已暂停");
                assert!(card.to_string().contains("失败步骤"));
                assert!(card.to_string().contains("重新执行失败步骤"));
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

        match format_plan_reply(&progress, false, &ApprovalPolicy::default()) {
            BridgeResponse::Card { fallback_text, card } => {
                assert!(fallback_text.contains("待审批步骤"));
                assert_eq!(card["header"]["title"]["content"], "等待批准");
                assert!(card.to_string().contains("批准"));
                assert!(card.to_string().contains("拒绝"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }
}