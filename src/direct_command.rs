use crate::card::{self, DirectoryChoice, ProjectChoice};
use crate::bridge::BridgeResponse;
use crate::bridge_context::BridgeContext;
use crate::plan::ExecutionOutcome;
use crate::reply;
use crate::session;
use crate::vscode;
use crate::Intent;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const PROJECT_MAPPINGS_ENV: &str = "BRIDGE_PROJECT_MAPPINGS";
const MAX_BROWSER_CHOICES: usize = 12;

pub fn execute_direct_command(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    intent: Intent,
) -> BridgeResponse {
    if let Intent::OpenFolder { path } = &intent {
        return select_project_folder(context, session_key, task_text, &intent, path);
    }

    if let Intent::ShowProjectPicker = &intent {
        return show_project_picker(context, session_key);
    }

    if let Intent::ShowProjectBrowser { path } = &intent {
        return show_project_browser(context, session_key, path.as_deref());
    }

    if let Intent::AskAgent { prompt } = &intent {
        return execute_agent_turn(
            context,
            session_key,
            task_text,
            prompt,
            &intent,
            "问 Copilot",
        );
    }

    if let Intent::ShowCurrentProject = &intent {
        let current_project = selected_project_path(context, session_key).or_else(default_workspace_path);
        let reply = match current_project.clone() {
            Some(path) => format!("📁 当前项目: {path}"),
            None => "⚠️ 当前还没有绑定项目。先发送「选择项目 <路径>」或「打开文件夹 <路径>」。".to_string(),
        };
        let outcome = ExecutionOutcome {
            success: true,
            reply,
        };
        return persist_direct_outcome(context, session_key, task_text, &intent, outcome, current_project);
    }

    if let Intent::GitStatus { repo } = &intent {
        let effective_repo = effective_repo_path(context, session_key, repo.as_deref());
        let result = vscode::git_status(effective_repo.as_deref());
        let outcome = ExecutionOutcome {
            success: result.success,
            reply: result.to_reply("Git 状态"),
        };
        return persist_direct_outcome(context, session_key, task_text, &intent, outcome, effective_repo);
    }

    if let Intent::GitSync { repo } = &intent {
        let effective_repo = effective_repo_path(context, session_key, repo.as_deref());
        let result = vscode::git_sync(effective_repo.as_deref());
        let outcome = ExecutionOutcome {
            success: result.success,
            reply: result.to_reply("同步 Git 状态"),
        };
        return persist_direct_outcome(context, session_key, task_text, &intent, outcome, effective_repo);
    }

    if let Intent::GitPull { repo } = &intent {
        let effective_repo = effective_repo_path(context, session_key, repo.as_deref());
        let result = vscode::git_pull(effective_repo.as_deref());
        let outcome = ExecutionOutcome {
            success: result.success,
            reply: result.to_reply("Git Pull"),
        };
        return persist_direct_outcome(context, session_key, task_text, &intent, outcome, effective_repo);
    }

    if let Intent::GitPushAll { repo, message } = &intent {
        let effective_repo = effective_repo_path(context, session_key, repo.as_deref());
        let result = vscode::git_push_all(effective_repo.as_deref(), message);
        let outcome = ExecutionOutcome {
            success: result.success,
            reply: result.to_reply("Git Push"),
        };
        return persist_direct_outcome(context, session_key, task_text, &intent, outcome, effective_repo);
    }

    if let Intent::ResetAgentSession = &intent {
        let result = vscode::reset_agent_session(session_key);
        let outcome = ExecutionOutcome {
            success: result.success,
            reply: result.to_reply("重置 Copilot 会话"),
        };
        return persist_direct_outcome(context, session_key, task_text, &intent, outcome, None);
    }

    let outcome = context.executor()(&intent);
    persist_direct_outcome(context, session_key, task_text, &intent, outcome, None)
}

pub fn execute_agent_turn(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    prompt: &str,
    intent: &Intent,
    action_label: &str,
) -> BridgeResponse {
    let prompt = augment_agent_prompt_with_project(
        prompt,
        selected_project_path(context, session_key).as_deref(),
    );
    let result = vscode::ask_agent(session_key, &prompt);
    let reply = reply::format_agent_reply_with_action(task_text, action_label, &result);
    let stored = session::stored_session_from_agent_result(task_text, intent, &result, &reply);
    let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    BridgeResponse::Text(reply)
}

fn persist_direct_outcome(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    intent: &Intent,
    outcome: ExecutionOutcome,
    current_project_path: Option<String>,
) -> BridgeResponse {
    let reply = outcome.reply.clone();
    let progress = session::progress_from_direct_execution(intent.clone(), outcome);
    let mut stored = session::build_stored_session(
        session::StoredSessionKind::Direct,
        None,
        task_text,
        "直接执行",
        &progress,
    );
    stored.current_project_path = current_project_path;
    let _ = session::persist_session(context.session_store_path(), session_key, &stored);
    BridgeResponse::Text(reply)
}

fn selected_project_path(context: &BridgeContext<'_>, session_key: &str) -> Option<String> {
    session::load_persisted_session(context.session_store_path(), session_key)
        .and_then(|stored| session::selected_project_path(&stored))
}

fn show_project_picker(context: &BridgeContext<'_>, session_key: &str) -> BridgeResponse {
    let choices = collect_project_choices(context, session_key);
    card::format_project_picker_reply(&choices)
}

fn show_project_browser(
    context: &BridgeContext<'_>,
    session_key: &str,
    path: Option<&str>,
) -> BridgeResponse {
    let current_project = selected_project_path(context, session_key).or_else(default_workspace_path);

    let browser = match build_project_browser(path) {
        Ok(browser) => browser,
        Err(error) => {
            return BridgeResponse::Text(format!(
                "⚠️ 浏览项目失败：{error}\n\n可以先发送「选择项目」，或直接发送「选择项目 <路径>」。"
            ));
        }
    };

    card::format_project_browser_reply(
        &browser.current_label,
        browser.current_path.as_deref(),
        browser.parent_path.as_deref(),
        &browser.choices,
        current_project.as_deref(),
        browser.truncated,
    )
}

fn collect_project_choices(context: &BridgeContext<'_>, session_key: &str) -> Vec<ProjectChoice> {
    let current_project = selected_project_path(context, session_key);
    let mut seen = HashSet::new();
    let mut choices = Vec::new();

    for (label, path, note) in configured_project_choices() {
        push_project_choice(
            &mut choices,
            &mut seen,
            current_project.as_deref(),
            label,
            &path,
            Some(note),
        );
    }

    let store = session::load_session_store(context.session_store_path());
    for path in store
        .values()
        .filter_map(session::selected_project_path)
        .collect::<Vec<_>>()
    {
        let label = project_choice_label(&path);
        push_project_choice(
            &mut choices,
            &mut seen,
            current_project.as_deref(),
            label,
            &path,
            Some("最近使用".to_string()),
        );
    }

    if let Some(path) = default_workspace_path() {
        push_project_choice(
            &mut choices,
            &mut seen,
            current_project.as_deref(),
            project_choice_label(&path),
            &path,
            Some("默认工作区".to_string()),
        );
    }

    choices.truncate(8);
    choices
}

fn configured_project_choices() -> Vec<(String, String, String)> {
    let Some(raw) = env::var(PROJECT_MAPPINGS_ENV).ok() else {
        return Vec::new();
    };

    raw.split(['\n', ';'])
        .filter_map(|entry| {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return None;
            }

            let (label, path) = trimmed.split_once('=')?;
            let label = label.trim();
            let path = path.trim();
            if label.is_empty() || path.is_empty() {
                return None;
            }

            let resolved = resolve_project_path(path).ok()?;
            Some((label.to_string(), resolved, "来自项目映射".to_string()))
        })
        .collect()
}

fn push_project_choice(
    choices: &mut Vec<ProjectChoice>,
    seen: &mut HashSet<String>,
    current_project: Option<&str>,
    label: String,
    path: &str,
    note: Option<String>,
) {
    let resolved = match resolve_project_path(path) {
        Ok(path) => path,
        Err(_) => return,
    };
    let key = resolved.to_lowercase();
    if !seen.insert(key) {
        return;
    }

    let is_current = current_project
        .map(|value| value.eq_ignore_ascii_case(&resolved))
        .unwrap_or(false);

    choices.push(ProjectChoice {
        label,
        path: resolved,
        note,
        is_current,
    });
}

fn project_choice_label(path: &str) -> String {
    PathBuf::from(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| path.to_string())
}

struct ProjectBrowserState {
    current_label: String,
    current_path: Option<String>,
    parent_path: Option<String>,
    choices: Vec<DirectoryChoice>,
    truncated: bool,
}

fn build_project_browser(path: Option<&str>) -> Result<ProjectBrowserState, String> {
    match path.map(str::trim).filter(|value| !value.is_empty()) {
        Some(path) => build_directory_browser(path),
        None => build_root_browser(),
    }
}

fn build_root_browser() -> Result<ProjectBrowserState, String> {
    let roots = root_directory_choices();
    if roots.is_empty() {
        return Err("当前系统上没有发现可浏览的根目录。".to_string());
    }

    Ok(ProjectBrowserState {
        current_label: if cfg!(target_os = "windows") {
            "Windows 磁盘".to_string()
        } else {
            "系统根目录".to_string()
        },
        current_path: None,
        parent_path: None,
        choices: roots,
        truncated: false,
    })
}

fn build_directory_browser(path: &str) -> Result<ProjectBrowserState, String> {
    let current_path = resolve_project_path(path)?;
    let current_path_buf = PathBuf::from(&current_path);
    let parent_path = parent_directory(&current_path_buf)
        .map(|value| normalize_project_path_string(&value.to_string_lossy()));

    let mut child_dirs = fs::read_dir(&current_path_buf)
        .map_err(|err| format!("无法读取目录 {}: {err}", current_path_buf.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }

            let label = entry.file_name().to_string_lossy().to_string();
            let resolved = path.canonicalize().unwrap_or(path);
            Some(DirectoryChoice {
                label,
                path: normalize_project_path_string(&resolved.to_string_lossy()),
                note: Some("目录".to_string()),
            })
        })
        .collect::<Vec<_>>();

    child_dirs.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    let truncated = child_dirs.len() > MAX_BROWSER_CHOICES;
    child_dirs.truncate(MAX_BROWSER_CHOICES);

    Ok(ProjectBrowserState {
        current_label: current_path.clone(),
        current_path: Some(current_path),
        parent_path,
        choices: child_dirs,
        truncated,
    })
}

fn parent_directory(path: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        let parent = path.parent()?;
        if parent.as_os_str().is_empty() || parent == path {
            return None;
        }
        return Some(parent.to_path_buf());
    }

    let parent = path.parent()?;
    if parent == path {
        None
    } else {
        Some(parent.to_path_buf())
    }
}

fn root_directory_choices() -> Vec<DirectoryChoice> {
    #[cfg(target_os = "windows")]
    {
        let mut roots = Vec::new();
        for drive in b'A'..=b'Z' {
            let path = format!("{}:\\", drive as char);
            let path_buf = PathBuf::from(&path);
            if !path_buf.is_dir() {
                continue;
            }

            roots.push(DirectoryChoice {
                label: format!("{}盘", drive as char),
                path: normalize_project_path_string(&path),
                note: Some("磁盘根目录".to_string()),
            });
        }
        return roots;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut roots = vec![DirectoryChoice {
            label: "/".to_string(),
            path: "/".to_string(),
            note: Some("系统根目录".to_string()),
        }];

        if let Ok(home) = env::var("HOME") {
            if PathBuf::from(&home).is_dir() {
                roots.push(DirectoryChoice {
                    label: "Home".to_string(),
                    path: home,
                    note: Some("用户目录".to_string()),
                });
            }
        }

        roots
    }
}

fn effective_repo_path(
    context: &BridgeContext<'_>,
    session_key: &str,
    repo: Option<&str>,
) -> Option<String> {
    repo.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| selected_project_path(context, session_key))
}

fn default_workspace_path() -> Option<String> {
    env::var(vscode::WORKSPACE_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_project_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("项目路径不能为空。".to_string());
    }

    let raw = PathBuf::from(trimmed);
    let absolute = if raw.is_absolute() {
        raw
    } else {
        env::current_dir()
            .map_err(|err| format!("无法获取当前目录: {err}"))?
            .join(raw)
    };

    if !absolute.exists() {
        return Err(format!("项目目录不存在: {}", absolute.display()));
    }
    if !absolute.is_dir() {
        return Err(format!("项目路径不是目录: {}", absolute.display()));
    }

    let canonical = absolute.canonicalize().unwrap_or(absolute);
    Ok(normalize_project_path_string(&canonical.to_string_lossy()))
}

fn normalize_project_path_string(path: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        if let Some(stripped) = path.strip_prefix("\\\\?\\") {
            return stripped.replace('\\', "/");
        }

        return path.replace('\\', "/");
    }

    #[cfg(not(target_os = "windows"))]
    path.to_string()
}

fn select_project_folder(
    context: &BridgeContext<'_>,
    session_key: &str,
    task_text: &str,
    intent: &Intent,
    path: &str,
) -> BridgeResponse {
    let resolved = match resolve_project_path(path) {
        Ok(path) => path,
        Err(error) => {
            let outcome = ExecutionOutcome {
                success: false,
                reply: format!("⚠️ 选择项目失败：{error}"),
            };
            return persist_direct_outcome(context, session_key, task_text, intent, outcome, None);
        }
    };

    let result = vscode::open_folder(&resolved);
    let reset = if result.success {
        Some(vscode::reset_agent_session(session_key))
    } else {
        None
    };

    let mut reply = result.to_reply("打开目录");
    if result.success {
        reply = format!("✅ 当前项目已切换为: {resolved}\n\n{reply}");
        if let Some(reset) = reset {
            if reset.success {
                reply.push_str("\n\n♻️ 已重置当前 Copilot 会话，避免串到旧项目。\n现在可以直接发送「同步 Git 状态」或「问 Copilot ...」。");
            }
        }
    }

    let outcome = ExecutionOutcome {
        success: result.success,
        reply,
    };
    persist_direct_outcome(
        context,
        session_key,
        task_text,
        intent,
        outcome,
        Some(resolved),
    )
}

fn augment_agent_prompt_with_project(prompt: &str, current_project_path: Option<&str>) -> String {
    let Some(current_project_path) = current_project_path.map(str::trim).filter(|value| !value.is_empty()) else {
        return prompt.to_string();
    };

    format!(
        "Current selected project: {current_project_path}\nFocus on this project as the primary coding context unless the user explicitly asks otherwise.\n\nUser request:\n{prompt}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::bridge::BridgeApp;
    use crate::session::{self, StoredResult, StoredSession, StoredSessionKind, StoredStep};
    use crate::test_support::unique_temp_path;
    use crate::ApprovalPolicy;

    #[test]
    fn direct_command_persists_session_context() {
        let session_path = unique_temp_path("direct-command", "direct-session");
        let file_path = unique_temp_path("direct-command", "direct-file");
        fs::write(&file_path, "alpha\nbeta\n").unwrap();

        fn fake_executor(intent: &Intent) -> ExecutionOutcome {
            ExecutionOutcome {
                success: true,
                reply: format!("ok: {}", reply::describe_intent(intent)),
            }
        }

        let app = BridgeApp::with_executor(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            fake_executor,
        );

        match app.dispatch(&format!("读取 {} 1-1", file_path.to_string_lossy()), "cli") {
            BridgeResponse::Text(text) => assert!(text.contains("ok: 读取文件")),
            BridgeResponse::Card { .. } => panic!("expected direct text reply"),
        }

        match app.dispatch("继续改刚才那个文件", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains(file_path.to_string_lossy().as_ref()));
                assert!(text.contains("alpha"));
            }
            BridgeResponse::Card { .. } => panic!("expected file continuation reply"),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_file(file_path);
    }

    #[test]
    fn show_current_project_reads_selected_project_from_session() {
        let session_path = unique_temp_path("direct-command", "current-project-session");
        let project_path = unique_temp_path("direct-command", "project-dir");
        fs::create_dir_all(&project_path).unwrap();

        let stored = StoredSession {
            session_kind: StoredSessionKind::Direct,
            agent_state: None,
            current_project_path: Some(project_path.to_string_lossy().to_string()),
            plan: None,
            current_task: Some("选择项目 demo".to_string()),
            pending_steps: Vec::new(),
            last_result: Some(StoredResult {
                status: "已完成".to_string(),
                summary: "已选择项目。".to_string(),
                success: true,
            }),
            last_action: Some("直接执行".to_string()),
            last_step: Some(StoredStep {
                description: "选择项目 demo".to_string(),
                reply: "ok".to_string(),
                success: true,
            }),
            last_file_path: None,
            recent_file_paths: Vec::new(),
            last_diff: None,
            last_patch: None,
        };
        session::persist_session(Some(&session_path), "cli", &stored).unwrap();

        fn fake_executor(intent: &Intent) -> ExecutionOutcome {
            ExecutionOutcome {
                success: true,
                reply: format!("ok: {}", reply::describe_intent(intent)),
            }
        }

        let app = BridgeApp::with_executor(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            fake_executor,
        );

        match app.dispatch("当前项目", "cli") {
            BridgeResponse::Text(text) => {
                assert!(text.contains("当前项目"));
                assert!(text.contains(project_path.to_string_lossy().as_ref()));
            }
            BridgeResponse::Card { .. } => panic!("expected direct text reply"),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_dir_all(project_path);
    }

    #[test]
    fn augment_agent_prompt_includes_selected_project_hint() {
        let prompt = augment_agent_prompt_with_project("继续检查未完成工作", Some("C:/work/demo"));

        assert!(prompt.contains("Current selected project: C:/work/demo"));
        assert!(prompt.contains("Focus on this project as the primary coding context"));
        assert!(prompt.contains("继续检查未完成工作"));
    }

    #[test]
    fn choose_project_without_path_returns_picker_card() {
        let session_path = unique_temp_path("direct-command", "project-picker-session");
        let project_path = unique_temp_path("direct-command", "mapped-project");
        fs::create_dir_all(&project_path).unwrap();

        let previous = std::env::var(PROJECT_MAPPINGS_ENV).ok();
        std::env::set_var(
            PROJECT_MAPPINGS_ENV,
            format!("HarborLookout={}", project_path.to_string_lossy()),
        );

        fn fake_executor(intent: &Intent) -> ExecutionOutcome {
            ExecutionOutcome {
                success: true,
                reply: format!("ok: {}", reply::describe_intent(intent)),
            }
        }

        let app = BridgeApp::with_executor(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            fake_executor,
        );

        match app.dispatch("选择项目", "cli") {
            BridgeResponse::Card { fallback_text, card } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("请选择项目"));
                assert!(fallback_text.contains(project_path.to_string_lossy().as_ref()));
                assert!(card_text.contains("HarborLookout"));
            }
            BridgeResponse::Text(text) => panic!("expected project picker card, got text: {text}"),
        }

        match previous {
            Some(value) => std::env::set_var(PROJECT_MAPPINGS_ENV, value),
            None => std::env::remove_var(PROJECT_MAPPINGS_ENV),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_dir_all(project_path);
    }

    #[test]
    fn browse_project_directory_returns_directory_card() {
        let session_path = unique_temp_path("direct-command", "project-browser-session");
        let root_dir = unique_temp_path("direct-command", "project-browser-root");
        let child_a = root_dir.join("Alpha");
        let child_b = root_dir.join("Beta");
        fs::create_dir_all(&child_a).unwrap();
        fs::create_dir_all(&child_b).unwrap();

        fn fake_executor(intent: &Intent) -> ExecutionOutcome {
            ExecutionOutcome {
                success: true,
                reply: format!("ok: {}", reply::describe_intent(intent)),
            }
        }

        let app = BridgeApp::with_executor(
            Some(session_path.clone()),
            ApprovalPolicy::from_spec("none"),
            fake_executor,
        );

        match app.dispatch(&format!("浏览项目 {}", root_dir.to_string_lossy()), "cli") {
            BridgeResponse::Card { fallback_text, card } => {
                let card_text = card.to_string();
                assert!(fallback_text.contains("浏览项目"));
                assert!(card_text.contains("Alpha"));
                assert!(card_text.contains("Beta"));
                assert!(card_text.contains("选择当前目录"));
            }
            BridgeResponse::Text(text) => panic!("expected project browser card, got text: {text}"),
        }

        let _ = fs::remove_file(session_path);
        let _ = fs::remove_dir_all(root_dir);
    }

    #[test]
    fn normalize_project_path_removes_windows_extended_prefix() {
        assert_eq!(
            normalize_project_path_string("\\\\?\\C:\\Users\\beanw\\OpenSource\\demo"),
            "C:\\Users\\beanw\\OpenSource\\demo"
        );
    }
}