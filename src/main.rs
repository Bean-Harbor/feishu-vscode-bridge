use std::sync::Mutex;

use feishu_vscode_bridge::feishu::{FeishuClient, InboundMessage};
use feishu_vscode_bridge::vscode;
use feishu_vscode_bridge::{help_text, parse_intent, Intent, MessageDedup};

static DEDUP: Mutex<Option<MessageDedup>> = Mutex::new(None);

fn main() {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());

    match arg.as_str() {
        "listen" | "监听" => run_listen(),
        _ => show_help(),
    }
}

fn run_listen() {
    // 初始化去重器（TTL 600 秒）
    *DEDUP.lock().unwrap() = Some(MessageDedup::new(600));

    let mut client = match FeishuClient::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = client.authenticate() {
        eprintln!("❌ 飞书认证失败: {e}");
        std::process::exit(1);
    }
    println!("✅ 飞书认证成功");

    if let Err(e) = client.listen(handle_message) {
        eprintln!("❌ WebSocket 断开: {e}");
        std::process::exit(1);
    }
}

fn handle_message(client: &FeishuClient, msg: InboundMessage) {
    // 去重
    if let Ok(mut guard) = DEDUP.lock() {
        if let Some(dedup) = guard.as_mut() {
            if dedup.is_duplicate(&msg.message_id) {
                return;
            }
        }
    }

    println!("📩 收到消息 [{}]: {}", msg.sender_id, msg.text);

    let reply_text = dispatch(&msg.text);

    if let Err(e) = client.reply(&msg, &reply_text) {
        eprintln!("❌ 回复失败: {e}");
    }
}

fn dispatch(text: &str) -> String {
    match parse_intent(text) {
        // ── VS Code ──
        Intent::OpenFile { path, line } => {
            let r = vscode::open_file(&path, line);
            r.to_reply(&format!("打开 {path}"))
        }
        Intent::OpenFolder { path } => {
            let r = vscode::open_folder(&path);
            r.to_reply(&format!("打开目录 {path}"))
        }
        Intent::InstallExtension { ext_id } => {
            let r = vscode::install_extension(&ext_id);
            r.to_reply(&format!("安装扩展 {ext_id}"))
        }
        Intent::UninstallExtension { ext_id } => {
            let r = vscode::uninstall_extension(&ext_id);
            r.to_reply(&format!("卸载扩展 {ext_id}"))
        }
        Intent::ListExtensions => {
            let r = vscode::list_extensions();
            r.to_reply("已安装扩展")
        }
        Intent::DiffFiles { file1, file2 } => {
            let r = vscode::diff_files(&file1, &file2);
            r.to_reply(&format!("diff {file1} {file2}"))
        }

        // ── Git ──
        Intent::GitStatus { repo } => {
            let r = vscode::git_status(repo.as_deref());
            r.to_reply("Git 状态")
        }
        Intent::GitPull { repo } => {
            let r = vscode::git_pull(repo.as_deref());
            r.to_reply("Git Pull")
        }
        Intent::GitPushAll { repo, message } => {
            let r = vscode::git_push_all(repo.as_deref(), &message);
            r.to_reply("Git Push")
        }

        // ── Shell ──
        Intent::RunShell { cmd } => {
            let r = vscode::run_shell(&cmd);
            r.to_reply(&format!("$ {cmd}"))
        }

        // ── 帮助 ──
        Intent::Help => help_text().to_string(),
        Intent::Unknown(raw) => {
            format!("❓ 无法识别指令: {raw}\n\n发送「帮助」查看可用命令")
        }
    }
}

fn show_help() {
    println!("bridge-cli 用法:");
    println!("  bridge-cli listen   - 启动飞书监听模式（WebSocket 长连接）");
    println!("  bridge-cli 监听     - 同上");
}
