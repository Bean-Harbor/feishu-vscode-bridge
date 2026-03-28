use std::sync::Mutex;

use feishu_vscode_bridge::bridge::{BridgeApp, BridgeResponse, feishu_session_key, render_bridge_response, response_kind};
use feishu_vscode_bridge::feishu::{FeishuClient, FeishuEvent, ReplyTarget};
use feishu_vscode_bridge::MessageDedup;

static DEDUP: Mutex<Option<MessageDedup>> = Mutex::new(None);

fn main() {
    let arg = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let app = BridgeApp::default();

    match arg.as_str() {
        "listen" | "监听" => run_listen(),
        "" => show_help(),
        _ => println!("{}", render_bridge_response(&app.dispatch(&arg, "cli"))),
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

    if let Err(e) = client.listen(handle_event) {
        eprintln!("❌ WebSocket 断开: {e}");
        std::process::exit(1);
    }
}

fn handle_event(client: &FeishuClient, event: FeishuEvent) {
    if let Ok(mut guard) = DEDUP.lock() {
        if let Some(dedup) = guard.as_mut() {
            if dedup.is_duplicate(event.dedup_id()) {
                return;
            }
        }
    }

    match event {
        FeishuEvent::Message(msg) => {
            println!("📩 收到消息 [{}]: {}", msg.sender_id, msg.text);

            let app = BridgeApp::default();
            let session_key = feishu_session_key(
                &msg.reply_target.receive_id_type,
                &msg.reply_target.receive_id,
            );
            let reply = app.dispatch(&msg.text, &session_key);

            println!("↩️ 准备回复 [{}]: {}", msg.sender_id, response_kind(&reply));

            if let Err(e) = send_bridge_response(client, &msg.reply_target, &reply) {
                eprintln!("❌ 回复失败: {e}");
            } else {
                println!("✅ 回复已发送 [{}]: {}", msg.sender_id, response_kind(&reply));
            }
        }
        FeishuEvent::CardAction(action) => {
            println!("🖱️ 收到卡片点击 [{}]: {}", action.sender_id, action.action_command);

            let app = BridgeApp::default();
            let session_key = feishu_session_key(
                &action.reply_target.receive_id_type,
                &action.reply_target.receive_id,
            );
            let reply = app.dispatch(&action.action_command, &session_key);

            println!("↩️ 准备卡片回复 [{}]: {}", action.sender_id, response_kind(&reply));

            if let Err(e) = send_bridge_response(client, &action.reply_target, &reply) {
                eprintln!("❌ 卡片回复失败: {e}");
            } else {
                println!("✅ 卡片回复已发送 [{}]: {}", action.sender_id, response_kind(&reply));
            }
        }
    }
}

fn send_bridge_response(
    client: &FeishuClient,
    target: &ReplyTarget,
    response: &BridgeResponse,
) -> Result<(), String> {
    match response {
        BridgeResponse::Text(text) => client.send_text_to(&target.receive_id, &target.receive_id_type, text),
        BridgeResponse::Card { fallback_text: _, card } => client.reply_card(target, card),
    }
}

fn show_help() {
    println!("bridge-cli 用法:");
    println!("  bridge-cli listen   - 启动飞书监听模式（WebSocket 长连接）");
    println!("  bridge-cli 监听     - 同上");
    println!("  bridge-cli \"执行计划 打开 Cargo.toml; git status\"   - 逐步执行计划");
    println!("  bridge-cli \"执行全部 打开 Cargo.toml; git status\"   - 连续执行计划");
}
