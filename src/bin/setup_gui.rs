//! setup-gui — feishu-vscode-bridge 配置向导
//!
//! 运行: cargo run --bin setup-gui
//!
//! 流程：
//!   1. 欢迎页      — 介绍向导功能
//!   2. VS Code 检测 — 自动检测安装状态；未安装时引导下载
//!   3. 飞书配置    — 填写 Webhook URL 与签名密钥
//!   4. 完成        — 配置已写入 .env

#[cfg(not(target_os = "macos"))]
use eframe::egui;
use std::io::{self, Write};
#[cfg(not(target_os = "macos"))]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

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

#[cfg(not(target_os = "macos"))]
const WINDOW_WIDTH: f32 = 720.0;
#[cfg(not(target_os = "macos"))]
const WINDOW_HEIGHT: f32 = 560.0;

fn workspace_dir() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|err| err.to_string())
}

fn open_workspace_directory() -> Result<(), String> {
    let workspace = workspace_dir()?;
    open::that(workspace).map_err(|err| err.to_string())?;
    Ok(())
}

fn launch_vscode_for_workspace() -> Result<(), String> {
    let workspace = workspace_dir()?;

    if let Ok(status) = Command::new("code")
        .arg(".")
        .current_dir(&workspace)
        .status()
    {
        if status.success() {
            return Ok(());
        }
    }

    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(PathBuf::from(format!(
                "{local}\\Programs\\Microsoft VS Code\\Code.exe"
            )));
        }
        candidates.push(PathBuf::from(
            r"C:\Program Files\Microsoft VS Code\Code.exe",
        ));
        candidates.push(PathBuf::from(
            r"C:\Program Files (x86)\Microsoft VS Code\Code.exe",
        ));

        for path in candidates {
            if path.exists() {
                let status = Command::new(path)
                    .arg(".")
                    .current_dir(&workspace)
                    .status()
                    .map_err(|err| err.to_string())?;
                if status.success() {
                    return Ok(());
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .args(["-a", "Visual Studio Code", "."])
            .current_dir(&workspace)
            .status()
            .map_err(|err| err.to_string())?;
        if status.success() {
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        for binary in [
            "code",
            "/usr/bin/code",
            "/usr/local/bin/code",
            "/snap/bin/code",
        ] {
            let result = Command::new(binary)
                .arg(".")
                .current_dir(&workspace)
                .status();
            if let Ok(status) = result {
                if status.success() {
                    return Ok(());
                }
            }
        }
    }

    Err("未能启动 VS Code，请确认已安装并允许从命令行打开。".to_string())
}

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
    save_error: Option<String>,
    action_message: Option<String>,
}

fn save_env_file(app_id: &str, app_secret: &str) -> Result<(), String> {
    let content = format!(
        "# feishu-vscode-bridge 配置（由 setup-gui 生成）\n\
         FEISHU_APP_ID={}\n\
         FEISHU_APP_SECRET={}\n",
        app_id.trim(),
        app_secret.trim(),
    );
    std::fs::write(".env", content).map_err(|err| err.to_string())
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
fn prompt_yes_no(prompt: &str) -> Result<bool, String> {
    let answer = prompt_line(prompt)?;
    Ok(matches!(answer.to_lowercase().as_str(), "y" | "yes"))
}

#[cfg(target_os = "macos")]
fn run_terminal_setup() -> Result<(), String> {
    println!("飞书 × VS Code Bridge 配置向导（macOS 终端模式）");
    println!("当前 macOS 上图形向导存在底层窗口库兼容问题，已自动切换到终端引导模式。\n");

    match detect_vscode() {
        VscodeStatus::Detected(version) => {
            println!("已检测到 VS Code：{version}");
        }
        VscodeStatus::NotFound => {
            println!("未检测到 VS Code。请先安装后再继续：https://code.visualstudio.com/");
            return Err("未检测到 VS Code".to_string());
        }
    }

    if prompt_yes_no("是否现在用 VS Code 打开当前项目？(y/N): ")? {
        launch_vscode_for_workspace()?;
        println!("已尝试用 VS Code 打开当前项目。");
    }

    if prompt_yes_no("是否打开当前项目目录？(y/N): ")? {
        open_workspace_directory()?;
        println!("已打开当前项目目录。");
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

    println!("\n配置已保存到项目根目录的 .env 文件。");
    println!("下一步可运行：cargo run --bin bridge-cli -- listen");
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
            save_error: None,
            action_message: None,
        }
    }
}

// ──────────────────────────── VS Code 检测 ────────────────────────────

fn detect_vscode() -> VscodeStatus {
    // 1. 尝试从 PATH 运行 `code --version`
    if let Ok(out) = Command::new("code").arg("--version").output() {
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
                ui.label("2. 若已安装，可直接打开当前项目目录或用 VS Code 打开项目");
                ui.label("3. 填写飞书机器人 Webhook 与签名密钥");
                ui.label("4. 自动生成项目根目录下的 .env 配置文件");
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
                        ui.label(egui::RichText::new("你现在可以直接执行").strong());
                        ui.add_space(8.0);
                        ui.label(format!("• 用 VS Code 打开 {workspace_name}"));
                        ui.label("• 打开当前工作目录，检查生成的配置文件");
                        ui.label("• 继续下一步，填写飞书机器人配置");
                    });

                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("用 VS Code 打开当前项目").clicked() {
                        self.set_action_result(
                            launch_vscode_for_workspace(),
                            "已尝试用 VS Code 打开当前项目。",
                        );
                    }
                    if ui.button("打开项目目录").clicked() {
                        self.set_action_result(open_workspace_directory(), "已打开当前项目目录。");
                    }
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

        let can_proceed =
            !self.app_id.trim().is_empty() && !self.app_secret.trim().is_empty();

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
                ui.label("1. 进入项目目录，确认 .env 内容");
                ui.label("2. 使用 VS Code 打开项目继续开发或调试");
                ui.label("3. 运行下面的命令启动 bridge-cli");
            });

        ui.add_space(14.0);
        ui.label("启动命令");
        ui.code("cargo run --bin bridge-cli -- listen");
        ui.add_space(14.0);

        ui.horizontal_wrapped(|ui| {
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
    fn save_config(&self) -> Result<(), String> {
        save_env_file(&self.app_id, &self.app_secret)
    }
}

// ──────────────────────────── 入口 ────────────────────────────

#[cfg(target_os = "macos")]
fn main() -> eframe::Result<()> {
    if let Err(err) = run_terminal_setup() {
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
