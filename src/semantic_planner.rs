use serde_json::Value;

use crate::agent_backend;
use crate::bridge::BridgeResponse;
use crate::bridge_context::BridgeContext;
use crate::card::{self, SemanticConfirmChoice};
use crate::session;
use crate::vscode::{SemanticPlanAction, SemanticPlanOption, SemanticPlanResult};
use crate::{ExecutionMode, Intent};

pub enum SemanticDispatch {
    Planned(Intent),
    Response(BridgeResponse),
}

pub fn plan_freeform_intent(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
) -> SemanticDispatch {
    let current_project = session::load_persisted_session(context.session_store_path(), session_key)
        .and_then(|stored| session::selected_project_path(&stored));

    let result = agent_backend::plan_semantic_intent(session_key, task_text, current_project.as_deref());
    route_plan_result(task_text, result)
}

pub fn show_plan_prompt(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    prompt: &str,
) -> BridgeResponse {
    let current_project = session::load_persisted_session(context.session_store_path(), session_key)
        .and_then(|stored| session::selected_project_path(&stored));
    let result = agent_backend::plan_semantic_intent(session_key, prompt, current_project.as_deref());
    let response = render_plan_mode_reply(task_text, &result);

    if result.success {
        let reply_text = match &response {
            BridgeResponse::Text(text) => text.clone(),
            BridgeResponse::Card { fallback_text, .. } => fallback_text.clone(),
        };
        let stored = session::stored_session_from_semantic_plan_result(
            task_text,
            &Intent::ShowPlanPrompt {
                prompt: prompt.to_string(),
            },
            &result,
            &reply_text,
            current_project,
            plan_result_pending_steps(&result),
        );
        let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    }

    response
}

fn route_plan_result(task_text: &str, result: SemanticPlanResult) -> SemanticDispatch {
    if !result.success {
        return SemanticDispatch::Response(BridgeResponse::Text(format!(
            "⚠️ 自然语言解析层当前不可用：{}\n\n请先确认 VS Code companion extension 已启动，且 http://127.0.0.1:8765/health 正常。",
            result.message
        )));
    }

    match result.decision.as_str() {
        "execute" => {
            if should_force_git_confirmation(task_text, &result) {
                let choices = confirmation_choices_for_ambiguous_git_request(&result);
                return SemanticDispatch::Response(card::format_semantic_confirm_reply(
                    result
                        .summary_for_user
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or(task_text),
                    non_empty_message(
                        result.message.as_str(),
                        "这句话可能表示只推送已提交内容，也可能表示自动提交后再推送，先确认更安全。",
                    ),
                    &choices,
                    result.confidence,
                    Some("medium"),
                ));
            }

            match actions_to_intent(&result.actions) {
                Ok(intent) => SemanticDispatch::Planned(intent),
                Err(error) => SemanticDispatch::Response(BridgeResponse::Text(format!(
                    "⚠️ 自然语言解析层返回了无法执行的动作：{error}\n\n原始请求：{task_text}"
                ))),
            }
        }
        "confirm" => {
            let choices = plan_options_to_choices(&result.options);
            if choices.is_empty() {
                SemanticDispatch::Response(BridgeResponse::Text(format!(
                    "🤔 这句话存在歧义或执行风险，建议先确认具体动作：{task_text}\n\n{}",
                    non_empty_message(
                        &result.message,
                        "可以补充你希望的是仅查看状态、只推送已提交内容，还是自动提交后再推送。"
                    )
                )))
            } else {
                SemanticDispatch::Response(card::format_semantic_confirm_reply(
                    result
                        .summary_for_user
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or(task_text),
                    non_empty_message(
                        result.message.as_str(),
                        "这句话有多个合理解释，先选一个更接近你真实意图的动作。",
                    ),
                    &choices,
                    result.confidence,
                    result.risk.as_deref(),
                ))
            }
        }
        "clarify" => SemanticDispatch::Response(BridgeResponse::Text(
            if result.message.trim().is_empty() {
                "⚠️ 我还不能可靠判断你要执行的 VS Code 动作，请补充目标项目、文件或期望结果。".to_string()
            } else {
                result.message
            },
        )),
        _ => SemanticDispatch::Response(BridgeResponse::Text(format!(
            "❓ 还不能把这句话稳定映射为可执行动作：{task_text}\n\n{}",
            if result.message.trim().is_empty() {
                "可以换一种说法，或直接描述目标、文件、项目和预期结果。".to_string()
            } else {
                result.message
            }
        ))),
    }
}

fn non_empty_message<'a>(message: &'a str, fallback: &'a str) -> &'a str {
    if message.trim().is_empty() {
        fallback
    } else {
        message
    }
}

fn plan_options_to_choices(options: &[SemanticPlanOption]) -> Vec<SemanticConfirmChoice> {
    options
        .iter()
        .filter_map(|option| {
            let label = option.label.trim();
            let command = option.command.trim();
            if label.is_empty() || command.is_empty() {
                return None;
            }

            Some(SemanticConfirmChoice {
                label: label.to_string(),
                command: command.to_string(),
                note: option
                    .note
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string),
                is_primary: option.primary,
            })
        })
        .collect()
}

fn should_force_git_confirmation(task_text: &str, result: &SemanticPlanResult) -> bool {
    if !looks_like_ambiguous_git_sync_request(task_text) {
        return false;
    }

    result.actions.iter().any(|action| {
        matches!(
            action.name.trim(),
            "git_sync" | "git_pull" | "git_push_all" | "git_status"
        )
    })
}

fn looks_like_ambiguous_git_sync_request(task_text: &str) -> bool {
    let lower = task_text.trim().to_ascii_lowercase();
    let mentions_github = lower.contains("github") || task_text.contains("GitHub");
    let mentions_sync = task_text.contains("同步") || lower.contains("sync");
    let mentions_local_changes = task_text.contains("本地")
        || task_text.contains("改动")
        || task_text.contains("变更")
        || task_text.contains("代码")
        || task_text.contains("提交");

    mentions_github && mentions_sync && mentions_local_changes
}

fn confirmation_choices_for_ambiguous_git_request(
    result: &SemanticPlanResult,
) -> Vec<SemanticConfirmChoice> {
    let choices = plan_options_to_choices(&result.options);
    if !choices.is_empty() {
        return choices;
    }

    vec![
        SemanticConfirmChoice {
            label: "仅推送已提交内容".to_string(),
            command: "git push".to_string(),
            note: Some("不会自动创建 commit。".to_string()),
            is_primary: true,
        },
        SemanticConfirmChoice {
            label: "自动提交并推送".to_string(),
            command: "git push auto commit via feishu-bridge".to_string(),
            note: Some("会自动 add/commit/push。".to_string()),
            is_primary: false,
        },
        SemanticConfirmChoice {
            label: "先看状态".to_string(),
            command: "同步 Git 状态".to_string(),
            note: Some("先确认当前仓库里有哪些未提交改动。".to_string()),
            is_primary: false,
        },
    ]
}

fn actions_to_intent(actions: &[SemanticPlanAction]) -> Result<Intent, String> {
    match actions.len() {
        0 => Err("planner 没有返回任何动作。".to_string()),
        1 => action_to_intent(&actions[0]),
        _ => {
            let mut steps = Vec::with_capacity(actions.len());
            for action in actions {
                let intent = action_to_intent(action)?;
                if !intent.is_runnable() {
                    return Err(format!("动作 {} 不能作为计划步骤执行。", action.name));
                }
                steps.push(intent);
            }

            Ok(Intent::RunPlan {
                steps,
                mode: ExecutionMode::StepByStep,
            })
        }
    }
}

fn action_to_intent(action: &SemanticPlanAction) -> Result<Intent, String> {
    match action.name.trim() {
        "ask_agent" => Ok(Intent::AskAgent {
            prompt: required_string_arg(&action.args, "prompt")?,
        }),
        "continue_agent" => Ok(Intent::ContinueAgent {
            prompt: optional_string_arg(&action.args, "prompt"),
        }),
        "continue_plan" => Ok(Intent::ContinuePlan),
        "continue_agent_suggested" => Ok(Intent::ContinueAgentSuggested),
        "show_project_picker" => Ok(Intent::ShowProjectPicker),
        "show_project_browser" => Ok(Intent::ShowProjectBrowser {
            path: optional_string_arg(&action.args, "path"),
        }),
        "show_current_project" => Ok(Intent::ShowCurrentProject),
        "open_folder" => Ok(Intent::OpenFolder {
            path: required_string_arg(&action.args, "path")?,
        }),
        "open_file" => Ok(Intent::OpenFile {
            path: required_string_arg(&action.args, "path")?,
            line: optional_u32_arg(&action.args, "line"),
        }),
        "read_file" => Ok(Intent::ReadFile {
            path: required_string_arg(&action.args, "path")?,
            start_line: optional_usize_arg(&action.args, "startLine"),
            end_line: optional_usize_arg(&action.args, "endLine"),
        }),
        "list_directory" => Ok(Intent::ListDirectory {
            path: optional_string_arg(&action.args, "path"),
        }),
        "search_text" => Ok(Intent::SearchText {
            query: required_string_arg(&action.args, "query")?,
            path: optional_string_arg(&action.args, "path"),
            is_regex: optional_bool_arg(&action.args, "isRegex").unwrap_or(false),
        }),
        "search_symbol" => Ok(Intent::SearchSymbol {
            query: required_string_arg(&action.args, "query")?,
            path: optional_string_arg(&action.args, "path"),
        }),
        "find_references" => Ok(Intent::FindReferences {
            query: required_string_arg(&action.args, "query")?,
            path: optional_string_arg(&action.args, "path"),
        }),
        "find_implementations" => Ok(Intent::FindImplementations {
            query: required_string_arg(&action.args, "query")?,
            path: optional_string_arg(&action.args, "path"),
        }),
        "run_tests" => Ok(Intent::RunTests {
            command: optional_string_arg(&action.args, "command"),
        }),
        "run_specific_test" => Ok(Intent::RunSpecificTest {
            filter: required_string_arg(&action.args, "filter")?,
        }),
        "run_test_file" => Ok(Intent::RunTestFile {
            path: required_string_arg(&action.args, "path")?,
        }),
        "git_diff" => Ok(Intent::GitDiff {
            path: optional_string_arg(&action.args, "path"),
        }),
        "git_status" => Ok(Intent::GitStatus {
            repo: optional_string_arg(&action.args, "repo"),
        }),
        "git_sync" => Ok(Intent::GitSync {
            repo: optional_string_arg(&action.args, "repo"),
        }),
        "git_pull" => Ok(Intent::GitPull {
            repo: optional_string_arg(&action.args, "repo"),
        }),
        "git_push_all" => Ok(Intent::GitPushAll {
            repo: optional_string_arg(&action.args, "repo"),
            message: optional_string_arg(&action.args, "message")
                .unwrap_or_else(|| "auto commit via feishu-bridge".to_string()),
        }),
        "git_log" => Ok(Intent::GitLog {
            count: optional_usize_arg(&action.args, "count"),
            path: optional_string_arg(&action.args, "path"),
        }),
        "git_blame" => Ok(Intent::GitBlame {
            path: required_string_arg(&action.args, "path")?,
        }),
        "reset_agent_session" => Ok(Intent::ResetAgentSession),
        "help" => Ok(Intent::Help),
        other => Err(format!("不支持的 planner 动作: {other}")),
    }
}

fn required_string_arg(args: &Value, key: &str) -> Result<String, String> {
    optional_string_arg(args, key).ok_or_else(|| format!("动作参数缺少 {key}"))
}

fn optional_string_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn optional_bool_arg(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn optional_usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|value| value.as_u64().and_then(|number| usize::try_from(number).ok()))
        .or_else(|| {
            args.get(key)
                .and_then(Value::as_str)
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
}

fn optional_u32_arg(args: &Value, key: &str) -> Option<u32> {
    optional_usize_arg(args, key).and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn converts_single_git_sync_action() {
        let intent = actions_to_intent(&[SemanticPlanAction {
            name: "git_sync".to_string(),
            args: json!({}),
        }])
        .unwrap();

        assert_eq!(intent, Intent::GitSync { repo: None });
    }

    #[test]
    fn converts_multiple_actions_into_step_plan() {
        let intent = actions_to_intent(&[
            SemanticPlanAction {
                name: "show_current_project".to_string(),
                args: json!({}),
            },
            SemanticPlanAction {
                name: "git_status".to_string(),
                args: json!({}),
            },
        ])
        .unwrap();

        match intent {
            Intent::RunPlan { steps, mode } => {
                assert_eq!(mode, ExecutionMode::StepByStep);
                assert_eq!(steps.len(), 2);
                assert_eq!(steps[0], Intent::ShowCurrentProject);
                assert_eq!(steps[1], Intent::GitStatus { repo: None });
            }
            other => panic!("expected run plan, got {other:?}"),
        }
    }

    #[test]
    fn confirm_options_convert_into_card_choices() {
        let choices = plan_options_to_choices(&[
            SemanticPlanOption {
                label: "仅推送已提交内容".to_string(),
                command: "git push".to_string(),
                note: Some("不会自动创建 commit。".to_string()),
                primary: true,
            },
            SemanticPlanOption {
                label: "先看状态".to_string(),
                command: "git status".to_string(),
                note: None,
                primary: false,
            },
        ]);

        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].command, "git push");
        assert!(choices[0].is_primary);
        assert_eq!(choices[1].label, "先看状态");
    }

    #[test]
    fn pending_steps_fall_back_to_options_when_actions_absent() {
        let result = SemanticPlanResult {
            success: true,
            decision: "confirm".to_string(),
            message: "需要先确认你想怎么做。".to_string(),
            summary: None,
            summary_for_user: Some("建议先确认执行方式。".to_string()),
            confidence: Some(0.7),
            risk: Some("medium".to_string()),
            actions: Vec::new(),
            options: vec![
                SemanticPlanOption {
                    label: "只看状态".to_string(),
                    command: "同步 Git 状态".to_string(),
                    note: None,
                    primary: true,
                },
                SemanticPlanOption {
                    label: "".to_string(),
                    command: "git push".to_string(),
                    note: None,
                    primary: false,
                },
            ],
            error: None,
        };

        assert_eq!(
            plan_result_pending_steps(&result),
            vec![
                "只看状态 -> 同步 Git 状态".to_string(),
                "git push".to_string(),
            ]
        );
    }

    #[test]
    fn ambiguous_github_sync_execute_result_is_forced_to_confirm_card() {
        let result = SemanticPlanResult {
            success: true,
            decision: "execute".to_string(),
            message: "Planner mapped the request to git_sync.".to_string(),
            summary: Some("同步仓库".to_string()),
            summary_for_user: Some("准备把当前项目的本地改动同步到 GitHub。".to_string()),
            confidence: Some(0.92),
            risk: Some("low".to_string()),
            actions: vec![SemanticPlanAction {
                name: "git_sync".to_string(),
                args: json!({}),
            }],
            options: Vec::new(),
            error: None,
        };

        match route_plan_result("把本地改动同步到github上", result) {
            SemanticDispatch::Response(BridgeResponse::Card { fallback_text, card }) => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("需要先确认"));
                assert!(card_text.contains("请确认下一步"));
                assert!(card_text.contains("仅推送已提交内容"));
                assert!(card_text.contains("自动提交并推送"));
                assert!(card_text.contains("同步 Git 状态"));
            }
            _ => panic!("expected confirmation card"),
        }
    }

    #[test]
    fn non_ambiguous_execute_result_still_dispatches_directly() {
        let result = SemanticPlanResult {
            success: true,
            decision: "execute".to_string(),
            message: "Planner mapped the request to git_status.".to_string(),
            summary: None,
            summary_for_user: None,
            confidence: Some(0.95),
            risk: Some("low".to_string()),
            actions: vec![SemanticPlanAction {
                name: "git_status".to_string(),
                args: json!({}),
            }],
            options: Vec::new(),
            error: None,
        };

        match route_plan_result("看看当前仓库状态", result) {
            SemanticDispatch::Planned(Intent::GitStatus { repo: None }) => {}
            _ => panic!("expected direct git status dispatch"),
        }
    }
}

fn render_plan_mode_reply(task_text: &str, result: &SemanticPlanResult) -> BridgeResponse {
    if !result.success {
        return BridgeResponse::Text(format!(
            "⚠️ Plan 模式当前不可用：{}\n\n请先确认 VS Code companion extension 已启动。",
            result.message
        ));
    }

    let choices = plan_options_to_choices(&result.options);
    let action_lines = result
        .actions
        .iter()
        .map(describe_semantic_action)
        .collect::<Vec<_>>();

    card::format_semantic_plan_reply(
        task_text,
        result.decision.as_str(),
        result
            .summary_for_user
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(task_text),
        non_empty_message(result.message.as_str(), "Planner returned an empty explanation."),
        &action_lines,
        &choices,
        result.confidence,
        result.risk.as_deref(),
    )
}

fn plan_result_pending_steps(result: &SemanticPlanResult) -> Vec<String> {
    let action_steps = result
        .actions
        .iter()
        .map(describe_semantic_action)
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();

    if !action_steps.is_empty() {
        return action_steps;
    }

    result
        .options
        .iter()
        .filter_map(|option| {
            let label = option.label.trim();
            let command = option.command.trim();
            match (label.is_empty(), command.is_empty()) {
                (true, true) => None,
                (false, true) => Some(label.to_string()),
                (true, false) => Some(command.to_string()),
                (false, false) => Some(format!("{} -> {}", label, command)),
            }
        })
        .collect()
}

fn describe_semantic_action(action: &SemanticPlanAction) -> String {
    let details = match action.name.as_str() {
        "ask_agent" => optional_string_arg(&action.args, "prompt").unwrap_or_else(|| "向 Copilot 追问".to_string()),
        "continue_agent" => optional_string_arg(&action.args, "prompt").unwrap_or_else(|| "继续最近的 agent 任务".to_string()),
        "read_file" => optional_string_arg(&action.args, "path").map(|path| format!("读取文件 {path}")).unwrap_or_else(|| "读取文件".to_string()),
        "search_text" => optional_string_arg(&action.args, "query").map(|query| format!("搜索文本 {query}")).unwrap_or_else(|| "搜索文本".to_string()),
        "run_tests" => optional_string_arg(&action.args, "command").map(|cmd| format!("运行测试 {cmd}")).unwrap_or_else(|| "运行默认测试".to_string()),
        "git_status" => "查看 Git 状态".to_string(),
        "git_sync" => "同步 Git 状态".to_string(),
        "git_pull" => "拉取 Git 更新".to_string(),
        "git_push_all" => "提交并推送改动".to_string(),
        other => format!("执行动作 {other}"),
    };

    format!("- {}", details)
}