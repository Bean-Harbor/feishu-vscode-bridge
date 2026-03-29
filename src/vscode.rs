//! VS Code CLI 操作：打开文件、安装/列出扩展、运行 shell 等

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::path::Component;
use std::process::Command;
use std::time::Instant;

use crate::executor::{run_cmd, run_cmd_in_dir, CmdResult};

pub const WORKSPACE_PATH_ENV: &str = "BRIDGE_WORKSPACE_PATH";
pub const TEST_COMMAND_ENV: &str = "BRIDGE_TEST_COMMAND";

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
                "未找到 rg，请先安装 ripgrep。".to_string()
            } else {
                format!("执行搜索失败: {err}")
            }
        })?;

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

    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .args(["/C", &test_command])
        .current_dir(&workspace)
        .output();

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("sh")
        .args(["-c", &test_command])
        .current_dir(&workspace)
        .output();

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

    #[cfg(target_os = "windows")]
    {
        run_cmd_in_dir("cmd", &["/C", cmd], 30, Some(&workspace))
    }
    #[cfg(not(target_os = "windows"))]
    {
        run_cmd_in_dir("sh", &["-c", cmd], 30, Some(&workspace))
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
    fn run_shell_uses_workspace_env_as_cwd() {
        let _guard = env_lock().lock().unwrap();
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
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "new\n");

        let reverse = reverse_patch(patch);
        assert!(reverse.success, "{}", reverse.to_reply("reverse patch"));
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "old\n");

        unsafe {
            std::env::remove_var(WORKSPACE_PATH_ENV);
        }
        let _ = fs::remove_dir_all(workspace);
    }
}
