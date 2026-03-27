//! VS Code CLI 操作：打开文件、安装/列出扩展、运行 shell 等

use crate::executor::{run_cmd, CmdResult};

/// 打开文件（可指定行号）
pub fn open_file(path: &str, line: Option<u32>) -> CmdResult {
    let target = match line {
        Some(l) => format!("--goto {path}:{l}"),
        None => path.to_string(),
    };
    run_cmd("code", &[&target], 10)
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
    match repo_path {
        Some(p) => run_cmd("git", &["-C", p, "status", "--short"], 10),
        None => run_cmd("git", &["status", "--short"], 10),
    }
}

/// Git pull
pub fn git_pull(repo_path: Option<&str>) -> CmdResult {
    match repo_path {
        Some(p) => run_cmd("git", &["-C", p, "pull"], 30),
        None => run_cmd("git", &["pull"], 30),
    }
}

/// Git add + commit + push
pub fn git_push_all(repo_path: Option<&str>, message: &str) -> CmdResult {
    let base = match repo_path {
        Some(p) => format!("git -C {p}"),
        None => "git".to_string(),
    };
    let cmd = format!(
        "{base} add -A && {base} commit -m \"{message}\" && {base} push",
    );
    #[cfg(target_os = "windows")]
    {
        run_cmd("cmd", &["/C", &cmd], 60)
    }
    #[cfg(not(target_os = "windows"))]
    {
        run_cmd("sh", &["-c", &cmd], 60)
    }
}
