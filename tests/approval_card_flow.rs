use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use feishu_vscode_bridge::ApprovalPolicy;
use feishu_vscode_bridge::Intent;
use feishu_vscode_bridge::bridge::{BridgeApp, BridgeResponse};
use feishu_vscode_bridge::plan::ExecutionOutcome;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn fake_executor(intent: &Intent) -> ExecutionOutcome {
    match intent {
        Intent::GitPull { .. } => ExecutionOutcome {
            success: true,
            reply: "fake git pull ok".to_string(),
        },
        Intent::GitStatus { .. } => ExecutionOutcome {
            success: true,
            reply: "fake git status ok".to_string(),
        },
        Intent::RunShell { cmd } => ExecutionOutcome {
            success: true,
            reply: format!("fake shell ok: {cmd}"),
        },
        _ => ExecutionOutcome {
            success: false,
            reply: "unexpected test intent".to_string(),
        },
    }
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "feishu-vscode-bridge-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn session_store_path(dir: &Path) -> PathBuf {
    dir.join(".feishu-vscode-bridge-session.json")
}

#[test]
fn execute_all_approval_flow_completes_after_approve() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = unique_temp_dir("approve");
    let session_path = session_store_path(&temp_dir);
    let app = BridgeApp::with_executor(
        Some(session_path.clone()),
        ApprovalPolicy::from_spec("git_pull"),
        fake_executor,
    );
    let session_key = "feishu:chat:approve-flow";

    let start = app.dispatch("执行全部 git pull; git status", session_key);
    match start {
        BridgeResponse::Card { fallback_text, card } => {
            assert!(fallback_text.contains("待审批步骤"));
            assert!(card.to_string().contains("确认继续"));
            assert!(card.to_string().contains("取消这步"));
        }
        BridgeResponse::Text(text) => panic!("expected approval card, got text: {text}"),
    }
    assert!(session_path.exists());

    let approved = app.dispatch("批准", session_key);
    match approved {
        BridgeResponse::Card { fallback_text, card } => {
            assert!(fallback_text.contains("计划执行完成"));
            assert_eq!(card["header"]["title"]["content"], "已完成");
            assert!(card.to_string().contains("fake git pull ok"));
            assert!(card.to_string().contains("查看当前仓库状态"));
        }
        BridgeResponse::Text(text) => panic!("expected completion card, got text: {text}"),
    }

    assert!(session_path.exists());
    let continued = app.dispatch("继续刚才的任务", session_key);
    match continued {
        BridgeResponse::Text(text) => {
            assert!(text.contains("🧭 任务连续性回放"));
            assert!(text.contains("🎯 当前任务: 执行全部 git pull; git status"));
            assert!(text.contains("📌 最近状态: 已完成"));
            assert!(text.contains("🧾 上次动作: 批准"));
        }
        BridgeResponse::Card { .. } => panic!("expected text summary after completed task"),
    }

    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn approval_reject_clears_pending_session() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = unique_temp_dir("reject");
    let session_path = session_store_path(&temp_dir);
    let app = BridgeApp::with_executor(
        Some(session_path.clone()),
        ApprovalPolicy::from_spec("git_pull"),
        fake_executor,
    );
    let session_key = "feishu:chat:reject-flow";

    let start = app.dispatch("执行计划 git pull; git status", session_key);
    match start {
        BridgeResponse::Card { fallback_text, card } => {
            assert!(fallback_text.contains("待审批步骤"));
            assert!(card.to_string().contains("确认继续"));
        }
        BridgeResponse::Text(text) => panic!("expected approval card, got text: {text}"),
    }
    assert!(session_path.exists());

    let rejected = app.dispatch("拒绝", session_key);
    match rejected {
        BridgeResponse::Text(text) => {
            assert!(text.contains("已拒绝当前待审批步骤"));
        }
        BridgeResponse::Card { .. } => panic!("expected text reply after rejection"),
    }

    assert!(session_path.exists());
    let continued = app.dispatch("继续", session_key);
    match continued {
        BridgeResponse::Text(text) => {
            assert!(text.contains("🧭 任务连续性回放"));
            assert!(text.contains("🎯 当前任务: 执行计划 git pull; git status"));
            assert!(text.contains("📌 最近状态: 已取消"));
            assert!(text.contains("🧾 上次动作: 拒绝"));
        }
        BridgeResponse::Card { .. } => panic!("expected text summary after rejected task"),
    }

    fs::remove_dir_all(temp_dir).unwrap();
}