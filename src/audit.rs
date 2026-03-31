use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::bridge::{BridgeResponse, render_bridge_response, response_kind};
use crate::plan::PlanProgress;
use crate::reply;
use crate::session::{self, StoredSession};

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub timestamp_ms: u128,
    pub source: String,
    pub session_key: String,
    pub chat_id: String,
    pub chat_type: Option<String>,
    pub sender_id: String,
    pub event_id: String,
    pub command: String,
    pub action_name: Option<String>,
    pub response_kind: String,
    pub response_preview: String,
    pub result_status: Option<String>,
    pub result_summary: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

fn default_audit_log_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("BRIDGE_AUDIT_LOG_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    std::env::current_dir()
        .ok()
        .map(|dir| dir.join(".feishu-vscode-bridge-audit.jsonl"))
}

pub fn feishu_session_key(chat_id: &str, sender_id: &str) -> String {
    format!("feishu:chat:{chat_id}:sender:{sender_id}")
}

pub fn new_audit_entry(
    source: &str,
    session_key: &str,
    chat_id: &str,
    chat_type: Option<&str>,
    sender_id: &str,
    event_id: &str,
    command: &str,
    response: &BridgeResponse,
    error: Option<&str>,
) -> AuditEntry {
    AuditEntry {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        source: source.to_string(),
        session_key: session_key.to_string(),
        chat_id: chat_id.to_string(),
        chat_type: chat_type.map(str::to_string),
        sender_id: sender_id.to_string(),
        event_id: event_id.to_string(),
        command: command.to_string(),
        action_name: None,
        response_kind: response_kind(response).to_string(),
        response_preview: reply::truncate_session_text(render_bridge_response(response), 300),
        result_status: None,
        result_summary: None,
        success: error.is_none(),
        error: error.map(str::to_string),
    }
}

pub(crate) fn new_plan_action_audit_entry(
    session_key: &str,
    action_name: &str,
    response: &BridgeResponse,
    stored: &StoredSession,
    progress: Option<&PlanProgress>,
) -> Option<AuditEntry> {
    let (chat_id, sender_id) = parse_feishu_session_key(session_key)?;
    let result = stored.last_result.as_ref();

    Some(AuditEntry {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
        source: "plan_action".to_string(),
        session_key: session_key.to_string(),
        chat_id,
        chat_type: None,
        sender_id,
        event_id: format!("plan_action:{action_name}"),
        command: action_name.to_string(),
        action_name: Some(action_name.to_string()),
        response_kind: response_kind(response).to_string(),
        response_preview: reply::truncate_session_text(render_bridge_response(response), 300),
        result_status: result.map(|item| item.status.clone()),
        result_summary: result.map(|item| item.summary.clone()).or_else(|| {
            progress.map(|item| session::stored_result_from_progress(item).summary)
        }),
        success: result.map(|item| item.success).unwrap_or(true),
        error: None,
    })
}

pub(crate) fn append_plan_action_audit(
    session_key: &str,
    action_name: &str,
    response: &BridgeResponse,
    stored: &StoredSession,
    progress: Option<&PlanProgress>,
) {
    let Some(entry) = new_plan_action_audit_entry(session_key, action_name, response, stored, progress) else {
        return;
    };

    if let Err(err) = append_audit_entry(&entry) {
        eprintln!("❌ 审计写入失败: {err}");
    }
}

pub(crate) fn parse_feishu_session_key(session_key: &str) -> Option<(String, String)> {
    let rest = session_key.strip_prefix("feishu:chat:")?;
    let (chat_id, sender_id) = rest.split_once(":sender:")?;
    Some((chat_id.to_string(), sender_id.to_string()))
}

pub fn append_audit_entry(entry: &AuditEntry) -> Result<(), String> {
    let Some(path) = default_audit_log_path() else {
        return Err("无法定位审计日志路径".to_string());
    };

    append_audit_entry_to_path(&path, entry)
}

pub(crate) fn append_audit_entry_to_path(path: &Path, entry: &AuditEntry) -> Result<(), String> {
    let line = serde_json::to_string(entry)
        .map_err(|err| format!("序列化审计日志失败: {err}"))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("打开审计日志失败: {err}"))?;

    writeln!(file, "{line}").map_err(|err| format!("写入审计日志失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::test_support::unique_temp_path;

    #[test]
    fn feishu_session_key_isolates_senders_in_same_chat() {
        let alice = feishu_session_key("oc_chat_demo", "ou_alice");
        let bob = feishu_session_key("oc_chat_demo", "ou_bob");

        assert_ne!(alice, bob);
        assert_eq!(alice, "feishu:chat:oc_chat_demo:sender:ou_alice");
    }

    #[test]
    fn parse_feishu_session_key_extracts_chat_and_sender() {
        let parsed = parse_feishu_session_key("feishu:chat:oc_chat_demo:sender:ou_alice").unwrap();

        assert_eq!(parsed.0, "oc_chat_demo");
        assert_eq!(parsed.1, "ou_alice");
    }

    #[test]
    fn new_plan_action_audit_entry_captures_result_status() {
        let stored = StoredSession {
            last_result: Some(session::StoredResult {
                status: "已取消".to_string(),
                summary: "当前待审批任务已被拒绝并取消。".to_string(),
                success: false,
            }),
            ..StoredSession::default()
        };

        let entry = new_plan_action_audit_entry(
            "feishu:chat:oc_chat_demo:sender:ou_alice",
            "拒绝",
            &BridgeResponse::Text("🛑 已拒绝当前待审批步骤，当前计划已取消。".to_string()),
            &stored,
            None,
        )
        .unwrap();

        assert_eq!(entry.source, "plan_action");
        assert_eq!(entry.command, "拒绝");
        assert_eq!(entry.action_name.as_deref(), Some("拒绝"));
        assert_eq!(entry.result_status.as_deref(), Some("已取消"));
        assert_eq!(entry.result_summary.as_deref(), Some("当前待审批任务已被拒绝并取消。"));
        assert!(!entry.success);
    }

    #[test]
    fn append_audit_entry_writes_jsonl_record() {
        let audit_path = unique_temp_path("audit", "audit-log");
        let entry = AuditEntry {
            timestamp_ms: 123,
            source: "message".to_string(),
            session_key: "feishu:chat:oc_chat_demo:sender:ou_alice".to_string(),
            chat_id: "oc_chat_demo".to_string(),
            chat_type: Some("group".to_string()),
            sender_id: "ou_alice".to_string(),
            event_id: "om_123".to_string(),
            command: "查看 diff".to_string(),
            action_name: None,
            response_kind: "文本".to_string(),
            response_preview: "ok".to_string(),
            result_status: None,
            result_summary: None,
            success: true,
            error: None,
        };

        append_audit_entry_to_path(&audit_path, &entry).unwrap();

        let content = fs::read_to_string(&audit_path).unwrap();
        assert!(content.contains("\"source\":\"message\""));
        assert!(content.contains("\"chat_type\":\"group\""));
        assert!(content.contains("\"command\":\"查看 diff\""));

        let _ = fs::remove_file(audit_path);
    }
}