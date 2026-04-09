use crate::plan::ExecutionOutcome;
use crate::vscode;
use crate::{help_text, Intent};

pub fn execute_runnable_intent(intent: &Intent) -> ExecutionOutcome {
    match intent {
        Intent::OpenFile { path, line } => {
            let result = vscode::open_file(path, *line);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("打开 {path}")),
            }
        }
        Intent::OpenFolder { path } => {
            let result = vscode::open_folder(path);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("打开目录 {path}")),
            }
        }
        Intent::ShowCurrentProject => ExecutionOutcome {
            success: false,
            reply: "⚠️ 当前项目只支持直接命令调用。".to_string(),
        },
        Intent::ShowPlanPrompt { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ Plan 模式只支持直接命令调用。".to_string(),
        },
        Intent::ShowProjectPicker => ExecutionOutcome {
            success: false,
            reply: "⚠️ 项目选择卡片只支持直接命令调用。".to_string(),
        },
        Intent::ShowProjectBrowser { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ 项目目录浏览只支持直接命令调用。".to_string(),
        },
        Intent::InstallExtension { ext_id } => {
            let result = vscode::install_extension(ext_id);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("安装扩展 {ext_id}")),
            }
        }
        Intent::UninstallExtension { ext_id } => {
            let result = vscode::uninstall_extension(ext_id);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("卸载扩展 {ext_id}")),
            }
        }
        Intent::ListExtensions => {
            let result = vscode::list_extensions();
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("已安装扩展"),
            }
        }
        Intent::DiffFiles { file1, file2 } => {
            let result = vscode::diff_files(file1, file2);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("diff {file1} {file2}")),
            }
        }
        Intent::ReadFile {
            path,
            start_line,
            end_line,
        } => {
            let result = vscode::read_file(path, *start_line, *end_line);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("读取文件 {path}")),
            }
        }
        Intent::ListDirectory { path } => {
            let result = vscode::list_directory(path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("列出目录"),
            }
        }
        Intent::SearchText {
            query,
            path,
            is_regex,
        } => {
            let result = vscode::search_text(query, path.as_deref(), *is_regex);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(if *is_regex {
                    "搜索正则"
                } else {
                    "搜索文本"
                }),
            }
        }
        Intent::RunTests { command } => {
            let result = vscode::run_tests(command.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("运行测试"),
            }
        }
        Intent::GitDiff { path } => {
            let result = vscode::git_diff(path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("查看 diff"),
            }
        }
        Intent::ApplyPatch { patch } => {
            let result = vscode::apply_patch(patch);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("应用补丁"),
            }
        }
        Intent::GitStatus { repo } => {
            let result = vscode::git_status(repo.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git 状态"),
            }
        }
        Intent::GitSync { repo } => {
            let result = vscode::git_sync(repo.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("同步 Git 状态"),
            }
        }
        Intent::GitPull { repo } => {
            let result = vscode::git_pull(repo.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git Pull"),
            }
        }
        Intent::GitPushAll { repo, message } => {
            let result = vscode::git_push_all(repo.as_deref(), message);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git Push"),
            }
        }
        Intent::GitLog { count, path } => {
            let result = vscode::git_log(*count, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("Git Log"),
            }
        }
        Intent::GitBlame { path } => {
            let result = vscode::git_blame(path);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("Git Blame {path}")),
            }
        }
        Intent::SearchSymbol { query, path } => {
            let result = vscode::search_symbol(query, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("搜索符号"),
            }
        }
        Intent::FindReferences { query, path } => {
            let result = vscode::find_references(query, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("查找引用"),
            }
        }
        Intent::FindImplementations { query, path } => {
            let result = vscode::find_implementations(query, path.as_deref());
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply("查找实现"),
            }
        }
        Intent::RunSpecificTest { filter } => {
            let result = vscode::run_specific_test(filter);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("运行测试 {filter}")),
            }
        }
        Intent::RunTestFile { path } => {
            let result = vscode::run_test_file(path);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("运行测试文件 {path}")),
            }
        }
        Intent::WriteFile { path, content } => {
            let result = vscode::write_file(path, content);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("写入 {path}")),
            }
        }
        Intent::RunShell { cmd } => {
            let result = vscode::run_shell(cmd);
            ExecutionOutcome {
                success: result.success,
                reply: result.to_reply(&format!("$ {cmd}")),
            }
        }
        Intent::AskAgent { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ 问 Copilot 目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::AskCodex { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ 问 Codex 目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::StartAgentRun { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ Agent Runtime 启动目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::ContinueAgentRun { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ Agent Runtime 续跑目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::ShowAgentRunStatus => ExecutionOutcome {
            success: false,
            reply: "⚠️ Agent Runtime 状态查询目前只支持直接命令调用，暂未接入计划执行器。"
                .to_string(),
        },
        Intent::ApproveAgentRun { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ Agent Runtime 审批目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::CancelAgentRun => ExecutionOutcome {
            success: false,
            reply: "⚠️ Agent Runtime 取消目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::ContinueAgent { .. } => ExecutionOutcome {
            success: false,
            reply: "⚠️ 继续 Agent 任务目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::ContinueAgentSuggested => ExecutionOutcome {
            success: false,
            reply: "⚠️ 按建议继续目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::ResetAgentSession => ExecutionOutcome {
            success: false,
            reply: "⚠️ 重置 Copilot 会话目前只支持直接命令调用，暂未接入计划执行器。".to_string(),
        },
        Intent::Help => ExecutionOutcome {
            success: true,
            reply: help_text().to_string(),
        },
        Intent::Unknown(raw) => ExecutionOutcome {
            success: false,
            reply: format!("❓ 无法识别指令: {raw}"),
        },
        Intent::RunPlan { .. }
        | Intent::ContinuePlan
        | Intent::RetryFailedStep
        | Intent::ExecuteAll
        | Intent::ApprovePending
        | Intent::RejectPending
        | Intent::ExplainLastFailure
        | Intent::ShowLastResult
        | Intent::ContinueLastFile
        | Intent::ShowLastDiff
        | Intent::ShowRecentFiles
        | Intent::UndoLastPatch => ExecutionOutcome {
            success: false,
            reply: "⚠️ 当前步骤不是可直接执行的底层命令。".to_string(),
        },
    }
}
