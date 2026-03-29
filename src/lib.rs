use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPolicy {
    pub require_shell: bool,
    pub require_git_push: bool,
    pub require_git_pull: bool,
    pub require_apply_patch: bool,
    pub require_extension_install: bool,
    pub require_extension_uninstall: bool,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            require_shell: true,
            require_git_push: true,
            require_git_pull: false,
            require_apply_patch: true,
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
                require_apply_patch: false,
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
                require_apply_patch: true,
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
                token if token.eq_ignore_ascii_case("apply_patch")
                    || token.eq_ignore_ascii_case("patch") =>
                {
                    policy.require_apply_patch = true;
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
            Intent::ApplyPatch { .. } => self.require_apply_patch,
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
        if self.require_apply_patch {
            items.push("apply_patch");
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
    ExplainLastFailure,
    ShowLastResult,
    ContinueLastFile,
    ShowLastDiff,
    ShowRecentFiles,
    UndoLastPatch,

    // VS Code 操作
    OpenFile { path: String, line: Option<u32> },
    OpenFolder { path: String },
    InstallExtension { ext_id: String },
    UninstallExtension { ext_id: String },
    ListExtensions,
    DiffFiles { file1: String, file2: String },
    ReadFile {
        path: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
    },
    ListDirectory { path: Option<String> },
    SearchText {
        query: String,
        path: Option<String>,
        is_regex: bool,
    },
    RunTests { command: Option<String> },
    GitDiff { path: Option<String> },
    ApplyPatch { patch: String },

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

    if matches!(
        lower.as_str(),
        "继续"
            | "continue"
            | "继续刚才的任务"
            | "继续刚才任务"
            | "继续上次任务"
            | "继续刚才的计划"
            | "continue last task"
    ) {
        return Intent::ContinuePlan;
    }

    if matches!(
        lower.as_str(),
        "重新执行失败步骤" | "重试失败步骤" | "retry failed step" | "retry failed"
    ) {
        return Intent::RetryFailedStep;
    }

    if matches!(
        lower.as_str(),
        "刚才为什么失败"
            | "上次为什么失败"
            | "刚才为何失败"
            | "为什么失败了"
            | "失败原因"
            | "why did that fail"
            | "why did it fail"
    ) {
        return Intent::ExplainLastFailure;
    }

    if matches!(
        lower.as_str(),
        "把上一步结果发我"
            | "上一步结果"
            | "上一步的结果"
            | "发我上一步结果"
            | "看看上一步"
            | "看上一步"
            | "show last result"
            | "last result"
    ) {
        return Intent::ShowLastResult;
    }

    if matches!(
        lower.as_str(),
        "把刚才的 diff 发我"
            | "把刚才的diff发我"
            | "刚才的 diff"
            | "刚才的diff"
            | "上一个 diff"
            | "上一个diff"
            | "看看 diff"
            | "看 diff"
            | "show last diff"
            | "show diff"
    ) {
        return Intent::ShowLastDiff;
    }

    if matches!(
        lower.as_str(),
        "把刚才改动的文件列表发我"
            | "把刚才修改的文件列表发我"
            | "刚才改了哪些文件"
            | "最近改动文件"
            | "看看文件列表"
            | "看文件列表"
            | "show recent files"
            | "show changed files"
    ) {
        return Intent::ShowRecentFiles;
    }

    if matches!(
        lower.as_str(),
        "继续改刚才那个文件"
            | "继续改上一个文件"
            | "继续处理刚才那个文件"
            | "继续这个文件"
            | "打开刚才那个文件"
            | "continue editing that file"
    ) {
        return Intent::ContinueLastFile;
    }

    if matches!(
        lower.as_str(),
        "撤回刚才的补丁"
            | "把刚才的补丁撤回"
            | "撤销刚才的补丁"
            | "撤回补丁"
            | "撤销补丁"
            | "undo last patch"
            | "revert last patch"
    ) {
        return Intent::UndoLastPatch;
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

    // ── 列出扩展 ──
    if matches!(lower, "扩展列表" | "列出扩展" | "插件列表" | "list extensions" | "list ext") {
        return Intent::ListExtensions;
    }

    // ── 读取文件 ──
    if let Some(rest) = strip_prefix_any(&lower, &["读取文件 ", "读取 ", "read file ", "read "]) {
        let rest = text[text.len() - rest.len()..].trim();
        let (path, start_line, end_line) = parse_read_target(rest);
        if !path.is_empty() {
            return Intent::ReadFile {
                path,
                start_line,
                end_line,
            };
        }
    }

    // ── 列目录 ──
    if matches!(lower, "列出目录" | "列目录" | "ls" | "list dir" | "list directory") {
        return Intent::ListDirectory { path: None };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["列出目录 ", "列目录 ", "list dir ", "list directory ", "ls "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::ListDirectory {
            path: Some(rest.to_string()),
        };
    }
    if let Some(rest) = lower.strip_prefix("列出 ") {
        if !rest.starts_with("扩展") {
            let rest = text[text.len() - rest.len()..].trim();
            return Intent::ListDirectory {
                path: Some(rest.to_string()),
            };
        }
    }

    // ── 搜索 ──
    if let Some(intent) = parse_search_intent(text, lower) {
        return intent;
    }

    // ── Git diff ──
    if matches!(lower, "查看 diff" | "查看git diff" | "git diff" | "查看差异" | "查看变更") {
        return Intent::GitDiff { path: None };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["查看 diff ", "查看git diff ", "git diff ", "查看差异 ", "查看变更 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::GitDiff {
            path: (!rest.is_empty()).then(|| rest.to_string()),
        };
    }

    // ── 应用补丁 ──
    if lower == "应用补丁" || lower == "apply patch" {
        return Intent::ApplyPatch {
            patch: String::new(),
        };
    }
    if let Some(rest) = strip_prefix_any(
        &lower,
        &["应用补丁\n", "应用补丁 ", "apply patch\n", "apply patch ", "按以下补丁修改\n", "按以下补丁修改 "],
    ) {
        let patch = text[text.len() - rest.len()..]
            .trim_start_matches(|c| matches!(c, ' ' | '\n' | '\r' | '\t'))
            .to_string();
        return Intent::ApplyPatch { patch };
    }

    // ── 运行测试 ──
    if matches!(lower, "运行测试" | "跑测试" | "测试" | "run tests" | "run test") {
        return Intent::RunTests { command: None };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["运行测试 ", "跑测试 ", "测试 ", "run tests ", "run test "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::RunTests {
            command: (!rest.is_empty()).then(|| rest.to_string()),
        };
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
            Intent::ExplainLastFailure
                | Intent::ShowLastResult
                | Intent::ContinueLastFile
                | Intent::ShowLastDiff
                | Intent::ShowRecentFiles
                | Intent::UndoLastPatch
                | Intent::OpenFile { .. }
                | Intent::OpenFolder { .. }
                | Intent::InstallExtension { .. }
                | Intent::UninstallExtension { .. }
                | Intent::ListExtensions
                | Intent::DiffFiles { .. }
                    | Intent::ReadFile { .. }
                    | Intent::ListDirectory { .. }
                    | Intent::SearchText { .. }
                | Intent::RunTests { .. }
                    | Intent::GitDiff { .. }
                    | Intent::ApplyPatch { .. }
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

fn parse_read_target(s: &str) -> (String, Option<usize>, Option<usize>) {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return (String::new(), None, None);
    }

    if let Some((path, range)) = trimmed.rsplit_once(' ') {
        if let Some((start_line, end_line)) = parse_line_range(range.trim()) {
            return (path.trim().to_string(), Some(start_line), Some(end_line));
        }
    }

    if let Some(colon) = trimmed.rfind(':') {
        let path = trimmed[..colon].trim();
        let range = trimmed[colon + 1..].trim();
        if !path.is_empty() {
            if let Some((start_line, end_line)) = parse_line_range(range) {
                return (path.to_string(), Some(start_line), Some(end_line));
            }
        }
    }

    (trimmed.to_string(), None, None)
}

fn parse_line_range(s: &str) -> Option<(usize, usize)> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((start, end)) = trimmed.split_once('-') {
        let start_line: usize = start.trim().parse().ok()?;
        let end_line: usize = end.trim().parse().ok()?;
        if start_line == 0 || end_line == 0 || end_line < start_line {
            return None;
        }
        return Some((start_line, end_line));
    }

    let line: usize = trimmed.parse().ok()?;
    if line == 0 {
        return None;
    }

    Some((line, line))
}

fn parse_search_intent(text: &str, lower: &str) -> Option<Intent> {
    let (rest, is_regex) = if let Some(rest) = strip_prefix_any(&lower, &["搜索正则 ", "search regex "]) {
        (rest, true)
    } else if let Some(rest) = strip_prefix_any(&lower, &["搜索文本 ", "search text "]) {
        (rest, false)
    } else if let Some(rest) = strip_prefix_any(&lower, &["搜索 ", "search "]) {
        (rest, false)
    } else {
        return None;
    };

    let rest = text[text.len() - rest.len()..].trim();
    let (query, path) = split_search_scope(rest);
    if query.is_empty() {
        return None;
    }

    Some(Intent::SearchText {
        query,
        path,
        is_regex,
    })
}

fn split_search_scope(s: &str) -> (String, Option<String>) {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }

    if let Some(index) = trimmed.rfind(" 在 ") {
        let query = trimmed[..index].trim();
        let path = trimmed[index + " 在 ".len()..].trim();
        if !query.is_empty() && !path.is_empty() {
            return (query.to_string(), Some(path.to_string()));
        }
    }

    if let Some(index) = trimmed.rfind(" in ") {
        let query = trimmed[..index].trim();
        let path = trimmed[index + " in ".len()..].trim();
        if !query.is_empty() && !path.is_empty() {
            return (query.to_string(), Some(path.to_string()));
        }
    }

    (trimmed.to_string(), None)
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

▸ 计划
    执行计划 <命令1>; <命令2>   — 一步一步执行
    执行全部 <命令1>; <命令2>   — 连续执行到结束或失败
    继续                        — 做下一步
    重新执行失败步骤            — 只重试失败那一步
    批准 / 拒绝                 — 处理待审批步骤

▸ 追问
    刚才为什么失败              — 看失败原因
    把上一步结果发我            — 看上一步输出
    继续改刚才那个文件          — 回到刚才那个文件
    把刚才的 diff 发我          — 看刚才的 diff / patch
    把刚才改动的文件列表发我    — 看刚才改了哪些文件
    撤回刚才的补丁              — 撤销刚才那次补丁

▸ VS Code
  打开 <文件路径>          — 用 VS Code 打开文件
  打开 <文件:行号>         — 打开并跳转到指定行
  打开文件夹 <路径>        — 打开目录
  安装扩展 <ext.id>        — 安装 VS Code 扩展
  卸载扩展 <ext.id>        — 卸载扩展
  扩展列表                 — 列出已安装扩展
  diff <文件1> <文件2>     — 对比两个文件

▸ 工作区
    读取 <文件> [1-120]      — 读取文件，可附带行号范围
    列出 <路径>              — 列出目录内容
    搜索 <关键字> [在 路径]  — 文本搜索
    搜索正则 <模式> [在 路径] — 正则搜索
    运行测试 [命令]          — 执行默认测试命令或指定测试命令
        应用补丁 <unified diff>  — 将补丁应用到当前工作区

▸ Git
    查看 diff [路径]         — 查看当前工作区未提交变更
    git diff [路径]          — 同上
  git status [仓库路径]    — 查看仓库状态
  git pull [仓库路径]      — 拉取代码
  git push [提交信息]      — 提交并推送
    未显式传仓库路径时       — 优先使用 BRIDGE_WORKSPACE_PATH

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
    fn parse_read_file_with_range() {
        assert_eq!(
            parse_intent("读取 src/lib.rs 1-20"),
            Intent::ReadFile {
                path: "src/lib.rs".to_string(),
                start_line: Some(1),
                end_line: Some(20),
            }
        );
    }

    #[test]
    fn parse_list_directory_short_form() {
        assert_eq!(
            parse_intent("列出 src"),
            Intent::ListDirectory {
                path: Some("src".to_string()),
            }
        );
    }

    #[test]
    fn parse_search_text_with_scope() {
        assert_eq!(
            parse_intent("搜索 parse_intent 在 src"),
            Intent::SearchText {
                query: "parse_intent".to_string(),
                path: Some("src".to_string()),
                is_regex: false,
            }
        );
    }

    #[test]
    fn parse_search_regex() {
        assert_eq!(
            parse_intent("搜索正则 parse_.*"),
            Intent::SearchText {
                query: "parse_.*".to_string(),
                path: None,
                is_regex: true,
            }
        );
    }

    #[test]
    fn parse_run_tests_default() {
        assert_eq!(
            parse_intent("运行测试"),
            Intent::RunTests { command: None }
        );
    }

    #[test]
    fn parse_run_tests_custom_command() {
        assert_eq!(
            parse_intent("运行测试 cargo test --lib"),
            Intent::RunTests {
                command: Some("cargo test --lib".to_string()),
            }
        );
    }

    #[test]
    fn parse_git_diff_default() {
        assert_eq!(parse_intent("查看 diff"), Intent::GitDiff { path: None });
        assert_eq!(parse_intent("git diff"), Intent::GitDiff { path: None });
    }

    #[test]
    fn parse_git_diff_with_path() {
        assert_eq!(
            parse_intent("查看 diff src/lib.rs"),
            Intent::GitDiff {
                path: Some("src/lib.rs".to_string()),
            }
        );
    }

    #[test]
    fn parse_apply_patch_multiline() {
        assert_eq!(
            parse_intent("应用补丁\n--- a/test.txt\n+++ b/test.txt"),
            Intent::ApplyPatch {
                patch: "--- a/test.txt\n+++ b/test.txt".to_string(),
            }
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
        assert_eq!(parse_intent("继续刚才的任务"), Intent::ContinuePlan);
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
    fn parse_follow_up_commands() {
        assert_eq!(parse_intent("刚才为什么失败"), Intent::ExplainLastFailure);
        assert_eq!(parse_intent("为什么失败了"), Intent::ExplainLastFailure);
        assert_eq!(parse_intent("把上一步结果发我"), Intent::ShowLastResult);
        assert_eq!(parse_intent("看上一步"), Intent::ShowLastResult);
        assert_eq!(parse_intent("继续改刚才那个文件"), Intent::ContinueLastFile);
        assert_eq!(parse_intent("继续这个文件"), Intent::ContinueLastFile);
        assert_eq!(parse_intent("把刚才的 diff 发我"), Intent::ShowLastDiff);
        assert_eq!(parse_intent("看 diff"), Intent::ShowLastDiff);
        assert_eq!(parse_intent("把刚才改动的文件列表发我"), Intent::ShowRecentFiles);
        assert_eq!(parse_intent("看文件列表"), Intent::ShowRecentFiles);
        assert_eq!(parse_intent("撤回刚才的补丁"), Intent::UndoLastPatch);
        assert_eq!(parse_intent("撤回补丁"), Intent::UndoLastPatch);
    }

    #[test]
    fn approval_policy_parses_default() {
        let policy = ApprovalPolicy::from_spec("default");

        assert!(policy.require_shell);
        assert!(policy.require_git_push);
        assert!(!policy.require_git_pull);
        assert!(policy.require_apply_patch);
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
        assert!(!policy.require_apply_patch);
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
        assert!(!policy.requires_approval(&Intent::ApplyPatch {
            patch: "x".to_string(),
        }));
        assert!(!policy.requires_approval(&Intent::GitPushAll {
            repo: None,
            message: "msg".to_string(),
        }));
    }

    #[test]
    fn approval_policy_checks_apply_patch() {
        let policy = ApprovalPolicy::from_spec("apply_patch");

        assert!(policy.requires_approval(&Intent::ApplyPatch {
            patch: "x".to_string(),
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
