//! setup-gui — feishu-vscode-bridge 配置向导
//!
//! 运行: cargo run --bin setup-gui
//!
//! 流程：
//!   1. 欢迎页      — 介绍向导功能
//!   2. VS Code 检测 — 自动检测安装状态；未安装时引导下载
//!   3. 飞书配置    — 填写 App ID 与 App Secret
//!   4. 完成        — 配置已写入 .env

#[cfg(not(target_os = "macos"))]
use eframe::egui;
use feishu_vscode_bridge::vscode::AGENT_BRIDGE_PORT_ENV;
use std::io::Read;
#[cfg(not(target_os = "macos"))]
use std::io::Write;
#[cfg(target_os = "macos")]
use std::io::{self, Write};
use std::net::TcpStream;
#[cfg(not(target_os = "macos"))]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

// ──────────────────────────── 数据模型 ────────────────────────────

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Welcome,
    VscodeCheck,
    FeishuConfig,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VscodeStatus {
    Detected(String),
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupTaskStatus {
    ok: bool,
    label: String,
    detail: String,
}

#[cfg(not(target_os = "macos"))]
const WINDOW_WIDTH: f32 = 720.0;
#[cfg(not(target_os = "macos"))]
const WINDOW_HEIGHT: f32 = 560.0;

#[cfg(not(target_os = "macos"))]
fn short_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), ToOwned::to_owned)
}

#[cfg(not(target_os = "macos"))]
struct SetupWizard {
    step: Step,
    vscode_status: Option<VscodeStatus>,
    app_id: String,
    app_secret: String,
    config_saved: bool,
    extension_status: Option<SetupTaskStatus>,
    health_check_status: Option<SetupTaskStatus>,
    save_error: Option<String>,
    action_message: Option<String>,
}

const COMPANION_EXTENSION_ID: &str = "bean-harbor.feishu-vscode-agent-bridge";
const DEFAULT_AGENT_BRIDGE_PORT: u16 = 8765;
const HEALTH_CHECK_ATTEMPTS: usize = 12;
const HEALTH_CHECK_INTERVAL_MS: u64 = 1000;

fn save_env_file(app_id: &str, app_secret: &str) -> Result<(), String> {
    let existing = std::fs::read_to_string(".env").unwrap_or_default();
    let content = merge_env_content(&existing, app_id.trim(), app_secret.trim());
    std::fs::write(".env", content).map_err(|err| err.to_string())
}

fn merge_env_content(existing: &str, app_id: &str, app_secret: &str) -> String {
    let mut lines = Vec::new();
    let mut saw_app_id = false;
    let mut saw_app_secret = false;

    for line in existing.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            lines.push(line.to_string());
            continue;
        }

        let Some((key, _value)) = trimmed.split_once('=') else {
            lines.push(line.to_string());
            continue;
        };

        match key.trim() {
            "FEISHU_APP_ID" => {
                if !saw_app_id {
                    lines.push(format!("FEISHU_APP_ID={app_id}"));
                    saw_app_id = true;
                }
            }
            "FEISHU_APP_SECRET" => {
                if !saw_app_secret {
                    lines.push(format!("FEISHU_APP_SECRET={app_secret}"));
                    saw_app_secret = true;
                }
            }
            _ => lines.push(line.to_string()),
        }
    }

    if lines.is_empty() {
        lines.push("# feishu-vscode-bridge 配置（由 setup-gui 生成）".to_string());
    }

    if !saw_app_id || !saw_app_secret {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
            lines.push(String::new());
        }
        if !saw_app_id {
            lines.push(format!("FEISHU_APP_ID={app_id}"));
        }
        if !saw_app_secret {
            lines.push(format!("FEISHU_APP_SECRET={app_secret}"));
        }
    }

    let mut content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content
}

fn vscode_cli_candidates() -> Vec<String> {
    let mut candidates = vec!["code".to_string()];

    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(format!(
                "{local}\\Programs\\Microsoft VS Code\\bin\\code.cmd"
            ));
            candidates.push(format!("{local}\\Programs\\Microsoft VS Code\\Code.exe"));
        }
        candidates.push(r"C:\Program Files\Microsoft VS Code\bin\code.cmd".to_string());
        candidates.push(r"C:\Program Files\Microsoft VS Code\Code.exe".to_string());
        candidates.push(r"C:\Program Files (x86)\Microsoft VS Code\bin\code.cmd".to_string());
        candidates.push(r"C:\Program Files (x86)\Microsoft VS Code\Code.exe".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(
            "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code".to_string(),
        );
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(format!(
                "{home}/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
            ));
        }
    }

    #[cfg(target_os = "linux")]
    {
        candidates.push("/usr/bin/code".to_string());
        candidates.push("/usr/local/bin/code".to_string());
        candidates.push("/snap/bin/code".to_string());
    }

    candidates
}

fn run_vscode_cli(args: &[&str]) -> Result<std::process::Output, String> {
    let mut last_spawn_error = None;

    for candidate in vscode_cli_candidates() {
        match Command::new(&candidate).args(args).output() {
            Ok(output) => return Ok(output),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                last_spawn_error = Some(format!("{candidate}: {err}"));
            }
            Err(err) => {
                return Err(format!("无法执行 VS Code CLI `{candidate}`：{err}"));
            }
        }
    }

    Err(last_spawn_error.unwrap_or_else(|| "未找到可用的 VS Code CLI。".to_string()))
}

fn format_command_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "命令已执行，但没有返回输出。".to_string()
    }
}

fn workspace_dir_any() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Ok(path);
        }
    }
    std::env::current_dir().map_err(|e| format!("无法获取当前目录：{e}"))
}

fn locate_bundled_vsix() -> Option<PathBuf> {
    let workspace = workspace_dir_any().ok()?;
    let candidates = [
        workspace.join("feishu-agent-bridge.vsix"),
        workspace.join("dist/feishu-agent-bridge.vsix"),
        workspace.join("vscode-agent-bridge/feishu-agent-bridge.vsix"),
        workspace.join("vscode-agent-bridge/dist/feishu-agent-bridge.vsix"),
        workspace.join("feishu-vscode-agent-bridge.vsix"),
        workspace.join("dist/feishu-vscode-agent-bridge.vsix"),
        workspace.join("vscode-agent-bridge/feishu-vscode-agent-bridge.vsix"),
        workspace.join("vscode-agent-bridge/dist/feishu-vscode-agent-bridge.vsix"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn install_companion_extension() -> SetupTaskStatus {
    let mut args = vec!["--install-extension"];
    let install_target;
    let detail_prefix;

    if let Some(vsix_path) = locate_bundled_vsix() {
        install_target = vsix_path.display().to_string();
        detail_prefix = "已使用 bundled .vsix 安装 companion extension";
    } else {
        install_target = COMPANION_EXTENSION_ID.to_string();
        detail_prefix = "未找到本地 .vsix，已回退到 Marketplace 扩展安装";
    }

    args.push(install_target.as_str());
    args.push("--force");

    match run_vscode_cli(&args) {
        Ok(output) if output.status.success() => SetupTaskStatus {
            ok: true,
            label: "Companion extension 已安装".to_string(),
            detail: format!("{detail_prefix}。{}", format_command_output(&output)),
        },
        Ok(output) => SetupTaskStatus {
            ok: false,
            label: "Companion extension 安装失败".to_string(),
            detail: format_command_output(&output),
        },
        Err(err) => SetupTaskStatus {
            ok: false,
            label: "Companion extension 安装失败".to_string(),
            detail: err,
        },
    }
}

fn agent_bridge_port() -> u16 {
    std::env::var(AGENT_BRIDGE_PORT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_AGENT_BRIDGE_PORT)
}

fn probe_agent_bridge_health(port: u16) -> Result<(), String> {
    let address = format!("127.0.0.1:{port}");
    let timeout = Duration::from_secs(2);
    let socket = address
        .parse()
        .map_err(|err| format!("health check 地址无效：{err}"))?;

    let mut stream = TcpStream::connect_timeout(&socket, timeout)
        .map_err(|err| format!("无法连接到本地 agent bridge：{err}"))?;

    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| format!("无法设置 health check 读取超时：{err}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|err| format!("无法设置 health check 写入超时：{err}"))?;

    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .map_err(|err| format!("无法发送 health check 请求：{err}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|err| format!("无法读取 health check 响应：{err}"))?;

    if response.contains("200") && response.contains("\"status\":\"ok\"") {
        Ok(())
    } else {
        Err(format!("health check 响应异常：{}", response.trim()))
    }
}

fn launch_vscode_workspace_and_check_health() -> SetupTaskStatus {
    let dir = match workspace_dir_any() {
        Ok(dir) => dir,
        Err(err) => {
            return SetupTaskStatus {
                ok: false,
                label: "本地 bridge 健康检查失败".to_string(),
                detail: err,
            };
        }
    };

    let workspace_arg = dir.display().to_string();
    match run_vscode_cli(&["--add", workspace_arg.as_str()]) {
        Ok(output) if !output.status.success() => {
            return SetupTaskStatus {
                ok: false,
                label: "本地 bridge 健康检查失败".to_string(),
                detail: format!(
                    "无法打开 VS Code 工作区。{}",
                    format_command_output(&output)
                ),
            };
        }
        Err(err) => {
            return SetupTaskStatus {
                ok: false,
                label: "本地 bridge 健康检查失败".to_string(),
                detail: format!("无法打开 VS Code 工作区：{err}"),
            };
        }
        Ok(_) => {}
    }

    let port = agent_bridge_port();
    for _ in 0..HEALTH_CHECK_ATTEMPTS {
        match probe_agent_bridge_health(port) {
            Ok(()) => {
                return SetupTaskStatus {
                    ok: true,
                    label: "本地 bridge 健康检查通过".to_string(),
                    detail: format!(
                        "已在 http://127.0.0.1:{port}/health 收到 OK 响应，说明 companion extension 已启动本地服务。"
                    ),
                };
            }
            Err(_) => std::thread::sleep(Duration::from_millis(HEALTH_CHECK_INTERVAL_MS)),
        }
    }

    SetupTaskStatus {
        ok: false,
        label: "本地 bridge 健康检查失败".to_string(),
        detail: format!(
            "VS Code 已尝试把当前仓库加入窗口，但在 {} 秒内没有等到 http://127.0.0.1:{port}/health 返回 OK。请先确认 companion extension 已启用；如果你在做开发态联调，请优先在仓库里按 F5 启动 `Run Feishu Agent Bridge Extension`，不要继续排查监听器或飞书鉴权。",
            HEALTH_CHECK_ATTEMPTS
        ),
    }
}

fn run_post_setup_tasks() -> (SetupTaskStatus, SetupTaskStatus) {
    let extension_status = install_companion_extension();
    let health_check_status = launch_vscode_workspace_and_check_health();
    (extension_status, health_check_status)
}

fn summarize_completion_message(
    extension_status: &SetupTaskStatus,
    health_check_status: &SetupTaskStatus,
) -> String {
    format!(
        "配置已保存到项目根目录的 .env 文件。\n\n扩展安装：{}\n{}\n\n健康检查：{}\n{}",
        extension_status.label,
        extension_status.detail,
        health_check_status.label,
        health_check_status.detail,
    )
}

#[cfg(target_os = "macos")]
const FORCE_TERMINAL_SETUP_ENV: &str = "SETUP_GUI_FORCE_TERMINAL";

#[cfg(target_os = "macos")]
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|err| format!("无法启动 osascript：{err}"))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.contains("User canceled") {
        Err("用户取消了配置向导".to_string())
    } else {
        Err(format!("无法运行 macOS 原生对话框：{stderr}"))
    }
}

#[cfg(target_os = "macos")]
fn macos_choose(
    title: &str,
    message: &str,
    buttons: &[&str],
    default_button: &str,
    cancel_button: Option<&str>,
) -> Result<String, String> {
    let button_list = buttons
        .iter()
        .map(|button| format!("\"{}\"", escape_applescript(button)))
        .collect::<Vec<_>>()
        .join(", ");

    let cancel_clause = cancel_button.map_or_else(String::new, |button| {
        format!(" cancel button \"{}\"", escape_applescript(button))
    });

    let script = format!(
        "button returned of (display dialog \"{}\" with title \"{}\" buttons {{{}}} default button \"{}\"{} with icon note)",
        escape_applescript(message),
        escape_applescript(title),
        button_list,
        escape_applescript(default_button),
        cancel_clause,
    );

    run_osascript(&script)
}

#[cfg(target_os = "macos")]
fn macos_show_info(title: &str, message: &str) -> Result<(), String> {
    macos_choose(title, message, &["继续"], "继续", None).map(|_| ())
}

#[cfg(target_os = "macos")]
fn macos_prompt(title: &str, message: &str, hidden: bool) -> Result<String, String> {
    let hidden_clause = if hidden { " hidden answer true" } else { "" };
    let script = format!(
        "text returned of (display dialog \"{}\" with title \"{}\" default answer \"\" buttons {{\"取消\", \"保存\"}} default button \"保存\"{} )",
        escape_applescript(message),
        escape_applescript(title),
        hidden_clause,
    );
    Ok(run_osascript(&script)?.trim().to_string())
}

#[cfg(target_os = "macos")]
fn macos_notify_cancelled(message: &str) -> Result<(), String> {
    macos_show_info("已取消", message)
}

#[cfg(target_os = "macos")]
fn macos_prompt_required(
    title: &str,
    message: &str,
    field_name: &str,
    hidden: bool,
) -> Result<String, String> {
    loop {
        match macos_prompt(title, message, hidden) {
            Ok(value) if !value.trim().is_empty() => return Ok(value),
            Ok(_) => {
                let choice = macos_choose(
                    title,
                    &format!("{} 不能为空。", field_name),
                    &["取消", "重新输入"],
                    "重新输入",
                    Some("取消"),
                )?;
                if choice == "取消" {
                    macos_notify_cancelled("本次没有保存任何飞书配置。")?;
                    return Err("用户取消了配置向导".to_string());
                }
            }
            Err(err) if err == "用户取消了配置向导" => {
                macos_notify_cancelled("本次没有保存任何飞书配置。")?;
                return Err(err);
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(target_os = "macos")]
fn save_env_file_with_retry(app_id: &str, app_secret: &str) -> Result<(), String> {
    loop {
        match save_env_file(app_id, app_secret) {
            Ok(()) => return Ok(()),
            Err(err) => {
                let choice = macos_choose(
                    "保存失败",
                    &format!("写入 .env 失败：{err}\n\n你可以重试，或先取消本次配置。"),
                    &["取消", "重试"],
                    "重试",
                    Some("取消"),
                )?;
                if choice == "取消" {
                    macos_notify_cancelled("配置未保存，你可以稍后重新运行 setup-gui。")?;
                    return Err("用户取消了配置向导".to_string());
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn run_macos_native_setup() -> Result<(), String> {
    macos_show_info(
        "飞书 × VS Code Bridge",
        "将使用 macOS 原生对话框完成 VS Code 检测和飞书配置。",
    )?;

    let detected_version = loop {
        match detect_vscode() {
            VscodeStatus::Detected(version) => break version,
            VscodeStatus::NotFound => {
                let choice = macos_choose(
                    "未检测到 VS Code",
                    "当前没有检测到 VS Code。你可以先打开官网下载页面，安装后再点重新检测。",
                    &["取消", "打开下载页", "重新检测"],
                    "重新检测",
                    Some("取消"),
                )?;

                match choice.as_str() {
                    "打开下载页" => {
                        open::that("https://code.visualstudio.com/")
                            .map_err(|err| err.to_string())?;
                    }
                    "重新检测" => {}
                    "取消" => {
                        macos_notify_cancelled("请先安装 VS Code，之后再重新运行 setup-gui。")?;
                        return Err("用户取消了配置向导".to_string());
                    }
                    _ => unreachable!(),
                }
            }
        }
    };

    macos_show_info("已检测到 VS Code", &format!("检测结果：{detected_version}"))?;

    let app_id = macos_prompt_required("飞书配置", "请输入飞书 App ID：", "App ID", false)?;
    let app_secret =
        macos_prompt_required("飞书配置", "请输入飞书 App Secret：", "App Secret", true)?;

    save_env_file_with_retry(&app_id, &app_secret)?;
    let (extension_status, health_check_status) = run_post_setup_tasks();

    macos_show_info(
        "配置完成",
        &summarize_completion_message(&extension_status, &health_check_status),
    )?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn prompt_line(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    io::stdout().flush().map_err(|err| err.to_string())?;

    let mut buffer = String::new();
    io::stdin()
        .read_line(&mut buffer)
        .map_err(|err| err.to_string())?;
    Ok(buffer.trim().to_string())
}

#[cfg(target_os = "macos")]
fn run_terminal_setup() -> Result<(), String> {
    println!("飞书 × VS Code Bridge 配置向导（macOS 终端模式）");
    println!("当前 macOS 原生对话框不可用，已回退到终端引导模式。\n");

    match detect_vscode() {
        VscodeStatus::Detected(version) => {
            println!("已检测到 VS Code：{version}");
        }
        VscodeStatus::NotFound => {
            println!("未检测到 VS Code。请先安装后再继续：https://code.visualstudio.com/");
            return Err("未检测到 VS Code".to_string());
        }
    }

    println!();
    let app_id = prompt_line("请输入飞书 App ID: ")?;
    if app_id.trim().is_empty() {
        return Err("App ID 不能为空".to_string());
    }

    let app_secret = prompt_line("请输入飞书 App Secret: ")?;
    if app_secret.trim().is_empty() {
        return Err("App Secret 不能为空".to_string());
    }

    save_env_file(&app_id, &app_secret)?;
    let (extension_status, health_check_status) = run_post_setup_tasks();

    println!("\n配置已保存到项目根目录的 .env 文件。");
    println!("扩展安装：{}", extension_status.label);
    println!("{}", extension_status.detail);
    println!();
    println!("健康检查：{}", health_check_status.label);
    println!("{}", health_check_status.detail);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
impl Default for SetupWizard {
    fn default() -> Self {
        Self {
            step: Step::Welcome,
            vscode_status: None,
            app_id: String::new(),
            app_secret: String::new(),
            config_saved: false,
            extension_status: None,
            health_check_status: None,
            save_error: None,
            action_message: None,
        }
    }
}

// ──────────────────────────── VS Code 检测 ────────────────────────────

fn detect_vscode() -> VscodeStatus {
    // 1. 尝试从 CLI 检测版本
    if let Ok(out) = run_vscode_cli(&["--version"]) {
        if out.status.success() {
            let ver = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("unknown")
                .to_string();
            return VscodeStatus::Detected(ver);
        }
    }

    // 2. Windows：检查常见安装路径
    #[cfg(target_os = "windows")]
    {
        let mut candidates: Vec<String> = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(format!("{local}\\Programs\\Microsoft VS Code\\Code.exe"));
        }
        candidates.push(r"C:\Program Files\Microsoft VS Code\Code.exe".into());
        candidates.push(r"C:\Program Files (x86)\Microsoft VS Code\Code.exe".into());
        for p in &candidates {
            if std::path::Path::new(p).exists() {
                return VscodeStatus::Detected("(检测到安装路径)".into());
            }
        }
    }

    // 3. macOS：检查系统和用户 Applications
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![PathBuf::from("/Applications/Visual Studio Code.app")];
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(PathBuf::from(home).join("Applications/Visual Studio Code.app"));
        }

        for path in candidates {
            if path.exists() {
                return VscodeStatus::Detected("(检测到 Applications 目录)".into());
            }
        }
    }

    // 4. Linux：检查常见可执行文件路径
    #[cfg(target_os = "linux")]
    for p in &["/usr/bin/code", "/usr/local/bin/code", "/snap/bin/code"] {
        if std::path::Path::new(p).exists() {
            return VscodeStatus::Detected("(检测到安装路径)".into());
        }
    }

    VscodeStatus::NotFound
}

// ──────────────────────────── 工作区辅助函数 ────────────────────────────

/// 返回当前工作区根目录（Cargo 项目根或 cwd）。
#[cfg(not(target_os = "macos"))]
fn workspace_dir() -> Result<PathBuf, String> {
    workspace_dir_any()
}

/// 用 VS Code 将工作区目录追加到当前窗口，避免直接打开文件夹导致当前会话被替换。
#[cfg(not(target_os = "macos"))]
fn launch_vscode_for_workspace() -> Result<(), String> {
    let dir = workspace_dir()?;
    let workspace = dir.display().to_string();
    let output = run_vscode_cli(&["--add", workspace.as_str()])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "无法启动 VS Code：{}",
            format_command_output(&output)
        ))
    }
}

/// 用系统文件管理器打开工作区目录。
#[cfg(not(target_os = "macos"))]
fn open_workspace_directory() -> Result<(), String> {
    let dir = workspace_dir()?;
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(dir.as_os_str())
            .spawn()
            .map_err(|e| format!("无法打开目录：{e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(dir.as_os_str())
            .spawn()
            .map_err(|e| format!("无法打开目录：{e}"))?;
    }
    Ok(())
}

// ──────────────────────────── 中文字体加载 ────────────────────────────

#[cfg(not(target_os = "macos"))]
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    #[cfg(target_os = "windows")]
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",   // 微软雅黑
        r"C:\Windows\Fonts\simsun.ttc", // 宋体
        r"C:\Windows\Fonts\simhei.ttf", // 黑体
    ];
    #[cfg(target_os = "macos")]
    let candidates = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        // macOS 数组长度固定为 2
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let candidates = [
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];

    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts
                .font_data
                .insert("cjk".to_owned(), egui::FontData::from_owned(data));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}

// ──────────────────────────── eframe App ────────────────────────────

#[cfg(not(target_os = "macos"))]
impl eframe::App for SetupWizard {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_shell(ui);
        });
    }
}

// ──────────────────────────── 各步骤 UI ────────────────────────────

#[cfg(not(target_os = "macos"))]
impl SetupWizard {
    fn current_step_index(&self) -> usize {
        match self.step {
            Step::Welcome => 0,
            Step::VscodeCheck => 1,
            Step::FeishuConfig => 2,
            Step::Done => 3,
        }
    }

    fn render_shell(&mut self, ui: &mut egui::Ui) {
        ui.add_space(18.0);
        ui.heading("飞书 × VS Code Bridge 配置向导");
        ui.label(
            egui::RichText::new("先确认 VS Code 可用，再完成飞书机器人接入配置。")
                .color(egui::Color32::from_gray(120)),
        );
        ui.add_space(14.0);

        self.render_stepper(ui);

        ui.add_space(16.0);
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(18.0))
            .show(ui, |ui| {
                let step = self.step.clone();
                match step {
                    Step::Welcome => self.ui_welcome(ui),
                    Step::VscodeCheck => self.ui_vscode(ui),
                    Step::FeishuConfig => self.ui_feishu(ui),
                    Step::Done => self.ui_done(ui),
                }
            });
    }

    fn render_stepper(&self, ui: &mut egui::Ui) {
        let current = self.current_step_index();
        let steps = [
            ("1", "开始"),
            ("2", "检测 VS Code"),
            ("3", "配置飞书"),
            ("4", "完成"),
        ];

        ui.columns(4, |columns| {
            for (index, column) in columns.iter_mut().enumerate() {
                let is_active = index == current;
                let is_done = index < current;
                let fill = if is_active {
                    egui::Color32::from_rgb(32, 120, 210)
                } else if is_done {
                    egui::Color32::from_rgb(50, 168, 82)
                } else {
                    egui::Color32::from_gray(70)
                };

                egui::Frame::group(column.style())
                    .fill(fill.gamma_multiply(0.16))
                    .stroke(egui::Stroke::new(1.0, fill.gamma_multiply(0.45)))
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .show(column, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(steps[index].0).strong().color(fill));
                            ui.label(
                                egui::RichText::new(steps[index].1)
                                    .small()
                                    .color(egui::Color32::from_gray(170)),
                            );
                        });
                    });
            }
        });
    }

    fn set_action_result(&mut self, result: Result<(), String>, success_text: &str) {
        self.action_message = Some(match result {
            Ok(()) => success_text.to_string(),
            Err(err) => format!("操作失败：{err}"),
        });
    }

    fn render_action_message(&self, ui: &mut egui::Ui) {
        if let Some(message) = &self.action_message {
            let color = if message.starts_with("操作失败") {
                egui::Color32::from_rgb(196, 55, 55)
            } else {
                egui::Color32::from_rgb(50, 168, 82)
            };
            ui.add_space(10.0);
            ui.label(egui::RichText::new(message).small().color(color));
        }
    }

    // ── 欢迎页 ──
    fn ui_welcome(&mut self, ui: &mut egui::Ui) {
        let workspace = workspace_dir().ok();

        ui.heading("开始前准备");
        ui.add_space(8.0);
        ui.label("这个向导会按顺序完成环境检查与飞书接入，不需要手动找配置文件。 ");
        ui.add_space(12.0);

        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("本次会完成").strong());
                ui.add_space(8.0);
                ui.label("1. 检测本机是否已安装 VS Code");
                ui.label("2. 若已安装，继续填写飞书应用凭证");
                ui.label("3. 填写飞书 App ID 与 App Secret");
                ui.label("4. 自动生成 .env、安装 companion extension、执行最小健康检查");
            });

        if let Some(path) = workspace {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!("当前工作目录：{}", path.display()))
                    .small()
                    .color(egui::Color32::from_gray(150)),
            );
        }

        ui.add_space(24.0);
        ui.horizontal(|ui| {
            if ui
                .add_sized([180.0, 36.0], egui::Button::new("开始检测 VS Code"))
                .clicked()
            {
                self.action_message = None;
                self.vscode_status = Some(detect_vscode());
                self.step = Step::VscodeCheck;
            }
        });
    }

    // ── VS Code 检测 ──
    fn ui_vscode(&mut self, ui: &mut egui::Ui) {
        match self.vscode_status.clone() {
            Some(VscodeStatus::Detected(ver)) => {
                let workspace_name = workspace_dir()
                    .ok()
                    .map(|path| short_path_label(&path))
                    .unwrap_or_else(|| "当前项目".to_string());

                ui.heading("VS Code 检测通过");
                ui.add_space(8.0);
                ui.colored_label(
                    egui::Color32::from_rgb(50, 168, 82),
                    format!("已检测到 VS Code：{ver}"),
                );
                ui.add_space(12.0);

                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("检测通过后即可继续配置").strong());
                        ui.add_space(8.0);
                        ui.label(format!("• 当前项目：{workspace_name}"));
                        ui.label("• 继续下一步，填写飞书 App ID 与 App Secret");
                        ui.label("• 保存后仅更新 .env 中的飞书相关配置项");
                    });

                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("重新检测").clicked() {
                        self.action_message = None;
                        self.vscode_status = Some(detect_vscode());
                    }
                });

                self.render_action_message(ui);
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if ui.button("← 返回").clicked() {
                        self.action_message = None;
                        self.step = Step::Welcome;
                    }
                    ui.add_space(8.0);
                    if ui
                        .add_sized([120.0, 32.0], egui::Button::new("下一步  →"))
                        .clicked()
                    {
                        self.action_message = None;
                        self.step = Step::FeishuConfig;
                    }
                });
            }

            Some(VscodeStatus::NotFound) => {
                ui.heading("需要先安装 VS Code");
                ui.add_space(8.0);
                ui.colored_label(egui::Color32::RED, "未检测到 VS Code");
                ui.add_space(10.0);
                ui.label("请先安装 VS Code，安装完成后点击「重新检测」继续。");
                ui.add_space(12.0);

                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("安装步骤").strong());
                        ui.add_space(4.0);
                        ui.label("1. 点击下方「打开下载页」按钮");
                        ui.label("2. 根据您的操作系统下载并运行安装包");
                        ui.label("3. 安装完成后，回到本页面点击「重新检测」");
                    });

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button("打开 VS Code 下载页  ↗").clicked() {
                        let _ = open::that("https://code.visualstudio.com/");
                    }
                    ui.add_space(8.0);
                    if ui.button("重新检测").clicked() {
                        self.vscode_status = Some(detect_vscode());
                    }
                    if ui.button("← 返回").clicked() {
                        self.step = Step::Welcome;
                    }
                });
            }

            // 初始状态：理论上不会出现，但做保底处理
            None => {
                ui.spinner();
                ui.label("正在检测...");
                self.vscode_status = Some(detect_vscode());
            }
        }
    }

    // ── 飞书配置 ──
    fn ui_feishu(&mut self, ui: &mut egui::Ui) {
        let input_width = ui.available_width();

        ui.heading("填写飞书应用配置");
        ui.add_space(8.0);

        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("你需要准备").strong());
                ui.add_space(8.0);
                ui.label("1. 在飞书开放平台创建应用后获取的 App ID");
                ui.label("2. 对应的 App Secret");
                ui.label("3. 保存后会直接写入项目根目录 .env");
            });

        ui.add_space(14.0);

        ui.label(egui::RichText::new("飞书 App ID  *").strong());
        ui.label(
            egui::RichText::new("在飞书开放平台「凭证与基础信息」页面获取。")
                .small()
                .color(egui::Color32::GRAY),
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.app_id)
                .hint_text("cli_xxxxxxxxxxxxxxxx")
                .desired_width(input_width),
        );
        ui.add_space(16.0);

        ui.label(egui::RichText::new("飞书 App Secret  *").strong());
        ui.label(
            egui::RichText::new("与 App ID 配对的密钥，请妥善保管。")
                .small()
                .color(egui::Color32::GRAY),
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.app_secret)
                .hint_text("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
                .password(true)
                .desired_width(input_width),
        );
        ui.add_space(20.0);

        // 错误提示
        if let Some(err) = &self.save_error.clone() {
            ui.colored_label(egui::Color32::RED, format!("⚠  保存失败：{}", err));
            ui.add_space(8.0);
        }

        let can_proceed = !self.app_id.trim().is_empty() && !self.app_secret.trim().is_empty();

        ui.horizontal(|ui| {
            if ui.button("← 上一步").clicked() {
                self.step = Step::VscodeCheck;
                self.save_error = None;
                self.action_message = None;
            }
            ui.add_space(8.0);
            if ui
                .add_enabled(can_proceed, egui::Button::new("保存并完成  →"))
                .clicked()
            {
                match self.save_config() {
                    Ok(()) => {
                        self.config_saved = true;
                        self.save_error = None;
                        self.step = Step::Done;
                    }
                    Err(e) => {
                        self.save_error = Some(e);
                    }
                }
            }
        });

        if !can_proceed {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("请填写 App ID 和 App Secret 后再继续")
                    .small()
                    .color(egui::Color32::from_rgb(220, 160, 0)),
            );
        }
    }

    // ── 完成页 ──
    fn ui_done(&mut self, ui: &mut egui::Ui) {
        ui.heading("配置完成");
        ui.add_space(10.0);

        if self.config_saved {
            ui.colored_label(
                egui::Color32::from_rgb(50, 168, 82),
                "配置已保存至项目根目录下的 .env 文件",
            );
        }

        ui.add_space(14.0);
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("接下来可以做什么").strong());
                ui.add_space(8.0);
                ui.label("1. 先看下面的扩展安装和健康检查结果");
                ui.label("2. 如果失败，可以直接重试安装或重试自检");
                ui.label("3. 仍然异常时，再手动打开项目继续排查");
            });

        ui.add_space(14.0);
        if let Some(status) = &self.extension_status {
            let color = if status.ok {
                egui::Color32::from_rgb(50, 168, 82)
            } else {
                egui::Color32::from_rgb(196, 55, 55)
            };
            ui.colored_label(color, &status.label);
            ui.label(&status.detail);
            ui.add_space(10.0);
        }

        if let Some(status) = &self.health_check_status {
            let color = if status.ok {
                egui::Color32::from_rgb(50, 168, 82)
            } else {
                egui::Color32::from_rgb(196, 55, 55)
            };
            ui.colored_label(color, &status.label);
            ui.label(&status.detail);
            ui.add_space(10.0);
        }

        ui.add_space(14.0);

        ui.horizontal_wrapped(|ui| {
            if ui.button("重新安装 extension").clicked() {
                self.extension_status = Some(install_companion_extension());
            }
            if ui.button("重新执行健康检查").clicked() {
                self.health_check_status = Some(launch_vscode_workspace_and_check_health());
            }
            if ui.button("用 VS Code 打开当前项目").clicked() {
                self.set_action_result(
                    launch_vscode_for_workspace(),
                    "已尝试用 VS Code 打开当前项目。",
                );
            }
            if ui.button("打开项目目录").clicked() {
                self.set_action_result(open_workspace_directory(), "已打开当前项目目录。");
            }
            if ui.button("重新配置").clicked() {
                *self = SetupWizard {
                    vscode_status: Some(detect_vscode()),
                    step: Step::VscodeCheck,
                    ..SetupWizard::default()
                };
            }
        });

        self.render_action_message(ui);
    }

    // ── 保存配置到 .env ──
    fn save_config(&mut self) -> Result<(), String> {
        save_env_file(&self.app_id, &self.app_secret)?;
        let (extension_status, health_check_status) = run_post_setup_tasks();
        self.extension_status = Some(extension_status);
        self.health_check_status = Some(health_check_status);
        Ok(())
    }
}

// ──────────────────────────── 入口 ────────────────────────────

#[cfg(target_os = "macos")]
fn main() -> eframe::Result<()> {
    let result = if std::env::var_os(FORCE_TERMINAL_SETUP_ENV).is_some() {
        run_terminal_setup()
    } else {
        match run_macos_native_setup() {
            Ok(()) => Ok(()),
            Err(err)
                if err.starts_with("无法启动 osascript")
                    || err.starts_with("无法运行 macOS 原生对话框") =>
            {
                eprintln!("macOS 原生对话框不可用，切换到终端模式：{err}");
                run_terminal_setup()
            }
            Err(err) => Err(err),
        }
    };

    if let Err(err) = result {
        eprintln!("配置失败：{err}");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("飞书 × VS Code Bridge 配置向导")
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "飞书 × VS Code Bridge 配置向导",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(SetupWizard::default()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::merge_env_content;

    #[test]
    fn merge_env_content_preserves_unrelated_entries() {
        let existing = "FOO=bar\nBAR=baz\n";

        let merged = merge_env_content(existing, "cli_new", "secret_new");

        assert!(merged.contains("FOO=bar\nBAR=baz\n"));
        assert!(merged.contains("FEISHU_APP_ID=cli_new\n"));
        assert!(merged.contains("FEISHU_APP_SECRET=secret_new\n"));
    }

    #[test]
    fn merge_env_content_replaces_existing_feishu_entries() {
        let existing = "# keep\nFEISHU_APP_ID=old_id\nFEISHU_APP_SECRET=old_secret\nOTHER=value\n";

        let merged = merge_env_content(existing, "cli_new", "secret_new");

        assert!(merged.contains("# keep\n"));
        assert!(merged.contains("OTHER=value\n"));
        assert!(merged.contains("FEISHU_APP_ID=cli_new\n"));
        assert!(merged.contains("FEISHU_APP_SECRET=secret_new\n"));
        assert!(!merged.contains("old_id"));
        assert!(!merged.contains("old_secret"));
    }

    #[test]
    fn merge_env_content_deduplicates_feishu_entries() {
        let existing = "FEISHU_APP_ID=old_id\nFEISHU_APP_ID=duplicate\nFEISHU_APP_SECRET=old_secret\nFEISHU_APP_SECRET=duplicate\n";

        let merged = merge_env_content(existing, "cli_new", "secret_new");

        assert_eq!(merged.matches("FEISHU_APP_ID=").count(), 1);
        assert_eq!(merged.matches("FEISHU_APP_SECRET=").count(), 1);
        assert!(merged.contains("FEISHU_APP_ID=cli_new\n"));
        assert!(merged.contains("FEISHU_APP_SECRET=secret_new\n"));
    }
}
