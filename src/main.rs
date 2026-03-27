use feishu_vscode_bridge::feishu::FeishuClient;
use feishu_vscode_bridge::{execute_continue_all, parse_intent, Intent, StepResult};

fn main() {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "help".to_string());

    match parse_intent(&arg) {
        Intent::ContinueAll => {
            // 1. 连接飞书
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

            // 2. 解析发送目标（P2P 单聊 / 群聊均可）
            let (receive_id, receive_id_type) = match client.resolve_target() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("❌ {e}");
                    std::process::exit(1);
                }
            };
            println!("📢 目标: {receive_id} ({receive_id_type})");

            // 3. 执行步骤并把结果发到飞书
            let mut steps = vec![
                "git -C harborbeacon-desktop status".to_string(),
                "git -C harborbeacon-desktop add -A".to_string(),
                "git -C harborbeacon-desktop push".to_string(),
            ];
            let summary = execute_continue_all(&mut steps, |s| {
                StepResult::Ok(format!("ok: {s}"))
            });

            let report = summary.lines.join("\n");
            println!("{report}");

            // 4. 发送到飞书
            match client.send_text_to(&receive_id, &receive_id_type, &report) {
                Ok(()) => println!("✅ 执行报告已发送到飞书"),
                Err(e) => eprintln!("❌ 发送飞书消息失败: {e}"),
            }
        }
        Intent::Continue => println!("收到继续。请在接入层执行下一步。"),
        Intent::Status => println!("状态查询：待接入会话存储。"),
        Intent::Help | Intent::Unknown => {
            println!("bridge-cli 用法:");
            println!("  bridge-cli 执行全部");
            println!("  bridge-cli 继续");
            println!("  bridge-cli 状态");
        }
    }
}
