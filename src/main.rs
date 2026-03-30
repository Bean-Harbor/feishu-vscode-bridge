use std::sync::Mutex;

use feishu_vscode_bridge::audit::{append_audit_entry, feishu_session_key, new_audit_entry};
use feishu_vscode_bridge::bridge::{
    BridgeApp, BridgeResponse, render_bridge_response, response_kind,
};
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
            println!(
                "📩 收到消息 [{}][{}]: {}",
                msg.sender_id, msg.message_type, msg.text
            );

            let session_key = feishu_session_key(&msg.chat_id, &msg.sender_id);
            let reply = if let Some(reason) = msg.unsupported_reason.as_ref() {
                println!("⚠️ 检测到非纯文本输入 [{}]: {}", msg.sender_id, msg.message_type);
                BridgeResponse::Text(reason.clone())
            } else {
                let app = BridgeApp::default();
                app.dispatch(&msg.text, &session_key)
            };

            println!("↩️ 准备回复 [{}]: {}", msg.sender_id, response_kind(&reply));

            let send_result = send_bridge_response(client, &msg.reply_target, &reply);

            if let Err(e) = &send_result {
                eprintln!("❌ 回复失败: {e}");
            } else {
                println!("✅ 回复已发送 [{}]: {}", msg.sender_id, response_kind(&reply));
            }

            let audit = new_audit_entry(
                "message",
                &session_key,
                &msg.chat_id,
                Some(&msg.chat_type),
                &msg.sender_id,
                &msg.message_id,
                &msg.text,
                &reply,
                send_result.as_ref().err().map(|err| err.as_str()),
            );
            if let Err(err) = append_audit_entry(&audit) {
                eprintln!("❌ 审计写入失败: {err}");
            }
        }
        FeishuEvent::CardAction(action) => {
            println!("🖱️ 收到卡片点击 [{}]: {}", action.sender_id, action.action_command);

            let app = BridgeApp::default();
            let session_key = feishu_session_key(&action.reply_target.receive_id, &action.sender_id);
            let reply = app.dispatch(&action.action_command, &session_key);

            println!("↩️ 准备卡片回复 [{}]: {}", action.sender_id, response_kind(&reply));

            let send_result = send_bridge_response(client, &action.reply_target, &reply);

            if let Err(e) = &send_result {
                eprintln!("❌ 卡片回复失败: {e}");
            } else {
                println!("✅ 卡片回复已发送 [{}]: {}", action.sender_id, response_kind(&reply));
            }

            let audit = new_audit_entry(
                "card_action",
                &session_key,
                &action.reply_target.receive_id,
                None,
                &action.sender_id,
                &action.event_id,
                &action.action_command,
                &reply,
                send_result.as_ref().err().map(|err| err.as_str()),
            );
            if let Err(err) = append_audit_entry(&audit) {
                eprintln!("❌ 审计写入失败: {err}");
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
