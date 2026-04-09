use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::agent_runtime::{
    AgentAuthorizationPolicy, AgentRunMode, AgentRunState, AgentRunStatus, ResultDisposition,
    RunBudget, RunCheckpoint,
};
use crate::executor::CmdResult;
use crate::vscode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const AGENT_BACKEND_ENV: &str = "BRIDGE_AGENT_BACKEND";
pub const COPILOT_CLI_PATH_ENV: &str = "BRIDGE_COPILOT_CLI_PATH";
pub const CODEX_CLI_PATH_ENV: &str = "BRIDGE_CODEX_CLI_PATH";
const COPILOT_CLI_RUN_STORE_FILE: &str = ".feishu-vscode-bridge-cli-runs.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CopilotCliRunStore {
    runs: BTreeMap<String, CopilotCliStoredRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CopilotCliStoredRun {
    session_id: String,
    run: AgentRunState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentBackendKind {
    VscodeCompanion,
    CopilotCli,
}

impl AgentBackendKind {
    pub fn from_env() -> Self {
        match std::env::var(AGENT_BACKEND_ENV) {
            Ok(value) if value.trim().eq_ignore_ascii_case("copilot_cli") => Self::CopilotCli,
            Ok(value) if value.trim().eq_ignore_ascii_case("copilot-cli") => Self::CopilotCli,
            Ok(value) if value.trim().eq_ignore_ascii_case("cli") => Self::CopilotCli,
            _ => Self::VscodeCompanion,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VscodeCompanion => "vscode_companion",
            Self::CopilotCli => "copilot_cli",
        }
    }
}

pub fn current_backend_kind() -> AgentBackendKind {
    AgentBackendKind::from_env()
}

pub fn ask_agent(
    session_id: &str,
    prompt: &str,
    _current_project: Option<&str>,
) -> vscode::AgentAskResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => vscode::ask_agent(session_id, prompt),
        AgentBackendKind::CopilotCli => ask_agent_via_copilot_cli(session_id, prompt),
    }
}

pub fn ask_codex(
    session_id: &str,
    prompt: &str,
    current_project: Option<&str>,
) -> vscode::AgentAskResult {
    ask_agent_via_codex_cli(session_id, prompt, current_project)
}

pub fn reset_agent_session(session_id: &str) -> CmdResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => vscode::reset_agent_session(session_id),
        AgentBackendKind::CopilotCli => not_implemented_cmd_result(),
    }
}

pub fn plan_semantic_intent(
    session_id: &str,
    prompt: &str,
    current_project: Option<&str>,
) -> vscode::SemanticPlanResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => {
            vscode::plan_semantic_intent(session_id, prompt, current_project)
        }
        AgentBackendKind::CopilotCli => not_implemented_plan_result(),
    }
}

pub fn start_agent_run(
    session_id: &str,
    prompt: &str,
    current_project: Option<&str>,
) -> vscode::AgentRunResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => {
            vscode::start_agent_run(session_id, prompt, current_project)
        }
        AgentBackendKind::CopilotCli => {
            start_agent_run_via_copilot_cli(session_id, prompt, current_project)
        }
    }
}

pub fn continue_agent_run(
    session_id: &str,
    run_id: &str,
    prompt: Option<&str>,
) -> vscode::AgentRunResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => vscode::continue_agent_run(session_id, run_id, prompt),
        AgentBackendKind::CopilotCli => {
            continue_agent_run_via_copilot_cli(session_id, run_id, prompt)
        }
    }
}

pub fn get_agent_run_status(session_id: &str, run_id: &str) -> vscode::AgentRunResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => vscode::get_agent_run_status(session_id, run_id),
        AgentBackendKind::CopilotCli => get_cli_agent_run_status(session_id, run_id),
    }
}

pub fn approve_agent_run(
    session_id: &str,
    run_id: &str,
    decision_id: &str,
    option_id: &str,
) -> vscode::AgentRunResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => {
            vscode::approve_agent_run(session_id, run_id, decision_id, option_id)
        }
        AgentBackendKind::CopilotCli => {
            approve_cli_agent_run(session_id, run_id, decision_id, option_id)
        }
    }
}

pub fn cancel_agent_run(session_id: &str, run_id: &str) -> vscode::AgentRunResult {
    match current_backend_kind() {
        AgentBackendKind::VscodeCompanion => vscode::cancel_agent_run(session_id, run_id),
        AgentBackendKind::CopilotCli => cancel_cli_agent_run(session_id, run_id),
    }
}

fn not_implemented_cmd_result() -> CmdResult {
    CmdResult {
        success: false,
        stdout: String::new(),
        stderr: format!(
            "Copilot CLI backend adapter is not implemented yet. Current backend: {}",
            current_backend_kind().as_str()
        ),
        exit_code: Some(2),
        duration_ms: 0,
    }
}

fn ask_agent_via_copilot_cli(session_id: &str, prompt: &str) -> vscode::AgentAskResult {
    let result = run_copilot_cli_prompt(prompt, None, None);
    let message = result.message();

    vscode::AgentAskResult {
        success: result.cmd.success,
        session_id: result
            .session_id
            .clone()
            .or_else(|| Some(session_id.trim().to_string()).filter(|value| !value.is_empty())),
        status: if result.cmd.success {
            "answered".to_string()
        } else {
            "blocked".to_string()
        },
        message: message.clone(),
        summary: Some(summarize_text(&message)),
        current_action: Some("Executed Copilot CLI prompt".to_string()),
        next_action: Some(if result.cmd.success {
            "You can continue the task, ask a narrower follow-up, or start an autonomous run."
                .to_string()
        } else {
            "Check Copilot CLI login/status or switch back to the VS Code companion backend."
                .to_string()
        }),
        related_files: Vec::new(),
        tool_call: None,
        tool_result_summary: None,
        run: None,
        duration_ms: result.cmd.duration_ms,
        error: (!result.cmd.success).then_some(message),
    }
}

fn ask_agent_via_codex_cli(
    session_id: &str,
    prompt: &str,
    current_project: Option<&str>,
) -> vscode::AgentAskResult {
    let project_path = current_project
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let result = run_codex_cli_prompt(prompt, project_path.as_deref());
    let message = result.message();

    vscode::AgentAskResult {
        success: result.cmd.success,
        session_id: Some(session_id.trim().to_string()).filter(|value| !value.is_empty()),
        status: if result.cmd.success {
            "answered".to_string()
        } else {
            "blocked".to_string()
        },
        message: message.clone(),
        summary: Some(summarize_text(&message)),
        current_action: Some("Executed Codex CLI prompt".to_string()),
        next_action: Some(if result.cmd.success {
            "You can continue with another /codex prompt, or switch back to Copilot/agent runtime when available.".to_string()
        } else {
            "Check Codex CLI authentication/configuration, or retry with a narrower prompt."
                .to_string()
        }),
        related_files: Vec::new(),
        tool_call: None,
        tool_result_summary: None,
        run: None,
        duration_ms: result.cmd.duration_ms,
        error: (!result.cmd.success).then_some(message),
    }
}

fn not_implemented_plan_result() -> vscode::SemanticPlanResult {
    let message = format!(
        "Copilot CLI backend adapter is not implemented yet. Current backend: {}",
        current_backend_kind().as_str()
    );

    vscode::SemanticPlanResult {
        success: false,
        decision: "clarify".to_string(),
        message: message.clone(),
        summary: Some(message.clone()),
        summary_for_user: Some(message.clone()),
        confidence: None,
        risk: Some("unknown".to_string()),
        actions: Vec::new(),
        options: Vec::new(),
        error: Some(message),
    }
}

fn start_agent_run_via_copilot_cli(
    session_id: &str,
    prompt: &str,
    current_project: Option<&str>,
) -> vscode::AgentRunResult {
    let fallback_run_id = generate_cli_run_id(session_id);
    execute_cli_agent_prompt(session_id, &fallback_run_id, None, prompt, current_project)
}

fn continue_agent_run_via_copilot_cli(
    session_id: &str,
    run_id: &str,
    prompt: Option<&str>,
) -> vscode::AgentRunResult {
    let Some(prompt) = prompt.map(str::trim).filter(|value| !value.is_empty()) else {
        return unsupported_cli_run_result(
            session_id,
            Some(run_id),
            "Copilot CLI continuation currently requires a new prompt payload.",
        );
    };

    execute_cli_agent_prompt(session_id, run_id, Some(run_id), prompt, None)
}

fn execute_cli_agent_prompt(
    session_id: &str,
    fallback_run_id: &str,
    resume_id: Option<&str>,
    prompt: &str,
    current_project: Option<&str>,
) -> vscode::AgentRunResult {
    let result = run_copilot_cli_prompt(prompt, current_project, resume_id);
    let message = result.message();
    let effective_run_id = result.session_id.as_deref().unwrap_or(fallback_run_id);
    let status = if result.cmd.success {
        AgentRunStatus::Completed
    } else {
        AgentRunStatus::Failed
    };
    let summary = summarize_text(&message);
    let checkpoint = RunCheckpoint {
        checkpoint_id: format!("cli-{}", current_timestamp_ms()),
        label: if result.cmd.success {
            "completed".to_string()
        } else {
            "failed".to_string()
        },
        status_summary: summary.clone(),
        timestamp_ms: current_timestamp_ms(),
    };

    let run = AgentRunState {
        run_id: effective_run_id.to_string(),
        mode: AgentRunMode::Agent,
        status,
        summary,
        current_action: "Executed Copilot CLI prompt".to_string(),
        next_action: if result.cmd.success {
            "Continue the run with another prompt using the same CLI session, or inspect the current result.".to_string()
        } else {
            "Check Copilot CLI authentication or backend permissions before retrying.".to_string()
        },
        current_step: Some(if result.cmd.success {
            "completed".to_string()
        } else {
            "failed".to_string()
        }),
        waiting_reason: None,
        authorization_policy: Some(AgentAuthorizationPolicy::default()),
        result_disposition: ResultDisposition::Pending,
        pending_user_decision: None,
        budget: RunBudget::default(),
        checkpoints: vec![checkpoint],
        reversible_artifacts: Vec::new(),
    };

    let _ = persist_cli_agent_run(session_id, &run);

    vscode::AgentRunResult {
        success: result.cmd.success,
        session_id: session_id.trim().to_string(),
        message: message.clone(),
        run: Some(run),
        error: (!result.cmd.success).then_some(message),
    }
}

fn unsupported_cli_run_result(
    session_id: &str,
    run_id: Option<&str>,
    message: &str,
) -> vscode::AgentRunResult {
    vscode::AgentRunResult {
        success: false,
        session_id: session_id.trim().to_string(),
        message: message.to_string(),
        run: run_id.map(|run_id| AgentRunState {
            run_id: run_id.to_string(),
            mode: AgentRunMode::Agent,
            status: AgentRunStatus::WaitingUser,
            summary: message.to_string(),
            current_action: "Copilot CLI backend needs more runtime integration".to_string(),
            next_action:
                "Use continue with a new prompt, or switch to the VS Code companion backend."
                    .to_string(),
            current_step: Some("waiting_user".to_string()),
            waiting_reason: None,
            authorization_policy: Some(AgentAuthorizationPolicy::default()),
            result_disposition: ResultDisposition::Pending,
            pending_user_decision: None,
            budget: RunBudget::default(),
            checkpoints: vec![RunCheckpoint {
                checkpoint_id: format!("unsupported-{}", current_timestamp_ms()),
                label: "unsupported".to_string(),
                status_summary: message.to_string(),
                timestamp_ms: current_timestamp_ms(),
            }],
            reversible_artifacts: Vec::new(),
        }),
        error: Some(message.to_string()),
    }
}

fn get_cli_agent_run_status(session_id: &str, run_id: &str) -> vscode::AgentRunResult {
    match load_cli_agent_run(session_id, run_id) {
        Some(run) => vscode::AgentRunResult {
            success: true,
            session_id: session_id.trim().to_string(),
            message: format!(
                "Loaded cached Copilot CLI run state for {}.",
                run_id.trim()
            ),
            run: Some(run),
            error: None,
        },
        None => unsupported_cli_run_result(
            session_id,
            Some(run_id),
            "No cached Copilot CLI run state was found for this run. Start or continue the run first.",
        ),
    }
}

fn approve_cli_agent_run(
    session_id: &str,
    run_id: &str,
    decision_id: &str,
    option_id: &str,
) -> vscode::AgentRunResult {
    let Some(mut run) = load_cli_agent_run(session_id, run_id) else {
        return unsupported_cli_run_result(
            session_id,
            Some(run_id),
            "No cached Copilot CLI run state was found for this run.",
        );
    };

    let Some(decision) = run.pending_user_decision.clone() else {
        return vscode::AgentRunResult {
            success: false,
            session_id: session_id.trim().to_string(),
            message: "Copilot CLI run has no pending decision to approve.".to_string(),
            run: Some(run),
            error: Some("Copilot CLI run has no pending decision to approve.".to_string()),
        };
    };

    if decision.decision_id != decision_id.trim() {
        return vscode::AgentRunResult {
            success: false,
            session_id: session_id.trim().to_string(),
            message: format!(
                "Decision id mismatch for Copilot CLI run: expected {}, got {}.",
                decision.decision_id,
                decision_id.trim()
            ),
            run: Some(run),
            error: Some("decision id mismatch".to_string()),
        };
    }

    let Some(selected) = decision
        .options
        .iter()
        .find(|option| option.option_id == option_id.trim())
        .cloned()
    else {
        return vscode::AgentRunResult {
            success: false,
            session_id: session_id.trim().to_string(),
            message: format!(
                "Unknown option id for Copilot CLI run decision {}: {}.",
                decision_id.trim(),
                option_id.trim()
            ),
            run: Some(run),
            error: Some("option id mismatch".to_string()),
        };
    };

    run.pending_user_decision = None;
    run.checkpoints.push(RunCheckpoint {
        checkpoint_id: format!("approval-{}", current_timestamp_ms()),
        label: "approval".to_string(),
        status_summary: format!("Approved {} with {}.", decision_id.trim(), selected.label),
        timestamp_ms: current_timestamp_ms(),
    });

    if selected.option_id == "cancel_run" {
        run.status = AgentRunStatus::Cancelled;
        run.current_action = "Cancelled Copilot CLI run from approval".to_string();
        run.next_action = "Start a new run when ready.".to_string();
        run.current_step = Some("cancelled".to_string());
        run.summary = "The Copilot CLI run was cancelled from a pending decision.".to_string();
    } else {
        run.status = AgentRunStatus::WaitingUser;
        run.current_action = "Recorded Copilot CLI approval".to_string();
        run.next_action = "Send a new continue prompt to advance this CLI-backed run.".to_string();
        run.current_step = Some("approval_recorded".to_string());
        run.summary = format!(
            "Recorded the approval decision \"{}\" for the Copilot CLI run.",
            selected.label
        );
    }

    let _ = persist_cli_agent_run(session_id, &run);

    vscode::AgentRunResult {
        success: true,
        session_id: session_id.trim().to_string(),
        message: run.summary.clone(),
        run: Some(run),
        error: None,
    }
}

fn cancel_cli_agent_run(session_id: &str, run_id: &str) -> vscode::AgentRunResult {
    let Some(mut run) = load_cli_agent_run(session_id, run_id) else {
        return unsupported_cli_run_result(
            session_id,
            Some(run_id),
            "No cached Copilot CLI run state was found for this run.",
        );
    };

    run.pending_user_decision = None;
    run.status = AgentRunStatus::Cancelled;
    run.current_action = "Cancelled Copilot CLI run".to_string();
    run.next_action = "Start a new run when ready.".to_string();
    run.current_step = Some("cancelled".to_string());
    run.summary =
        "The Copilot CLI run was marked as cancelled in bridge runtime state.".to_string();
    run.checkpoints.push(RunCheckpoint {
        checkpoint_id: format!("cancel-{}", current_timestamp_ms()),
        label: "cancelled".to_string(),
        status_summary: run.summary.clone(),
        timestamp_ms: current_timestamp_ms(),
    });

    let _ = persist_cli_agent_run(session_id, &run);

    vscode::AgentRunResult {
        success: true,
        session_id: session_id.trim().to_string(),
        message: run.summary.clone(),
        run: Some(run),
        error: None,
    }
}

fn copilot_cli_run_store_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|dir| dir.join(COPILOT_CLI_RUN_STORE_FILE))
}

fn load_cli_run_store() -> CopilotCliRunStore {
    let Some(path) = copilot_cli_run_store_path() else {
        return CopilotCliRunStore::default();
    };

    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<CopilotCliRunStore>(&content).unwrap_or_default(),
        Err(_) => CopilotCliRunStore::default(),
    }
}

fn save_cli_run_store(store: &CopilotCliRunStore) -> Result<(), String> {
    let Some(path) = copilot_cli_run_store_path() else {
        return Err("unable to determine Copilot CLI run store path".to_string());
    };

    let content = serde_json::to_string_pretty(store)
        .map_err(|error| format!("failed to serialize Copilot CLI run store: {error}"))?;
    std::fs::write(path, content)
        .map_err(|error| format!("failed to write Copilot CLI run store: {error}"))
}

fn persist_cli_agent_run(session_id: &str, run: &AgentRunState) -> Result<(), String> {
    let mut store = load_cli_run_store();
    store.runs.insert(
        run.run_id.clone(),
        CopilotCliStoredRun {
            session_id: session_id.trim().to_string(),
            run: run.clone(),
        },
    );
    save_cli_run_store(&store)
}

fn load_cli_agent_run(session_id: &str, run_id: &str) -> Option<AgentRunState> {
    let store = load_cli_run_store();
    store
        .runs
        .get(run_id.trim())
        .filter(|entry| entry.session_id == session_id.trim())
        .map(|entry| entry.run.clone())
}

struct CopilotCliPromptResult {
    cmd: CmdResult,
    session_id: Option<String>,
    assistant_message: Option<String>,
}

impl CopilotCliPromptResult {
    fn message(&self) -> String {
        self.assistant_message
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| preferred_output(&self.cmd))
    }
}

struct CodexCliPromptResult {
    cmd: CmdResult,
    assistant_message: Option<String>,
}

impl CodexCliPromptResult {
    fn message(&self) -> String {
        self.assistant_message
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| preferred_output(&self.cmd))
    }
}

fn run_copilot_cli_prompt(
    prompt: &str,
    current_project: Option<&str>,
    resume_id: Option<&str>,
) -> CopilotCliPromptResult {
    let trimmed_prompt = prompt.trim();
    if trimmed_prompt.is_empty() {
        return CopilotCliPromptResult {
            cmd: CmdResult {
                success: false,
                stdout: String::new(),
                stderr: "prompt cannot be empty".to_string(),
                exit_code: Some(2),
                duration_ms: 0,
            },
            session_id: None,
            assistant_message: None,
        };
    }

    let cli_path = std::env::var(COPILOT_CLI_PATH_ENV).unwrap_or_else(|_| {
        if cfg!(target_os = "windows") {
            "copilot.ps1".to_string()
        } else {
            "copilot".to_string()
        }
    });

    let start = Instant::now();
    let output = if cfg!(target_os = "windows") {
        let mut command = Command::new("powershell");
        command.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &cli_path,
            "-p",
            trimmed_prompt,
            "-s",
            "--no-color",
            "--output-format",
            "json",
            "--allow-all-tools",
            "--no-ask-user",
        ]);

        if let Some(project) = current_project
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            command.args(["--add-dir", project]);
            command.current_dir(project);
        }

        if let Some(resume_id) = resume_id.map(str::trim).filter(|value| !value.is_empty()) {
            command.arg(format!("--resume={resume_id}"));
        }

        command.output()
    } else {
        let mut command = Command::new(&cli_path);
        command.args([
            "-p",
            trimmed_prompt,
            "-s",
            "--no-color",
            "--output-format",
            "json",
            "--allow-all-tools",
            "--no-ask-user",
        ]);

        if let Some(project) = current_project
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            command.args(["--add-dir", project]);
            command.current_dir(project);
        }

        if let Some(resume_id) = resume_id.map(str::trim).filter(|value| !value.is_empty()) {
            command.arg(format!("--resume={resume_id}"));
        }

        command.output()
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let parsed = parse_copilot_cli_json_output(&stdout);

            CopilotCliPromptResult {
                cmd: CmdResult {
                    success: output.status.success(),
                    stdout,
                    stderr,
                    exit_code: output.status.code(),
                    duration_ms,
                },
                session_id: parsed.0,
                assistant_message: parsed.1,
            }
        }
        Err(error) => CopilotCliPromptResult {
            cmd: CmdResult {
                success: false,
                stdout: String::new(),
                stderr: format!("failed to execute Copilot CLI: {error}"),
                exit_code: None,
                duration_ms,
            },
            session_id: None,
            assistant_message: None,
        },
    }
}

fn parse_copilot_cli_json_output(stdout: &str) -> (Option<String>, Option<String>) {
    let mut session_id = None;
    let mut assistant_message = None;

    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "assistant.message" => {
                assistant_message = value
                    .get("data")
                    .and_then(|data| data.get("content"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or(assistant_message);
            }
            "result" => {
                session_id = value
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or(session_id);
            }
            _ => {}
        }
    }

    (session_id, assistant_message)
}

fn run_codex_cli_prompt(
    prompt: &str,
    current_project: Option<&std::path::Path>,
) -> CodexCliPromptResult {
    let trimmed_prompt = prompt.trim();
    if trimmed_prompt.is_empty() {
        return CodexCliPromptResult {
            cmd: CmdResult {
                success: false,
                stdout: String::new(),
                stderr: "prompt cannot be empty".to_string(),
                exit_code: Some(2),
                duration_ms: 0,
            },
            assistant_message: None,
        };
    }

    let cli_path = std::env::var(CODEX_CLI_PATH_ENV).unwrap_or_else(|_| "codex".to_string());
    let output_path = std::env::temp_dir().join(format!(
        "feishu-vscode-bridge-codex-{}.txt",
        current_timestamp_ms()
    ));
    let start = Instant::now();
    let mut command = Command::new(&cli_path);
    command.args(["exec", "--skip-git-repo-check", "--json", "-o"]);
    command.arg(&output_path);

    if let Some(project) = current_project {
        command.args(["-C"]);
        command.arg(project);
        command.current_dir(project);
    }

    command.arg(trimmed_prompt);
    let output = command.output();
    let duration_ms = start.elapsed().as_millis() as u64;
    let assistant_message = std::fs::read_to_string(&output_path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let _ = std::fs::remove_file(&output_path);

    match output {
        Ok(output) => CodexCliPromptResult {
            cmd: CmdResult {
                success: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                exit_code: output.status.code(),
                duration_ms,
            },
            assistant_message,
        },
        Err(error) => CodexCliPromptResult {
            cmd: CmdResult {
                success: false,
                stdout: String::new(),
                stderr: format!("failed to execute Codex CLI: {error}"),
                exit_code: None,
                duration_ms,
            },
            assistant_message,
        },
    }
}

fn preferred_output(result: &CmdResult) -> String {
    if !result.stdout.trim().is_empty() {
        result.stdout.trim().to_string()
    } else if !result.stderr.trim().is_empty() {
        result.stderr.trim().to_string()
    } else {
        "(no Copilot CLI output)".to_string()
    }
}

fn summarize_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 220 {
        normalized
    } else {
        normalized.chars().take(219).collect::<String>() + "…"
    }
}

fn generate_cli_run_id(session_id: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in session_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    let part1 = (now & 0xffff_ffff) as u32;
    let part2 = ((now >> 32) & 0xffff) as u16;
    let part3 = 0x4000 | (((now >> 48) & 0x0fff) as u16);
    let part4 = 0x8000 | ((hash & 0x0fff) as u16);
    let part5 = (((now >> 60) as u64) << 44) | (hash & 0x0fff_ffff_ffff);
    format!("{part1:08x}-{part2:04x}-{part3:04x}-{part4:04x}-{part5:012x}")
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
