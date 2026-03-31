use crate::bridge::{BridgeContext, BridgeResponse};
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