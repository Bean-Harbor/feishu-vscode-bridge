use serde_json::json;

use crate::bridge::BridgeResponse;
use crate::plan::PlanProgress;
use crate::reply;
use crate::session::StoredSession;
use crate::vscode;
use crate::ApprovalPolicy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectChoice {
    pub label: String,
    pub path: String,
    pub note: Option<String>,
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryChoice {
    pub label: String,
    pub path: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticConfirmChoice {
    pub label: String,
    pub command: String,
    pub note: Option<String>,
    pub is_primary: bool,
}

pub fn format_semantic_plan_reply(
    task_text: &str,
    decision: &str,
    summary_for_user: &str,
    message: &str,
    action_lines: &[String],
    choices: &[SemanticConfirmChoice],
    confidence: Option<f32>,
    risk: Option<&str>,
) -> BridgeResponse {
    let template = match decision.trim().to_ascii_lowercase().as_str() {
        "confirm" => "orange",
        "clarify" => "red",
        _ => "blue",
    };

    let mut fallback_lines = vec![format!("🧭 Plan 模式\n\n任务: {}", task_text.trim())];
    fallback_lines.push(format!("结果: {}", summary_for_user.trim()));
    if !message.trim().is_empty() && message.trim() != summary_for_user.trim() {
        fallback_lines.push(message.trim().to_string());
    }
    if !action_lines.is_empty() {
        fallback_lines.push(format!("候选动作:\n{}", action_lines.join("\n")));
    }

    let mut elements = vec![json!({
        "tag": "div",
        "fields": [
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**任务**\n{}", task_text.trim())
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**决策**\n{}", decision)
                }
            }
        ]
    })];

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**规划摘要**\n{}", summary_for_user.trim())
        }
    }));

    if !message.trim().is_empty() && message.trim() != summary_for_user.trim() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**说明**\n{}", message.trim())
            }
        }));
    }

    let mut meta_fields = Vec::new();
    if let Some(risk) = risk_label(risk) {
        meta_fields.push(json!({
            "is_short": true,
            "text": {
                "tag": "lark_md",
                "content": format!("**风险**\n{}", risk)
            }
        }));
    }
    if let Some(confidence_label) = confidence_label(confidence) {
        meta_fields.push(json!({
            "is_short": true,
            "text": {
                "tag": "lark_md",
                "content": format!("**置信度**\n{}", confidence_label)
            }
        }));
    }
    if !meta_fields.is_empty() {
        elements.push(json!({
            "tag": "div",
            "fields": meta_fields
        }));
    }

    if !action_lines.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**候选动作**\n{}", action_lines.join("\n"))
            }
        }));
    }

    if !choices.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**可选下一步**"
            }
        }));

        for chunk in choices.chunks(3) {
            let actions = chunk
                .iter()
                .map(|choice| {
                    let mut action = json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": choice.label
                        },
                        "value": {
                            "command": choice.command
                        }
                    });

                    if choice.is_primary {
                        action["type"] = json!("primary");
                    }

                    action
                })
                .collect::<Vec<_>>();

            elements.push(json!({
                "tag": "action",
                "actions": actions
            }));
        }
    }

    BridgeResponse::Card {
        fallback_text: fallback_lines.join("\n\n"),
        card: json!({
            "config": {
                "wide_screen_mode": true
            },
            "header": {
                "template": template,
                "title": {
                    "tag": "plain_text",
                    "content": "Plan 模式"
                }
            },
            "elements": elements
        }),
    }
}

pub fn format_agent_reply_card(
    task_text: &str,
    action_label: &str,
    result: &vscode::AgentAskResult,
) -> BridgeResponse {
    let fallback_text = reply::format_agent_reply_with_action(task_text, action_label, result);
    let template = match result.status.trim().to_ascii_lowercase().as_str() {
        "blocked" => "red",
        "waiting_user" => "orange",
        "completed" | "answered" => "green",
        _ => "blue",
    };

    let mut elements = vec![json!({
        "tag": "div",
        "fields": [
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**任务**\n{}", task_text.trim())
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**上次动作**\n{}", action_label)
                }
            },
            {
                "is_short": true,
                "text": {
                    "tag": "lark_md",
                    "content": format!("**状态**\n{}", reply::format_agent_status(&result.status))
                }
            }
        ]
    })];

    if let Some(current_action) = result
        .current_action
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let summary = result
            .summary
            .as_deref()
            .unwrap_or(result.message.as_str())
            .trim();
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**当前动作**\n{}\n\n**摘要**\n{}", current_action.trim(), summary)
            }
        }));
    }

    if !result.related_files.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**相关文件**\n{}", result.related_files.join("、"))
            }
        }));
    }

    if let Some(tool_call) = result
        .tool_call
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let tool_summary = result.tool_result_summary.as_deref().unwrap_or("");
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**工具动作**\n{}\n\n**工具结果**\n{}", tool_call.trim(), tool_summary.trim())
            }
        }));
    }

    if let Some(next_action) = result
        .next_action
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**下一步**\n{}", next_action.trim())
            }
        }));
        elements.push(json!({
            "tag": "action",
            "actions": [
                {
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "按建议继续"
                    },
                    "value": {
                        "command": "按建议继续"
                    }
                }
            ]
        }));
    }

    if let Some(run) = result
        .run
        .as_ref()
        .and_then(|run| run.pending_user_decision.as_ref())
    {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**待决策**\n{}", run.summary.trim())
            }
        }));
        for chunk in run.options.chunks(3) {
            let actions = chunk
                .iter()
                .map(|option| {
                    let command = reply::agent_decision_command(run, option);
                    let mut action = json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": option.label
                        },
                        "value": {
                            "command": command
                        }
                    });
                    if option.primary {
                        action["type"] = json!("primary");
                    }
                    action
                })
                .collect::<Vec<_>>();

            elements.push(json!({
                "tag": "action",
                "actions": actions
            }));
        }
    }

    BridgeResponse::Card {
        fallback_text,
        card: json!({
            "config": {
                "wide_screen_mode": true
            },
            "header": {
                "template": template,
                "title": {
                    "tag": "plain_text",
                    "content": "Agent 任务更新"
                }
            },
            "elements": elements
        }),
    }
}

pub fn format_agent_run_reply_card(
    task_text: &str,
    action_label: &str,
    result: &vscode::AgentRunResult,
) -> BridgeResponse {
    let fallback_text = reply::format_agent_run_reply(task_text, action_label, result);
    let Some(run) = result.run.as_ref() else {
        return BridgeResponse::Text(fallback_text);
    };

    let template = match run.status.as_str() {
        "failed" => "red",
        "waiting_user" => "orange",
        "completed" => "green",
        "cancelled" => "grey",
        _ => "blue",
    };

    let mut elements = vec![
        json!({
            "tag": "div",
            "fields": [
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**任务**\n{}", task_text.trim())
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**上次动作**\n{}", action_label)
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**状态**\n{}", reply::format_agent_run_status(run.status.as_str()))
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**结果处置**\n{}", match run.result_disposition {
                            crate::agent_runtime::ResultDisposition::Pending => "待决定",
                            crate::agent_runtime::ResultDisposition::Kept => "已保留",
                            crate::agent_runtime::ResultDisposition::Reverted => "已回滚",
                            crate::agent_runtime::ResultDisposition::Abandoned => "已放弃",
                        })
                    }
                }
            ]
        }),
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**当前动作**\n{}\n\n**摘要**\n{}", run.current_action.trim(), run.summary.trim())
            }
        }),
    ];

    if !run.next_action.trim().is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**下一步**\n{}", run.next_action.trim())
            }
        }));
    }

    if run.status == crate::agent_runtime::AgentRunStatus::WaitingUser {
        if let Some(waiting_reason) = reply::format_agent_waiting_reason(
            run.waiting_reason.as_deref(),
            run.pending_user_decision.as_ref(),
        ) {
            elements.push(json!({
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": format!("**等待类型**\n{}", waiting_reason)
                }
            }));
        }
    }

    if let Some(decision) = run.pending_user_decision.as_ref() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**待决策**\n{}", decision.summary.trim())
            }
        }));

        let quick_commands = reply::agent_decision_quick_commands(decision);
        if !quick_commands.is_empty() {
            elements.push(json!({
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": format!("**可直接发送**\n{}", quick_commands.join(" / "))
                }
            }));
        }

        for chunk in decision.options.chunks(3) {
            let actions = chunk
                .iter()
                .map(|option| {
                    let command = reply::agent_decision_command(decision, option);
                    let mut action = json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": option.label
                        },
                        "value": {
                            "command": command
                        }
                    });

                    if option.primary {
                        action["type"] = json!("primary");
                    }

                    action
                })
                .collect::<Vec<_>>();

            elements.push(json!({
                "tag": "action",
                "actions": actions
            }));
        }
    }

    if !run.reversible_artifacts.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "**可回滚产物**\n{}",
                    run.reversible_artifacts
                        .iter()
                        .map(|artifact| artifact.summary.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        }));
    }

    BridgeResponse::Card {
        fallback_text,
        card: json!({
            "config": {
                "wide_screen_mode": true
            },
            "header": {
                "template": template,
                "title": {
                    "tag": "plain_text",
                    "content": "Agent Runtime 更新"
                }
            },
            "elements": elements
        }),
    }
}

pub fn format_semantic_confirm_reply(
    summary_for_user: &str,
    message: &str,
    choices: &[SemanticConfirmChoice],
    confidence: Option<f32>,
    risk: Option<&str>,
) -> BridgeResponse {
    let mut fallback_lines = vec![format!("🤔 需要先确认\n\n{}", summary_for_user.trim())];

    if !message.trim().is_empty() && message.trim() != summary_for_user.trim() {
        fallback_lines.push(message.trim().to_string());
    }

    if let Some(risk) = risk_label(risk) {
        fallback_lines.push(format!("⚠️ 风险等级: {risk}"));
    }

    if let Some(confidence_label) = confidence_label(confidence) {
        fallback_lines.push(format!("🎯 置信度: {confidence_label}"));
    }

    if !choices.is_empty() {
        fallback_lines.push("可选动作:".to_string());
        fallback_lines.extend(choices.iter().map(|choice| {
            let note = choice
                .note
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!(" ({})", value.trim()))
                .unwrap_or_default();
            format!("- {} -> {}{}", choice.label, choice.command, note)
        }));
    }

    let mut elements = vec![json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**解析结果**\n{}", summary_for_user.trim())
        }
    })];

    if !message.trim().is_empty() && message.trim() != summary_for_user.trim() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**说明**\n{}", message.trim())
            }
        }));
    }

    let mut meta_fields = Vec::new();
    if let Some(risk) = risk_label(risk) {
        meta_fields.push(json!({
            "is_short": true,
            "text": {
                "tag": "lark_md",
                "content": format!("**风险**\n{}", risk)
            }
        }));
    }
    if let Some(confidence_label) = confidence_label(confidence) {
        meta_fields.push(json!({
            "is_short": true,
            "text": {
                "tag": "lark_md",
                "content": format!("**置信度**\n{}", confidence_label)
            }
        }));
    }
    if !meta_fields.is_empty() {
        elements.push(json!({
            "tag": "div",
            "fields": meta_fields
        }));
    }

    if !choices.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**候选动作**\n请选择一个更符合你意图的动作。"
            }
        }));

        for chunk in choices.chunks(3) {
            let actions = chunk
                .iter()
                .map(|choice| {
                    let mut action = json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": choice.label
                        },
                        "value": {
                            "command": choice.command
                        }
                    });

                    if choice.is_primary {
                        action["type"] = json!("primary");
                    }

                    action
                })
                .collect::<Vec<_>>();

            elements.push(json!({
                "tag": "action",
                "actions": actions
            }));
        }

        let note_lines = choices
            .iter()
            .filter_map(|choice| {
                choice
                    .note
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|note| format!("- {}: {}", choice.label, note.trim()))
            })
            .collect::<Vec<_>>();

        if !note_lines.is_empty() {
            elements.push(json!({
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": format!("**不确定点 / 说明**\n{}", note_lines.join("\n"))
                }
            }));
        }
    }

    BridgeResponse::Card {
        fallback_text: fallback_lines.join("\n\n"),
        card: json!({
            "config": {
                "wide_screen_mode": true
            },
            "header": {
                "template": semantic_confirm_template(risk),
                "title": {
                    "tag": "plain_text",
                    "content": "请确认下一步"
                }
            },
            "elements": elements
        }),
    }
}

fn semantic_confirm_template(risk: Option<&str>) -> &'static str {
    match risk.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "high" => "red",
        "medium" => "orange",
        _ => "blue",
    }
}

fn risk_label(risk: Option<&str>) -> Option<&'static str> {
    match risk.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "low" => Some("低"),
        "medium" => Some("中"),
        "high" => Some("高"),
        _ => None,
    }
}

fn confidence_label(confidence: Option<f32>) -> Option<&'static str> {
    let value = confidence?;
    if value >= 0.85 {
        Some("高")
    } else if value >= 0.6 {
        Some("中")
    } else if value >= 0.0 {
        Some("低")
    } else {
        None
    }
}

pub fn format_project_picker_reply(choices: &[ProjectChoice]) -> BridgeResponse {
    let current_project = choices
        .iter()
        .find(|choice| choice.is_current)
        .map(|choice| choice.path.as_str());

    let fallback_lines = choices
        .iter()
        .map(|choice| {
            let suffix = if choice.is_current {
                " (当前项目)"
            } else {
                ""
            };
            format!("- {}{}: {}", choice.label, suffix, choice.path)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut elements = vec![json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": "**使用方式**\n点击下面的项目按钮即可切换当前飞书会话绑定的项目。"
        }
    })];

    if let Some(current_project) = current_project {
        elements.push(json!({
            "tag": "note",
            "elements": [
                {
                    "tag": "plain_text",
                    "content": format!("当前项目: {current_project}")
                }
            ]
        }));
    }

    if choices.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**最近项目 / 预设项目**\n当前还没有最近项目或预设项目，可以直接浏览本机文件夹。"
            }
        }));
    } else {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**最近项目 / 预设项目**\n点击下面的按钮即可切换项目。"
            }
        }));

        for chunk in choices.chunks(4) {
            let actions = chunk
                .iter()
                .map(|choice| {
                    let button_label = if choice.is_current {
                        format!("{} · 当前", choice.label)
                    } else {
                        choice.label.clone()
                    };
                    let mut action = json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": button_label
                        },
                        "value": {
                            "command": format!("选择项目 {}", choice.path)
                        }
                    });

                    if choice.is_current {
                        action["type"] = json!("primary");
                    }

                    action
                })
                .collect::<Vec<_>>();

            elements.push(json!({
                "tag": "action",
                "actions": actions
            }));
        }
    }

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": "**浏览本机文件夹**\n如果最近项目里没有你要的目录，可以从磁盘根目录开始逐级选择。"
        }
    }));
    elements.push(json!({
        "tag": "action",
        "actions": [
            {
                "tag": "button",
                "type": "primary",
                "text": {
                    "tag": "plain_text",
                    "content": "浏览文件夹"
                },
                "value": {
                    "command": "浏览项目"
                }
            }
        ]
    }));

    BridgeResponse::Card {
        fallback_text: format!(
            "📁 请选择项目\n\n{}\n\n点击卡片按钮即可切换项目，也可以直接发送「选择项目 <路径>」。",
            fallback_lines
        ),
        card: json!({
            "config": {
                "wide_screen_mode": true
            },
            "header": {
                "template": "blue",
                "title": {
                    "tag": "plain_text",
                    "content": "选择项目"
                }
            },
            "elements": elements
        }),
    }
}

pub fn format_project_browser_reply(
    current_label: &str,
    current_path: Option<&str>,
    parent_path: Option<&str>,
    choices: &[DirectoryChoice],
    selected_project: Option<&str>,
    truncated: bool,
) -> BridgeResponse {
    let mut elements = vec![json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**当前位置**\n{}", current_label)
        }
    })];

    if let Some(selected_project) = selected_project.filter(|value| !value.trim().is_empty()) {
        elements.push(json!({
            "tag": "note",
            "elements": [
                {
                    "tag": "plain_text",
                    "content": format!("当前项目: {selected_project}")
                }
            ]
        }));
    }

    let mut nav_actions = vec![json!({
        "tag": "button",
        "text": {
            "tag": "plain_text",
            "content": "最近项目"
        },
        "value": {
            "command": "选择项目"
        }
    })];

    if let Some(parent_path) = parent_path {
        nav_actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "上一级"
            },
            "value": {
                "command": format!("浏览项目 {}", parent_path)
            }
        }));
    }

    if let Some(current_path) = current_path {
        nav_actions.push(json!({
            "tag": "button",
            "type": "primary",
            "text": {
                "tag": "plain_text",
                "content": "选择当前目录"
            },
            "value": {
                "command": format!("选择项目 {}", current_path)
            }
        }));
    }

    elements.push(json!({
        "tag": "action",
        "actions": nav_actions
    }));

    let summary = if choices.is_empty() {
        "当前目录下没有可继续浏览的子目录。可以直接选择当前目录，或返回上一级。".to_string()
    } else if truncated {
        format!(
            "当前仅展示前 {} 个子目录。若没看到目标目录，请逐步缩小范围后再进入。",
            choices.len()
        )
    } else {
        format!("当前可继续进入 {} 个子目录。", choices.len())
    };

    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": format!("**目录浏览**\n{}", summary)
        }
    }));

    if !choices.is_empty() {
        let directory_lines = choices
            .iter()
            .map(|choice| {
                let note = choice
                    .note
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("\n{}", value.trim()))
                    .unwrap_or_default();
                format!("**{}**\n{}{}", choice.label, choice.path, note)
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**可浏览目录**\n{}", directory_lines)
            }
        }));

        for chunk in choices.chunks(4) {
            let actions = chunk
                .iter()
                .map(|choice| {
                    json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": choice.label
                        },
                        "value": {
                            "command": format!("浏览项目 {}", choice.path)
                        }
                    })
                })
                .collect::<Vec<_>>();

            elements.push(json!({
                "tag": "action",
                "actions": actions
            }));
        }
    }

    let fallback_lines = if choices.is_empty() {
        "(当前没有可继续浏览的子目录)".to_string()
    } else {
        choices
            .iter()
            .map(|choice| format!("- {}: {}", choice.label, choice.path))
            .collect::<Vec<_>>()
            .join("\n")
    };

    BridgeResponse::Card {
        fallback_text: format!(
            "📂 浏览项目\n\n当前位置: {}\n\n{}\n\n可直接发送「选择项目 <路径>」选择当前目录。",
            current_label, fallback_lines
        ),
        card: json!({
            "config": {
                "wide_screen_mode": true
            },
            "header": {
                "template": "wathet",
                "title": {
                    "tag": "plain_text",
                    "content": "浏览项目"
                }
            },
            "elements": elements
        }),
    }
}

pub fn format_plan_reply(
    progress: &PlanProgress,
    auto_run: bool,
    approval_policy: &ApprovalPolicy,
    stored: &StoredSession,
) -> BridgeResponse {
    if progress.executed.is_empty() && progress.completed {
        return BridgeResponse::Card {
            fallback_text: "✅ 当前计划已经执行完成，没有剩余步骤。".to_string(),
            card: build_plan_card(
                progress,
                auto_run,
                approval_policy,
                "✅ 当前计划已经执行完成，没有剩余步骤。",
                stored,
            ),
        };
    }

    let mut lines = vec![format!("🧭 状态: {}", plan_status_label(progress))];
    lines.push(format!(
        "✅ 已完成: {} / {} 步",
        progress.next_step, progress.total_steps
    ));

    if progress.completed {
        lines.push("⏭ 当前步骤: 无，计划已完成。".to_string());
    } else {
        lines.push(format!(
            "⏭ 当前步骤: 第 {} / {} 步",
            progress.next_step + 1,
            progress.total_steps
        ));
        lines.push(format!(
            "📦 剩余步骤: {} 步",
            progress.total_steps.saturating_sub(progress.next_step)
        ));
    }

    if let Some(failed_step) = plan_failed_step(progress) {
        lines.push(format!(
            "❌ 失败步骤: 第 {} / {} 步 - {}",
            failed_step.step_number,
            progress.total_steps,
            reply::describe_intent(&failed_step.intent)
        ));
    }

    if let Some(approval_request) = progress.approval_request.as_ref() {
        lines.push(format!(
            "🔐 待审批步骤: 第 {} / {} 步 - {}",
            approval_request.step_number, progress.total_steps, approval_request.action_label
        ));
        lines.push(format!("📝 审批原因: {}", approval_request.reason));
        lines.push(format!("⚠️ 风险提示: {}", approval_request.risk_summary));
    }

    lines.push("📝 本次执行: ".to_string());

    for step in &progress.executed {
        lines.push(format!(
            "{} 第 {}/{} 步: {}",
            if step.outcome.success { "✅" } else { "❌" },
            step.step_number,
            progress.total_steps,
            reply::describe_intent(&step.intent)
        ));
        lines.push(step.outcome.reply.clone());
    }

    if progress.completed {
        lines.push(format!("✅ 计划执行完成，共 {} 步。", progress.total_steps));
        let text = lines.join("\n\n");
        return BridgeResponse::Card {
            fallback_text: text.clone(),
            card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
        };
    }

    let next_step = progress.next_step + 1;
    let remaining = progress.total_steps.saturating_sub(progress.next_step);

    if progress.paused_on_approval {
        lines.push(format!(
            "🔐 第 {} 步需要批准后才能继续。发送「批准」执行该步骤，或发送「拒绝」取消当前计划。",
            next_step
        ));
        let text = lines.join("\n\n");
        return BridgeResponse::Card {
            fallback_text: text.clone(),
            card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
        };
    }

    if progress.paused_on_failure {
        lines.push(format!(
            "⏸ 已在第 {} 步失败后暂停。发送「重新执行失败步骤」重试该步骤，或发送「执行全部」连续执行剩余 {} 步。",
            next_step, remaining
        ));
        let text = lines.join("\n\n");
        return BridgeResponse::Card {
            fallback_text: text.clone(),
            card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
        };
    }

    if auto_run {
        lines.push(format!(
            "⏭ 已暂停，下一步是第 {} 步。发送「继续」执行下一步，或发送「执行全部」连续执行剩余 {} 步。",
            next_step, remaining
        ));
    } else {
        lines.push(format!(
            "⏭ 下一步是第 {} 步。发送「继续」执行下一步，或发送「执行全部」连续执行剩余 {} 步。",
            next_step, remaining
        ));
    }

    let text = lines.join("\n\n");
    BridgeResponse::Card {
        fallback_text: text.clone(),
        card: build_plan_card(progress, auto_run, approval_policy, &text, stored),
    }
}

fn plan_status_label(progress: &PlanProgress) -> &'static str {
    if progress.completed {
        "已完成"
    } else if progress.paused_on_approval {
        "待审批"
    } else if progress.paused_on_failure {
        "失败暂停"
    } else {
        "待继续"
    }
}

fn plan_failed_step(progress: &PlanProgress) -> Option<&crate::plan::StepExecution> {
    progress
        .executed
        .iter()
        .rev()
        .find(|step| !step.outcome.success)
}

fn build_plan_card(
    progress: &PlanProgress,
    auto_run: bool,
    approval_policy: &ApprovalPolicy,
    summary: &str,
    stored: &StoredSession,
) -> serde_json::Value {
    let status_label = plan_status_label(progress);
    let (template, title, subtitle) = if progress.completed {
        ("green", "已完成", "这轮任务已经跑完。")
    } else if progress.paused_on_approval {
        ("orange", "等你确认", "当前步骤确认后才会继续。")
    } else if progress.paused_on_failure {
        ("orange", "已暂停", "这一步失败了，可以重试或继续往后跑。")
    } else if auto_run {
        ("blue", "待继续", "连续执行已暂停，还有后续步骤。")
    } else {
        ("blue", "待继续", "已经停在安全点，可以继续下一步。")
    };

    let current_step = if progress.completed {
        "无".to_string()
    } else {
        format!(
            "第 {} / {} 步",
            progress.next_step + 1,
            progress.total_steps
        )
    };
    let remaining_steps = progress.total_steps.saturating_sub(progress.next_step);
    let current_task = stored
        .current_task
        .as_deref()
        .filter(|task| !task.is_empty())
        .unwrap_or("(未记录任务描述)");
    let last_result = stored
        .last_result
        .as_ref()
        .map(|result| format!("{}: {}", result.status, result.summary))
        .unwrap_or_else(|| "(暂无结果摘要)".to_string());

    let mut elements = vec![
        json!({
            "tag": "div",
            "fields": [
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**状态**\n{}", status_label)
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**已完成**\n{} / {} 步", progress.next_step, progress.total_steps)
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**当前步骤**\n{}", current_step)
                    }
                },
                {
                    "is_short": true,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**剩余步骤**\n{} 步", remaining_steps)
                    }
                }
            ]
        }),
        json!({
            "tag": "div",
            "fields": [
                {
                    "is_short": false,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**当前任务**\n{}", current_task)
                    }
                },
                {
                    "is_short": false,
                    "text": {
                        "tag": "lark_md",
                        "content": format!("**最近结果**\n{}", last_result)
                    }
                }
            ]
        }),
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**摘要**\n{}", summary)
            }
        }),
    ];

    if let Some(failed_step) = plan_failed_step(progress) {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "**失败步骤**\n第 {} / {} 步：{}",
                    failed_step.step_number,
                    progress.total_steps,
                    reply::describe_intent(&failed_step.intent)
                )
            }
        }));
    }

    if let Some(approval_request) = progress.approval_request.as_ref() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "**待审批步骤**\n第 {} / {} 步：{}\n\n**审批原因**\n{}\n\n**风险提示**\n{}",
                    approval_request.step_number,
                    progress.total_steps,
                    approval_request.action_label,
                    approval_request.reason,
                    approval_request.risk_summary
                )
            }
        }));
    }

    if !progress.executed.is_empty() {
        let execution_lines = progress
            .executed
            .iter()
            .map(|step| {
                format!(
                    "- {} 第 {}/{} 步：{}",
                    if step.outcome.success {
                        "成功"
                    } else {
                        "失败"
                    },
                    step.step_number,
                    progress.total_steps,
                    reply::describe_intent(&step.intent)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**本次执行**\n{}", execution_lines)
            }
        }));
    }

    elements.push(json!({
        "tag": "note",
        "elements": [
            {
                "tag": "plain_text",
                "content": subtitle
            }
        ]
    }));

    let approval_summary = approval_policy.summary();
    let approval_summary = if approval_summary.is_empty() {
        "当前无需审批的命令类型。".to_string()
    } else {
        format!("当前审批策略：{}", approval_summary.join(", "))
    };

    elements.push(json!({
        "tag": "note",
        "elements": [
            {
                "tag": "plain_text",
                "content": approval_summary
            }
        ]
    }));

    if !stored.recent_file_paths.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!("**最近文件**\n{}", stored.recent_file_paths.join("、"))
            }
        }));
    }

    let main_actions = if !progress.completed {
        if progress.paused_on_approval {
            vec![
                json!({
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "确认继续"
                    },
                    "value": {
                        "command": "批准"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "取消这步"
                    },
                    "value": {
                        "command": "拒绝"
                    }
                }),
            ]
        } else if progress.paused_on_failure {
            vec![
                json!({
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "重试这步"
                    },
                    "value": {
                        "command": "重新执行失败步骤"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "继续全部"
                    },
                    "value": {
                        "command": "执行全部"
                    }
                }),
            ]
        } else {
            vec![
                json!({
                    "tag": "button",
                    "type": "primary",
                    "text": {
                        "tag": "plain_text",
                        "content": "继续下一步"
                    },
                    "value": {
                        "command": "继续"
                    }
                }),
                json!({
                    "tag": "button",
                    "text": {
                        "tag": "plain_text",
                        "content": "继续全部"
                    },
                    "value": {
                        "command": "执行全部"
                    }
                }),
            ]
        }
    } else {
        Vec::new()
    };

    if !main_actions.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**下一步**"
            }
        }));
        elements.push(json!({
            "tag": "action",
            "actions": main_actions
        }));
    }

    let follow_up_actions = build_follow_up_actions(stored);
    if !follow_up_actions.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "**继续问**"
            }
        }));
        elements.push(json!({
            "tag": "action",
            "actions": follow_up_actions
        }));
    }

    json!({
        "config": {
            "wide_screen_mode": true
        },
        "header": {
            "template": template,
            "title": {
                "tag": "plain_text",
                "content": title
            }
        },
        "elements": elements
    })
}

fn build_follow_up_actions(stored: &StoredSession) -> Vec<serde_json::Value> {
    let mut actions = Vec::new();

    if stored
        .last_result
        .as_ref()
        .is_some_and(|result| !result.success)
    {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "为什么失败了"
            },
            "value": {
                "command": "刚才为什么失败"
            }
        }));
    }

    if stored.last_step.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "看上一步"
            },
            "value": {
                "command": "把上一步结果发我"
            }
        }));
    }

    if stored.current_project_path.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "当前项目"
            },
            "value": {
                "command": "当前项目"
            }
        }));
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "同步 Git"
            },
            "value": {
                "command": "同步 Git 状态"
            }
        }));
    }

    if !stored.recent_file_paths.is_empty() || stored.last_file_path.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "继续这个文件"
            },
            "value": {
                "command": "继续改刚才那个文件"
            }
        }));
    }

    if stored.last_diff.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "看 diff"
            },
            "value": {
                "command": "把刚才的 diff 发我"
            }
        }));
    }

    if !stored.recent_file_paths.is_empty() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "看文件列表"
            },
            "value": {
                "command": "把刚才改动的文件列表发我"
            }
        }));
    }

    if stored.last_patch.is_some() {
        actions.push(json!({
            "tag": "button",
            "text": {
                "tag": "plain_text",
                "content": "撤回补丁"
            },
            "value": {
                "command": "撤回刚才的补丁"
            }
        }));
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::agent_runtime::{
        AgentDecisionOption, AgentRunMode, AgentRunState, AgentRunStatus, ControlPointKind,
        PendingUserDecision, ResultDisposition, ReversibleArtifact, ReversibleArtifactKind,
        RunBudget, RunCheckpoint,
    };
    use crate::plan::{ApprovalRequest, ExecutionOutcome};
    use crate::session::{StoredDiff, StoredPatch, StoredResult, StoredSession, StoredSessionKind};
    use crate::vscode::AgentRunResult;
    use crate::Intent;

    fn shell_intent(cmd: &str) -> Intent {
        Intent::RunShell {
            cmd: cmd.to_string(),
        }
    }

    fn stored_task(task: &str, status: &str, summary: &str) -> StoredSession {
        StoredSession {
            session_kind: StoredSessionKind::Plan,
            agent_state: None,
            current_project_path: None,
            plan: None,
            current_task: Some(task.to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: status.to_string(),
                summary: summary.to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: None,
            last_file_path: None,
            recent_file_paths: Vec::new(),
            last_diff: None,
            last_patch: None,
        }
    }

    #[test]
    fn completion_reply_returns_completion_card() {
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 1,
                intent: shell_intent("pwd"),
                outcome: ExecutionOutcome {
                    success: true,
                    reply: "ok".to_string(),
                },
            }],
            total_steps: 1,
            next_step: 1,
            completed: true,
            paused_on_failure: false,
            paused_on_approval: false,
            approval_request: None,
        };

        let stored = stored_task("执行计划 $ pwd", "已完成", "计划已完成，共执行 1 步。");

        match format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                assert!(fallback_text.contains("状态: 已完成"));
                assert_eq!(card["header"]["title"]["content"], "已完成");
                assert!(card.to_string().contains("当前任务"));
                assert!(card.to_string().contains("执行计划 $ pwd"));
                assert!(card.to_string().contains("最近结果"));
                assert!(card["elements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|element| element["tag"] != "action"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn paused_reply_contains_failed_step_details() {
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 2,
                intent: shell_intent("false"),
                outcome: ExecutionOutcome {
                    success: false,
                    reply: "failed".to_string(),
                },
            }],
            total_steps: 3,
            next_step: 1,
            completed: false,
            paused_on_failure: true,
            paused_on_approval: false,
            approval_request: None,
        };

        let stored = stored_task(
            "执行全部 $ false; $ pwd",
            "失败暂停",
            "第 2 / 3 步失败：执行命令 false",
        );

        match format_plan_reply(&progress, true, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                assert!(fallback_text.contains("失败步骤: 第 2 / 3 步"));
                assert_eq!(card["header"]["title"]["content"], "已暂停");
                assert!(card.to_string().contains("执行全部 $ false; $ pwd"));
                assert!(card.to_string().contains("失败暂停: 第 2 / 3 步失败"));
                assert!(card.to_string().contains("失败步骤"));
                assert!(card.to_string().contains("重试这步"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn approval_reply_contains_approve_actions() {
        let progress = PlanProgress {
            executed: Vec::new(),
            total_steps: 1,
            next_step: 0,
            completed: false,
            paused_on_failure: false,
            paused_on_approval: true,
            approval_request: Some(ApprovalRequest {
                step_index: 0,
                step_number: 1,
                intent: shell_intent("pwd"),
                action_label: "执行命令 pwd".to_string(),
                reason: "shell 命令默认需要人工确认。".to_string(),
                risk_summary: "会在本地 shell 中执行命令，并可能修改工作区或系统状态。".to_string(),
                run_all_after_approval: false,
            }),
        };

        let stored = stored_task("执行计划 git pull", "待审批", "第 1 / 1 步等待批准。");

        match format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                assert!(fallback_text.contains("待审批步骤"));
                assert_eq!(card["header"]["title"]["content"], "等你确认");
                assert!(card.to_string().contains("执行计划 git pull"));
                assert!(card.to_string().contains("待审批: 第 1 / 1 步等待批准。"));
                assert!(card.to_string().contains("确认继续"));
                assert!(card.to_string().contains("取消这步"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn completion_card_includes_follow_up_actions_when_context_exists() {
        let progress = PlanProgress {
            executed: vec![crate::plan::StepExecution {
                step_number: 1,
                intent: Intent::ApplyPatch {
                    patch: "diff --git a/src/demo.rs b/src/demo.rs\n--- a/src/demo.rs\n+++ b/src/demo.rs\n@@ -1 +1 @@\n-old\n+new\n".to_string(),
                },
                outcome: ExecutionOutcome {
                    success: true,
                    reply: "ok".to_string(),
                },
            }],
            total_steps: 1,
            next_step: 1,
            completed: true,
            paused_on_failure: false,
            paused_on_approval: false,
            approval_request: None,
        };
        let stored = StoredSession {
            session_kind: StoredSessionKind::Plan,
            agent_state: None,
            current_project_path: None,
            plan: None,
            current_task: Some("应用补丁 demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "补丁已应用。".to_string(),
                success: true,
            }),
            last_action: Some("执行计划".to_string()),
            last_step: Some(crate::session::StoredStep {
                description: "应用补丁到当前工作区".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: Some("src/demo.rs".to_string()),
            recent_file_paths: vec!["src/demo.rs".to_string()],
            last_diff: Some(StoredDiff {
                description: "应用补丁到当前工作区".to_string(),
                content: "diff --git a/src/demo.rs b/src/demo.rs".to_string(),
            }),
            last_patch: Some(StoredPatch {
                content: "diff --git a/src/demo.rs b/src/demo.rs".to_string(),
                file_paths: vec!["src/demo.rs".to_string()],
            }),
        };

        match format_plan_reply(&progress, false, &ApprovalPolicy::default(), &stored) {
            BridgeResponse::Card { card, .. } => {
                let card_text = card.to_string();
                assert!(card_text.contains("看上一步"));
                assert!(card_text.contains("继续这个文件"));
                assert!(card_text.contains("看 diff"));
                assert!(card_text.contains("看文件列表"));
                assert!(card_text.contains("撤回补丁"));
                assert!(card_text.contains("继续问"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn project_picker_reply_returns_selection_card() {
        let choices = vec![
            ProjectChoice {
                label: "HarborLookout".to_string(),
                path: "C:/Users/beanw/OpenSource/HarborLookout".to_string(),
                note: Some("来自项目映射".to_string()),
                is_current: true,
            },
            ProjectChoice {
                label: "feishu-vscode-bridge".to_string(),
                path: "C:/Users/beanw/OpenSource/feishu-vscode-bridge".to_string(),
                note: Some("默认工作区".to_string()),
                is_current: false,
            },
        ];

        match format_project_picker_reply(&choices) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("请选择项目"));
                assert!(card_text.contains("HarborLookout"));
                assert!(card_text.contains("选择项目 C:/Users/beanw/OpenSource/HarborLookout"));
                assert!(card_text.contains("当前项目"));
                assert!(card_text.contains("浏览文件夹"));
                assert!(card_text.contains("点击下面的按钮即可切换项目"));
                assert!(!card_text.contains("\"type\":\"default\""));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn semantic_confirm_reply_returns_confirmation_card() {
        let choices = vec![
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
                note: Some("先确认是否存在未提交改动。".to_string()),
                is_primary: false,
            },
        ];

        match format_semantic_confirm_reply(
            "准备把当前项目的本地改动同步到 GitHub",
            "这句话可能表示只推送已提交内容，也可能表示自动提交后再推送。",
            &choices,
            Some(0.72),
            Some("medium"),
        ) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("需要先确认"));
                assert!(fallback_text.contains("git push"));
                assert!(card_text.contains("请确认下一步"));
                assert!(card_text.contains("仅推送已提交内容"));
                assert!(card_text.contains("同步 Git 状态"));
                assert!(card_text.contains("不确定点 / 说明"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn semantic_plan_reply_returns_planner_card() {
        let choices = vec![
            SemanticConfirmChoice {
                label: "直接执行".to_string(),
                command: "执行计划 修复当前测试失败".to_string(),
                note: Some("按当前理解直接开跑。".to_string()),
                is_primary: true,
            },
            SemanticConfirmChoice {
                label: "先确认范围".to_string(),
                command: "先只修 parser 相关测试".to_string(),
                note: Some("缩小执行范围，避免误改。".to_string()),
                is_primary: false,
            },
        ];

        let action_lines = vec![
            "1. 查看失败测试输出".to_string(),
            "2. 定位 parser 分支".to_string(),
            "3. 修改后回归验证".to_string(),
        ];

        match format_semantic_plan_reply(
            "修复当前测试失败",
            "confirm",
            "我建议先确认修复范围，再开始改代码。",
            "当前请求可能涉及多个失败点，直接执行风险偏高。",
            &action_lines,
            &choices,
            Some(0.68),
            Some("medium"),
        ) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("Plan 模式"));
                assert!(fallback_text.contains("候选动作"));
                assert_eq!(card["header"]["title"]["content"], "Plan 模式");
                assert_eq!(card["header"]["template"], "orange");
                assert!(card_text.contains("规划摘要"));
                assert!(card_text.contains("查看失败测试输出"));
                assert!(card_text.contains("执行计划 修复当前测试失败"));
                assert!(card_text.contains("先只修 parser 相关测试"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn ask_agent_reply_card_contains_follow_up_action() {
        let result = AgentRunResult {
            success: true,
            session_id: "unused".to_string(),
            message: "unused".to_string(),
            run: None,
            error: None,
        };

        let ask_result = crate::vscode::AgentAskResult {
            success: true,
            session_id: Some("session-1".to_string()),
            status: "waiting_user".to_string(),
            message: "已经完成第一轮分析。".to_string(),
            summary: Some("已经完成第一轮分析。".to_string()),
            current_action: Some("分析当前代码上下文".to_string()),
            next_action: Some("继续检查 src/lib.rs 里的 parse_intent 分支".to_string()),
            related_files: vec!["src/lib.rs".to_string()],
            tool_call: Some("read_file(src/lib.rs:1-120)".to_string()),
            tool_result_summary: Some("已读取关键代码片段。".to_string()),
            run: result.run,
            duration_ms: 123,
            error: None,
        };

        match format_agent_reply_card(
            "问 Copilot parse_intent 这个函数是干什么的",
            "问 Copilot",
            &ask_result,
        ) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("按建议继续"));
                assert!(card_text.contains("Agent 任务更新"));
                assert!(card_text.contains("按建议继续"));
                assert!(card_text.contains("src/lib.rs"));
                assert!(card_text.contains("read_file(src/lib.rs:1-120)"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn agent_runtime_reply_card_contains_result_disposition_actions() {
        let result = AgentRunResult {
            success: true,
            session_id: "session-1".to_string(),
            message: "The run changed the workspace and is waiting for result disposition."
                .to_string(),
            run: Some(AgentRunState {
                run_id: "run-1".to_string(),
                mode: AgentRunMode::Agent,
                status: AgentRunStatus::WaitingUser,
                summary: "Applied a targeted patch and now waiting for result disposition."
                    .to_string(),
                current_action: "Waiting for result disposition after applying workspace changes"
                    .to_string(),
                next_action:
                    "Keep the result, revert the applied changes, or abandon this run result."
                        .to_string(),
                current_step: Some("waiting_result_disposition".to_string()),
                waiting_reason: Some("result_disposition".to_string()),
                authorization_policy: None,
                result_disposition: ResultDisposition::Pending,
                pending_user_decision: Some(PendingUserDecision {
                    decision_id: "decision-1".to_string(),
                    control_kind: ControlPointKind::ResultDisposition,
                    summary: "This run changed the workspace. Decide what to do with the result."
                        .to_string(),
                    options: vec![
                        AgentDecisionOption {
                            option_id: "keep_result".to_string(),
                            label: "Keep result".to_string(),
                            note: None,
                            primary: true,
                        },
                        AgentDecisionOption {
                            option_id: "revert_result".to_string(),
                            label: "Revert result".to_string(),
                            note: None,
                            primary: false,
                        },
                        AgentDecisionOption {
                            option_id: "abandon_result".to_string(),
                            label: "Abandon result".to_string(),
                            note: None,
                            primary: false,
                        },
                    ],
                    recommended_option_id: Some("keep_result".to_string()),
                }),
                budget: RunBudget::default(),
                checkpoints: vec![RunCheckpoint {
                    checkpoint_id: "cp-1".to_string(),
                    label: "result-disposition".to_string(),
                    status_summary: "waiting for result disposition".to_string(),
                    timestamp_ms: 1,
                }],
                reversible_artifacts: vec![ReversibleArtifact {
                    artifact_id: "artifact-1".to_string(),
                    kind: ReversibleArtifactKind::Patch,
                    summary: "Patched src/lib.rs".to_string(),
                    file_paths: vec!["src/lib.rs".to_string()],
                }],
            }),
            error: None,
        };

        match format_agent_run_reply_card("修复当前测试失败", "查看 Agent Runtime 状态", &result)
        {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("结果处置"));
                assert!(card_text.contains("Agent Runtime 更新"));
                assert!(card_text.contains("保留 agent 结果"));
                assert!(card_text.contains("回滚 agent 结果"));
                assert!(card_text.contains("放弃 agent 结果"));
                assert!(card_text.contains("可回滚产物"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn agent_runtime_reply_card_surfaces_pacing_commands() {
        let result = AgentRunResult {
            success: true,
            session_id: "session-1".to_string(),
            message: "The run paused after reaching the current budget.".to_string(),
            run: Some(AgentRunState {
                run_id: "run-1".to_string(),
                mode: AgentRunMode::Agent,
                status: AgentRunStatus::WaitingUser,
                summary: "Paused after reaching current loop budget.".to_string(),
                current_action: "Reached the current loop budget".to_string(),
                next_action: "Approve the pacing decision to continue, or cancel the run."
                    .to_string(),
                current_step: Some("waiting_user".to_string()),
                waiting_reason: Some("pacing".to_string()),
                authorization_policy: None,
                result_disposition: ResultDisposition::Pending,
                pending_user_decision: Some(PendingUserDecision {
                    decision_id: "decision-1".to_string(),
                    control_kind: ControlPointKind::Pacing,
                    summary: "Continue for another batch, or stop here.".to_string(),
                    options: vec![
                        AgentDecisionOption {
                            option_id: "continue_run".to_string(),
                            label: "Continue run".to_string(),
                            note: None,
                            primary: true,
                        },
                        AgentDecisionOption {
                            option_id: "cancel_run".to_string(),
                            label: "Stop here".to_string(),
                            note: None,
                            primary: false,
                        },
                    ],
                    recommended_option_id: Some("continue_run".to_string()),
                }),
                budget: RunBudget::default(),
                checkpoints: Vec::new(),
                reversible_artifacts: Vec::new(),
            }),
            error: None,
        };

        match format_agent_run_reply_card("继续推进当前任务", "查看 Agent Runtime 状态", &result)
        {
            BridgeResponse::Card { card, .. } => {
                let card_text = card.to_string();
                assert!(card_text.contains("继续本轮 agent"));
                assert!(card_text.contains("先停在这里"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn project_picker_reply_normalizes_windows_paths_for_buttons() {
        let choices = vec![ProjectChoice {
            label: "Bridge".to_string(),
            path: "C:/Users/beanw/OpenSource/feishu-vscode-bridge".to_string(),
            note: None,
            is_current: false,
        }];

        match format_project_picker_reply(&choices) {
            BridgeResponse::Card { card, .. } => {
                let card_text = card.to_string();
                assert!(
                    card_text.contains("选择项目 C:/Users/beanw/OpenSource/feishu-vscode-bridge")
                );
                assert!(!card_text
                    .contains(r#"选择项目 C:\\Users\\beanw\\OpenSource\\feishu-vscode-bridge"#));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }

    #[test]
    fn project_browser_reply_returns_navigation_card() {
        let choices = vec![
            DirectoryChoice {
                label: "Users".to_string(),
                path: "C:/Users".to_string(),
                note: Some("目录".to_string()),
            },
            DirectoryChoice {
                label: "Program Files".to_string(),
                path: "C:/Program Files".to_string(),
                note: Some("目录".to_string()),
            },
        ];

        match format_project_browser_reply(
            "C:/",
            Some("C:/"),
            Some("/"),
            &choices,
            Some("C:/Users/beanw/OpenSource/HarborLookout"),
            false,
        ) {
            BridgeResponse::Card {
                fallback_text,
                card,
            } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("浏览项目"));
                assert!(card_text.contains("选择当前目录"));
                assert!(card_text.contains("浏览项目 C:/Users"));
                assert!(card_text.contains("最近项目"));
                assert!(card_text.contains("上一级"));
            }
            BridgeResponse::Text(text) => panic!("expected card reply, got text: {text}"),
        }
    }
}
