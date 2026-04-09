use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPolicy {
    pub require_shell: bool,
    pub require_git_push: bool,
    pub require_git_pull: bool,
    pub require_apply_patch: bool,
    pub require_write_file: bool,
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
            require_write_file: true,
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
                require_write_file: false,
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
                require_write_file: true,
                require_extension_install: true,
                require_extension_uninstall: true,
            }
        } else {
            Self::from_spec("none")
        };

        for token in normalized
            .split(',')
            .map(str::trim)
            .filter(|token| !token.is_empty())
        {
            match token {
                token if token.eq_ignore_ascii_case("default") => policy = Self::default(),
                token if token.eq_ignore_ascii_case("shell") => policy.require_shell = true,
                token
                    if token.eq_ignore_ascii_case("git_push")
                        || token.eq_ignore_ascii_case("push") =>
                {
                    policy.require_git_push = true;
                }
                token
                    if token.eq_ignore_ascii_case("git_pull")
                        || token.eq_ignore_ascii_case("pull") =>
                {
                    policy.require_git_pull = true;
                }
                token
                    if token.eq_ignore_ascii_case("apply_patch")
                        || token.eq_ignore_ascii_case("patch") =>
                {
                    policy.require_apply_patch = true;
                }
                token
                    if token.eq_ignore_ascii_case("write_file")
                        || token.eq_ignore_ascii_case("write") =>
                {
                    policy.require_write_file = true;
                }
                token
                    if token.eq_ignore_ascii_case("install_extension")
                        || token.eq_ignore_ascii_case("extension_install")
                        || token.eq_ignore_ascii_case("extensions") =>
                {
                    policy.require_extension_install = true;
                }
                token
                    if token.eq_ignore_ascii_case("uninstall_extension")
                        || token.eq_ignore_ascii_case("extension_uninstall")
                        || token.eq_ignore_ascii_case("extensions") =>
                {
                    policy.require_extension_uninstall = true;
                }
                token if token.eq_ignore_ascii_case("git") => {
                    policy.require_git_push = true;
                    policy.require_git_pull = true;
                }
                token
                    if token.eq_ignore_ascii_case("all") || token.eq_ignore_ascii_case("none") => {}
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
            Intent::WriteFile { .. } => self.require_write_file,
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
        if self.require_write_file {
            items.push("write_file");
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
    ShowPlanPrompt {
        prompt: String,
    },
    StartAgentRun {
        prompt: String,
    },
    ContinueAgentRun {
        prompt: Option<String>,
    },
    ShowAgentRunStatus,
    ApproveAgentRun {
        option_id: Option<String>,
    },
    CancelAgentRun,
    ContinueAgent {
        prompt: Option<String>,
    },
    ContinueAgentSuggested,
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
    OpenFile {
        path: String,
        line: Option<u32>,
    },
    OpenFolder {
        path: String,
    },
    InstallExtension {
        ext_id: String,
    },
    UninstallExtension {
        ext_id: String,
    },
    ListExtensions,
    DiffFiles {
        file1: String,
        file2: String,
    },
    ReadFile {
        path: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
    },
    ListDirectory {
        path: Option<String>,
    },
    SearchText {
        query: String,
        path: Option<String>,
        is_regex: bool,
    },
    RunTests {
        command: Option<String>,
    },
    GitDiff {
        path: Option<String>,
    },
    ApplyPatch {
        patch: String,
    },
    SearchSymbol {
        query: String,
        path: Option<String>,
    },
    FindReferences {
        query: String,
        path: Option<String>,
    },
    FindImplementations {
        query: String,
        path: Option<String>,
    },
    RunSpecificTest {
        filter: String,
    },
    RunTestFile {
        path: String,
    },
    WriteFile {
        path: String,
        content: String,
    },
    AskAgent {
        prompt: String,
    },
    AskCodex {
        prompt: String,
    },
    ResetAgentSession,
    ShowProjectPicker,
    ShowProjectBrowser {
        path: Option<String>,
    },
    ShowCurrentProject,

    // Git 操作
    GitStatus {
        repo: Option<String>,
    },
    GitSync {
        repo: Option<String>,
    },
    GitPull {
        repo: Option<String>,
    },
    GitPushAll {
        repo: Option<String>,
        message: String,
    },
    GitLog {
        count: Option<usize>,
        path: Option<String>,
    },
    GitBlame {
        path: String,
    },

    // Shell
    RunShell {
        cmd: String,
    },

    // 通用
    Help,
    Unknown(String),
}

pub mod agent_backend;
pub mod agent_runtime;
pub mod audit;
pub mod bridge;
pub mod bridge_context;
pub mod card;
pub mod direct_command;
pub mod executor;
pub mod feishu;
pub mod follow_up;
pub mod intent_executor;
pub mod plan;
pub mod plan_dispatch;
pub mod reply;
pub mod semantic_planner;
pub mod session;
pub mod vscode;

#[cfg(test)]
pub(crate) mod test_support;

/// 解析用户消息为意图
pub fn parse_intent(text: &str) -> Intent {
    parse_intent_internal(text, true)
}

/// 仅解析显式命令，不尝试自然语言语义推断。
pub fn parse_explicit_intent(text: &str) -> Intent {
    parse_intent_internal(text, false)
}

fn parse_intent_internal(text: &str, allow_natural_language: bool) -> Intent {
    let text = text.trim();
    let lower = text.to_lowercase();

    if matches!(
        lower.as_str(),
        "批准" | "同意" | "允许执行" | "approve" | "approve pending"
    ) {
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

    if allow_natural_language && looks_like_continue_plan_phrase(&lower) {
        return Intent::ContinuePlan;
    }

    if let Some(prompt) = parse_plan_prompt_intent(text, &lower) {
        return Intent::ShowPlanPrompt { prompt };
    }

    if let Some(intent) = parse_agent_continue_intent(text, &lower) {
        return intent;
    }

    if let Some(intent) = parse_agent_runtime_action_intent(text, &lower) {
        return intent;
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
        "按建议继续"
            | "按建议执行"
            | "执行建议动作"
            | "继续建议的动作"
            | "follow suggested next step"
            | "accept next action"
    ) {
        return Intent::ContinueAgentSuggested;
    }

    if matches!(
        lower.as_str(),
        "选择项目" | "切换项目" | "select project" | "choose project" | "use project"
    ) {
        return Intent::ShowProjectPicker;
    }

    if matches!(
        lower.as_str(),
        "浏览项目" | "浏览文件夹" | "browse project" | "browse folder"
    ) {
        return Intent::ShowProjectBrowser { path: None };
    }

    if matches!(
        lower.as_str(),
        "当前项目" | "current project" | "project status"
    ) {
        return Intent::ShowCurrentProject;
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

    if let Some(rest) = strip_prefix_any(&lower, &["执行计划 ", "计划 ", "step by step ", "plan "])
    {
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

    parse_single_intent(text, &lower, allow_natural_language)
}

fn parse_plan(text: &str, mode: ExecutionMode) -> Option<Intent> {
    let steps: Vec<Intent> = split_plan_steps(text)
        .into_iter()
        .filter_map(|step| {
            let lower = step.to_lowercase();
            let intent = parse_single_intent(step, &lower, false);
            intent.is_runnable().then_some(intent)
        })
        .collect();

    if steps.is_empty() {
        return None;
    }

    Some(Intent::RunPlan { steps, mode })
}

fn parse_plan_prompt_intent(text: &str, lower: &str) -> Option<String> {
    let prompt = strip_command_prefix(
        text,
        lower,
        &["/plan", "规划", "给我计划", "帮我规划", "plan prompt"],
    )?
    .trim();

    if prompt.is_empty() {
        None
    } else {
        Some(prompt.to_string())
    }
}

fn split_plan_steps(text: &str) -> Vec<&str> {
    text.split(|c| matches!(c, ';' | '；' | '\n'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_single_intent(text: &str, lower: &str, allow_natural_language: bool) -> Intent {
    // ── 帮助 ──
    if lower.is_empty() || matches!(lower, "help" | "帮助" | "?") {
        return Intent::Help;
    }

    // ── 列出扩展 ──
    if matches!(
        lower,
        "扩展列表" | "列出扩展" | "插件列表" | "list extensions" | "list ext"
    ) {
        return Intent::ListExtensions;
    }

    // ── 读取文件 ──
    if let Some(rest) = strip_prefix_any(&lower, &["读取文件 ", "读取 ", "read file ", "read "])
    {
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
    if matches!(
        lower,
        "列出目录" | "列目录" | "ls" | "list dir" | "list directory"
    ) {
        return Intent::ListDirectory { path: None };
    }
    if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "列出目录 ",
            "列目录 ",
            "list dir ",
            "list directory ",
            "ls ",
        ],
    ) {
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

    // ── 搜索符号 / 引用 / 实现 ──（放在一般搜索之前，避免被 "search " 截断）
    if let Some(intent) = parse_symbol_search_intent(text, lower) {
        return intent;
    }

    // ── 询问 Copilot / Codex / Agent ──
    if let Some(intent) = parse_agent_reset_intent(lower) {
        return intent;
    }
    if let Some(intent) = parse_agent_ask_intent(text, lower) {
        return intent;
    }

    // ── 搜索 ──
    if let Some(intent) = parse_search_intent(text, lower) {
        return intent;
    }

    // ── 写入文件 ──
    if let Some(intent) = parse_write_file_intent(text, lower) {
        return intent;
    }

    // ── Git diff ──
    if matches!(
        lower,
        "查看 diff" | "查看git diff" | "git diff" | "查看差异" | "查看变更"
    ) {
        return Intent::GitDiff { path: None };
    }
    if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "查看 diff ",
            "查看git diff ",
            "git diff ",
            "查看差异 ",
            "查看变更 ",
        ],
    ) {
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
        &[
            "应用补丁\n",
            "应用补丁 ",
            "apply patch\n",
            "apply patch ",
            "按以下补丁修改\n",
            "按以下补丁修改 ",
        ],
    ) {
        let patch = text[text.len() - rest.len()..]
            .trim_start_matches(|c| matches!(c, ' ' | '\n' | '\r' | '\t'))
            .to_string();
        return Intent::ApplyPatch { patch };
    }

    // ── 运行测试 ──
    if matches!(
        lower,
        "运行测试" | "跑测试" | "测试" | "run tests" | "run test"
    ) {
        return Intent::RunTests { command: None };
    }
    if let Some(rest) = strip_prefix_any(
        &lower,
        &["运行测试 ", "跑测试 ", "测试 ", "run tests ", "run test "],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::RunTests {
            command: (!rest.is_empty()).then(|| rest.to_string()),
        };
    }

    // ── 运行指定测试 ──
    if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "运行指定测试 ",
            "跑指定测试 ",
            "指定测试 ",
            "test filter ",
            "test name ",
        ],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        if !rest.is_empty() {
            return Intent::RunSpecificTest {
                filter: rest.to_string(),
            };
        }
    }

    // ── 运行测试文件 ──
    if let Some(rest) = strip_prefix_any(
        &lower,
        &["运行测试文件 ", "测试文件 ", "run test file ", "test file "],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        if !rest.is_empty() {
            return Intent::RunTestFile {
                path: rest.to_string(),
            };
        }
    }

    // ── VS Code 打开文件夹 ──
    if let Some(rest) = strip_prefix_any(
        &lower,
        &["选择项目 ", "切换项目 ", "select project ", "use project "],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::OpenFolder {
            path: rest.to_string(),
        };
    }

    if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "浏览项目 ",
            "浏览文件夹 ",
            "browse project ",
            "browse folder ",
        ],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::ShowProjectBrowser {
            path: Some(rest.to_string()),
        };
    }

    if let Some(rest) = strip_prefix_any(&lower, &["打开文件夹 ", "打开目录 ", "open folder "])
    {
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
        &[
            "安装扩展 ",
            "安装插件 ",
            "install extension ",
            "install ext ",
            "install ",
        ],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::InstallExtension {
            ext_id: rest.to_string(),
        };
    }

    // ── 卸载扩展 ──
    if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "卸载扩展 ",
            "卸载插件 ",
            "uninstall extension ",
            "uninstall ext ",
            "uninstall ",
        ],
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

    // ── Git sync ──
    if matches!(
        lower,
        "git sync"
            | "sync git"
            | "同步git状态"
            | "同步 git 状态"
            | "同步github状态"
            | "同步 github 状态"
            | "同步代码状态"
            | "同步当前项目"
    ) {
        return Intent::GitSync { repo: None };
    }
    if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "git sync ",
            "sync git ",
            "同步git状态 ",
            "同步 git 状态 ",
            "同步github状态 ",
            "同步 github 状态 ",
            "同步代码状态 ",
        ],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::GitSync {
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

    // ── Git log ──
    if matches!(lower, "git log" | "提交历史" | "提交记录" | "历史记录") {
        return Intent::GitLog {
            count: None,
            path: None,
        };
    }
    if let Some(rest) =
        strip_prefix_any(&lower, &["git log ", "提交历史 ", "提交记录 ", "历史记录 "])
    {
        let rest = text[text.len() - rest.len()..].trim();
        let (count, path) = parse_git_log_args(rest);
        return Intent::GitLog { count, path };
    }

    // ── Git blame ──
    if let Some(rest) = strip_prefix_any(&lower, &["git blame ", "blame ", "追溯 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        if !rest.is_empty() {
            return Intent::GitBlame {
                path: rest.to_string(),
            };
        }
    }

    // ── 执行 shell ──
    if let Some(rest) = strip_prefix_any(&lower, &["run ", "执行 ", "运行 ", "shell ", "$ "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::RunShell {
            cmd: rest.to_string(),
        };
    }

    if allow_natural_language {
        if let Some(intent) = parse_natural_language_intent(text, lower) {
            return intent;
        }
    }

    // 无法识别
    Intent::Unknown(text.to_string())
}

impl Intent {
    pub fn is_runnable(&self) -> bool {
        matches!(
            self,
            Intent::ExplainLastFailure
                | Intent::ShowPlanPrompt { .. }
                | Intent::ShowLastResult
                | Intent::ShowAgentRunStatus
                | Intent::ApproveAgentRun { .. }
                | Intent::CancelAgentRun
                | Intent::ContinueAgentSuggested
                | Intent::ShowProjectPicker
                | Intent::ShowProjectBrowser { .. }
                | Intent::ShowCurrentProject
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
                | Intent::SearchSymbol { .. }
                | Intent::FindReferences { .. }
                | Intent::FindImplementations { .. }
                | Intent::RunSpecificTest { .. }
                | Intent::RunTestFile { .. }
                | Intent::WriteFile { .. }
                | Intent::AskAgent { .. }
                | Intent::AskCodex { .. }
                | Intent::StartAgentRun { .. }
                | Intent::ContinueAgentRun { .. }
                | Intent::ContinueAgent { .. }
                | Intent::ResetAgentSession
                | Intent::GitStatus { .. }
                | Intent::GitSync { .. }
                | Intent::GitPull { .. }
                | Intent::GitPushAll { .. }
                | Intent::GitLog { .. }
                | Intent::GitBlame { .. }
                | Intent::RunShell { .. }
        )
    }
}

fn looks_like_continue_plan_phrase(lower: &str) -> bool {
    let has_continue_plan = lower.contains("继续plan")
        || lower.contains("继续 plan")
        || lower.contains("continue plan");
    let has_pending_work = lower.contains("没完成")
        || lower.contains("未完成")
        || lower.contains("unfinished")
        || lower.contains("pending");

    has_continue_plan && has_pending_work
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

fn strip_command_prefix<'a>(text: &'a str, lower: &str, prefixes: &[&str]) -> Option<&'a str> {
    for prefix in prefixes {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if rest.is_empty() {
                return Some("");
            }

            if rest.chars().next().is_some_and(char::is_whitespace) {
                let prefix_len = lower.len() - rest.len();
                let original_rest = &text[prefix_len..];
                return Some(original_rest.trim_start_matches(|c: char| c.is_whitespace()));
            }
        }

        if let Some(prefix_len) = match_command_words(lower, prefix) {
            let original_rest = &text[prefix_len..];
            return Some(original_rest.trim_start_matches(|c: char| c.is_whitespace()));
        }
    }

    None
}

fn match_command_words(lower: &str, prefix: &str) -> Option<usize> {
    let parts = prefix.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }

    let mut offset = 0;
    for (index, part) in parts.iter().enumerate() {
        let rest = &lower[offset..];
        if !rest.starts_with(part) {
            return None;
        }

        offset += part.len();
        if index < parts.len() - 1 {
            let whitespace_len = lower[offset..]
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(char::len_utf8)
                .sum::<usize>();
            if whitespace_len == 0 {
                return None;
            }
            offset += whitespace_len;
        }
    }

    if lower[offset..].is_empty()
        || lower[offset..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
    {
        Some(offset)
    } else {
        None
    }
}

fn matches_command_any(lower: &str, commands: &[&str]) -> bool {
    let normalized = normalize_command_whitespace(lower);
    commands
        .iter()
        .any(|command| normalized == normalize_command_whitespace(command))
}

fn normalize_command_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_natural_language_intent(text: &str, lower: &str) -> Option<Intent> {
    parse_natural_project_intent(text, lower)
        .or_else(|| parse_natural_git_intent(lower))
        .or_else(|| parse_natural_agent_request(text, lower))
}

fn parse_natural_project_intent(text: &str, lower: &str) -> Option<Intent> {
    if contains_any(
        lower,
        &["当前项目", "现在在哪个项目", "现在在哪个仓库", "当前仓库"],
    ) {
        return Some(Intent::ShowCurrentProject);
    }

    let looks_like_navigation_request = !contains_any(
        lower,
        &[
            "工作",
            "任务",
            "实现",
            "修复",
            "继续",
            "未完成",
            "readme",
            "docs/",
            "src/",
        ],
    );

    if contains_any(lower, &["浏览", "看看", "看一下", "查看"])
        && contains_any(lower, &["项目", "目录", "文件夹", "本地文件夹"])
        && looks_like_navigation_request
    {
        return Some(Intent::ShowProjectBrowser { path: None });
    }

    if contains_any(lower, &["切到", "切换到", "打开", "进入", "使用", "改到"]) {
        if let Some(path) = extract_path_candidate(text) {
            return Some(Intent::OpenFolder { path });
        }
    }

    None
}

fn parse_natural_git_intent(lower: &str) -> Option<Intent> {
    let mentions_remote = contains_any(lower, &["github", "远程", "仓库", "origin"]);
    let mentions_local_changes = contains_any(lower, &["本地", "改动", "变更", "代码", "提交"]);

    if contains_any(lower, &["同步", "推送", "提交", "上传", "发上去"])
        && mentions_remote
        && mentions_local_changes
    {
        return Some(Intent::GitPushAll {
            repo: None,
            message: "auto commit via feishu-bridge".to_string(),
        });
    }

    if contains_any(lower, &["拉取", "更新本地", "同步到本地", "拉下来"]) && mentions_remote
    {
        return Some(Intent::GitPull { repo: None });
    }

    if contains_any(
        lower,
        &[
            "看看状态",
            "查看状态",
            "仓库状态",
            "git状态",
            "git 状态",
            "有没有改动",
            "有哪些改动",
            "本地改动",
            "工作区状态",
        ],
    ) {
        return Some(Intent::GitStatus { repo: None });
    }

    None
}

fn parse_natural_agent_request(text: &str, lower: &str) -> Option<Intent> {
    let has_task_verb = contains_any(
        lower,
        &[
            "检查",
            "分析",
            "解释",
            "看看",
            "查看",
            "阅读",
            "读取",
            "搜索",
            "查找",
            "继续",
            "修复",
            "修改",
            "实现",
            "重构",
            "review",
            "inspect",
            "analyze",
            "explain",
            "check",
            "read",
            "search",
            "find",
            "continue",
            "fix",
            "implement",
            "update",
            "modify",
        ],
    );
    let has_code_context = contains_any(
        lower,
        &[
            "readme", "docs/", "src/", ".rs", ".ts", "代码", "项目", "函数", "文件", "测试",
            "plan", "diff", "copilot", "bug",
        ],
    );

    (has_task_verb && has_code_context).then(|| Intent::AskAgent {
        prompt: text.trim().to_string(),
    })
}

fn extract_path_candidate(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(trim_wrapping_punctuation)
        .find(|token| looks_like_path_candidate(token))
        .map(ToString::to_string)
}

fn trim_wrapping_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '，' | '。' | '：' | ':' | '；' | ';' | '（' | '）' | '(' | ')' | '、'
        )
    })
}

fn looks_like_path_candidate(token: &str) -> bool {
    let trimmed = token.trim();
    if trimmed.len() < 2 {
        return false;
    }

    trimmed.contains(":/")
        || trimmed.contains(":\\")
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('/')
        || trimmed.contains("\\")
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
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
    let (rest, is_regex) =
        if let Some(rest) = strip_prefix_any(&lower, &["搜索正则 ", "search regex "]) {
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

fn parse_symbol_search_intent(text: &str, lower: &str) -> Option<Intent> {
    let rest = if let Some(rest) = strip_prefix_any(
        &lower,
        &["搜索符号 ", "查找符号 ", "search symbol ", "find symbol "],
    ) {
        rest
    } else if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "查找引用 ",
            "搜索引用 ",
            "find references ",
            "find refs ",
            "references ",
        ],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        let (query, path) = split_search_scope(rest);
        if query.is_empty() {
            return None;
        }
        return Some(Intent::FindReferences { query, path });
    } else if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "查找实现 ",
            "搜索实现 ",
            "find implementations ",
            "find impl ",
            "implementations ",
        ],
    ) {
        let rest = text[text.len() - rest.len()..].trim();
        let (query, path) = split_search_scope(rest);
        if query.is_empty() {
            return None;
        }
        return Some(Intent::FindImplementations { query, path });
    } else if let Some(rest) = strip_prefix_any(
        &lower,
        &[
            "搜索定义 ",
            "查找定义 ",
            "跳定义 ",
            "search definition ",
            "find definition ",
            "go to definition ",
        ],
    ) {
        rest
    } else {
        return None;
    };

    let rest = text[text.len() - rest.len()..].trim();
    let (query, path) = split_search_scope(rest);
    if query.is_empty() {
        return None;
    }

    Some(Intent::SearchSymbol { query, path })
}

fn parse_write_file_intent(text: &str, lower: &str) -> Option<Intent> {
    let rest = if let Some(rest) =
        strip_prefix_any(&lower, &["写入文件 ", "写入 ", "write file ", "write "])
    {
        rest
    } else if let Some(rest) = strip_prefix_any(&lower, &["创建文件 ", "create file "]) {
        rest
    } else {
        return None;
    };

    let rest = text[text.len() - rest.len()..].trim();

    // Format: "写入文件 path\ncontent" or "写入文件 path content" (first whitespace-delimited token is path)
    let (path, content) = if let Some(nl) = rest.find('\n') {
        let path = rest[..nl].trim();
        let content = rest[nl + 1..].to_string();
        (path.to_string(), content)
    } else {
        // Single line: first token is path, rest is content
        let mut parts = rest.splitn(2, char::is_whitespace);
        let path = parts.next().unwrap_or("").trim().to_string();
        let content = parts.next().unwrap_or("").trim().to_string();
        (path, content)
    };

    if path.is_empty() {
        return None;
    }

    Some(Intent::WriteFile { path, content })
}

fn parse_agent_ask_intent(text: &str, lower: &str) -> Option<Intent> {
    let codex_prompt =
        strip_command_prefix(text, lower, &["/codex", "问 codex", "问codex", "ask codex"]);
    if let Some(prompt) = codex_prompt {
        if prompt.is_empty() {
            return None;
        }

        return Some(Intent::AskCodex {
            prompt: prompt.to_string(),
        });
    }

    let prompt = strip_command_prefix(
        text,
        lower,
        &[
            "/copilot",
            "问 copilot",
            "问copilot",
            "问 agent",
            "问agent",
            "ask copilot",
            "ask agent",
        ],
    )?;

    if prompt.is_empty() {
        return None;
    }

    Some(Intent::AskAgent {
        prompt: prompt.to_string(),
    })
}

fn parse_agent_runtime_action_intent(text: &str, lower: &str) -> Option<Intent> {
    if matches_command_any(
        lower,
        &[
            "agent 状态",
            "agent状态",
            "查看 agent 状态",
            "查看agent状态",
            "show agent status",
            "agent status",
        ],
    ) {
        return Some(Intent::ShowAgentRunStatus);
    }

    if matches_command_any(
        lower,
        &[
            "取消 agent",
            "取消agent",
            "停止 agent",
            "停止agent",
            "cancel agent",
            "stop agent",
        ],
    ) {
        return Some(Intent::CancelAgentRun);
    }

    if let Some(rest) = strip_command_prefix(
        text,
        lower,
        &[
            "继续 agent",
            "继续agent",
            "continue agent",
            "agent continue",
        ],
    ) {
        let prompt = rest.trim();
        return Some(Intent::ContinueAgentRun {
            prompt: (!prompt.is_empty()).then(|| prompt.to_string()),
        });
    }

    if let Some(rest) = strip_command_prefix(
        text,
        lower,
        &[
            "批准 agent",
            "批准agent",
            "同意 agent",
            "同意agent",
            "approve agent",
        ],
    ) {
        let option_id = rest.trim();
        return Some(Intent::ApproveAgentRun {
            option_id: (!option_id.is_empty()).then(|| option_id.to_string()),
        });
    }

    if matches_command_any(
        lower,
        &[
            "继续本轮 agent",
            "继续当前 agent",
            "继续这轮 agent",
            "继续本轮agent",
            "继续当前agent",
            "继续这轮agent",
            "continue this run",
            "continue run",
        ],
    ) {
        return Some(Intent::ApproveAgentRun {
            option_id: Some("continue_run".to_string()),
        });
    }

    if matches_command_any(
        lower,
        &[
            "先停在这里",
            "停在这里",
            "先停这轮 agent",
            "停止在这里",
            "stop here",
        ],
    ) {
        return Some(Intent::ApproveAgentRun {
            option_id: Some("cancel_run".to_string()),
        });
    }

    if matches_command_any(
        lower,
        &[
            "批准本次写入",
            "批准这次写入",
            "允许本次写入",
            "approve this write",
        ],
    ) {
        return Some(Intent::ApproveAgentRun {
            option_id: Some("approve_tool".to_string()),
        });
    }

    if matches_command_any(
        lower,
        &[
            "拒绝本次写入",
            "拒绝这次写入",
            "不允许本次写入",
            "reject this write",
        ],
    ) {
        return Some(Intent::ApproveAgentRun {
            option_id: Some("reject_tool".to_string()),
        });
    }

    if matches_command_any(
        lower,
        &[
            "保留 agent 结果",
            "保留agent结果",
            "keep agent result",
            "keep result",
        ],
    ) {
        return Some(Intent::ApproveAgentRun {
            option_id: Some("keep_result".to_string()),
        });
    }

    if matches_command_any(
        lower,
        &[
            "回滚 agent 结果",
            "回滚agent结果",
            "撤销 agent 结果",
            "撤销agent结果",
            "revert agent result",
            "undo agent result",
            "revert result",
        ],
    ) {
        return Some(Intent::ApproveAgentRun {
            option_id: Some("revert_result".to_string()),
        });
    }

    if matches_command_any(
        lower,
        &[
            "放弃 agent 结果",
            "放弃agent结果",
            "abandon agent result",
            "abandon result",
        ],
    ) {
        return Some(Intent::ApproveAgentRun {
            option_id: Some("abandon_result".to_string()),
        });
    }

    if let Some(prompt) = strip_command_prefix(
        text,
        lower,
        &[
            "/agent",
            "agent",
            "让 agent 做",
            "让agent做",
            "自动完成",
            "start agent",
        ],
    ) {
        let prompt = prompt.trim();
        if !prompt.is_empty() {
            return Some(Intent::StartAgentRun {
                prompt: prompt.to_string(),
            });
        }
    }

    None
}

fn parse_agent_continue_intent(text: &str, lower: &str) -> Option<Intent> {
    const PREFIXES: &[&str] = &[
        "继续，",
        "继续,",
        "继续：",
        "继续:",
        "继续刚才的任务，",
        "继续刚才的任务,",
        "继续刚才的任务：",
        "继续刚才的任务:",
        "continue,",
        "continue:",
        "continue，",
        "continue：",
        "continue last task,",
        "continue last task:",
        "continue last task，",
        "continue last task：",
    ];

    for prefix in PREFIXES {
        if lower.starts_with(prefix) {
            let rest = text[prefix.len()..].trim();
            if rest.is_empty() {
                return None;
            }

            return Some(Intent::ContinueAgent {
                prompt: Some(rest.to_string()),
            });
        }
    }

    None
}

fn parse_agent_reset_intent(lower: &str) -> Option<Intent> {
    if matches_command_any(
        lower,
        &[
            "重置 copilot 会话",
            "重置copilot会话",
            "重置 agent 会话",
            "重置agent会话",
            "reset copilot session",
            "reset agent session",
        ],
    ) {
        return Some(Intent::ResetAgentSession);
    }

    None
}

fn parse_git_log_args(s: &str) -> (Option<usize>, Option<String>) {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    // Try parsing leading number as count
    let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
    if let Ok(n) = parts[0].parse::<usize>() {
        let path = parts
            .get(1)
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        return (Some(n), path);
    }

    // Otherwise treat as file path
    (None, Some(trimmed.to_string()))
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
    /plan <任务>                  — 只展示 planner 结果，不直接执行
    规划 <任务> / 给我计划 <任务> — 同上，走显式 plan mode
    执行计划 <命令1>; <命令2>   — 一步一步执行
    执行全部 <命令1>; <命令2>   — 连续执行到结束或失败
    继续                        — 做下一步
    重新执行失败步骤            — 只重试失败那一步
    批准 / 拒绝                 — 处理待审批步骤

▸ 追问
    刚才为什么失败              — 看失败原因
    把上一步结果发我            — 看上一步输出
    按建议继续                  — 直接采用上一轮 agent 的下一步建议
    继续改刚才那个文件          — 回到刚才那个文件
    把刚才的 diff 发我          — 看刚才的 diff / patch
    把刚才改动的文件列表发我    — 看刚才改了哪些文件
    撤回刚才的补丁              — 撤销刚才那次补丁

▸ VS Code
  打开 <文件路径>          — 用 VS Code 打开文件
  打开 <文件:行号>         — 打开并跳转到指定行
        选择项目                 — 打开项目选择卡片
        浏览项目                 — 浏览本机目录并选择项目
  打开文件夹 <路径>        — 打开目录
    选择项目 <路径>          — 设为当前项目并在 VS Code 打开目录
    当前项目                 — 查看当前飞书会话绑定的项目
  安装扩展 <ext.id>        — 安装 VS Code 扩展
  卸载扩展 <ext.id>        — 卸载扩展
  扩展列表                 — 列出已安装扩展
  diff <文件1> <文件2>     — 对比两个文件

▸ 工作区
    读取 <文件> [1-120]      — 读取文件，可附带行号范围
    列出 <路径>              — 列出目录内容
    搜索 <关键字> [在 路径]  — 文本搜索
    搜索正则 <模式> [在 路径] — 正则搜索
    搜索符号 <名称> [在 路径] — 搜索函数/结构体/类型定义
    查找定义 <名称> [在 路径] — 同上，也支持“跳定义”
    查找引用 <名称> [在 路径] — 搜索符号引用位置
    查找实现 <名称> [在 路径] — 搜索 impl / implements 位置
    运行测试 [命令]          — 执行默认测试命令或指定测试命令
    运行指定测试 <过滤词>    — 只运行匹配的测试
    运行测试文件 <路径>      — 按测试文件执行测试
    写入文件 <路径>\n<内容>  — 创建或覆盖文件（需审批）
    问 Copilot <问题>        — 通过 companion extension 发起一次 ask-style agent 会话
    /copilot <问题>          — 直接调用 Copilot 提问当前项目
    /codex <问题>            — 直接调用本机 Codex CLI 提问当前项目
    /agent <任务>            — 启动 autonomous agent runtime
    agent 状态              — 查看当前 agent runtime 状态
    继续 agent [要求]       — 在当前 agent runtime 中继续推进
    继续本轮 agent          — 当 agent 暂停等待时，按推荐节奏继续本轮推进
    批准 agent [option_id]  — 批准当前 agent runtime 决策；不传时默认用推荐选项
    批准本次写入           — 当 agent 等待写入审批时，批准本次工具写入
    拒绝本次写入           — 当 agent 等待写入审批时，拒绝本次工具写入
    先停在这里             — 当 agent 等待继续决策时，保持当前结果并先暂停
    保留 agent 结果         — 显式保留当前 runtime 结果快照
    回滚 agent 结果         — 显式回滚当前 runtime 结果快照
    放弃 agent 结果         — 显式放弃当前 runtime 结果快照
    取消 agent              — 取消当前 agent runtime
    继续刚才的任务          — 无待执行计划时，优先继续最近一次 agent 任务
    继续，<新的推进要求>    — 在同一 agent 会话里继续追问，例如「继续，给我最小修复建议」
    重置 Copilot 会话        — 清空当前飞书会话对应的 extension ask 历史
    应用补丁 <unified diff>  — 将补丁应用到当前工作区

▸ Git
    查看 diff [路径]         — 查看当前工作区未提交变更
    git diff [路径]          — 同上
  git status [仓库路径]    — 查看仓库状态
    同步 Git 状态 [仓库路径] — 依次执行 git status / git pull / git status
  git pull [仓库路径]      — 拉取代码
  git push [提交信息]      — 提交并推送
    git log [条数] [路径]    — 查看提交历史
    git blame <文件>         — 查看文件逐行追溯
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
        assert_eq!(parse_intent("git status"), Intent::GitStatus { repo: None });
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
        assert_eq!(parse_intent("运行测试"), Intent::RunTests { command: None });
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
    fn parse_ask_agent_chinese() {
        assert_eq!(
            parse_intent("问 Copilot parse_intent 这个函数是干什么的"),
            Intent::AskAgent {
                prompt: "parse_intent 这个函数是干什么的".to_string(),
            }
        );
    }

    #[test]
    fn parse_ask_agent_english() {
        assert_eq!(
            parse_intent("ask copilot explain parse_intent"),
            Intent::AskAgent {
                prompt: "explain parse_intent".to_string(),
            }
        );
    }

    #[test]
    fn parse_slash_copilot() {
        assert_eq!(
            parse_intent("/copilot explain parse_intent"),
            Intent::AskAgent {
                prompt: "explain parse_intent".to_string(),
            }
        );
    }

    #[test]
    fn parse_slash_codex() {
        assert_eq!(
            parse_intent("/codex explain parse_intent"),
            Intent::AskCodex {
                prompt: "explain parse_intent".to_string(),
            }
        );
    }

    #[test]
    fn parse_ask_agent_with_fullwidth_space() {
        assert_eq!(
            parse_intent("问　Copilot　parse_intent 这个函数是干什么的"),
            Intent::AskAgent {
                prompt: "parse_intent 这个函数是干什么的".to_string(),
            }
        );
    }

    #[test]
    fn parse_ask_codex_chinese() {
        assert_eq!(
            parse_intent("问 Codex parse_intent 这个函数是干什么的"),
            Intent::AskCodex {
                prompt: "parse_intent 这个函数是干什么的".to_string(),
            }
        );
    }

    #[test]
    fn parse_reset_agent_session() {
        assert_eq!(parse_intent("重置 Copilot 会话"), Intent::ResetAgentSession);
        assert_eq!(
            parse_intent("reset agent session"),
            Intent::ResetAgentSession
        );
    }

    #[test]
    fn parse_start_agent_run() {
        assert_eq!(
            parse_intent("/agent 修复当前测试失败"),
            Intent::StartAgentRun {
                prompt: "修复当前测试失败".to_string(),
            }
        );
    }

    #[test]
    fn parse_explicit_plan_prompt() {
        assert_eq!(
            parse_intent("/plan 修复当前测试失败应该怎么做"),
            Intent::ShowPlanPrompt {
                prompt: "修复当前测试失败应该怎么做".to_string(),
            }
        );
        assert_eq!(
            parse_intent("规划 当前 parser 入口还缺什么"),
            Intent::ShowPlanPrompt {
                prompt: "当前 parser 入口还缺什么".to_string(),
            }
        );
    }

    #[test]
    fn parse_agent_runtime_status() {
        assert_eq!(parse_intent("agent 状态"), Intent::ShowAgentRunStatus);
    }

    #[test]
    fn parse_agent_runtime_continue() {
        assert_eq!(
            parse_intent("继续 agent 给我最小修复"),
            Intent::ContinueAgentRun {
                prompt: Some("给我最小修复".to_string()),
            }
        );
    }

    #[test]
    fn parse_agent_result_disposition_aliases() {
        assert_eq!(
            parse_intent("保留 agent 结果"),
            Intent::ApproveAgentRun {
                option_id: Some("keep_result".to_string()),
            }
        );
        assert_eq!(
            parse_intent("回滚 agent 结果"),
            Intent::ApproveAgentRun {
                option_id: Some("revert_result".to_string()),
            }
        );
        assert_eq!(
            parse_intent("放弃 agent 结果"),
            Intent::ApproveAgentRun {
                option_id: Some("abandon_result".to_string()),
            }
        );
    }

    #[test]
    fn parse_agent_runtime_waiting_state_aliases() {
        assert_eq!(
            parse_intent("继续本轮 agent"),
            Intent::ApproveAgentRun {
                option_id: Some("continue_run".to_string()),
            }
        );
        assert_eq!(
            parse_intent("先停在这里"),
            Intent::ApproveAgentRun {
                option_id: Some("cancel_run".to_string()),
            }
        );
        assert_eq!(
            parse_intent("批准本次写入"),
            Intent::ApproveAgentRun {
                option_id: Some("approve_tool".to_string()),
            }
        );
        assert_eq!(
            parse_intent("拒绝本次写入"),
            Intent::ApproveAgentRun {
                option_id: Some("reject_tool".to_string()),
            }
        );
    }

    #[test]
    fn parse_reset_agent_session_with_fullwidth_space() {
        assert_eq!(
            parse_intent("重置　Copilot　会话"),
            Intent::ResetAgentSession
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
    fn parse_continue_agent_command() {
        assert_eq!(
            parse_intent("继续，给我最小修复建议"),
            Intent::ContinueAgent {
                prompt: Some("给我最小修复建议".to_string()),
            }
        );
        assert_eq!(
            parse_intent("continue, give me the smallest fix"),
            Intent::ContinueAgent {
                prompt: Some("give me the smallest fix".to_string()),
            }
        );
    }

    #[test]
    fn parse_project_and_git_sync_commands() {
        assert_eq!(parse_intent("选择项目"), Intent::ShowProjectPicker);
        assert_eq!(
            parse_intent("浏览项目"),
            Intent::ShowProjectBrowser { path: None }
        );
        assert_eq!(
            parse_intent("选择项目 C:/work/demo"),
            Intent::OpenFolder {
                path: "C:/work/demo".to_string(),
            }
        );
        assert_eq!(
            parse_intent("浏览项目 C:/work"),
            Intent::ShowProjectBrowser {
                path: Some("C:/work".to_string()),
            }
        );
        assert_eq!(parse_intent("当前项目"), Intent::ShowCurrentProject);
        assert_eq!(
            parse_intent("同步 Git 状态"),
            Intent::GitSync { repo: None }
        );
    }

    #[test]
    fn parse_natural_language_vscode_actions() {
        assert_eq!(
            parse_intent("把本地的改动同步到github上"),
            Intent::GitPushAll {
                repo: None,
                message: "auto commit via feishu-bridge".to_string(),
            }
        );
        assert_eq!(parse_intent("看看当前项目"), Intent::ShowCurrentProject);
        assert_eq!(
            parse_intent("浏览一下本地项目目录"),
            Intent::ShowProjectBrowser { path: None }
        );
        assert_eq!(
            parse_intent("切换到 C:/Users/beanw/OpenSource/feishu-vscode-bridge"),
            Intent::OpenFolder {
                path: "C:/Users/beanw/OpenSource/feishu-vscode-bridge".to_string(),
            }
        );
        assert_eq!(
            parse_intent("检查 README 和 docs/work_log，继续未完成的项目浏览工作"),
            Intent::AskAgent {
                prompt: "检查 README 和 docs/work_log，继续未完成的项目浏览工作".to_string(),
            }
        );
    }

    #[test]
    fn parse_natural_continue_plan_phrase() {
        assert_eq!(
            parse_intent("检查Plan或在readme，继续plan里没完成的工作"),
            Intent::ContinuePlan
        );
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
        assert_eq!(parse_intent("按建议继续"), Intent::ContinueAgentSuggested);
        assert_eq!(parse_intent("继续改刚才那个文件"), Intent::ContinueLastFile);
        assert_eq!(parse_intent("继续这个文件"), Intent::ContinueLastFile);
        assert_eq!(parse_intent("把刚才的 diff 发我"), Intent::ShowLastDiff);
        assert_eq!(parse_intent("看 diff"), Intent::ShowLastDiff);
        assert_eq!(
            parse_intent("把刚才改动的文件列表发我"),
            Intent::ShowRecentFiles
        );
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

    // ── P2.3 新命令解析测试 ──

    #[test]
    fn parse_search_symbol() {
        assert_eq!(
            parse_intent("搜索符号 parse_intent"),
            Intent::SearchSymbol {
                query: "parse_intent".to_string(),
                path: None,
            }
        );
    }

    #[test]
    fn parse_search_symbol_with_scope() {
        assert_eq!(
            parse_intent("搜索符号 Bridge 在 src"),
            Intent::SearchSymbol {
                query: "Bridge".to_string(),
                path: Some("src".to_string()),
            }
        );
    }

    #[test]
    fn parse_search_symbol_english() {
        assert_eq!(
            parse_intent("search symbol run_shell"),
            Intent::SearchSymbol {
                query: "run_shell".to_string(),
                path: None,
            }
        );
    }

    #[test]
    fn parse_go_to_definition_alias() {
        assert_eq!(
            parse_intent("跳定义 parse_intent 在 src"),
            Intent::SearchSymbol {
                query: "parse_intent".to_string(),
                path: Some("src".to_string()),
            }
        );
    }

    #[test]
    fn parse_find_references() {
        assert_eq!(
            parse_intent("查找引用 parse_intent 在 src"),
            Intent::FindReferences {
                query: "parse_intent".to_string(),
                path: Some("src".to_string()),
            }
        );
    }

    #[test]
    fn parse_find_implementations_english() {
        assert_eq!(
            parse_intent("find implementations Bridge"),
            Intent::FindImplementations {
                query: "Bridge".to_string(),
                path: None,
            }
        );
    }

    #[test]
    fn parse_run_specific_test() {
        assert_eq!(
            parse_intent("运行指定测试 parse_search"),
            Intent::RunSpecificTest {
                filter: "parse_search".to_string(),
            }
        );
    }

    #[test]
    fn parse_run_specific_test_english() {
        assert_eq!(
            parse_intent("test filter my_test_name"),
            Intent::RunSpecificTest {
                filter: "my_test_name".to_string(),
            }
        );
    }

    #[test]
    fn parse_run_test_file() {
        assert_eq!(
            parse_intent("运行测试文件 tests/approval_card_flow.rs"),
            Intent::RunTestFile {
                path: "tests/approval_card_flow.rs".to_string(),
            }
        );
    }

    #[test]
    fn parse_write_file_multiline() {
        assert_eq!(
            parse_intent("写入文件 src/demo.txt\nhello\nworld"),
            Intent::WriteFile {
                path: "src/demo.txt".to_string(),
                content: "hello\nworld".to_string(),
            }
        );
    }

    #[test]
    fn parse_write_file_english() {
        assert_eq!(
            parse_intent("write file test.txt\nsome content"),
            Intent::WriteFile {
                path: "test.txt".to_string(),
                content: "some content".to_string(),
            }
        );
    }

    #[test]
    fn parse_create_file() {
        assert_eq!(
            parse_intent("创建文件 new.txt\nfoo"),
            Intent::WriteFile {
                path: "new.txt".to_string(),
                content: "foo".to_string(),
            }
        );
    }

    #[test]
    fn parse_git_log_default() {
        assert_eq!(
            parse_intent("git log"),
            Intent::GitLog {
                count: None,
                path: None
            }
        );
        assert_eq!(
            parse_intent("提交历史"),
            Intent::GitLog {
                count: None,
                path: None
            }
        );
    }

    #[test]
    fn parse_git_log_with_count() {
        assert_eq!(
            parse_intent("git log 5"),
            Intent::GitLog {
                count: Some(5),
                path: None
            }
        );
    }

    #[test]
    fn parse_git_log_with_path() {
        assert_eq!(
            parse_intent("git log src/lib.rs"),
            Intent::GitLog {
                count: None,
                path: Some("src/lib.rs".to_string())
            }
        );
    }

    #[test]
    fn parse_git_log_with_count_and_path() {
        assert_eq!(
            parse_intent("git log 10 src/bridge.rs"),
            Intent::GitLog {
                count: Some(10),
                path: Some("src/bridge.rs".to_string())
            }
        );
    }

    #[test]
    fn parse_git_blame() {
        assert_eq!(
            parse_intent("git blame src/lib.rs"),
            Intent::GitBlame {
                path: "src/lib.rs".to_string()
            }
        );
        assert_eq!(
            parse_intent("追溯 src/main.rs"),
            Intent::GitBlame {
                path: "src/main.rs".to_string()
            }
        );
    }

    #[test]
    fn approval_policy_checks_write_file() {
        let policy = ApprovalPolicy::from_spec("write_file");

        assert!(policy.requires_approval(&Intent::WriteFile {
            path: "test.txt".to_string(),
            content: "hello".to_string(),
        }));

        let default_policy = ApprovalPolicy::default();
        assert!(default_policy.require_write_file);
    }
}
