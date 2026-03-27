//! 真实 shell 命令执行器

use std::process::Command;
use std::time::Instant;

/// 命令执行结果
#[derive(Debug, Clone)]
pub struct CmdResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

impl CmdResult {
    /// 格式化为飞书消息文本
    pub fn to_reply(&self, label: &str) -> String {
        let status = if self.success { "✅" } else { "❌" };
        let mut out = format!("{status} {label}  ({}ms)\n", self.duration_ms);

        let combined = if !self.stdout.is_empty() && !self.stderr.is_empty() {
            format!("{}\n{}", self.stdout.trim(), self.stderr.trim())
        } else if !self.stdout.is_empty() {
            self.stdout.trim().to_string()
        } else {
            self.stderr.trim().to_string()
        };

        // 截断过长输出
        if combined.len() > 2000 {
            out.push_str(&combined[..2000]);
            out.push_str("\n… (输出过长已截断)");
        } else if !combined.is_empty() {
            out.push_str(&combined);
        } else {
            out.push_str("(无输出)");
        }
        out
    }
}

/// 执行一条 shell 命令，捕获 stdout / stderr
pub fn run_cmd(program: &str, args: &[&str], timeout_secs: u64) -> CmdResult {
    let start = Instant::now();

    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", &format!("{} {}", program, args.join(" "))])
        .output();

    #[cfg(not(target_os = "windows"))]
    let result = Command::new(program).args(args).output();

    let duration_ms = start.elapsed().as_millis() as u64;
    let _ = timeout_secs; // 超时机制可后续补充

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
