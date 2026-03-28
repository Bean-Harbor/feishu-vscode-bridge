use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPolicy {
    pub require_shell: bool,
    pub require_git_push: bool,
    pub require_git_pull: bool,
    pub require_extension_install: bool,
    pub require_extension_uninstall: bool,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            require_shell: true,
            require_git_push: true,
            require_git_pull: false,
            require_extension_install: false,
            require_extension_uninstall: false,
        }
    }
}

impl ApprovalPolicy {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        match std::env::var("BRIDGE_APPROVAL_REQUIRED") {
            Ok(value) => Self::from_spec(&value),
            Err(_) => Self::default(),
        }
    }

    pub fn from_spec(spec: &str) -> Self {
        let normalized = spec.trim();
        if normalized.is_empty() || normalized.eq_ignore_ascii_case("default") {
            return Self::default();
        }

        if normalized.eq_ignore_ascii_case("none") {
            return Self {
                require_shell: false,
                require_git_push: false,
                require_git_pull: false,
                require_extension_install: false,
                require_extension_uninstall: false,
            };
        }

        let mut policy = if normalized
            .split(',')
            .map(str::trim)
            .any(|token| token.eq_ignore_ascii_case("all"))
        {
            Self {
                require_shell: true,
                require_git_push: true,
                require_git_pull: true,
                require_extension_install: true,
                require_extension_uninstall: true,
            }
        } else {
            Self::from_spec("none")
        };

        for token in normalized.split(',').map(str::trim).filter(|token| !token.is_empty()) {
            match token {
                token if token.eq_ignore_ascii_case("default") => policy = Self::default(),
                token if token.eq_ignore_ascii_case("shell") => policy.require_shell = true,
                token if token.eq_ignore_ascii_case("git_push") || token.eq_ignore_ascii_case("push") => {
                    policy.require_git_push = true;
                }
                token if token.eq_ignore_ascii_case("git_pull") || token.eq_ignore_ascii_case("pull") => {
                    policy.require_git_pull = true;
                }
                token if token.eq_ignore_ascii_case("install_extension")
                    || token.eq_ignore_ascii_case("extension_install")
                    || token.eq_ignore_ascii_case("extensions") =>
                {
                    policy.require_extension_install = true;
                }
                token if token.eq_ignore_ascii_case("uninstall_extension")
                    || token.eq_ignore_ascii_case("extension_uninstall")
                    || token.eq_ignore_ascii_case("extensions") =>
                {
                    policy.require_extension_uninstall = true;
                }
                token if token.eq_ignore_ascii_case("git") => {
                    policy.require_git_push = true;
                    policy.require_git_pull = true;
                }
                token if token.eq_ignore_ascii_case("all") || token.eq_ignore_ascii_case("none") => {}
                _ => {}
            }
        }

        policy
    }

    pub fn requires_approval(&self, intent: &Intent) -> bool {
        match intent {
            Intent::InstallExtension { .. } => self.require_extension_install,
            Intent::UninstallExtension { .. } => self.require_extension_uninstall,
            Intent::GitPull { .. } => self.require_git_pull,
            Intent::GitPushAll { .. } => self.require_git_push,
            Intent::RunShell { .. } => self.require_shell,
            _ => false,
        }
    }

    pub fn summary(&self) -> Vec<&'static str> {
        let mut items = Vec::new();

        if self.require_shell {
            items.push("shell");
        }
        if self.require_git_push {
            items.push("git_push");
        }
        if self.require_git_pull {
            items.push("git_pull");
        }
        if self.require_extension_install {
            items.push("install_extension");
        }
        if self.require_extension_uninstall {
            items.push("uninstall_extension");
        }

        items
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    StepByStep,
    ContinueAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    // 计划执行
    RunPlan {
        steps: Vec<Intent>,
        mode: ExecutionMode,
    },
    ContinuePlan,
    RetryFailedStep,
    ExecuteAll,
    ApprovePending,
    RejectPending,

    // VS Code 操作
    OpenFile { path: String, line: Option<u32> },
    OpenFolder { path: String },
    InstallExtension { ext_id: String },
    UninstallExtension { ext_id: String },
    ListExtensions,
    DiffFiles { file1: String, file2: String },

    // Git 操作
    GitStatus { repo: Option<String> },
    GitPull { repo: Option<String> },
    GitPushAll { repo: Option<String>, message: String },

    // Shell
    RunShell { cmd: String },

    // 通用
    Help,
    Unknown(String),
}

pub mod executor;
pub mod bridge;
pub mod feishu;
pub mod plan;
pub mod vscode;

/// 解析用户消息为意图
pub fn parse_intent(text: &str) -> Intent {
    let text = text.trim();
    let lower = text.to_lowercase();

    if matches!(lower.as_str(), "批准" | "同意" | "允许执行" | "approve" | "approve pending") {
        return Intent::ApprovePending;
    }

    if matches!(lower.as_str(), "拒绝" | "取消执行" | "reject" | "deny") {
        return Intent::RejectPending;
    }

    if matches!(lower.as_str(), "继续" | "continue") {
        return Intent::ContinuePlan;
    }

    if matches!(
        lower.as_str(),
        "重新执行失败步骤" | "重试失败步骤" | "retry failed step" | "retry failed"
    ) {
        return Intent::RetryFailedStep;
    }

    if lower == "执行全部" {
        return Intent::ExecuteAll;
    }

    if let Some(rest) = strip_prefix_any(
        &lower,
        &["执行计划 ", "计划 ", "step by step ", "plan "],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        if let Some(intent) = parse_plan(rest, ExecutionMode::StepByStep) {
            return intent;
        }
    }

    if let Some(rest) = strip_prefix_any(&lower, &["执行全部 ", "run all ", "continue all "]) {
        let rest = text[text.len() - rest.len()..].trim();
        if let Some(intent) = parse_plan(rest, ExecutionMode::ContinueAll) {
            return intent;
        }
    }

    parse_single_intent(text, &lower)
}

fn parse_plan(text: &str, mode: ExecutionMode) -> Option<Intent> {
    let steps: Vec<Intent> = split_plan_steps(text)
        .into_iter()
        .filter_map(|step| {
            let lower = step.to_lowercase();
            let intent = parse_single_intent(step, &lower);
            intent.is_runnable().then_some(intent)
        })
        .collect();

    if steps.is_empty() {
        return None;
    }

    Some(Intent::RunPlan { steps, mode })
}

fn split_plan_steps(text: &str) -> Vec<&str> {
    text.split(|c| matches!(c, ';' | '；' | '\n'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_single_intent(text: &str, lower: &str) -> Intent {

    // ── 帮助 ──
    if lower.is_empty() || matches!(lower, "help" | "帮助" | "?") {
        return Intent::Help;
    }

    // ── VS Code 打开文件夹 ──
    if let Some(rest) = strip_prefix_any(&lower, &["打开文件夹 ", "打开目录 ", "open folder "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::OpenFolder {
            path: rest.to_string(),
        };
    }

    // ── VS Code 打开文件 ──
    // "打开 src/main.rs" / "open src/main.rs" / "打开 src/main.rs:42"
    if let Some(rest) = strip_prefix_any(&lower, &["打开文件 ", "打开 ", "open "]) {
        let rest = text[text.len() - rest.len()..].trim();
        if let Some((path, line)) = parse_file_with_line(rest) {
            return Intent::OpenFile {
                path,
                line: Some(line),
            };
        }
        return Intent::OpenFile {
            path: rest.to_string(),
            line: None,
        };
    }

    // ── 安装扩展 ──
    if let Some(rest) = strip_prefix_any(
        &lower,
        &["安装扩展 ", "安装插件 ", "install extension ", "install ext ", "install "],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::InstallExtension {
            ext_id: rest.to_string(),
        };
    }

    // ── 卸载扩展 ──
    if let Some(rest) = strip_prefix_any(
        &lower,
        &["卸载扩展 ", "卸载插件 ", "uninstall extension ", "uninstall ext ", "uninstall "],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::UninstallExtension {
            ext_id: rest.to_string(),
        };
    }

    // ── 列出扩展 ──
    if matches!(lower, "扩展列表" | "列出扩展" | "插件列表" | "list extensions" | "list ext") {
        return Intent::ListExtensions;
    }

    // ── Diff ──
    if let Some(rest) = strip_prefix_any(&lower, &["diff ", "对比 ", "比较 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            return Intent::DiffFiles {
                file1: parts[0].to_string(),
                file2: parts[1].to_string(),
            };
        }
    }

    // ── Git status ──
    if matches!(lower, "git status" | "git 状态" | "仓库状态" | "代码状态") {
        return Intent::GitStatus { repo: None };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["git status ", "仓库状态 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::GitStatus {
            repo: Some(rest.to_string()),
        };
    }

    // ── Git pull ──
    if matches!(lower, "git pull" | "拉取" | "拉取代码") {
        return Intent::GitPull { repo: None };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["git pull ", "拉取 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::GitPull {
            repo: Some(rest.to_string()),
        };
    }

    // ── Git push all ──
    if matches!(lower, "git push" | "推送" | "推送代码" | "提交推送") {
        return Intent::GitPushAll {
            repo: None,
            message: "auto commit via feishu-bridge".to_string(),
        };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["git push ", "推送 ", "提交推送 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::GitPushAll {
            repo: None,
            message: rest.to_string(),
        };
    }

    // ── 执行 shell ──
    if let Some(rest) = strip_prefix_any(&lower, &["run ", "执行 ", "运行 ", "shell ", "$ "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::RunShell {
            cmd: rest.to_string(),
        };
    }

    // 无法识别
    Intent::Unknown(text.to_string())
}

impl Intent {
    pub fn is_runnable(&self) -> bool {
        matches!(
            self,
            Intent::OpenFile { .. }
                | Intent::OpenFolder { .. }
                | Intent::InstallExtension { .. }
                | Intent::UninstallExtension { .. }
                | Intent::ListExtensions
                | Intent::DiffFiles { .. }
                | Intent::GitStatus { .. }
                | Intent::GitPull { .. }
                | Intent::GitPushAll { .. }
                | Intent::RunShell { .. }
        )
    }
}

/// 辅助：尝试匹配多个前缀，返回去掉前缀后的剩余文本
fn strip_prefix_any<'a>(lower: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    for prefix in prefixes {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    None
}

/// 解析 "path:line" 格式
fn parse_file_with_line(s: &str) -> Option<(String, u32)> {
    let colon = s.rfind(':')?;
    let path = &s[..colon];
    let line: u32 = s[colon + 1..].parse().ok()?;
    if path.is_empty() || line == 0 {
        return None;
    }
    Some((path.to_string(), line))
}

// ── 消息去重 ──

use std::collections::HashMap;
use std::time::Instant;

pub struct MessageDedup {
    seen: HashMap<String, Instant>,
    ttl_secs: u64,
}

impl MessageDedup {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            seen: HashMap::new(),
            ttl_secs,
        }
    }

    /// 如果该 message_id 在 TTL 内已见过，返回 true（应跳过）
    pub fn is_duplicate(&mut self, message_id: &str) -> bool {
        let now = Instant::now();
        // 清理过期条目
        self.seen
            .retain(|_, ts| now.duration_since(*ts).as_secs() < self.ttl_secs);

        if self.seen.contains_key(message_id) {
            return true;
        }
        self.seen.insert(message_id.to_string(), now);
        false
    }
}

// ── 帮助文本 ──

pub fn help_text() -> &'static str {
    "\
📋 飞书 × VS Code Bridge 指令

▸ 计划执行
    执行计划 <命令1>; <命令2>   — 逐步执行计划，每次执行一步
    执行全部 <命令1>; <命令2>   — 连续执行剩余步骤，失败自动暂停
    继续                        — 执行当前计划的下一步或重试失败步骤
    重新执行失败步骤            — 仅重试当前失败步骤
    批准                        — 执行当前待审批步骤
    拒绝                        — 取消当前待审批计划

▸ VS Code
  打开 <文件路径>          — 用 VS Code 打开文件
  打开 <文件:行号>         — 打开并跳转到指定行
  打开文件夹 <路径>        — 打开目录
  安装扩展 <ext.id>        — 安装 VS Code 扩展
  卸载扩展 <ext.id>        — 卸载扩展
  扩展列表                 — 列出已安装扩展
  diff <文件1> <文件2>     — 对比两个文件

▸ Git
  git status [仓库路径]    — 查看仓库状态
  git pull [仓库路径]      — 拉取代码
  git push [提交信息]      — 提交并推送

▸ Shell
  run <命令>               — 执行 shell 命令
  $ <命令>                 — 同上

▸ 其他
  帮助 / help              — 显示本帮助"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open_file() {
        assert_eq!(
            parse_intent("打开 src/main.rs"),
            Intent::OpenFile {
                path: "src/main.rs".to_string(),
                line: None,
            }
        );
    }

    #[test]
    fn parse_open_file_with_line() {
        assert_eq!(
            parse_intent("打开 src/main.rs:42"),
            Intent::OpenFile {
                path: "src/main.rs".to_string(),
                line: Some(42),
            }
        );
    }

    #[test]
    fn parse_install_ext() {
        assert_eq!(
            parse_intent("安装扩展 rust-analyzer"),
            Intent::InstallExtension {
                ext_id: "rust-analyzer".to_string(),
            }
        );
    }

    #[test]
    fn parse_open_folder_english() {
        assert_eq!(
            parse_intent("open folder /tmp/demo"),
            Intent::OpenFolder {
                path: "/tmp/demo".to_string(),
            }
        );
    }

    #[test]
    fn parse_shell() {
        assert_eq!(
            parse_intent("$ echo hello"),
            Intent::RunShell {
                cmd: "echo hello".to_string(),
            }
        );
    }

    #[test]
    fn parse_git_status() {
        assert_eq!(
            parse_intent("git status"),
            Intent::GitStatus { repo: None }
        );
    }

    #[test]
    fn parse_help() {
        assert_eq!(parse_intent("帮助"), Intent::Help);
        assert_eq!(parse_intent("help"), Intent::Help);
    }

    #[test]
    fn parse_unknown() {
        assert!(matches!(parse_intent("random text"), Intent::Unknown(_)));
    }

    #[test]
    fn parse_step_by_step_plan() {
        assert_eq!(
            parse_intent("执行计划 打开 src/main.rs; git status"),
            Intent::RunPlan {
                steps: vec![
                    Intent::OpenFile {
                        path: "src/main.rs".to_string(),
                        line: None,
                    },
                    Intent::GitStatus { repo: None },
                ],
                mode: ExecutionMode::StepByStep,
            }
        );
    }

    #[test]
    fn parse_continue_all_plan() {
        assert_eq!(
            parse_intent("执行全部 打开 src/main.rs；git status"),
            Intent::RunPlan {
                steps: vec![
                    Intent::OpenFile {
                        path: "src/main.rs".to_string(),
                        line: None,
                    },
                    Intent::GitStatus { repo: None },
                ],
                mode: ExecutionMode::ContinueAll,
            }
        );
    }

    #[test]
    fn parse_continue_command() {
        assert_eq!(parse_intent("继续"), Intent::ContinuePlan);
        assert_eq!(parse_intent("执行全部"), Intent::ExecuteAll);
    }

    #[test]
    fn parse_approval_commands() {
        assert_eq!(parse_intent("批准"), Intent::ApprovePending);
        assert_eq!(parse_intent("approve"), Intent::ApprovePending);
        assert_eq!(parse_intent("拒绝"), Intent::RejectPending);
        assert_eq!(parse_intent("reject"), Intent::RejectPending);
    }

    #[test]
    fn approval_policy_parses_default() {
        let policy = ApprovalPolicy::from_spec("default");

        assert!(policy.require_shell);
        assert!(policy.require_git_push);
        assert!(!policy.require_git_pull);
    }

    #[test]
    fn approval_policy_parses_none() {
        let policy = ApprovalPolicy::from_spec("none");

        assert_eq!(policy.summary(), Vec::<&'static str>::new());
    }

    #[test]
    fn approval_policy_parses_custom_tokens() {
        let policy = ApprovalPolicy::from_spec("git_pull, install_extension");

        assert!(!policy.require_shell);
        assert!(!policy.require_git_push);
        assert!(policy.require_git_pull);
        assert!(policy.require_extension_install);
        assert!(!policy.require_extension_uninstall);
    }

    #[test]
    fn approval_policy_checks_intents() {
        let policy = ApprovalPolicy::from_spec("shell,git_pull");

        assert!(policy.requires_approval(&Intent::RunShell {
            cmd: "pwd".to_string(),
        }));
        assert!(policy.requires_approval(&Intent::GitPull { repo: None }));
        assert!(!policy.requires_approval(&Intent::GitPushAll {
            repo: None,
            message: "msg".to_string(),
        }));
    }

    #[test]
    fn parse_retry_failed_step_command() {
        assert_eq!(parse_intent("重新执行失败步骤"), Intent::RetryFailedStep);
        assert_eq!(parse_intent("重试失败步骤"), Intent::RetryFailedStep);
    }

    #[test]
    fn dedup_blocks_repeat() {
        let mut dedup = MessageDedup::new(600);
        assert!(!dedup.is_duplicate("msg_001"));
        assert!(dedup.is_duplicate("msg_001"));
        assert!(!dedup.is_duplicate("msg_002"));
    }
}
