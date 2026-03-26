use feishu_vscode_bridge::{execute_continue_all, parse_intent, Intent, StepResult};

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "help".to_string());
    match parse_intent(&arg) {
        Intent::ContinueAll => {
            let mut steps = vec![
                "git -C harborbeacon-desktop status".to_string(),
                "git -C harborbeacon-desktop add -A".to_string(),
                "git -C harborbeacon-desktop push".to_string(),
            ];
            let summary = execute_continue_all(&mut steps, |s| {
                // Demo executor: replace this with real Feishu/VS Code execution.
                StepResult::Ok(format!("ok: {s}"))
            });
            for line in summary.lines {
                println!("{}", line);
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