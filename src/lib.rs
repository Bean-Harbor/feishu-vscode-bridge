#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
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
pub mod feishu;
pub mod vscode;

/// 解析用户消息为意图
pub fn parse_intent(text: &str) -> Intent {
    let text = text.trim();
    let lower = text.to_lowercase();

    // ── 帮助 ──
    if lower.is_empty() || matches!(lower.as_str(), "help" | "帮助" | "?") {
        return Intent::Help;
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

    // ── VS Code 打开文件夹 ──
    if let Some(rest) = strip_prefix_any(&lower, &["打开文件夹 ", "打开目录 ", "open folder "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::OpenFolder {
            path: rest.to_string(),
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
    if matches!(
        lower.as_str(),
        "扩展列表" | "列出扩展" | "插件列表" | "list extensions" | "list ext"
    ) {
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
    if matches!(
        lower.as_str(),
        "git status" | "git 状态" | "仓库状态" | "代码状态"
    ) {
        return Intent::GitStatus { repo: None };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["git status ", "仓库状态 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::GitStatus {
            repo: Some(rest.to_string()),
        };
    }

    // ── Git pull ──
    if matches!(lower.as_str(), "git pull" | "拉取" | "拉取代码") {
        return Intent::GitPull { repo: None };
    }
    if let Some(rest) = strip_prefix_any(&lower, &["git pull ", "拉取 "]) {
        let rest = text[text.len() - rest.len()..].trim();
        return Intent::GitPull {
            repo: Some(rest.to_string()),
        };
    }

    // ── Git push all ──
    if matches!(lower.as_str(), "git push" | "推送" | "推送代码" | "提交推送") {
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
    fn dedup_blocks_repeat() {
        let mut dedup = MessageDedup::new(600);
        assert!(!dedup.is_duplicate("msg_001"));
        assert!(dedup.is_duplicate("msg_001"));
        assert!(!dedup.is_duplicate("msg_002"));
    }
}
