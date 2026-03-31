use crate::bridge::BridgeResponse;
use crate::bridge_context::BridgeContext;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::bridge::BridgeApp;
    use crate::session::{StoredPatch, StoredSession};
    use crate::ApprovalPolicy;

    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "feishu-vscode-bridge-follow-up-tests-{name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn explain_last_failure_returns_last_step_detail() {
        let session_path = unique_temp_path("failure");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("执行计划 $ false; $ pwd".to_string()),
            pending_steps: vec!["执行命令 pwd".to_string()],
            last_result: Some(StoredResult {
                status: "失败暂停".to_string(),
                summary: "第 1 / 2 步失败：执行命令 false".to_string(),
                success: false,
            }),
            last_action: Some("继续".to_string()),
            last_step: Some(StoredStep {
                description: "执行命令 false".to_string(),
                reply: "❌ $ false  (1ms)\n(exit code 1)".to_string(),
                success: false,
            }),
            last_file_path: None,
            recent_file_paths: Vec::new(),
            last_diff: None,
            last_patch: None,
        };

        session::persist_session(Some(&session_path), session_key, &stored).unwrap();

        match app.dispatch("刚才为什么失败", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 失败原因回放"));
                assert!(text.contains("🎯 当前任务: 执行计划 $ false; $ pwd"));
                assert!(text.contains("上次失败状态: 失败暂停"));
                assert!(text.contains("卡住的位置: 执行命令 false"));
                assert!(text.contains("关键报错:"));
                assert!(text.contains("下一步建议:"));
                assert!(text.contains("执行命令 false"));
                assert!(text.contains("$ false"));
            }
            BridgeResponse::Card { .. } => panic!("expected text failure explanation"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn show_last_result_returns_last_step_and_file() {
        let session_path = unique_temp_path("last-result");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("读取 src/lib.rs 1-20".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "计划已完成，共执行 1 步。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(StoredStep {
                description: "读取文件 src/lib.rs:1-20".to_string(),
                reply: "✅ 读取文件 src/lib.rs  (1ms)".to_string(),
                success: true,
            }),
            last_file_path: Some("src/lib.rs".to_string()),
            recent_file_paths: vec!["src/lib.rs".to_string()],
            last_diff: None,
            last_patch: None,
        };

        session::persist_session(Some(&session_path), session_key, &stored).unwrap();

        match app.dispatch("把上一步结果发我", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 上一步结果回放"));
                assert!(text.contains("📌 最近状态: 已完成"));
                assert!(text.contains("上一步结果: 成功"));
                assert!(text.contains("导语: 上一步已经完成"));
                assert!(text.contains("结果摘要:"));
                assert!(text.contains("下一步建议:"));
                assert!(text.contains("读取文件 src/lib.rs:1-20"));
                assert!(text.contains("相关文件: src/lib.rs"));
            }
            BridgeResponse::Card { .. } => panic!("expected text last-result reply"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn continue_last_file_reads_the_file() {
        let session_path = unique_temp_path("last-file-session");
        let file_path = unique_temp_path("last-file-target");
        fs::write(&file_path, "alpha\nbeta\n").unwrap();

        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("继续修改 demo 文件".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "最近一次读取成功。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(StoredStep {
                description: "读取文件 demo.txt".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some(file_path.to_string_lossy().to_string()),
            recent_file_paths: vec![file_path.to_string_lossy().to_string()],
            last_diff: None,
            last_patch: None,
        };

        session::persist_session(Some(&session_path), session_key, &stored).unwrap();

        match app.dispatch("继续改刚才那个文件", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 继续文件上下文"));
                assert!(text.contains("继续处理刚才的文件"));
                assert!(text.contains(file_path.to_string_lossy().as_ref()));
                assert!(text.contains("alpha"));
            }
            BridgeResponse::Card { .. } => panic!("expected text file continuation reply"),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_file(file_path);
    }

    #[test]
    fn show_last_diff_returns_patch_content() {
        let session_path = unique_temp_path("last-diff");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("应用补丁 demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "补丁已应用。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(StoredStep {
                description: "应用补丁到当前工作区".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some("src/demo.rs".to_string()),
            recent_file_paths: vec!["src/demo.rs".to_string()],
            last_diff: Some(StoredDiff {
                description: "应用补丁到当前工作区".to_string(),
                content: "diff --git a/src/demo.rs b/src/demo.rs\n--- a/src/demo.rs\n+++ b/src/demo.rs".to_string(),
            }),
            last_patch: Some(StoredPatch {
                content: "diff --git a/src/demo.rs b/src/demo.rs\n--- a/src/demo.rs\n+++ b/src/demo.rs".to_string(),
                file_paths: vec!["src/demo.rs".to_string()],
            }),
        };

        session::persist_session(Some(&session_path), session_key, &stored).unwrap();

        match app.dispatch("把刚才的 diff 发我", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 最近 diff 回放"));
                assert!(text.contains("最近一次 diff"));
                assert!(text.contains("src/demo.rs"));
                assert!(text.contains("diff --git a/src/demo.rs b/src/demo.rs"));
            }
            BridgeResponse::Card { .. } => panic!("expected text last-diff reply"),
        }

        let _ = fs::remove_file(session_path);
    }

    #[test]
    fn show_recent_files_returns_recent_file_list() {
        let session_path = unique_temp_path("recent-files");
        let app = BridgeApp::new(Some(session_path.clone()), ApprovalPolicy::default());
        let session_key = "cli";
        let stored = StoredSession {
            plan: None,
            current_task: Some("应用补丁 demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "补丁已应用。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: None,
            last_file_path: Some("src/a.rs".to_string()),
            recent_file_paths: vec!["src/a.rs".to_string(), "src/b.rs".to_string()],
            last_diff: None,
            last_patch: None,
        };

        session::persist_session(Some(&session_path), session_key, &stored).unwrap();

        match app.dispatch("把刚才改动的文件列表发我", session_key) {
            BridgeResponse::Text(text) => {
                assert!(text.contains("🧭 最近文件回放"));
                assert!(text.contains("最近改动文件列表"));
                assert!(text.contains("1. src/a.rs"));
                assert!(text.contains("2. src/b.rs"));
            }
            BridgeResponse::Card { .. } => panic!("expected text recent-files reply"),
        }

        let _ = fs::remove_file(session_path);
    }
}