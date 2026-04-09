use crate::audit;
use crate::bridge::BridgeResponse;
use crate::bridge_context::BridgeContext;
use crate::card;
use crate::plan::{ApprovalRequest, PlanSession};
use crate::reply;
use crate::session::{self, StoredSession};
use crate::{ApprovalPolicy, ExecutionMode, Intent};

pub fn start_plan(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    steps: Vec<Intent>,
    mode: ExecutionMode,
) -> BridgeResponse {
    let mut session = PlanSession::new(steps);
    let progress = match mode {
        ExecutionMode::StepByStep => session.execute_next_with_policy(
            context.executor(),
            |step_index, step_number, intent, run_all_after_approval| {
                build_approval_request(
                    context.approval_policy(),
                    step_index,
                    step_number,
                    intent,
                    run_all_after_approval,
                )
            },
        ),
        ExecutionMode::ContinueAll => session.execute_remaining_with_policy(
            context.executor(),
            |step_index, step_number, intent, run_all_after_approval| {
                build_approval_request(
                    context.approval_policy(),
                    step_index,
                    step_number,
                    intent,
                    run_all_after_approval,
                )
            },
        ),
    };
    let stored = session::build_stored_session(
        session::StoredSessionKind::Plan,
        if progress.completed {
            None
        } else {
            Some(session.clone())
        },
        task_text,
        action_label_for_mode(&mode),
        &progress,
    );
    let reply = card::format_plan_reply(
        &progress,
        matches!(mode, ExecutionMode::ContinueAll),
        context.approval_policy(),
        &stored,
    );
    let _ = session::persist_session(context.session_store_path(), session_key, &stored);

    reply
}

pub fn resume_plan(
    context: &BridgeContext<'_>,
    session_key: &str,
    run_all: bool,
    action_name: &str,
) -> BridgeResponse {
    let Some(mut stored) =
        session::load_persisted_session(context.session_store_path(), session_key)
    else {
        return BridgeResponse::Text("⚠️ 当前没有待继续的计划。\n\n发送「执行计划 <命令1>; <命令2>」创建逐步计划，或发送「执行全部 <命令1>; <命令2>」连续执行。".to_string());
    };

    let Some(mut session) = stored.plan.take() else {
        return BridgeResponse::Text(reply::format_stored_session_summary(&stored));
    };

    let progress = if run_all {
        session.execute_remaining_with_policy(
            context.executor(),
            |step_index, step_number, intent, run_all_after_approval| {
                build_approval_request(
                    context.approval_policy(),
                    step_index,
                    step_number,
                    intent,
                    run_all_after_approval,
                )
            },
        )
    } else {
        session.execute_next_with_policy(
            context.executor(),
            |step_index, step_number, intent, run_all_after_approval| {
                build_approval_request(
                    context.approval_policy(),
                    step_index,
                    step_number,
                    intent,
                    run_all_after_approval,
                )
            },
        )
    };
    stored = session::build_stored_session(
        session::StoredSessionKind::Plan,
        if progress.completed {
            None
        } else {
            Some(session.clone())
        },
        stored.current_task.as_deref().unwrap_or("继续当前计划"),
        action_name,
        &progress,
    );
    let reply = card::format_plan_reply(&progress, run_all, context.approval_policy(), &stored);
    let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    audit::append_plan_action_audit(session_key, action_name, &reply, &stored, Some(&progress));

    reply
}

pub fn approve_plan(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(mut stored) =
        session::load_persisted_session(context.session_store_path(), session_key)
    else {
        return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
    };

    let Some(mut session) = stored.plan.take() else {
        return BridgeResponse::Text(reply::format_stored_session_summary(&stored));
    };

    if !session.has_pending_approval() {
        return BridgeResponse::Text(
            "⚠️ 当前没有待审批步骤。可以发送「继续」或「执行全部」推进计划。".to_string(),
        );
    }

    let progress = session.approve_pending_with_policy(
        context.executor(),
        |step_index, step_number, intent, run_all_after_approval| {
            build_approval_request(
                context.approval_policy(),
                step_index,
                step_number,
                intent,
                run_all_after_approval,
            )
        },
    );
    stored = session::build_stored_session(
        session::StoredSessionKind::Plan,
        if progress.completed {
            None
        } else {
            Some(session.clone())
        },
        stored.current_task.as_deref().unwrap_or("批准当前计划"),
        "批准",
        &progress,
    );
    let reply = card::format_plan_reply(&progress, false, context.approval_policy(), &stored);
    let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    audit::append_plan_action_audit(session_key, "批准", &reply, &stored, Some(&progress));

    reply
}

pub fn reject_plan(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let Some(mut stored) =
        session::load_persisted_session(context.session_store_path(), session_key)
    else {
        return BridgeResponse::Text("⚠️ 当前没有待审批的计划。".to_string());
    };

    let Some(mut session) = stored.plan.take() else {
        return BridgeResponse::Text(reply::format_stored_session_summary(&stored));
    };

    if !session.reject_pending() {
        return BridgeResponse::Text("⚠️ 当前没有待审批步骤。".to_string());
    }

    stored.plan = None;
    stored.pending_steps = Vec::new();
    stored.last_action = Some("拒绝".to_string());
    stored.last_result = Some(session::StoredResult {
        status: "已取消".to_string(),
        summary: "当前待审批任务已被拒绝并取消。".to_string(),
        success: false,
    });
    stored.last_step = None;
    let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    let reply = BridgeResponse::Text("🛑 已拒绝当前待审批步骤，当前计划已取消。".to_string());
    audit::append_plan_action_audit(session_key, "拒绝", &reply, &stored, None);
    reply
}

fn build_approval_request(
    approval_policy: &ApprovalPolicy,
    step_index: usize,
    step_number: usize,
    intent: &Intent,
    run_all_after_approval: bool,
) -> Option<ApprovalRequest> {
    if !approval_policy.requires_approval(intent) {
        return None;
    }

    let (reason, risk_summary) = match intent {
        Intent::RunShell { .. } => (
            "shell 命令默认需要人工确认。".to_string(),
            "会在本地 shell 中执行命令，并可能修改工作区或系统状态。".to_string(),
        ),
        Intent::ApplyPatch { .. } => (
            "补丁会直接修改工作区文件。".to_string(),
            "会把补丁写入当前仓库中的一个或多个文件。".to_string(),
        ),
        Intent::WriteFile { path, .. } => (
            format!("写入文件 {path} 前需要人工确认。"),
            format!("会创建或覆盖文件 {path}。"),
        ),
        Intent::GitPushAll { .. } => (
            "推送到远端仓库前需要人工确认。".to_string(),
            "会提交当前改动并把提交推送到远端。".to_string(),
        ),
        Intent::GitPull { .. } => (
            "拉取远端仓库前需要人工确认。".to_string(),
            "会把远端变更合入本地工作区。".to_string(),
        ),
        Intent::InstallExtension { ext_id } => (
            format!("安装扩展 {ext_id} 前需要人工确认。"),
            format!("会在当前 VS Code 环境里安装扩展 {ext_id}。"),
        ),
        Intent::UninstallExtension { ext_id } => (
            format!("卸载扩展 {ext_id} 前需要人工确认。"),
            format!("会从当前 VS Code 环境里移除扩展 {ext_id}。"),
        ),
        _ => (
            "该步骤已命中当前审批策略。".to_string(),
            "执行前需要人工确认。".to_string(),
        ),
    };

    Some(ApprovalRequest {
        step_index,
        step_number,
        intent: intent.clone(),
        action_label: reply::describe_intent(intent),
        reason,
        risk_summary,
        run_all_after_approval,
    })
}

fn action_label_for_mode(mode: &ExecutionMode) -> &'static str {
    match mode {
        ExecutionMode::StepByStep => "执行计划",
        ExecutionMode::ContinueAll => "执行全部",
    }
}

#[allow(dead_code)]
fn _stored_session_type_hint(_: &StoredSession) {}
