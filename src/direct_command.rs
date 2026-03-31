use crate::bridge::BridgeResponse;
use crate::bridge_context::BridgeContext;
use crate::plan::ExecutionOutcome;
use crate::reply;
use crate::session;
use crate::vscode;
use crate::Intent;

pub fn execute_direct_command(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    intent: Intent,
) -> BridgeResponse {
    if let Intent::AskAgent { prompt } = &intent {
        let result = vscode::ask_agent(session_key, prompt);
        let reply = reply::format_agent_reply(task_text, &result);
        let stored = session::stored_session_from_agent_result(task_text, &intent, &result, &reply);
        let _ = session::persist_session(context.session_store_path(), session_key, &stored);
        return BridgeResponse::Text(reply);
    }

    if let Intent::ResetAgentSession = &intent {
        let result = vscode::reset_agent_session(session_key);
        let outcome = ExecutionOutcome {
            success: result.success,
            reply: result.to_reply("重置 Copilot 会话"),
        };
        let progress = session::progress_from_direct_execution(intent, outcome.clone());
        let stored = session::build_stored_session(None, task_text, "直接执行", &progress);
        let _ = session::persist_session(context.session_store_path(), session_key, &stored);
        return BridgeResponse::Text(outcome.reply);
    }

    let outcome = context.executor()(&intent);
    let reply = outcome.reply.clone();
    let progress = session::progress_from_direct_execution(intent, outcome);
    let stored = session::build_stored_session(None, task_text, "直接执行", &progress);
    let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    BridgeResponse::Text(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::bridge::BridgeApp;
    use crate::test_support::unique_temp_path;
    use crate::ApprovalPolicy;

    #[test]
    fn direct_command_persists_session_context() {
        let session_path = unique_temp_path("direct-command", "direct-session");
        let file_path = unique_temp_path("direct-command", "direct-file");
        fs::write(&file_path, "alpha\nbeta\n").unwrap();

        fn fake_executor(intent: &Intent) -> ExecutionOutcome {
            ExecutionOutcome {
                success: true,
                reply: format!("ok: {}", reply::describe_intent(intent)),
            }
        }

        let app = BridgeApp::with_executor(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            fake_executor,
        );

        match app.dispatch(&format!("读取 {} 1-1", file_path.to_string_lossy()), "cli") {
            BridgeResponse::Text(text) => assert!(text.contains("ok: 读取文件")),
            BridgeResponse::Card { .. } => panic!("expected direct text reply"),
        }

        match app.dispatch("继续改刚才那个文件", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains(file_path.to_string_lossy().as_ref()));
                assert!(text.contains("alpha"));
            }
            BridgeResponse::Card { .. } => panic!("expected file continuation reply"),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_file(file_path);
    }
}