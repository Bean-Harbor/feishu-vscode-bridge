use feishu_vscode_bridge::feishu::FeishuClient;
use feishu_vscode_bridge::{execute_continue_all, parse_intent, Intent, StepResult};

fn main() {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());

    match arg.as_str() {
        "listen" | "监听" => run_listen(),
        _ => show_help(),
    }
}

/// 启动 WebSocket 长连接，监听飞书消息并自动回复
fn run_listen() {
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

/// 处理一条来自飞书的用户消息
fn handle_message(client: &FeishuClient, msg: feishu_vscode_bridge::feishu::InboundMessage) {
    println!("📩 收到消息 [{}]: {}", msg.sender_id, msg.text);

    let reply_text = match parse_intent(&msg.text) {
        Intent::ContinueAll => {
            let mut steps = vec![
                "git -C harborbeacon-desktop status".to_string(),
                "git -C harborbeacon-desktop add -A".to_string(),
                "git -C harborbeacon-desktop push".to_string(),
            ];
            let summary = execute_continue_all(&mut steps, |s| {
                StepResult::Ok(format!("ok: {s}"))
            });
            summary.lines.join("\n")
        }
        Intent::Continue => "收到继续。请在接入层执行下一步。".to_string(),
        Intent::Status => "状态查询：待接入会话存储。".to_string(),
        Intent::Help | Intent::Unknown => {
            "bridge-cli 指令:\n  执行全部 - 一键执行所有步骤\n  继续 - 执行下一步\n  状态 - 查看进度"
                .to_string()
        }
    };

    if let Err(e) = client.reply(&msg, &reply_text) {
        eprintln!("❌ 回复失败: {e}");
    }
}

fn show_help() {
    println!("bridge-cli 用法:");
    println!("  bridge-cli listen   - 启动飞书监听模式（WebSocket 长连接）");
    println!("  bridge-cli 监听     - 同上");
}
