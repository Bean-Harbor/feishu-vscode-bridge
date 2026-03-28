//! VS Code CLI 操作：打开文件、安装/列出扩展、运行 shell 等

use std::process::Command;
use std::time::Instant;

use crate::executor::{run_cmd, CmdResult};

pub const WORKSPACE_PATH_ENV: &str = "BRIDGE_WORKSPACE_PATH";

/// 打开文件（可指定行号）
pub fn open_file(path: &str, line: Option<u32>) -> CmdResult {
    match line {
        Some(line) => {
            let target = format!("{path}:{line}");
            run_cmd("code", &["--goto", &target], 10)
        }
        None => run_cmd("code", &[path], 10),
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
    run_cmd("code", &[path], 10)
}

/// 用 VS Code 执行 diff
pub fn diff_files(file1: &str, file2: &str) -> CmdResult {
    run_cmd("code", &["--diff", file1, file2], 10)
}

/// 执行任意 shell 命令（用户通过飞书发送时需要谨慎）
pub fn run_shell(cmd: &str) -> CmdResult {
    #[cfg(target_os = "windows")]
    {
        run_cmd("cmd", &["/C", cmd], 30)
    }
    #[cfg(not(target_os = "windows"))]
    {
        run_cmd("sh", &["-c", cmd], 30)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_repo_path_prefers_explicit_value() {
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
        unsafe {
            std::env::set_var(WORKSPACE_PATH_ENV, "   ");
        }

        assert_eq!(resolve_repo_path(None), None);

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }
    }
}
