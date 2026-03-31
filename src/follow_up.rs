use crate::bridge::{BridgeContext, BridgeResponse};
use crate::reply;
use crate::session::{self, StoredDiff, StoredResult, StoredStep};
use crate::vscode;

const NO_SESSION_TEXT: &str = "⚠️ 当前没有可回看的任务记录。";

pub fn explain_last_failure(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(stored) = session::load_persisted_session(context.session_store_path(), session_key) else {
        return BridgeResponse::Text(NO_SESSION_TEXT.to_string());
    };

    BridgeResponse::Text(reply::format_last_failure_reply(&stored))
}

pub fn show_last_result(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(stored) = session::load_persisted_session(context.session_store_path(), session_key) else {
        return BridgeResponse::Text(NO_SESSION_TEXT.to_string());
    };

    BridgeResponse::Text(reply::format_last_result_reply(&stored))
}

pub fn continue_last_file(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(stored) = session::load_persisted_session(context.session_store_path(), session_key) else {
        return BridgeResponse::Text(NO_SESSION_TEXT.to_string());
    };

    let Some(path) = stored
        .recent_file_paths
        .first()
        .map(String::as_str)
        .or(stored.last_file_path.as_deref())
    else {
        return BridgeResponse::Text("⚠️ 最近一次任务里没有记录到明确的文件路径。可以先发送「读取 <文件>」或「打开 <文件>」。".to_string());
    };

    let result = vscode::read_file(path, None, None);
    let mut blocks = vec![format!("📄 继续处理刚才的文件: {}", path)];

    if let Some(last_step) = stored.last_step.as_ref() {
        blocks.push(format!("🧾 最近一步: {}", last_step.description));
    }
    if stored.recent_file_paths.len() > 1 {
        blocks.push(format!(
            "🗂 其他最近文件: {}",
            stored.recent_file_paths[1..].join("、")
        ));
    }

    blocks.push(result.to_reply(&format!("读取文件 {path}")));
    BridgeResponse::Text(reply::format_follow_up_reply("继续文件上下文", &stored, blocks))
}

pub fn show_last_diff(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(stored) = session::load_persisted_session(context.session_store_path(), session_key) else {
        return BridgeResponse::Text(NO_SESSION_TEXT.to_string());
    };

    BridgeResponse::Text(reply::format_last_diff_reply(&stored))
}

pub fn show_recent_files(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(stored) = session::load_persisted_session(context.session_store_path(), session_key) else {
        return BridgeResponse::Text(NO_SESSION_TEXT.to_string());
    };

    BridgeResponse::Text(reply::format_recent_files_reply(&stored))
}

pub fn undo_last_patch(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(mut stored) = session::load_persisted_session(context.session_store_path(), session_key) else {
        return BridgeResponse::Text(NO_SESSION_TEXT.to_string());
    };

    let Some(last_patch) = stored.last_patch.clone() else {
        return BridgeResponse::Text("⚠️ 最近一次任务里没有可撤回的补丁记录。请先发送「应用补丁 ...」。".to_string());
    };

    let result = vscode::reverse_patch(&last_patch.content);
    let reply = result.to_reply("撤回补丁");
    stored.plan = None;
    stored.current_task = Some("撤回刚才的补丁".to_string());
    stored.pending_steps.clear();
    stored.last_action = Some("撤回补丁".to_string());
    stored.last_result = Some(StoredResult {
        status: if result.success {
            "已完成".to_string()
        } else {
            "失败暂停".to_string()
        },
        summary: if result.success {
            format!("最近一次补丁已撤回，共涉及 {} 个文件。", last_patch.file_paths.len())
        } else {
            "撤回最近一次补丁失败。".to_string()
        },
        success: result.success,
    });
    stored.last_step = Some(StoredStep {
        description: "撤回刚才的补丁".to_string(),
        reply: reply.clone(),
        success: result.success,
    });
    stored.last_file_path = last_patch.file_paths.first().cloned();
    stored.recent_file_paths = last_patch.file_paths.clone();
    stored.last_diff = Some(StoredDiff {
        description: "撤回刚才的补丁".to_string(),
        content: reply::truncate_session_text(&last_patch.content, 4000),
    });
    if result.success {
        stored.last_patch = None;
    }

    let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    BridgeResponse::Text(reply)
}