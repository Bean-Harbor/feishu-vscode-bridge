use serde_json::Value;

use crate::bridge_context::BridgeContext;
use crate::session;
use crate::vscode::{self, SemanticPlanAction, SemanticPlanResult};
use crate::{ExecutionMode, Intent};

pub enum SemanticDispatch {
    Planned(Intent),
    Reply(String),
}

pub fn plan_freeform_intent(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
) -> SemanticDispatch {
    let current_project = session::load_persisted_session(context.session_store_path(), session_key)
        .and_then(|stored| session::selected_project_path(&stored));

    let result = vscode::plan_semantic_intent(session_key, task_text, current_project.as_deref());
    route_plan_result(task_text, result)
}

fn route_plan_result(task_text: &str, result: SemanticPlanResult) -> SemanticDispatch {
    if !result.success {
        return SemanticDispatch::Reply(format!(
            "⚠️ 自然语言解析层当前不可用：{}\n\n请先确认 VS Code companion extension 已启动，且 http://127.0.0.1:8765/health 正常。",
            result.message
        ));
    }

    match result.status.as_str() {
        "planned" => match actions_to_intent(&result.actions) {
            Ok(intent) => SemanticDispatch::Planned(intent),
            Err(error) => SemanticDispatch::Reply(format!(
                "⚠️ 自然语言解析层返回了无法执行的动作：{error}\n\n原始请求：{task_text}"
            )),
        },
        "clarify" => SemanticDispatch::Reply(if result.message.trim().is_empty() {
            "⚠️ 我还不能可靠判断你要执行的 VS Code 动作，请补充目标项目、文件或期望结果。".to_string()
        } else {
            result.message
        }),
        _ => SemanticDispatch::Reply(format!(
            "❓ 还不能把这句话稳定映射为可执行动作：{task_text}\n\n{}",
            if result.message.trim().is_empty() {
                "可以换一种说法，或直接描述目标、文件、项目和预期结果。".to_string()
            } else {
                result.message
            }
        )),
    }
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
}