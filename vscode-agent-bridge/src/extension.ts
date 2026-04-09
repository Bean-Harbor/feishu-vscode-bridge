import * as http from 'http';
import * as nodePath from 'path';
import { exec } from 'child_process';
import { promisify } from 'util';
import * as vscode from 'vscode';

type AskRequest = {
    sessionId: string;
    prompt: string;
};

type ResetRequest = {
    sessionId: string;
};

type SemanticPlanRequest = {
    sessionId: string;
    prompt: string;
    currentProject?: string | null;
};

type AgentStartRequest = {
    sessionId: string;
    prompt: string;
    currentProject?: string | null;
};

type AgentContinueRequest = {
    sessionId: string;
    runId: string;
    prompt?: string;
};

type AgentStatusRequest = {
    sessionId: string;
    runId: string;
};

type AgentApproveRequest = {
    sessionId: string;
    runId: string;
    decisionId: string;
    optionId: string;
};

type AgentCancelRequest = {
    sessionId: string;
    runId: string;
};

type AgentToolName = 'read_file' | 'search_text' | 'run_tests' | 'write_file' | 'apply_patch';

type AgentToolCall = {
    name: AgentToolName;
    args: Record<string, unknown>;
    summary: string;
};

type ToolResultPayload = {
    success: boolean;
    output: string;
    summary: string;
    relatedFiles: string[];
    artifactSnapshots?: ArtifactFileSnapshot[];
};

type ArtifactFileSnapshot = {
    path: string;
    existed: boolean;
    previousContent: string;
};

type ToolResultRequest = {
    sessionId: string;
    toolRequest: AgentToolCall;
    toolResult: ToolResultPayload;
};

type PendingToolRequest = {
    prompt: string;
    toolRequest: AgentToolCall;
    requestedAt: number;
};

type PendingRuntimeToolRequest = {
    prompt: string;
    toolRequest: AgentToolCall;
    requestedAt: number;
};

type SessionState = {
    messages: vscode.LanguageModelChatMessage[];
    lastResponseSummary?: string;
    recentUserPrompts: string[];
    recentWorkspaceFiles: string[];
    recentRuntimeToolRequests: AgentToolCall[];
    currentTask?: string;
    pendingToolRequest?: PendingToolRequest;
    pendingRuntimeToolRequest?: PendingRuntimeToolRequest;
    runtimeArtifactSnapshots: Record<string, ArtifactFileSnapshot[]>;
    agentRun?: AgentRunState;
    updatedAt: number;
};

type AgentRuntimeLoopResult = {
    message: string;
    run: AgentRunState;
};

type AgentRuntimeConfig = {
    mode: AgentRunMode;
    maxIterations: number;
    maxToolCalls: number;
    maxWriteOperations: number;
    allowWriteTools: boolean;
    allowTestTool: boolean;
    summaryLabel: string;
};

type AgentRunMode = 'ask' | 'plan' | 'agent';

type AgentRunStatus = 'initialized' | 'running' | 'waiting_user' | 'completed' | 'cancelled' | 'failed';

type ControlPointKind = 'authorization' | 'result_disposition' | 'goal_revision' | 'pacing';

type ResultDisposition = 'pending' | 'kept' | 'reverted' | 'abandoned';

type ReversibleArtifactKind = 'patch' | 'file_write' | 'command_side_effect' | 'other';

type AgentAuthorizationPolicy = {
    requireWriteApproval: boolean;
    requireShellApproval: boolean;
    requireDestructiveApproval: boolean;
    allowBypassForSession: boolean;
};

type AgentDecisionOption = {
    optionId: string;
    label: string;
    note?: string;
    primary: boolean;
};

type PendingUserDecision = {
    decisionId: string;
    controlKind: ControlPointKind;
    summary: string;
    options: AgentDecisionOption[];
    recommendedOptionId?: string;
};

type ReversibleArtifact = {
    artifactId: string;
    kind: ReversibleArtifactKind;
    summary: string;
    filePaths: string[];
};

type RunBudget = {
    maxIterations: number;
    maxToolCalls: number;
    maxWriteOperations: number;
};

type RunCheckpoint = {
    checkpointId: string;
    label: string;
    statusSummary: string;
    timestampMs: number;
};

type AgentRunState = {
    runId: string;
    mode: AgentRunMode;
    status: AgentRunStatus;
    summary: string;
    currentAction: string;
    nextAction: string;
    currentStep?: string;
    waitingReason?: string;
    authorizationPolicy?: AgentAuthorizationPolicy;
    resultDisposition: ResultDisposition;
    pendingUserDecision?: PendingUserDecision;
    budget: RunBudget;
    checkpoints: RunCheckpoint[];
    reversibleArtifacts: ReversibleArtifact[];
};

type AgentRunResponse = {
    sessionId: string;
    message: string;
    run: AgentRunState;
};

type AgentRunTransition = {
    status?: AgentRunStatus;
    currentAction?: string;
    nextAction?: string;
    currentStep?: string;
    summary?: string;
    waitingReason?: string | null;
    pendingUserDecision?: PendingUserDecision | null;
    resultDisposition?: ResultDisposition;
    checkpoint?: {
        label: string;
        statusSummary: string;
    };
};

type AgentStatus = 'answered' | 'working' | 'needs_tool' | 'waiting_user' | 'blocked' | 'completed';

type AgentTaskState = {
    status: AgentStatus;
    currentAction: string;
    resultSummary: string;
    nextAction: string;
    relatedFiles: string[];
    toolCall: string | null;
    toolResultSummary: string | null;
};

type AskResponse = {
    sessionId: string;
    status: AgentStatus;
    message: string;
    summary: string;
    currentAction: string;
    nextAction: string;
    relatedFiles: string[];
    toolCall: string | null;
    toolResultSummary: string | null;
    toolRequest: AgentToolCall | null;
    taskState: AgentTaskState;
    context: string;
    run?: AgentRunState;
};

type SemanticActionName =
    | 'ask_agent'
    | 'continue_agent'
    | 'continue_plan'
    | 'continue_agent_suggested'
    | 'show_project_picker'
    | 'show_project_browser'
    | 'show_current_project'
    | 'open_folder'
    | 'open_file'
    | 'read_file'
    | 'list_directory'
    | 'search_text'
    | 'search_symbol'
    | 'find_references'
    | 'find_implementations'
    | 'run_tests'
    | 'run_specific_test'
    | 'run_test_file'
    | 'git_diff'
    | 'git_status'
    | 'git_sync'
    | 'git_pull'
    | 'git_push_all'
    | 'git_log'
    | 'git_blame'
    | 'reset_agent_session'
    | 'help';

type SemanticPlannedAction = {
    name: SemanticActionName;
    args: Record<string, unknown>;
};

type SemanticConfirmOption = {
    label: string;
    command: string;
    note: string;
    primary: boolean;
};

type SemanticPlanResponse = {
    decision: 'execute' | 'confirm' | 'clarify';
    message: string;
    summary: string;
    summaryForUser: string;
    confidence: number | null;
    risk: 'low' | 'medium' | 'high' | 'unknown';
    actions: SemanticPlannedAction[];
    options: SemanticConfirmOption[];
};

type BridgeContext = {
    server?: http.Server;
    output: vscode.OutputChannel;
    sessions: Map<string, SessionState>;
};

const HEALTH_PATH = '/health';
const ASK_PATH = '/v1/chat/ask';
const RESET_PATH = '/v1/chat/reset';
const TOOL_RESULT_PATH = '/v1/chat/tool-result';
const PLAN_PATH = '/v1/chat/plan';
const AGENT_START_PATH = '/v1/chat/agent/start';
const AGENT_CONTINUE_PATH = '/v1/chat/agent/continue';
const AGENT_STATUS_PATH = '/v1/chat/agent/status';
const AGENT_APPROVE_PATH = '/v1/chat/agent/approve';
const AGENT_CANCEL_PATH = '/v1/chat/agent/cancel';
const MAX_SUMMARY_LENGTH = 200;
const MAX_WORKSPACE_SNIPPETS = 3;
const MAX_FILE_BYTES = 512 * 1024;
const MAX_SURROUNDING_LINES = 4;
const MAX_SESSION_MESSAGES = 12;
const MAX_RECENT_USER_PROMPTS = 3;
const MAX_RECENT_WORKSPACE_FILES = 3;
const SESSION_TTL_MS = 30 * 60 * 1000;
const MAX_TOOL_RESULT_CHARS = 12_000;
const DEFAULT_READ_FILE_LINE_SPAN = 120;
const MAX_READ_FILE_LINE_SPAN = 200;
const WORKSPACE_FILE_GLOB = '**/*.{rs,ts,tsx,js,jsx,py,toml}';
const WORKSPACE_EXCLUDE_GLOB = '**/{.git,node_modules,target,out,dist,build,.next,coverage}/**';
const BOOTSTRAP_WORKSPACE_ENV = 'BRIDGE_AGENT_BOOTSTRAP_WORKSPACE';
const execAsync = promisify(exec);

type SnippetCandidate = {
    term: string;
    relativePath: string;
    excerpt: string;
    score: number;
};

export async function activate(context: vscode.ExtensionContext): Promise<void> {
    const bridge: BridgeContext = {
        output: vscode.window.createOutputChannel('Feishu Agent Bridge'),
        sessions: new Map(),
    };

    context.subscriptions.push(bridge.output);

    context.subscriptions.push(vscode.commands.registerCommand(
        'feishuVscodeBridge.showAgentBridgeStatus',
        async () => {
            const port = getConfiguredPort();
            const message = bridge.server
                ? `Agent bridge is listening on http://127.0.0.1:${port} with ${bridge.sessions.size} session(s).`
                : 'Agent bridge server is not running.';
            void vscode.window.showInformationMessage(message);
        },
    ));

    context.subscriptions.push(vscode.commands.registerCommand(
        'feishuVscodeBridge.resetAgentBridgeSessions',
        async () => {
            bridge.sessions.clear();
            bridge.output.appendLine('Cleared agent bridge sessions.');
            void vscode.window.showInformationMessage('Feishu agent bridge sessions cleared.');
        },
    ));

    context.subscriptions.push({
        dispose: () => {
            bridge.server?.close();
        },
    });

    if (await ensureBootstrapWorkspace(bridge)) {
        return;
    }

    if (vscode.workspace.getConfiguration('feishuVscodeBridge').get<boolean>('agentBridge.autoStart', true)) {
        if (!hasBridgeWorkspaceContext()) {
            bridge.output.appendLine('Skipping agent bridge auto-start because this window has no workspace context.');
            return;
        }
        await startBridgeServer(bridge);
    }
}

function hasBridgeWorkspaceContext(): boolean {
    if ((vscode.workspace.workspaceFolders?.length ?? 0) > 0) {
        return true;
    }

    return Boolean(process.env[BOOTSTRAP_WORKSPACE_ENV]?.trim());
}

async function ensureBootstrapWorkspace(bridge: BridgeContext): Promise<boolean> {
    if ((vscode.workspace.workspaceFolders?.length ?? 0) > 0) {
        return false;
    }

    const bootstrapPath = process.env[BOOTSTRAP_WORKSPACE_ENV]?.trim();
    if (!bootstrapPath) {
        return false;
    }

    bridge.output.appendLine(`No workspace opened. Adding bootstrap workspace: ${bootstrapPath}`);

    const folderUri = vscode.Uri.file(bootstrapPath);
    const added = vscode.workspace.updateWorkspaceFolders(0, 0, {
        uri: folderUri,
        name: vscode.Uri.file(bootstrapPath).path.split('/').filter(Boolean).pop(),
    });

    if (!added) {
        bridge.output.appendLine('Failed to add bootstrap workspace folder.');
        return false;
    }

    await waitForWorkspaceFolders();
    return false;
}

async function waitForWorkspaceFolders(): Promise<void> {
    if ((vscode.workspace.workspaceFolders?.length ?? 0) > 0) {
        return;
    }

    await new Promise<void>((resolve) => {
        const timeout = setTimeout(() => {
            disposable.dispose();
            resolve();
        }, 1500);

        const disposable = vscode.workspace.onDidChangeWorkspaceFolders(() => {
            if ((vscode.workspace.workspaceFolders?.length ?? 0) > 0) {
                clearTimeout(timeout);
                disposable.dispose();
                resolve();
            }
        });
    });
}

async function startBridgeServer(bridge: BridgeContext): Promise<void> {
    if (bridge.server) {
        return;
    }

    const port = getConfiguredPort();
    const server = http.createServer(async (req, res) => {
        try {
            pruneExpiredSessions(bridge);

            if (!req.url) {
                respondJson(res, 400, { error: 'missing url' });
                return;
            }

            if (req.method === 'GET' && req.url === HEALTH_PATH) {
                respondJson(res, 200, {
                    status: 'ok',
                    port,
                    sessions: bridge.sessions.size,
                });
                return;
            }

            if (req.method === 'POST' && req.url === ASK_PATH) {
                const payload = await readJsonBody<AskRequest>(req);
                const result = await handleAsk(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === RESET_PATH) {
                const payload = await readJsonBody<ResetRequest>(req);
                const result = handleReset(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === PLAN_PATH) {
                const payload = await readJsonBody<SemanticPlanRequest>(req);
                const result = await handleSemanticPlan(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === AGENT_START_PATH) {
                const payload = await readJsonBody<AgentStartRequest>(req);
                const result = await handleAgentStart(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === AGENT_CONTINUE_PATH) {
                const payload = await readJsonBody<AgentContinueRequest>(req);
                const result = await handleAgentContinue(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === AGENT_STATUS_PATH) {
                const payload = await readJsonBody<AgentStatusRequest>(req);
                const result = handleAgentStatus(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === AGENT_APPROVE_PATH) {
                const payload = await readJsonBody<AgentApproveRequest>(req);
                const result = await handleAgentApprove(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === AGENT_CANCEL_PATH) {
                const payload = await readJsonBody<AgentCancelRequest>(req);
                const result = handleAgentCancel(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            if (req.method === 'POST' && req.url === TOOL_RESULT_PATH) {
                const payload = await readJsonBody<ToolResultRequest>(req);
                const result = await handleToolResult(bridge, payload);
                respondJson(res, 200, result);
                return;
            }

            respondJson(res, 404, { error: 'not found' });
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            bridge.output.appendLine(`Request failed: ${message}`);
            respondJson(res, 500, { error: message });
        }
    });

    await new Promise<void>((resolve, reject) => {
        server.once('error', reject);
        server.listen(port, '127.0.0.1', () => resolve());
    });

    bridge.server = server;
    bridge.output.appendLine(`Agent bridge listening on http://127.0.0.1:${port}`);
}

async function handleAsk(bridge: BridgeContext, payload: AskRequest): Promise<AskResponse> {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    const prompt = payload.prompt?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }
    if (!prompt) {
        throw new Error('prompt is required');
    }

    const session = bridge.sessions.get(sessionId) ?? createSession();
    const config = runtimeConfigForMode('ask');
    const contextSummary = collectEditorContext();
    const workspaceContext = await collectWorkspaceContext(prompt);
    const sessionSummary = buildSessionSummary(session);
    session.currentTask = prompt;

    rememberRecentPrompt(session, prompt);
    rememberRecentWorkspaceFiles(session, workspaceContext.files);
    session.updatedAt = Date.now();
    session.pendingToolRequest = undefined;
    const run = createAgentRunState(sessionId, prompt, null, config);
    bridge.sessions.set(sessionId, session);

    const result = await runAgentRuntimeLoop(bridge, sessionId, session, run, prompt, 'start', config);
    const relatedFiles = dedupeStrings(session.recentWorkspaceFiles);
    const taskState = buildTaskStateFromRuntimeRun(prompt, result.run, relatedFiles);

    return {
        sessionId,
        status: taskState.status,
        message: result.message,
        summary: result.run.summary,
        currentAction: taskState.currentAction,
        nextAction: taskState.nextAction,
        relatedFiles: taskState.relatedFiles,
        toolCall: taskState.toolCall,
        toolResultSummary: taskState.toolResultSummary,
        toolRequest: null,
        taskState,
        context: [sessionSummary, contextSummary, workspaceContext.summary]
            .filter((value) => value && value.trim().length > 0)
            .join('\n\n'),
        run: result.run,
    };
}

async function handleToolResult(bridge: BridgeContext, payload: ToolResultRequest): Promise<AskResponse> {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }

    const session = bridge.sessions.get(sessionId);
    if (!session) {
        throw new Error(`Unknown sessionId: ${sessionId}`);
    }

    const pending = session.pendingToolRequest;
    if (!pending) {
        throw new Error(`No pending tool request for session: ${sessionId}`);
    }

    const incomingToolRequest = sanitizeToolRequest(payload.toolRequest);
    if (incomingToolRequest && !areEquivalentToolRequests(incomingToolRequest, pending.toolRequest)) {
        throw new Error(
            `toolRequest mismatch for session ${sessionId}: expected ${formatToolRequest(pending.toolRequest)}, received ${formatToolRequest(incomingToolRequest)}`,
        );
    }

    const toolRequest = incomingToolRequest ?? pending.toolRequest;
    const toolResult = normalizeToolResultPayload(payload.toolResult);
    const relatedFiles = dedupeStrings([
        ...toolResult.relatedFiles,
        ...extractRelatedFilesFromToolRequest(toolRequest),
        ...session.recentWorkspaceFiles,
    ]);
    const contextSummary = collectEditorContext();
    const sessionSummary = buildSessionSummary(session);

    if (!toolResult.success) {
        session.pendingToolRequest = undefined;
        session.updatedAt = Date.now();
        rememberRecentWorkspaceFiles(session, relatedFiles);
        bridge.sessions.set(sessionId, session);

        const taskState = buildBlockedToolTaskState(toolRequest, toolResult, relatedFiles);
        return {
            sessionId,
            status: taskState.status,
            message: toolResult.output,
            summary: toolResult.summary,
            currentAction: taskState.currentAction,
            nextAction: taskState.nextAction,
            relatedFiles: taskState.relatedFiles,
            toolCall: taskState.toolCall,
            toolResultSummary: taskState.toolResultSummary,
            toolRequest,
            taskState,
            context: [sessionSummary, contextSummary]
                .filter((value) => value && value.trim().length > 0)
                .join('\n\n'),
            run: session.agentRun,
        };
    }

    const model = await selectChatModel();
    const toolContextPrompt = buildToolResultPrompt(pending.prompt, toolRequest, toolResult);
    session.messages.push(vscode.LanguageModelChatMessage.User(toolContextPrompt));

    const text = await sendModelRequest(model, session.messages);
    const summary = summarize(text);
    const taskState = buildToolAnsweredTaskState(pending.prompt, toolRequest, toolResult, summary, relatedFiles);

    session.pendingToolRequest = undefined;
    session.messages.push(vscode.LanguageModelChatMessage.Assistant(text));
    session.lastResponseSummary = summary;
    rememberRecentWorkspaceFiles(session, relatedFiles);
    trimSessionMessages(session);
    session.updatedAt = Date.now();
    bridge.sessions.set(sessionId, session);

    return {
        sessionId,
        status: taskState.status,
        message: text,
        summary,
        currentAction: taskState.currentAction,
        nextAction: taskState.nextAction,
        relatedFiles: taskState.relatedFiles,
        toolCall: taskState.toolCall,
        toolResultSummary: taskState.toolResultSummary,
        toolRequest,
        taskState,
        context: [sessionSummary, contextSummary]
            .filter((value) => value && value.trim().length > 0)
            .join('\n\n'),
        run: session.agentRun,
    };
}

async function handleAgentStart(bridge: BridgeContext, payload: AgentStartRequest): Promise<AgentRunResponse> {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    const prompt = payload.prompt?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }
    if (!prompt) {
        throw new Error('prompt is required');
    }

    const session = bridge.sessions.get(sessionId) ?? createSession();
    const config = runtimeConfigForMode('agent');
    const run = createAgentRunState(sessionId, prompt, payload.currentProject ?? null, config);

    session.currentTask = prompt;
    session.updatedAt = Date.now();
    rememberRecentPrompt(session, prompt);
    bridge.sessions.set(sessionId, session);

    const result = await runAgentRuntimeLoop(bridge, sessionId, session, run, prompt, 'start', config);

    return {
        sessionId,
        message: result.message,
        run: result.run,
    };
}

async function handleAgentContinue(bridge: BridgeContext, payload: AgentContinueRequest): Promise<AgentRunResponse> {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    const runId = payload.runId?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }
    if (!runId) {
        throw new Error('runId is required');
    }

    const session = getAgentSession(bridge, sessionId);
    const run = assertAgentRun(session, runId);
    const config = runtimeConfigForMode(run.mode);
    const prompt = payload.prompt?.trim();

    if (prompt) {
        session.currentTask = prompt;
        rememberRecentPrompt(session, prompt);
    }

    const result = await runAgentRuntimeLoop(bridge, sessionId, session, run, prompt ?? null, 'continue', config);

    return {
        sessionId,
        message: result.message,
        run: result.run,
    };
}

function handleAgentStatus(bridge: BridgeContext, payload: AgentStatusRequest): AgentRunResponse {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    const runId = payload.runId?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }
    if (!runId) {
        throw new Error('runId is required');
    }

    const session = getAgentSession(bridge, sessionId);
    const run = assertAgentRun(session, runId);
    return {
        sessionId,
        message: 'Agent runtime state retrieved.',
        run,
    };
}

async function handleAgentApprove(bridge: BridgeContext, payload: AgentApproveRequest): Promise<AgentRunResponse> {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    const runId = payload.runId?.trim();
    const decisionId = payload.decisionId?.trim();
    const optionId = payload.optionId?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }
    if (!runId) {
        throw new Error('runId is required');
    }
    if (!decisionId) {
        throw new Error('decisionId is required');
    }
    if (!optionId) {
        throw new Error('optionId is required');
    }

    const session = getAgentSession(bridge, sessionId);
    const run = assertAgentRun(session, runId);
    const pending = run.pendingUserDecision;
    if (!pending || pending.decisionId !== decisionId) {
        throw new Error(`No matching pending decision for run: ${runId}`);
    }

    const selected = pending.options.find((option) => option.optionId === optionId);
    if (!selected) {
        throw new Error(`Unknown optionId for decision ${decisionId}: ${optionId}`);
    }

    if (pending.controlKind === 'authorization' && selected.optionId === 'reject_tool') {
        session.pendingRuntimeToolRequest = undefined;
        applyAgentRunTransition(run, {
            status: 'waiting_user',
            currentAction: 'Rejected the pending runtime tool request',
            nextAction: 'Continue with a narrower instruction, or approve a different write action later.',
            currentStep: 'authorization_rejected',
            summary: 'The pending write action was rejected and not executed.',
            waitingReason: 'authorization_rejected',
            pendingUserDecision: null,
            checkpoint: {
                label: 'authorization-rejected',
                statusSummary: 'The pending write action was rejected and not executed.',
            },
        });
        persistAgentRunSession(bridge, sessionId, session, run);

        return {
            sessionId,
            message: run.summary,
            run,
        };
    }

    if (pending.controlKind === 'result_disposition') {
        applyAgentRunTransition(run, { pendingUserDecision: null, waitingReason: null });

        if (selected.optionId === 'keep_result') {
            applyAgentRunTransition(run, {
                resultDisposition: 'kept',
                status: 'completed',
                currentAction: 'Marked the current runtime result as kept',
                nextAction: 'You can continue the run with a follow-up request, or start a new task.',
                currentStep: 'result_kept',
                summary: 'The runtime result was kept.',
                checkpoint: {
                    label: 'result-kept',
                    statusSummary: 'The runtime result was kept.',
                },
            });
            persistAgentRunSession(bridge, sessionId, session, run);

            return {
                sessionId,
                message: run.summary,
                run,
            };
        }

        if (selected.optionId === 'revert_result') {
            const reverted = await revertRuntimeArtifacts(session, run);
            if (!reverted.success) {
                applyAgentRunTransition(run, {
                    status: 'failed',
                    currentAction: 'Failed to revert the current runtime result',
                    nextAction: 'Inspect the workspace state and retry or recover manually.',
                    currentStep: 'result_revert_failed',
                    summary: reverted.summary,
                    checkpoint: {
                        label: 'result-revert-failed',
                        statusSummary: reverted.summary,
                    },
                });
            } else {
                applyAgentRunTransition(run, {
                    resultDisposition: 'reverted',
                    status: 'completed',
                    currentAction: 'Reverted the current runtime result',
                    nextAction: 'You can continue with a narrower follow-up request or start a new task.',
                    currentStep: 'result_reverted',
                    summary: reverted.summary,
                    checkpoint: {
                        label: 'result-reverted',
                        statusSummary: reverted.summary,
                    },
                });
            }

            persistAgentRunSession(bridge, sessionId, session, run);

            return {
                sessionId,
                message: run.summary,
                run,
            };
        }

        if (selected.optionId === 'abandon_result') {
            applyAgentRunTransition(run, {
                resultDisposition: 'abandoned',
                status: 'completed',
                currentAction: 'Marked the current runtime result as abandoned',
                nextAction: 'Start a fresh run if you want to retry with a different approach.',
                currentStep: 'result_abandoned',
                summary: 'The runtime result was abandoned without being kept.',
                checkpoint: {
                    label: 'result-abandoned',
                    statusSummary: 'The runtime result was abandoned without being kept.',
                },
            });
            persistAgentRunSession(bridge, sessionId, session, run);

            return {
                sessionId,
                message: run.summary,
                run,
            };
        }
    }

    applyAgentRunTransition(run, {
        pendingUserDecision: null,
        waitingReason: null,
        checkpoint: {
            label: 'approval',
            statusSummary: `Approved decision ${decisionId} with ${selected.label}`,
        },
    });

    if (selected.optionId === 'cancel_run') {
        session.pendingRuntimeToolRequest = undefined;
        applyAgentRunTransition(run, {
            status: 'cancelled',
            currentAction: 'Cancelled agent runtime from approval decision',
            nextAction: 'Start a new run when ready.',
            currentStep: 'cancelled',
            waitingReason: null,
        });
        persistAgentRunSession(bridge, sessionId, session, run);

        return {
            sessionId,
            message: 'Agent runtime cancelled.',
            run,
        };
    }

    const config = runtimeConfigForMode(run.mode);
    const result = await runAgentRuntimeLoop(bridge, sessionId, session, run, null, 'approve', config);

    return {
        sessionId,
        message: result.message,
        run: result.run,
    };
}

function handleAgentCancel(bridge: BridgeContext, payload: AgentCancelRequest): AgentRunResponse {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    const runId = payload.runId?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }
    if (!runId) {
        throw new Error('runId is required');
    }

    const session = getAgentSession(bridge, sessionId);
    const run = assertAgentRun(session, runId);

    session.pendingRuntimeToolRequest = undefined;
    applyAgentRunTransition(run, {
        pendingUserDecision: null,
        status: 'cancelled',
        currentAction: 'Cancelled agent runtime skeleton',
        nextAction: 'Start a new run when ready.',
        currentStep: 'runtime_scaffold_cancelled',
        waitingReason: null,
        checkpoint: {
            label: 'cancel',
            statusSummary: 'Cancelled by user request',
        },
    });
    persistAgentRunSession(bridge, sessionId, session, run);

    return {
        sessionId,
        message: 'Agent runtime cancelled.',
        run,
    };
}

function handleReset(bridge: BridgeContext, payload: ResetRequest): Record<string, unknown> {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }

    const reset = bridge.sessions.delete(sessionId);
    if (reset) {
        bridge.output.appendLine(`Reset agent bridge session: ${sessionId}`);
    }

    return {
        sessionId,
        reset,
        remainingSessions: bridge.sessions.size,
    };
}

async function handleSemanticPlan(
    bridge: BridgeContext,
    payload: SemanticPlanRequest,
): Promise<SemanticPlanResponse> {
    pruneExpiredSessions(bridge);

    const sessionId = payload.sessionId?.trim();
    const prompt = payload.prompt?.trim();
    if (!sessionId) {
        throw new Error('sessionId is required');
    }
    if (!prompt) {
        throw new Error('prompt is required');
    }

    const session = bridge.sessions.get(sessionId) ?? createSession();
    const contextSummary = collectEditorContext();
    const workspaceContext = await collectWorkspaceContext(prompt);
    const sessionSummary = buildSessionSummary(session);
    const projectSummary = payload.currentProject?.trim()
        ? `Current project: ${payload.currentProject.trim()}`
        : 'Current project: none selected';
    const promptContext = [sessionSummary, contextSummary, projectSummary, workspaceContext.summary]
        .filter((value) => value && value.trim().length > 0)
        .join('\n\n');

    rememberRecentPrompt(session, prompt);
    rememberRecentWorkspaceFiles(session, workspaceContext.files);
    session.currentTask = prompt;
    session.updatedAt = Date.now();
    trimSessionMessages(session);
    bridge.sessions.set(sessionId, session);

    const model = await selectChatModel();
    const decision = await decideSemanticPlan(
        model,
        prompt,
        promptContext,
        workspaceContext.files,
        payload.currentProject ?? null,
        runtimeConfigForMode('plan'),
    );

    const stabilizedDecision = stabilizeSemanticPlanDecision(prompt, decision);
    if (stabilizedDecision) {
        return stabilizedDecision;
    }

    return {
        decision: 'clarify',
        message: '当前还不能稳定判断这句话对应的 VS Code 动作，请补充目标项目、文件或预期结果。',
        summary: 'semantic planner could not classify the request',
        summaryForUser: '还需要更多上下文才能安全决定下一步。',
        confidence: null,
        risk: 'unknown',
        actions: [],
        options: [],
    };
}

function stabilizeSemanticPlanDecision(
    prompt: string,
    decision: SemanticPlanResponse | null,
): SemanticPlanResponse | null {
    if (!looksLikeAmbiguousGitSyncRequest(prompt)) {
        return decision;
    }

    const canonicalOptions = canonicalAmbiguousGitSyncOptions();
    const summaryForUser = decision?.summaryForUser?.trim()
        ? decision.summaryForUser.trim()
        : '这句话可能表示不同的 Git 同步动作，先确认更安全。';
    const confidence = decision?.confidence ?? null;

    return {
        decision: 'confirm',
        message: '这句话可能表示只推送已提交内容，也可能表示自动提交后再推送，先确认更安全。',
        summary: decision?.summary?.trim()
            ? decision.summary.trim()
            : 'ambiguous github sync request requires confirmation',
        summaryForUser,
        confidence,
        risk: 'medium',
        actions: [],
        options: canonicalOptions,
    };
}

function looksLikeAmbiguousGitSyncRequest(taskText: string): boolean {
    const trimmed = taskText.trim();
    const lower = trimmed.toLowerCase();
    const mentionsGithub = lower.includes('github');
    const mentionsSync = trimmed.includes('同步') || lower.includes('sync');
    const mentionsLocalChanges = trimmed.includes('本地')
        || trimmed.includes('改动')
        || trimmed.includes('变更')
        || trimmed.includes('代码')
        || trimmed.includes('提交');

    return mentionsGithub && mentionsSync && mentionsLocalChanges;
}

function canonicalAmbiguousGitSyncOptions(): SemanticConfirmOption[] {
    return [
        {
            label: '仅推送已提交内容',
            command: 'git push',
            note: '不会自动创建 commit。',
            primary: true,
        },
        {
            label: '自动提交并推送',
            command: 'git push auto commit via feishu-bridge',
            note: '会自动 add/commit/push。',
            primary: false,
        },
        {
            label: '先看状态',
            command: '同步 Git 状态',
            note: '先确认当前仓库里有哪些未提交改动。',
            primary: false,
        },
    ];
}

function collectEditorContext(): string {
    const parts: string[] = [];
    const folders = vscode.workspace.workspaceFolders?.map((folder) => folder.uri.fsPath) ?? [];
    if (folders.length > 0) {
        parts.push(`Workspace: ${folders.join(', ')}`);
    }

    const editor = vscode.window.activeTextEditor;
    if (editor) {
        parts.push(`Active file: ${editor.document.uri.fsPath}`);
        if (!editor.selection.isEmpty) {
            const selected = editor.document.getText(editor.selection).trim();
            if (selected) {
                parts.push(`Selected text:\n${selected}`);
            }
        }
    }

    return parts.join('\n');
}

function createSession(): SessionState {
    return {
        messages: [
            vscode.LanguageModelChatMessage.User(
                'You are the VS Code side of a Feishu remote agent bridge. Answer as a pragmatic coding agent. Prefer concrete explanations grounded in retrieved workspace snippets. If snippets are insufficient, say what is missing instead of guessing.',
            ),
        ],
        recentUserPrompts: [],
        recentWorkspaceFiles: [],
        recentRuntimeToolRequests: [],
        runtimeArtifactSnapshots: {},
        updatedAt: Date.now(),
    };
}

function runtimeConfigForMode(mode: AgentRunMode): AgentRuntimeConfig {
    if (mode === 'ask') {
        return {
            mode,
            maxIterations: 3,
            maxToolCalls: 2,
            maxWriteOperations: 0,
            allowWriteTools: false,
            allowTestTool: false,
            summaryLabel: 'Ask runtime',
        };
    }

    if (mode === 'plan') {
        return {
            mode,
            maxIterations: 2,
            maxToolCalls: 1,
            maxWriteOperations: 0,
            allowWriteTools: false,
            allowTestTool: false,
            summaryLabel: 'Plan runtime',
        };
    }

    return {
        mode,
        maxIterations: 12,
        maxToolCalls: 24,
        maxWriteOperations: 6,
        allowWriteTools: true,
        allowTestTool: true,
        summaryLabel: 'Agent runtime',
    };
}

function createAgentRunState(
    sessionId: string,
    prompt: string,
    currentProject: string | null,
    config: AgentRuntimeConfig,
): AgentRunState {
    const now = Date.now();
    const runId = `${sessionId}-${now}`;
    const scopeSuffix = currentProject ? ` in ${currentProject}` : '';

    return {
        runId,
        mode: config.mode,
        status: 'initialized',
        summary: `${config.summaryLabel} created for ${summarize(prompt)}${scopeSuffix}`,
        currentAction: `Initialized ${config.summaryLabel.toLowerCase()} and persisted the initial goal`,
        nextAction: 'The configured runtime loop will start immediately.',
        currentStep: 'initialized',
        waitingReason: undefined,
        authorizationPolicy: {
            requireWriteApproval: config.allowWriteTools,
            requireShellApproval: true,
            requireDestructiveApproval: true,
            allowBypassForSession: false,
        },
        resultDisposition: 'pending',
        pendingUserDecision: undefined,
        budget: {
            maxIterations: config.maxIterations,
            maxToolCalls: config.maxToolCalls,
            maxWriteOperations: config.maxWriteOperations,
        },
        checkpoints: [
            {
                checkpointId: `initialized-${now}`,
                label: 'initialized',
                statusSummary: `Created ${config.summaryLabel.toLowerCase()} for ${summarize(prompt)}`,
                timestampMs: now,
            },
        ],
        reversibleArtifacts: [],
    };
}

function getAgentSession(bridge: BridgeContext, sessionId: string): SessionState {
    const session = bridge.sessions.get(sessionId);
    if (!session) {
        throw new Error(`Unknown sessionId: ${sessionId}`);
    }
    return session;
}

function assertAgentRun(session: SessionState, runId: string): AgentRunState {
    const run = session.agentRun;
    if (!run || run.runId !== runId) {
        throw new Error(`Unknown runId: ${runId}`);
    }

    return {
        ...run,
        checkpoints: [...run.checkpoints],
        reversibleArtifacts: [...run.reversibleArtifacts],
        pendingUserDecision: run.pendingUserDecision
            ? {
                ...run.pendingUserDecision,
                options: [...run.pendingUserDecision.options],
            }
            : undefined,
    };
}

function applyAgentRunTransition(run: AgentRunState, transition: AgentRunTransition): void {
    if (transition.status) {
        run.status = transition.status;
    }
    if (transition.currentAction) {
        run.currentAction = transition.currentAction;
    }
    if (transition.nextAction) {
        run.nextAction = transition.nextAction;
    }
    if (transition.currentStep) {
        run.currentStep = transition.currentStep;
    }
    if (transition.summary) {
        run.summary = transition.summary;
    }
    if (Object.prototype.hasOwnProperty.call(transition, 'waitingReason')) {
        run.waitingReason = transition.waitingReason ?? undefined;
    }
    if (Object.prototype.hasOwnProperty.call(transition, 'pendingUserDecision')) {
        run.pendingUserDecision = transition.pendingUserDecision ?? undefined;
    }
    if (transition.resultDisposition) {
        run.resultDisposition = transition.resultDisposition;
    }
    if (transition.checkpoint) {
        run.checkpoints.push(createCheckpoint(transition.checkpoint.label, transition.checkpoint.statusSummary));
    }
}

function persistAgentRunSession(
    bridge: BridgeContext,
    sessionId: string,
    session: SessionState,
    run: AgentRunState,
): void {
    session.agentRun = run;
    session.updatedAt = Date.now();
    bridge.sessions.set(sessionId, session);
}

function createAuthorizationPendingDecision(runId: string, toolRequest: AgentToolCall): PendingUserDecision {
    return {
        decisionId: `${runId}:authorization:${Date.now()}`,
        controlKind: 'authorization',
        summary: `The agent wants to execute ${formatToolRequest(toolRequest)}. This action can change workspace state and requires approval.`,
        options: [
            {
                optionId: 'approve_tool',
                label: 'Approve tool',
                note: 'Allow this pending tool call and continue the run.',
                primary: true,
            },
            {
                optionId: 'reject_tool',
                label: 'Reject tool',
                note: 'Do not execute this write action. Keep the run paused.',
                primary: false,
            },
            {
                optionId: 'cancel_run',
                label: 'Cancel run',
                note: 'Stop the current autonomous run entirely.',
                primary: false,
            },
        ],
        recommendedOptionId: 'approve_tool',
    };
}

function createResultDispositionPendingDecision(runId: string): PendingUserDecision {
    return {
        decisionId: `${runId}:result:${Date.now()}`,
        controlKind: 'result_disposition',
        summary: 'This run changed the workspace. Decide whether to keep the result, revert the changes, or abandon this result snapshot.',
        options: [
            {
                optionId: 'keep_result',
                label: 'Keep result',
                note: 'Accept the current workspace changes and keep this run result.',
                primary: true,
            },
            {
                optionId: 'revert_result',
                label: 'Revert result',
                note: 'Restore the previous file contents recorded before this run wrote to the workspace.',
                primary: false,
            },
            {
                optionId: 'abandon_result',
                label: 'Abandon result',
                note: 'Do not keep relying on this result snapshot. Leave the run completed without accepting it.',
                primary: false,
            },
        ],
        recommendedOptionId: 'keep_result',
    };
}

function createPacingPendingDecision(runId: string): PendingUserDecision {
    return {
        decisionId: `${runId}:pacing:${Date.now()}`,
        controlKind: 'pacing',
        summary: 'The run reached its current budget. Continue for another batch, or stop here.',
        options: [
            {
                optionId: 'continue_run',
                label: 'Continue run',
                note: 'Allow another batch of autonomous planning, validation, and controlled execution.',
                primary: true,
            },
            {
                optionId: 'cancel_run',
                label: 'Stop here',
                note: 'Cancel this run and keep the current state snapshot.',
                primary: false,
            },
        ],
        recommendedOptionId: 'continue_run',
    };
}

function createCheckpoint(label: string, statusSummary: string): RunCheckpoint {
    return {
        checkpointId: `${label}-${Date.now()}`,
        label,
        statusSummary,
        timestampMs: Date.now(),
    };
}

async function selectChatModel(): Promise<vscode.LanguageModelChat> {
    const [model] = await vscode.lm.selectChatModels({
        vendor: vscode.workspace.getConfiguration('feishuVscodeBridge').get<string>('agentBridge.vendor', 'copilot'),
    });

    if (!model) {
        throw new Error('No compatible chat model is available. Ensure GitHub Copilot Chat or another LM provider is active in VS Code.');
    }

    return model;
}

async function sendModelRequest(
    model: vscode.LanguageModelChat,
    messages: vscode.LanguageModelChatMessage[],
): Promise<string> {
    const tokenSource = new vscode.CancellationTokenSource();
    try {
        const response = await model.sendRequest(messages, {}, tokenSource.token);
        let text = '';
        for await (const fragment of response.text) {
            text += fragment;
        }

        if (!text.trim()) {
            throw new Error('Model returned an empty response.');
        }

        return text;
    } finally {
        tokenSource.dispose();
    }
}

type AgentPlannerDecision = {
    status: 'answered' | 'needs_tool';
    message: string;
    summary: string;
    currentAction: string;
    nextAction: string;
    relatedFiles: string[];
    toolRequest: AgentToolCall | null;
};

type AgentPlannerOutcome = {
    decision: AgentPlannerDecision | null;
    error: string | null;
};

async function decideSemanticPlan(
    model: vscode.LanguageModelChat,
    prompt: string,
    promptContext: string,
    relatedFiles: string[],
    currentProject: string | null,
    config: AgentRuntimeConfig,
): Promise<SemanticPlanResponse | null> {
    const plannerPrompt = [
        'You are the semantic planning layer for a Feishu -> VS Code local agent bridge.',
        `Current runtime mode: ${config.mode}.`,
        'Your job is to convert arbitrary natural language into a structured routing decision for the bridge.',
        'Return JSON only with this shape: {"decision":"execute"|"confirm"|"clarify","message":"...","summary":"...","summaryForUser":"...","confidence":0.0,"risk":"low"|"medium"|"high"|"unknown","actions":[{"name":"...","args":{...}}],"options":[{"label":"...","command":"...","note":"...","primary":true|false}]}.',
        'Use decision=execute only for high-confidence, low-risk requests that map cleanly to one or more concrete supported actions.',
        'Use decision=confirm for ambiguous, medium-risk, or high-risk requests where the user should choose among candidate actions before anything executes.',
        'Use decision=clarify when the request is missing required information such as a path, file, repo, or intended outcome.',
        'Prefer deterministic project/git/navigation actions when the user is clearly asking for project selection, browsing, current project, git status, git sync, git pull, or git push.',
        'For general coding work like analyzing code, fixing bugs, continuing unfinished work, checking README/docs, or implementing changes, do not force a single direct execution action when the user intent is broad or risky. Prefer decision=confirm with safe candidate commands, or decision=clarify if the target is missing.',
        'Important: phrases like “把本地改动同步到 GitHub 上” or “同步到 github” are ambiguous. Do not map them directly to git pull. Prefer decision=confirm with candidates such as git push, git push with auto-commit, and git status.',
        'Important: requests like “帮我修一下这个问题”, “把这个改了然后提交”, or “继续把没完成的工作做完” should usually be decision=confirm unless the scope is already narrow and explicitly safe.',
        'Do not emit shell, apply_patch, write_file, install_extension, or uninstall_extension actions in this planner.',
        'Plan mode is a restricted runtime configuration. Keep the output focused on planning, confirmation, clarification, or narrowly scoped read-only execution.',
        'If the request combines multiple clear operations and is still low-risk, you may return multiple actions in sequence for decision=execute.',
        'For decision=confirm, prefer options that use existing explicit bridge commands in the command field, for example: "git push", "git push auto commit via feishu-bridge", "同步 Git 状态", "问 Copilot 分析这个问题并给出最小修复建议".',
        'Set summaryForUser to a concise Chinese sentence suitable for a confirmation card header/body.',
        'Set confidence to a number between 0 and 1.',
        'Supported actions and args:',
        '- ask_agent {"prompt":"string"}',
        '- continue_agent {"prompt":"optional string"}',
        '- continue_plan {}',
        '- continue_agent_suggested {}',
        '- show_project_picker {}',
        '- show_project_browser {"path":"optional string"}',
        '- show_current_project {}',
        '- open_folder {"path":"string"}',
        '- open_file {"path":"string","line":number optional}',
        '- read_file {"path":"string","startLine":number optional,"endLine":number optional}',
        '- list_directory {"path":"optional string"}',
        '- search_text {"query":"string","path":"optional string","isRegex":boolean optional}',
        '- search_symbol {"query":"string","path":"optional string"}',
        '- find_references {"query":"string","path":"optional string"}',
        '- find_implementations {"query":"string","path":"optional string"}',
        '- run_tests {"command":"optional string"}',
        '- run_specific_test {"filter":"string"}',
        '- run_test_file {"path":"string"}',
        '- git_diff {"path":"optional string"}',
        '- git_status {"repo":"optional string"}',
        '- git_sync {"repo":"optional string"}',
        '- git_pull {"repo":"optional string"}',
        '- git_push_all {"repo":"optional string","message":"optional string"}',
        '- git_log {"count":number optional,"path":"optional string"}',
        '- git_blame {"path":"string"}',
        '- reset_agent_session {}',
        '- help {}',
        currentProject ? `Current project: ${currentProject}` : 'Current project: none selected',
        relatedFiles.length > 0 ? `Known related files: ${relatedFiles.join(', ')}` : 'Known related files: none yet.',
        'Current context:',
        promptContext || '(no extra context)',
        'User request:',
        prompt,
    ].join('\n\n');

    try {
        const raw = await sendModelRequest(model, [vscode.LanguageModelChatMessage.User(plannerPrompt)]);
        return parseSemanticPlanDecision(raw, prompt);
    } catch {
        return null;
    }
}

function parseSemanticPlanDecision(raw: string, originalPrompt: string): SemanticPlanResponse | null {
    const normalized = raw
        .trim()
        .replace(/^```json\s*/i, '')
        .replace(/^```\s*/i, '')
        .replace(/```$/i, '')
        .trim();
    const start = normalized.indexOf('{');
    const end = normalized.lastIndexOf('}');
    if (start === -1 || end === -1 || end < start) {
        return null;
    }

    try {
        const parsed = JSON.parse(normalized.slice(start, end + 1)) as Record<string, unknown>;
        const rawDecision = parsed.decision === 'execute' || parsed.decision === 'confirm' || parsed.decision === 'clarify'
            ? parsed.decision
            : 'clarify';
        const actions = Array.isArray(parsed.actions)
            ? parsed.actions
                .map(sanitizeSemanticAction)
                .filter((value): value is SemanticPlannedAction => value !== null)
            : [];
        const options = Array.isArray(parsed.options)
            ? parsed.options
                .map(sanitizeSemanticConfirmOption)
                .filter((value): value is SemanticConfirmOption => value !== null)
            : [];
        const message = typeof parsed.message === 'string' && parsed.message.trim()
            ? parsed.message.trim()
            : defaultSemanticPlannerMessage(rawDecision, originalPrompt);
        const summary = typeof parsed.summary === 'string' && parsed.summary.trim()
            ? parsed.summary.trim()
            : summarize(message);
        const summaryForUser = typeof parsed.summaryForUser === 'string' && parsed.summaryForUser.trim()
            ? parsed.summaryForUser.trim()
            : summary;
        const confidence = typeof parsed.confidence === 'number' && Number.isFinite(parsed.confidence)
            ? Math.max(0, Math.min(1, parsed.confidence))
            : null;
        const risk = sanitizeSemanticRisk(parsed.risk);
        const decision = rawDecision === 'execute' && actions.length === 0
            ? 'clarify'
            : rawDecision === 'confirm' && options.length === 0
                ? 'clarify'
                : rawDecision;

        return {
            decision,
            message,
            summary,
            summaryForUser,
            confidence,
            risk,
            actions: decision === 'execute' ? actions : [],
            options: decision === 'confirm' ? options : [],
        };
    } catch {
        return null;
    }
}

function sanitizeSemanticAction(value: unknown): SemanticPlannedAction | null {
    if (!value || typeof value !== 'object') {
        return null;
    }

    const candidate = value as Record<string, unknown>;
    const name = typeof candidate.name === 'string' ? candidate.name.trim() as SemanticActionName : '';
    if (!name) {
        return null;
    }

    const args = candidate.args && typeof candidate.args === 'object' && !Array.isArray(candidate.args)
        ? candidate.args as Record<string, unknown>
        : {};

    return { name, args };
}

function sanitizeSemanticConfirmOption(value: unknown): SemanticConfirmOption | null {
    if (!value || typeof value !== 'object') {
        return null;
    }

    const candidate = value as Record<string, unknown>;
    const label = typeof candidate.label === 'string' ? candidate.label.trim() : '';
    const command = typeof candidate.command === 'string' ? candidate.command.trim() : '';
    if (!label || !command) {
        return null;
    }

    const note = typeof candidate.note === 'string' ? candidate.note.trim() : '';
    const primary = candidate.primary === true;
    return { label, command, note, primary };
}

function sanitizeSemanticRisk(value: unknown): 'low' | 'medium' | 'high' | 'unknown' {
    if (value === 'low' || value === 'medium' || value === 'high') {
        return value;
    }

    return 'unknown';
}

function defaultSemanticPlannerMessage(decision: 'execute' | 'confirm' | 'clarify', originalPrompt: string): string {
    if (decision === 'execute') {
        return `已将这句自然语言映射为可执行动作: ${originalPrompt}`;
    }

    if (decision === 'confirm') {
        return '这句话存在歧义或执行风险，建议先确认要采取的动作。';
    }

    if (decision === 'clarify') {
        return '还需要更具体的目标，例如项目路径、文件路径、仓库或预期结果。';
    }

    return '还需要更具体的目标，例如项目路径、文件路径、仓库或预期结果。';
}

async function decideAgentAction(
    model: vscode.LanguageModelChat,
    prompt: string,
    promptContext: string,
    relatedFiles: string[],
    config: AgentRuntimeConfig,
): Promise<AgentPlannerOutcome> {
    const plannerPrompt = [
        `Current runtime mode: ${config.mode}.`,
        'You are planning the next step for a coding agent that can perform at most one tool call before replanning or answering.',
        'Return JSON only with this shape: {"status":"answered"|"needs_tool","message":"...","summary":"...","currentAction":"...","nextAction":"...","relatedFiles":["..."],"toolRequest":null|{"name":"read_file"|"search_text"|"run_tests"|"write_file"|"apply_patch","args":{},"summary":"..."}}.',
        'Choose needs_tool when the answer depends on repository inspection, validation, or a narrowly scoped code change that is still missing from the current context.',
        'Prefer read_file once you already know the likely file to inspect. Use search_text mainly to discover the first relevant file or symbol location.',
        'Do not repeat the same broad search_text query after a prior search already identified a likely target file.',
        'If recent context already points to one likely file, inspect that file with read_file before issuing another broad search.',
        'When using read_file with line numbers, always provide both startLine and endLine.',
        'Keep read_file ranges focused and no longer than 200 lines.',
        'Use read_file args: {"path":"relative/or/absolute","startLine":number,"endLine":number}.',
        'Use search_text args: {"query":"text","path":"optional/path","isRegex":false}.',
        config.allowTestTool
            ? 'Use run_tests args: {"command":"optional explicit test command"}. Prefer a repo-wide validation command only when it is the smallest safe next check.'
            : 'Do not request run_tests in this mode.',
        config.allowWriteTools
            ? 'Use write_file args: {"path":"relative/or/absolute","content":"full new file content"}. Only use this for small, targeted file writes.'
            : 'Do not request write_file in this mode.',
        config.allowWriteTools
            ? 'Use apply_patch args: {"path":"relative/or/absolute","search":"exact existing text","replace":"new text"}. Only use this for precise targeted replacements.'
            : 'Do not request apply_patch in this mode.',
        'Never propose broad rewrites. For write tools, keep changes minimal and localized.',
        relatedFiles.length > 0 ? `Known related files: ${relatedFiles.join(', ')}` : 'Known related files: none yet.',
        'Current context:',
        promptContext || '(no extra context)',
        'User request:',
        prompt,
    ].join('\n\n');

    try {
        const raw = await sendModelRequest(model, [vscode.LanguageModelChatMessage.User(plannerPrompt)]);
        return {
            decision: parsePlannerDecision(raw),
            error: null,
        };
    } catch (error) {
        return {
            decision: null,
            error: error instanceof Error ? error.message : String(error),
        };
    }
}

function isModelAvailabilityIssue(message: string | null | undefined): boolean {
    if (!message || !message.trim()) {
        return false;
    }

    const normalized = message.toLowerCase();
    return normalized.includes('premium model quota')
        || normalized.includes('premium requests')
        || normalized.includes('allowance to renew')
        || normalized.includes('model unavailable')
        || normalized.includes('provider unavailable')
        || normalized.includes('no compatible chat model')
        || normalized.includes('language model');
}

function parsePlannerDecision(raw: string): AgentPlannerDecision | null {
    const normalized = raw
        .trim()
        .replace(/^```json\s*/i, '')
        .replace(/^```\s*/i, '')
        .replace(/```$/i, '')
        .trim();
    const start = normalized.indexOf('{');
    const end = normalized.lastIndexOf('}');
    if (start === -1 || end === -1 || end < start) {
        return null;
    }

    try {
        const parsed = JSON.parse(normalized.slice(start, end + 1)) as Record<string, unknown>;
        const status = parsed.status === 'needs_tool' ? 'needs_tool' : 'answered';
        const message = typeof parsed.message === 'string' && parsed.message.trim()
            ? parsed.message.trim()
            : (status === 'needs_tool' ? '当前上下文不足，准备调用一个只读工具补充信息。' : '已准备直接回答当前任务。');
        const summary = typeof parsed.summary === 'string' && parsed.summary.trim()
            ? parsed.summary.trim()
            : summarize(message);
        const currentAction = typeof parsed.currentAction === 'string' && parsed.currentAction.trim()
            ? parsed.currentAction.trim()
            : (status === 'needs_tool' ? '分析当前任务并准备读取更多代码上下文' : '结合当前上下文生成回答');
        const nextAction = typeof parsed.nextAction === 'string' && parsed.nextAction.trim()
            ? parsed.nextAction.trim()
            : (status === 'needs_tool' ? '等待只读工具结果后继续完成分析。' : '可以继续追问、要求读取更多代码，或基于当前结论推进任务。');
        const relatedFiles = Array.isArray(parsed.relatedFiles)
            ? parsed.relatedFiles.filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
            : [];
        const toolRequest = sanitizeToolRequest(parsed.toolRequest);

        return {
            status: status === 'needs_tool' && toolRequest ? 'needs_tool' : 'answered',
            message,
            summary,
            currentAction,
            nextAction,
            relatedFiles,
            toolRequest: status === 'needs_tool' ? toolRequest : null,
        };
    } catch {
        return fallbackPlannerDecisionFromText(normalized);
    }
}

function fallbackPlannerDecisionFromText(raw: string): AgentPlannerDecision | null {
    const normalized = raw.trim();
    if (!normalized) {
        return null;
    }

    const readFileRequest = matchReadFileToolRequest(normalized);
    if (readFileRequest) {
        return {
            status: 'needs_tool',
            message: '当前上下文不足，准备读取相关代码后继续回答。',
            summary: readFileRequest.summary ?? `读取文件 ${String(readFileRequest.args.path)}`,
            currentAction: '根据模型提供的文本线索读取相关文件',
            nextAction: '等待读取代码结果后继续分析。',
            relatedFiles: [String(readFileRequest.args.path)],
            toolRequest: readFileRequest,
        };
    }

    const searchTextRequest = matchSearchTextToolRequest(normalized);
    if (searchTextRequest) {
        return {
            status: 'needs_tool',
            message: '当前还需要先搜索工作区里的相关代码位置。',
            summary: searchTextRequest.summary ?? `搜索 ${String(searchTextRequest.args.query)}`,
            currentAction: '根据模型提供的文本线索搜索相关代码',
            nextAction: '等待搜索结果后继续分析。',
            relatedFiles: typeof searchTextRequest.args.path === 'string' ? [searchTextRequest.args.path] : [],
            toolRequest: searchTextRequest,
        };
    }

    return {
        status: 'answered',
        message: normalized,
        summary: summarize(normalized),
        currentAction: '根据当前上下文直接生成回答',
        nextAction: '可以继续追问、要求读取更多代码，或基于当前结论推进任务。',
        relatedFiles: [],
        toolRequest: null,
    };
}

function matchReadFileToolRequest(raw: string): AgentToolCall | null {
    const match = raw.match(/read_file\(\s*([^) :\n]+)(?::(\d+)-(\d+))?\s*\)/i);
    if (!match) {
        return null;
    }

    const path = match[1]?.trim();
    if (!path) {
        return null;
    }

    const args: Record<string, unknown> = { path };
    if (match[2] && match[3]) {
        const startLine = Number.parseInt(match[2], 10);
        const endLine = Number.parseInt(match[3], 10);
        if (Number.isInteger(startLine) && Number.isInteger(endLine) && startLine > 0 && endLine > 0) {
            args.startLine = Math.min(startLine, endLine);
            args.endLine = Math.max(startLine, endLine);
        }
    }

    return sanitizeToolRequest({
        name: 'read_file',
        args,
        summary: `读取文件 ${path}`,
    });
}

function matchSearchTextToolRequest(raw: string): AgentToolCall | null {
    const match = raw.match(/search_text\(\s*("?)([^,"\n)]+)\1(?:\s*,\s*path\s*=\s*("?)([^,"\n)]+)\3)?/i);
    if (!match) {
        return null;
    }

    const query = match[2]?.trim();
    if (!query) {
        return null;
    }

    const args: Record<string, unknown> = {
        query,
        isRegex: false,
    };
    const scopedPath = match[4]?.trim();
    if (scopedPath) {
        args.path = scopedPath;
    }

    return sanitizeToolRequest({
        name: 'search_text',
        args,
        summary: `搜索 ${query}`,
    });
}

function sanitizeToolRequest(value: unknown): AgentToolCall | null {
    if (!value || typeof value !== 'object') {
        return null;
    }

    const record = value as Record<string, unknown>;
    const name = record.name;
    if (name !== 'read_file' && name !== 'search_text' && name !== 'run_tests' && name !== 'write_file' && name !== 'apply_patch') {
        return null;
    }

    const args = typeof record.args === 'object' && record.args
        ? { ...(record.args as Record<string, unknown>) }
        : {};

    if (name === 'read_file') {
        const path = typeof args.path === 'string' ? args.path.trim() : '';
        if (!path) {
            return null;
        }

        const sanitizedArgs: Record<string, unknown> = { path };
        const lineRange = normalizeReadFileRange(args);
        if (lineRange) {
            sanitizedArgs.startLine = lineRange.startLine;
            sanitizedArgs.endLine = lineRange.endLine;
        }

        return {
            name,
            args: sanitizedArgs,
            summary: typeof record.summary === 'string' && record.summary.trim()
                ? record.summary.trim()
                : `读取文件 ${path}`,
        };
    }

    if (name === 'run_tests') {
        const sanitizedArgs: Record<string, unknown> = {};
        if (typeof args.command === 'string' && args.command.trim()) {
            sanitizedArgs.command = args.command.trim();
        }

        return {
            name,
            args: sanitizedArgs,
            summary: typeof record.summary === 'string' && record.summary.trim()
                ? record.summary.trim()
                : '运行测试以验证当前改动或假设',
        };
    }

    if (name === 'write_file') {
        const path = typeof args.path === 'string' ? args.path.trim() : '';
        const content = typeof args.content === 'string' ? args.content : '';
        if (!path) {
            return null;
        }

        return {
            name,
            args: { path, content },
            summary: typeof record.summary === 'string' && record.summary.trim()
                ? record.summary.trim()
                : `写入文件 ${path}`,
        };
    }

    if (name === 'apply_patch') {
        const path = typeof args.path === 'string' ? args.path.trim() : '';
        const search = typeof args.search === 'string' ? args.search : '';
        const replace = typeof args.replace === 'string' ? args.replace : '';
        if (!path || !search) {
            return null;
        }

        return {
            name,
            args: { path, search, replace },
            summary: typeof record.summary === 'string' && record.summary.trim()
                ? record.summary.trim()
                : `补丁更新 ${path}`,
        };
    }

    const query = typeof args.query === 'string' ? args.query.trim() : '';
    if (!query) {
        return null;
    }

    const sanitizedArgs: Record<string, unknown> = {
        query,
        isRegex: args.isRegex === true,
    };
    if (typeof args.path === 'string' && args.path.trim()) {
        sanitizedArgs.path = args.path.trim();
    }

    return {
        name,
        args: sanitizedArgs,
        summary: typeof record.summary === 'string' && record.summary.trim()
            ? record.summary.trim()
            : `搜索文本 ${query}`,
    };
}

function normalizeToolResultPayload(payload: ToolResultPayload): ToolResultPayload {
    const output = typeof payload?.output === 'string' ? payload.output.trim() : '';
    const summary = typeof payload?.summary === 'string' && payload.summary.trim()
        ? payload.summary.trim()
        : summarize(output || '工具已返回结果。');

    return {
        success: payload?.success !== false,
        output: output ? truncateToolResult(output) : '(无输出)',
        summary,
        relatedFiles: Array.isArray(payload?.relatedFiles)
            ? payload.relatedFiles.filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
            : [],
    };
}

function normalizeReadFileRange(args: Record<string, unknown>): { startLine: number; endLine: number } | null {
    const rawStartLine = asPositiveInteger(args.startLine);
    const rawEndLine = asPositiveInteger(args.endLine);

    if (rawStartLine === undefined && rawEndLine === undefined) {
        return null;
    }

    let startLine = rawStartLine ?? Math.max(1, (rawEndLine ?? 1) - DEFAULT_READ_FILE_LINE_SPAN + 1);
    let endLine = rawEndLine ?? (startLine + DEFAULT_READ_FILE_LINE_SPAN - 1);

    if (endLine < startLine) {
        [startLine, endLine] = [endLine, startLine];
    }

    if (endLine - startLine + 1 > MAX_READ_FILE_LINE_SPAN) {
        endLine = startLine + MAX_READ_FILE_LINE_SPAN - 1;
    }

    return { startLine, endLine };
}

function areEquivalentToolRequests(left: AgentToolCall, right: AgentToolCall): boolean {
    return left.name === right.name && formatToolRequest(left) === formatToolRequest(right);
}

async function normalizeRuntimeToolRequest(session: SessionState, toolRequest: AgentToolCall, goal: string): Promise<AgentToolCall> {
    const preferredDefinitionTool = await resolvePreferredDefinitionToolRequest(goal, toolRequest);
    if (preferredDefinitionTool) {
        return preferredDefinitionTool;
    }

    if (toolRequest.name !== 'search_text') {
        return toolRequest;
    }

    const hasPriorSearch = session.recentRuntimeToolRequests.some((entry) => entry.name === 'search_text');
    const knownFile = selectPreferredRecentWorkspaceFileForSearch(session, toolRequest);
    if (!hasPriorSearch || !knownFile) {
        return toolRequest;
    }

    const scopedPath = typeof toolRequest.args.path === 'string' ? toolRequest.args.path.trim() : '';
    if (scopedPath && scopedPath !== knownFile) {
        return toolRequest;
    }

    return {
        name: 'read_file',
        args: {
            path: knownFile,
            startLine: 1,
            endLine: DEFAULT_READ_FILE_LINE_SPAN,
        },
        summary: `读取文件 ${knownFile}`,
    };
}

async function resolvePreferredDefinitionToolRequest(goal: string, toolRequest: AgentToolCall): Promise<AgentToolCall | null> {
    if (toolRequest.name !== 'search_text' && toolRequest.name !== 'read_file') {
        return null;
    }

    const preferredLocation = await findPreferredDefinitionLocation(goal);
    if (!preferredLocation) {
        return null;
    }

    if (toolRequest.name === 'read_file') {
        const requestedPath = typeof toolRequest.args.path === 'string' ? toolRequest.args.path.trim().replace(/\\/g, '/') : '';
        if (requestedPath === preferredLocation.path.replace(/\\/g, '/')) {
            return toolRequest;
        }
    }

    return {
        name: 'read_file',
        args: {
            path: preferredLocation.path,
            startLine: preferredLocation.startLine,
            endLine: preferredLocation.endLine,
        },
        summary: `读取文件 ${preferredLocation.path}`,
    };
}

async function findPreferredDefinitionLocation(goal: string): Promise<{ path: string; startLine: number; endLine: number } | null> {
    const primarySymbol = extractPrimarySymbolCandidate(goal);
    const searchTerms = primarySymbol ? [primarySymbol] : extractSearchTerms(goal);
    if (searchTerms.length === 0) {
        return null;
    }

    const files = await vscode.workspace.findFiles(WORKSPACE_FILE_GLOB, WORKSPACE_EXCLUDE_GLOB, 200);
    let bestMatch: { path: string; lineNumber: number; score: number } | null = null;

    for (const file of files) {
        const relativePath = vscode.workspace.asRelativePath(file, false);
        if (shouldSkipWorkspaceFile(relativePath)) {
            continue;
        }

        try {
            const stat = await vscode.workspace.fs.stat(file);
            if (stat.size > MAX_FILE_BYTES) {
                continue;
            }

            const raw = await vscode.workspace.fs.readFile(file);
            const text = Buffer.from(raw).toString('utf8');
            const lines = text.split(/\r?\n/);

            for (const term of searchTerms) {
                const definitionLine = findDefinitionLine(lines, term);
                if (definitionLine === -1) {
                    continue;
                }

                const matchedLine = lines[definitionLine] ?? '';
                const score = scoreWorkspaceSnippet(relativePath, matchedLine, term) + 200;
                if (!bestMatch || score > bestMatch.score) {
                    bestMatch = {
                        path: relativePath,
                        lineNumber: definitionLine + 1,
                        score,
                    };
                }

                break;
            }
        } catch {
            // Skip unreadable files and continue with best-effort symbol grounding.
        }
    }

    if (!bestMatch) {
        return null;
    }

    return {
        path: bestMatch.path,
        startLine: Math.max(1, bestMatch.lineNumber - 20),
        endLine: bestMatch.lineNumber + 60,
    };
}

function extractPrimarySymbolCandidate(goal: string): string | null {
    const matches = goal.match(/[A-Za-z_][A-Za-z0-9_]{2,}/g) ?? [];
    const filtered = matches
        .map((value) => value.trim())
        .filter((value) => value.length > 0)
        .filter((value) => !['agent', 'copilot', 'function', 'read', 'file', 'code'].includes(value.toLowerCase()));

    const symbolLike = filtered.find((value) => value.includes('_'))
        ?? filtered.find((value) => /[a-z][A-Z]/.test(value))
        ?? filtered[0];

    return symbolLike ?? null;
}

function isDefinitionGroundingTask(goal: string): boolean {
    const normalized = goal.toLowerCase();
    return (
        normalized.includes('这个函数是干什么')
        || normalized.includes('what does')
        || normalized.includes('what is')
        || normalized.includes('analyze')
        || normalized.includes('分析')
        || normalized.includes('explain')
    );
}

function findDefinitionLine(lines: string[], term: string): number {
    const definitionPatterns = [
        new RegExp(`^\\s*pub\\s+fn\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*fn\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*function\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*const\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*let\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*type\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*struct\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*enum\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*class\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`^\\s*interface\\s+${escapeForRegExp(term)}\\b`, 'i'),
    ];

    for (const pattern of definitionPatterns) {
        const index = lines.findIndex((line) => isProbableCodeDefinitionLine(line) && pattern.test(line));
        if (index !== -1) {
            return index;
        }
    }

    return -1;
}

function isProbableCodeDefinitionLine(line: string): boolean {
    const trimmed = line.trimStart();
    if (!trimmed) {
        return false;
    }

    return !trimmed.startsWith('"')
        && !trimmed.startsWith("'")
        && !trimmed.startsWith('`');
}

function selectPreferredRecentWorkspaceFileForSearch(session: SessionState, toolRequest: AgentToolCall): string | null {
    const recentFiles = session.recentWorkspaceFiles.filter((file) => file.trim().length > 0);
    if (recentFiles.length === 0) {
        return null;
    }

    if (recentFiles.length === 1) {
        return recentFiles[0];
    }

    const scopedPath = typeof toolRequest.args.path === 'string' ? toolRequest.args.path.trim() : '';
    if (scopedPath) {
        const normalizedScope = scopedPath.replace(/\\/g, '/').toLowerCase();
        const scopedMatches = recentFiles.filter((file) => file.replace(/\\/g, '/').toLowerCase().includes(normalizedScope));
        return scopedMatches.length === 1 ? scopedMatches[0] : null;
    }

    return null;
}

function buildToolRequestTaskState(
    decision: AgentPlannerDecision,
    relatedFiles: string[],
): AgentTaskState {
    return {
        status: 'needs_tool',
        currentAction: decision.currentAction,
        resultSummary: decision.summary,
        nextAction: decision.nextAction,
        relatedFiles,
        toolCall: decision.toolRequest ? formatToolRequest(decision.toolRequest) : null,
        toolResultSummary: null,
    };
}

function buildToolAnsweredTaskState(
    prompt: string,
    toolRequest: AgentToolCall,
    toolResult: ToolResultPayload,
    summary: string,
    relatedFiles: string[],
): AgentTaskState {
    return {
        status: 'answered',
        currentAction: `已执行 ${toolRequest.summary} 并结合结果完成分析`,
        resultSummary: summary,
        nextAction: `可以继续追问、要求进一步检查，或直接基于当前结论推进任务：${prompt.trim()}`,
        relatedFiles,
        toolCall: formatToolRequest(toolRequest),
        toolResultSummary: toolResult.summary,
    };
}

function buildBlockedToolTaskState(
    toolRequest: AgentToolCall,
    toolResult: ToolResultPayload,
    relatedFiles: string[],
): AgentTaskState {
    return {
        status: 'blocked',
        currentAction: `执行 ${toolRequest.summary} 时失败`,
        resultSummary: toolResult.summary,
        nextAction: '请调整任务描述、检查路径参数，或改为更明确的文件/符号后重试。',
        relatedFiles,
        toolCall: formatToolRequest(toolRequest),
        toolResultSummary: toolResult.summary,
    };
}

function buildToolResultPrompt(
    prompt: string,
    toolRequest: AgentToolCall,
    toolResult: ToolResultPayload,
): string {
    return [
        'Continue the current Feishu coding-agent task using the tool result below.',
        'Ground the answer in the provided tool output. If the tool output is still insufficient, say exactly what is missing instead of guessing.',
        'Original task:',
        prompt,
        'Tool request:',
        formatToolRequest(toolRequest),
        'Tool result summary:',
        toolResult.summary,
        'Tool result output:',
        truncateToolResult(toolResult.output),
    ].join('\n\n');
}

function buildEvidenceOnlyAnswerPrompt(goal: string): string {
    return [
        'The runtime already gathered repository evidence for this task.',
        'Do not emit planner JSON.',
        'Answer the user directly from the current evidence in context.',
        'If the evidence is still insufficient, say exactly what is missing and why.',
        'Keep the answer grounded and concise.',
        'Original task:',
        goal,
    ].join('\n\n');
}

function formatToolRequest(toolRequest: AgentToolCall): string {
    if (toolRequest.name === 'read_file') {
        const path = typeof toolRequest.args.path === 'string' ? toolRequest.args.path : '(unknown path)';
        const startLine = asPositiveInteger(toolRequest.args.startLine);
        const endLine = asPositiveInteger(toolRequest.args.endLine);
        if (startLine !== undefined && endLine !== undefined) {
            return `read_file(${path}:${startLine}-${endLine})`;
        }
        return `read_file(${path})`;
    }

    if (toolRequest.name === 'run_tests') {
        const command = typeof toolRequest.args.command === 'string' && toolRequest.args.command.trim()
            ? toolRequest.args.command.trim()
            : 'default';
        return `run_tests(${command})`;
    }

    if (toolRequest.name === 'write_file') {
        const path = typeof toolRequest.args.path === 'string' ? toolRequest.args.path : '(unknown path)';
        return `write_file(${path})`;
    }

    if (toolRequest.name === 'apply_patch') {
        const path = typeof toolRequest.args.path === 'string' ? toolRequest.args.path : '(unknown path)';
        return `apply_patch(${path})`;
    }

    const query = typeof toolRequest.args.query === 'string' ? toolRequest.args.query : '(empty query)';
    const path = typeof toolRequest.args.path === 'string' && toolRequest.args.path.trim()
        ? `, path=${toolRequest.args.path}`
        : '';
    const regexFlag = toolRequest.args.isRegex === true ? ', regex=true' : '';
    return `search_text(${query}${path}${regexFlag})`;
}

function extractRelatedFilesFromToolRequest(toolRequest: AgentToolCall): string[] {
    if (toolRequest.name === 'run_tests') {
        return [];
    }

    const path = toolRequest.args.path;
    if (typeof path === 'string' && path.trim()) {
        return [path.trim()];
    }
    return [];
}

function dedupeStrings(values: string[]): string[] {
    const unique: string[] = [];
    for (const value of values) {
        const trimmed = value.trim();
        if (trimmed && !unique.includes(trimmed)) {
            unique.push(trimmed);
        }
    }
    return unique;
}

function asPositiveInteger(value: unknown): number | undefined {
    if (typeof value === 'number' && Number.isInteger(value) && value > 0) {
        return value;
    }
    if (typeof value === 'string') {
        const parsed = Number.parseInt(value, 10);
        if (Number.isInteger(parsed) && parsed > 0) {
            return parsed;
        }
    }
    return undefined;
}

function truncateToolResult(output: string): string {
    if (output.length <= MAX_TOOL_RESULT_CHARS) {
        return output;
    }
    return `${output.slice(0, MAX_TOOL_RESULT_CHARS - 1)}…`;
}

function buildAnsweredTaskState(prompt: string, summary: string, relatedFiles: string[]): AgentTaskState {
    return {
        status: 'answered',
        currentAction: relatedFiles.length > 0
            ? '已结合当前工作区和最近上下文生成回答'
            : '已基于当前会话上下文生成回答',
        resultSummary: summary,
        nextAction: `可以继续追问、要求读取更多代码，或直接基于当前结果推进任务：${prompt.trim()}`,
        relatedFiles,
        toolCall: null,
        toolResultSummary: null,
    };
}

function buildTaskStateFromRuntimeRun(prompt: string, run: AgentRunState, relatedFiles: string[]): AgentTaskState {
    const status: AgentStatus = run.status === 'completed'
        ? 'answered'
        : run.status === 'waiting_user'
            ? 'waiting_user'
            : run.status === 'failed'
                ? 'blocked'
                : run.status === 'cancelled'
                    ? 'blocked'
                    : 'working';

    return {
        status,
        currentAction: run.currentAction,
        resultSummary: run.summary,
        nextAction: run.nextAction || `可以继续推进当前任务：${prompt.trim()}`,
        relatedFiles,
        toolCall: null,
        toolResultSummary: null,
    };
}

async function collectWorkspaceContext(prompt: string): Promise<{ summary: string; files: string[] }> {
    const folders = vscode.workspace.workspaceFolders ?? [];
    if (folders.length === 0) {
        return { summary: '', files: [] };
    }

    const searchTerms = extractSearchTerms(prompt);
    if (searchTerms.length === 0) {
        return { summary: '', files: [] };
    }

    const files = await vscode.workspace.findFiles(WORKSPACE_FILE_GLOB, WORKSPACE_EXCLUDE_GLOB, 200);
    const candidates: SnippetCandidate[] = [];

    for (const file of files) {
        try {
            const relativePath = vscode.workspace.asRelativePath(file, false);
            if (shouldSkipWorkspaceFile(relativePath)) {
                continue;
            }

            const stat = await vscode.workspace.fs.stat(file);
            if (stat.size > MAX_FILE_BYTES) {
                continue;
            }

            const raw = await vscode.workspace.fs.readFile(file);
            const text = Buffer.from(raw).toString('utf8');
            const lines = text.split(/\r?\n/);
            const lowerLines = lines.map((line) => line.toLowerCase());

            for (const term of searchTerms) {
                const lowerTerm = term.toLowerCase();
                const matchLine = findBestMatchLine(lines, lowerLines, term);
                if (matchLine === -1) {
                    continue;
                }

                const start = Math.max(0, matchLine - MAX_SURROUNDING_LINES);
                const end = Math.min(lines.length, matchLine + MAX_SURROUNDING_LINES + 1);
                const excerpt = lines
                    .slice(start, end)
                    .map((line, index) => `${start + index + 1}: ${line}`)
                    .join('\n');

                candidates.push({
                    term,
                    relativePath,
                    excerpt,
                    score: scoreWorkspaceSnippet(relativePath, lines[matchLine] ?? '', term),
                });
                break;
            }
        } catch {
            // Skip unreadable files and continue collecting best-effort context.
        }
    }

    const snippets = selectBestSnippets(candidates);
    if (snippets.length === 0) {
        return { summary: '', files: [] };
    }

    return {
        summary: `Retrieved workspace context:\n${snippets.map(formatWorkspaceSnippet).join('\n\n')}`,
        files: snippets.map((snippet) => snippet.relativePath),
    };
}

function shouldSkipWorkspaceFile(relativePath: string): boolean {
    const normalized = relativePath.replace(/\\/g, '/').toLowerCase();
    if (normalized.includes('/tests/') || normalized.includes('/test/') || normalized.includes('/__tests__/')) {
        return true;
    }
    if (normalized.startsWith('docs/')) {
        return true;
    }
    if (normalized === 'readme.md') {
        return true;
    }
    if (normalized.endsWith('.feishu-vscode-bridge-session.json') || normalized.endsWith('.feishu-vscode-bridge-audit.jsonl')) {
        return true;
    }
    return false;
}

function findBestMatchLine(lines: string[], lowerLines: string[], term: string): number {
    const lowerTerm = term.toLowerCase();
    const definitionPatterns = [
        new RegExp(`\\bfn\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`\\bpub\\s+fn\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`\\bfunction\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`\\bconst\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`\\blet\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`\\benum\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`\\bstruct\\s+${escapeForRegExp(term)}\\b`, 'i'),
        new RegExp(`\\btype\\s+${escapeForRegExp(term)}\\b`, 'i'),
    ];

    for (const pattern of definitionPatterns) {
        const index = lines.findIndex((line) => pattern.test(line));
        if (index !== -1) {
            return index;
        }
    }

    return lowerLines.findIndex((line) => line.includes(lowerTerm));
}

function scoreWorkspaceSnippet(relativePath: string, matchedLine: string, term: string): number {
    const normalized = relativePath.replace(/\\/g, '/').toLowerCase();
    const lowerLine = matchedLine.toLowerCase();
    const lowerTerm = term.toLowerCase();
    let score = 0;

    if (normalized.startsWith('src/')) {
        score += 100;
    }
    if (normalized.endsWith('.rs')) {
        score += 40;
    }
    if (normalized.includes(lowerTerm)) {
        score += 20;
    }
    if (/\b(pub\s+)?fn\s+/i.test(matchedLine) || /\bfunction\s+/i.test(matchedLine)) {
        score += 120;
    }
    if (new RegExp(`\\b${escapeForRegExp(term)}\\b`, 'i').test(matchedLine)) {
        score += 20;
    }
    if (lowerLine.includes('assert') || lowerLine.includes('test')) {
        score -= 80;
    }

    return score;
}

function selectBestSnippets(candidates: SnippetCandidate[]): SnippetCandidate[] {
    const sorted = [...candidates].sort((left, right) => right.score - left.score);
    const seenPaths = new Set<string>();
    const snippets: SnippetCandidate[] = [];

    for (const candidate of sorted) {
        if (seenPaths.has(candidate.relativePath)) {
            continue;
        }

        seenPaths.add(candidate.relativePath);
        snippets.push(candidate);
        if (snippets.length >= MAX_WORKSPACE_SNIPPETS) {
            break;
        }
    }

    return snippets;
}

function formatWorkspaceSnippet(candidate: SnippetCandidate): string {
    return `Workspace snippet for ${candidate.term} in ${candidate.relativePath}:\n${candidate.excerpt}`;
}

function buildSessionSummary(session: SessionState): string {
    const parts: string[] = [];

    if (session.recentUserPrompts.length > 0) {
        parts.push(`Recent user requests:\n${session.recentUserPrompts.map((prompt) => `- ${prompt}`).join('\n')}`);
    }

    if (session.lastResponseSummary) {
        parts.push(`Last assistant summary:\n${session.lastResponseSummary}`);
    }

    if (session.recentWorkspaceFiles.length > 0) {
        parts.push(`Recent workspace files:\n${session.recentWorkspaceFiles.map((file) => `- ${file}`).join('\n')}`);
    }

    if (session.recentRuntimeToolRequests.length > 0) {
        parts.push(`Recent runtime tool requests:\n${session.recentRuntimeToolRequests.map((tool) => `- ${formatToolRequest(tool)}`).join('\n')}`);
    }

    if (parts.length === 0) {
        return '';
    }

    return `Session bridge context:\n${parts.join('\n\n')}`;
}

function rememberRecentPrompt(session: SessionState, prompt: string): void {
    const trimmed = prompt.trim();
    if (!trimmed) {
        return;
    }

    session.recentUserPrompts = [trimmed, ...session.recentUserPrompts.filter((entry) => entry !== trimmed)]
        .slice(0, MAX_RECENT_USER_PROMPTS);
}

function rememberRecentWorkspaceFiles(session: SessionState, files: string[]): void {
    if (files.length === 0) {
        return;
    }

    const merged = [...files, ...session.recentWorkspaceFiles];
    const deduped: string[] = [];

    for (const file of merged) {
        if (!deduped.includes(file)) {
            deduped.push(file);
        }
        if (deduped.length >= MAX_RECENT_WORKSPACE_FILES) {
            break;
        }
    }

    session.recentWorkspaceFiles = deduped;
}

function rememberRecentRuntimeToolRequest(session: SessionState, toolRequest: AgentToolCall): void {
    session.recentRuntimeToolRequests = [
        toolRequest,
        ...session.recentRuntimeToolRequests.filter((entry) => !areEquivalentToolRequests(entry, toolRequest)),
    ].slice(0, 6);
}

function trimSessionMessages(session: SessionState): void {
    if (session.messages.length <= MAX_SESSION_MESSAGES + 1) {
        return;
    }

    const [systemMessage, ...rest] = session.messages;
    session.messages = [systemMessage, ...rest.slice(-MAX_SESSION_MESSAGES)];
}

function pruneExpiredSessions(bridge: BridgeContext): void {
    const now = Date.now();

    for (const [sessionId, session] of bridge.sessions.entries()) {
        if (now - session.updatedAt > SESSION_TTL_MS) {
            bridge.sessions.delete(sessionId);
            bridge.output.appendLine(`Expired agent bridge session: ${sessionId}`);
        }
    }
}

function escapeForRegExp(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function extractSearchTerms(prompt: string): string[] {
    const matches = prompt.match(/[A-Za-z_][A-Za-z0-9_]{2,}/g) ?? [];
    const unique = new Set<string>();

    for (const match of matches) {
        const normalized = match.trim();
        if (!normalized) {
            continue;
        }

        const lower = normalized.toLowerCase();
        if (['copilot', 'agent', 'what', 'does', 'this', 'function'].includes(lower)) {
            continue;
        }

        unique.add(normalized);
        if (unique.size >= 5) {
            break;
        }
    }

    return Array.from(unique);
}

function summarize(text: string): string {
    const normalized = text.replace(/\s+/g, ' ').trim();
    if (normalized.length <= MAX_SUMMARY_LENGTH) {
        return normalized;
    }
    return `${normalized.slice(0, MAX_SUMMARY_LENGTH - 1)}…`;
}

function getConfiguredPort(): number {
    return vscode.workspace.getConfiguration('feishuVscodeBridge').get<number>('agentBridge.port', 8765);
}

function respondJson(res: http.ServerResponse, statusCode: number, body: Record<string, unknown>): void {
    res.statusCode = statusCode;
    res.setHeader('Content-Type', 'application/json; charset=utf-8');
    res.end(JSON.stringify(body));
}

async function readJsonBody<T>(req: http.IncomingMessage): Promise<T> {
    const chunks: Buffer[] = [];
    for await (const chunk of req) {
        chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    }
    const raw = Buffer.concat(chunks).toString('utf8').trim();
    if (!raw) {
        throw new Error('request body is empty');
    }
    return JSON.parse(raw) as T;
}

export function deactivate(): void {
    // Disposable server shutdown is handled through context subscriptions.
}

async function runAgentRuntimeLoop(
    bridge: BridgeContext,
    sessionId: string,
    session: SessionState,
    run: AgentRunState,
    promptOverride: string | null,
    reason: 'start' | 'continue' | 'approve',
    config: AgentRuntimeConfig,
): Promise<AgentRuntimeLoopResult> {
    const goal = (promptOverride?.trim() || session.currentTask || '').trim();
    if (!goal) {
        throw new Error('agent runtime requires a current goal');
    }

    const model = await selectChatModel();
    const isFreshRun = run.currentStep === 'initialized';

    if (isFreshRun) {
        const initialContext = buildAgentRuntimeUserPrompt(goal, session, config);
        session.messages.push(vscode.LanguageModelChatMessage.User(initialContext));
    } else if (promptOverride?.trim()) {
        session.messages.push(vscode.LanguageModelChatMessage.User([
            'Continue the current autonomous coding task with this new user instruction.',
            promptOverride.trim(),
        ].join('\n\n')));
    }

    trimSessionMessages(session);
    applyAgentRunTransition(run, {
        status: 'running',
        waitingReason: null,
        pendingUserDecision: null,
        currentAction: reason === 'start'
            ? `Starting the ${config.mode} runtime loop`
            : reason === 'continue'
                ? `Continuing the ${config.mode} runtime loop`
                : 'Resuming after user decision',
        nextAction: 'Planning the next best step.',
        currentStep: 'planning',
    });

    const existingToolSteps = run.checkpoints.filter((checkpoint) => checkpoint.label.startsWith('tool-')).length;
    const existingWriteSteps = run.checkpoints.filter((checkpoint) => checkpoint.label.startsWith('write-')).length;
    let toolSteps = existingToolSteps;
    let writeSteps = existingWriteSteps;
    let iteration = 0;
    let finalMessage = run.summary;

    while (iteration < run.budget.maxIterations) {
        iteration += 1;

        if (session.pendingRuntimeToolRequest) {
            if (toolSteps >= run.budget.maxToolCalls) {
                break;
            }

            const pendingTool = session.pendingRuntimeToolRequest.toolRequest;
            rememberRecentRuntimeToolRequest(session, pendingTool);
            const isWriteTool = pendingTool.name === 'write_file' || pendingTool.name === 'apply_patch';
            if (isWriteTool && writeSteps >= run.budget.maxWriteOperations) {
                break;
            }

            const toolResult = await executeAgentRuntimeTool(session.pendingRuntimeToolRequest.toolRequest);
            session.pendingRuntimeToolRequest = undefined;
            toolSteps += 1;
            if (isWriteTool) {
                writeSteps += 1;
            }

            const relatedFiles = dedupeStrings([
                ...toolResult.relatedFiles,
                ...extractRelatedFilesFromToolRequest(pendingTool),
            ]);
            rememberRecentWorkspaceFiles(session, relatedFiles);

            if (!toolResult.success) {
                applyAgentRunTransition(run, {
                    status: 'failed',
                    currentAction: `Tool execution failed: ${pendingTool.summary}`,
                    nextAction: 'Adjust the task or tool arguments and continue the run.',
                    currentStep: 'tool_failed',
                    summary: toolResult.summary,
                    waitingReason: null,
                    checkpoint: {
                        label: `tool-failed-${iteration}`,
                        statusSummary: toolResult.summary,
                    },
                });
                persistAgentRunSession(bridge, sessionId, session, run);
                return {
                    message: toolResult.output,
                    run,
                };
            }

            if (isWriteTool) {
                const artifactId = `${run.runId}:artifact:${Date.now()}`;
                run.reversibleArtifacts.push({
                    artifactId,
                    kind: pendingTool.name === 'apply_patch' ? 'patch' : 'file_write',
                    summary: toolResult.summary,
                    filePaths: relatedFiles,
                });
                if (toolResult.artifactSnapshots && toolResult.artifactSnapshots.length > 0) {
                    session.runtimeArtifactSnapshots[artifactId] = toolResult.artifactSnapshots;
                }
                run.checkpoints.push(createCheckpoint(`write-${writeSteps}`, toolResult.summary));
            } else {
                run.checkpoints.push(createCheckpoint(`tool-${toolSteps}`, toolResult.summary));
            }

            session.messages.push(vscode.LanguageModelChatMessage.User(
                buildToolResultPrompt(goal, pendingTool, toolResult),
            ));
            trimSessionMessages(session);
            applyAgentRunTransition(run, {
                currentAction: `Executed ${pendingTool.summary}`,
                nextAction: 'Observing the tool result and replanning the next step.',
                currentStep: isWriteTool ? `write_${writeSteps}` : `tool_${toolSteps}`,
                waitingReason: null,
            });
            finalMessage = toolResult.summary;
            continue;
        }

        if (
            toolSteps === 0
            && session.recentRuntimeToolRequests.length === 0
            && session.recentWorkspaceFiles.length === 0
            && isDefinitionGroundingTask(goal)
        ) {
            const preferredDefinition = await findPreferredDefinitionLocation(goal);
            if (preferredDefinition) {
                const initialReadRequest: AgentToolCall = {
                    name: 'read_file',
                    args: {
                        path: preferredDefinition.path,
                        startLine: preferredDefinition.startLine,
                        endLine: Math.min(preferredDefinition.endLine, preferredDefinition.startLine + MAX_READ_FILE_LINE_SPAN - 1),
                    },
                    summary: `读取文件 ${preferredDefinition.path}`,
                };

                rememberRecentRuntimeToolRequest(session, initialReadRequest);
                const toolResult = await executeAgentRuntimeTool(initialReadRequest);
                toolSteps += 1;

                const relatedFiles = dedupeStrings([
                    ...toolResult.relatedFiles,
                    preferredDefinition.path,
                ]);
                rememberRecentWorkspaceFiles(session, relatedFiles);

                if (!toolResult.success) {
                    applyAgentRunTransition(run, {
                        status: 'failed',
                        currentAction: `Tool execution failed: ${initialReadRequest.summary}`,
                        nextAction: 'Adjust the task or file path and continue the run.',
                        currentStep: 'tool_failed',
                        summary: toolResult.summary,
                        waitingReason: null,
                        checkpoint: {
                            label: `tool-failed-${iteration}`,
                            statusSummary: toolResult.summary,
                        },
                    });
                    persistAgentRunSession(bridge, sessionId, session, run);
                    return {
                        message: toolResult.output,
                        run,
                    };
                }

                session.messages.push(vscode.LanguageModelChatMessage.User(
                    buildToolResultPrompt(goal, initialReadRequest, toolResult),
                ));
                trimSessionMessages(session);
                applyAgentRunTransition(run, {
                    currentAction: `Executed ${initialReadRequest.summary}`,
                    nextAction: 'Observing the definition context and replanning the next step.',
                    currentStep: `tool_${toolSteps}`,
                    waitingReason: null,
                    checkpoint: {
                        label: `tool-${toolSteps}`,
                        statusSummary: toolResult.summary,
                    },
                });
                finalMessage = toolResult.summary;
                continue;
            }
        }

        const contextSummary = collectEditorContext();
        const workspaceContext = await collectWorkspaceContext(goal);
        const sessionSummary = buildSessionSummary(session);
        const promptContext = [sessionSummary, contextSummary, workspaceContext.summary]
            .filter((value) => value && value.trim().length > 0)
            .join('\n\n');

        rememberRecentWorkspaceFiles(session, workspaceContext.files);
        const plannerOutcome = await decideAgentAction(model, goal, promptContext, dedupeStrings([
            ...session.recentWorkspaceFiles,
            ...workspaceContext.files,
        ]), config);
        const decision = plannerOutcome.decision;

        if (!decision) {
            if (isModelAvailabilityIssue(plannerOutcome.error)) {
                applyAgentRunTransition(run, {
                    status: 'waiting_user',
                    currentAction: 'The runtime is blocked because the configured model is currently unavailable',
                    nextAction: 'Retry this run later, or switch the VS Code chat model to an available option before continuing.',
                    currentStep: 'waiting_model_availability',
                    summary: plannerOutcome.error ?? 'The configured model is currently unavailable.',
                    waitingReason: 'model_availability',
                    checkpoint: {
                        label: `waiting-model-${iteration}`,
                        statusSummary: plannerOutcome.error ?? 'The configured model is currently unavailable.',
                    },
                });
                persistAgentRunSession(bridge, sessionId, session, run);
                return {
                    message: run.summary,
                    run,
                };
            }

            if (toolSteps > 0) {
                session.messages.push(vscode.LanguageModelChatMessage.User(
                    buildEvidenceOnlyAnswerPrompt(goal),
                ));
                trimSessionMessages(session);

                const finalText = await sendModelRequest(model, session.messages);
                const summary = summarize(finalText);
                session.messages.push(vscode.LanguageModelChatMessage.Assistant(finalText));
                session.lastResponseSummary = summary;
                trimSessionMessages(session);

                applyAgentRunTransition(run, {
                    status: 'completed',
                    currentAction: `Completed the ${config.mode} runtime loop from gathered evidence`,
                    nextAction: config.mode === 'ask'
                        ? 'You can ask a follow-up question or continue the same task with a narrower instruction.'
                        : 'You can continue the run with a follow-up request, or start a new agent task.',
                    currentStep: 'completed',
                    summary,
                    waitingReason: null,
                    checkpoint: {
                        label: `completed-${iteration}`,
                        statusSummary: summary,
                    },
                });
                persistAgentRunSession(bridge, sessionId, session, run);
                return {
                    message: finalText,
                    run,
                };
            }

            applyAgentRunTransition(run, {
                status: 'failed',
                currentAction: 'Failed to plan the next autonomous step',
                nextAction: 'Try continuing the run with a narrower task or inspect the backend state.',
                currentStep: 'failed',
                summary: 'The runtime could not obtain a valid planning decision from the model.',
                waitingReason: null,
                checkpoint: {
                    label: `failed-${iteration}`,
                    statusSummary: 'The runtime could not obtain a valid planning decision from the model.',
                },
            });
            persistAgentRunSession(bridge, sessionId, session, run);
            return {
                message: run.summary,
                run,
            };
        }

        applyAgentRunTransition(run, {
            currentAction: decision.currentAction,
            nextAction: decision.nextAction,
            summary: decision.summary,
            currentStep: `iteration_${iteration}`,
            waitingReason: null,
            checkpoint: {
                label: `plan-${iteration}`,
                statusSummary: decision.summary,
            },
        });

        if (decision.status === 'needs_tool' && decision.toolRequest) {
            const normalizedToolRequest = await normalizeRuntimeToolRequest(session, decision.toolRequest, goal);
            const previousToolRequest = session.recentRuntimeToolRequests[0];

            if (previousToolRequest && areEquivalentToolRequests(previousToolRequest, normalizedToolRequest)) {
                session.messages.push(vscode.LanguageModelChatMessage.User([
                    'The same tool request was just executed and its result is already in context.',
                    'Do not repeat the same tool call.',
                    'Answer the user with the current evidence, and if anything is still missing, state the exact missing detail instead of requesting the same tool again.',
                ].join('\n')));
                trimSessionMessages(session);

                const finalText = await sendModelRequest(model, session.messages);
                const summary = summarize(finalText);
                session.messages.push(vscode.LanguageModelChatMessage.Assistant(finalText));
                session.lastResponseSummary = summary;
                trimSessionMessages(session);

                applyAgentRunTransition(run, {
                    status: 'completed',
                    currentAction: `Completed the ${config.mode} runtime loop`,
                    nextAction: config.mode === 'ask'
                        ? 'You can ask a follow-up question or continue the same task with a narrower instruction.'
                        : 'You can continue the run with a follow-up request, or start a new agent task.',
                    currentStep: 'completed',
                    summary,
                    waitingReason: null,
                    checkpoint: {
                        label: `completed-${iteration}`,
                        statusSummary: summary,
                    },
                });
                persistAgentRunSession(bridge, sessionId, session, run);

                return {
                    message: finalText,
                    run,
                };
            }

            if (toolSteps >= run.budget.maxToolCalls) {
                break;
            }

            if ((normalizedToolRequest.name === 'write_file' || normalizedToolRequest.name === 'apply_patch') && writeSteps >= run.budget.maxWriteOperations) {
                break;
            }

            const requiresAuthorization = requiresAuthorizationForTool(run, normalizedToolRequest, config);
            if (requiresAuthorization) {
                session.pendingRuntimeToolRequest = {
                    prompt: goal,
                    toolRequest: normalizedToolRequest,
                    requestedAt: Date.now(),
                };
                applyAgentRunTransition(run, {
                    status: 'waiting_user',
                    currentAction: `Waiting for authorization to execute ${normalizedToolRequest.summary}`,
                    nextAction: 'Approve the pending tool request to continue, reject it, or cancel the run.',
                    currentStep: 'waiting_authorization',
                    waitingReason: 'authorization',
                    pendingUserDecision: createAuthorizationPendingDecision(run.runId, normalizedToolRequest),
                    checkpoint: {
                        label: `authorization-${iteration}`,
                        statusSummary: decision.summary,
                    },
                });
                persistAgentRunSession(bridge, sessionId, session, run);

                return {
                    message: decision.message,
                    run,
                };
            }

            rememberRecentRuntimeToolRequest(session, normalizedToolRequest);
            const toolResult = await executeAgentRuntimeTool(normalizedToolRequest);
            toolSteps += 1;

            const relatedFiles = dedupeStrings([
                ...toolResult.relatedFiles,
                ...extractRelatedFilesFromToolRequest(normalizedToolRequest),
                ...decision.relatedFiles,
            ]);
            rememberRecentWorkspaceFiles(session, relatedFiles);

            if (!toolResult.success) {
                applyAgentRunTransition(run, {
                    status: 'failed',
                    currentAction: `Tool execution failed: ${normalizedToolRequest.summary}`,
                    nextAction: 'Adjust the task or file path and continue the run.',
                    currentStep: 'tool_failed',
                    summary: toolResult.summary,
                    waitingReason: null,
                    checkpoint: {
                        label: `tool-failed-${iteration}`,
                        statusSummary: toolResult.summary,
                    },
                });
                persistAgentRunSession(bridge, sessionId, session, run);
                return {
                    message: toolResult.output,
                    run,
                };
            }

            session.messages.push(vscode.LanguageModelChatMessage.User(
                buildToolResultPrompt(goal, normalizedToolRequest, toolResult),
            ));
            trimSessionMessages(session);
            applyAgentRunTransition(run, {
                currentAction: `Executed ${normalizedToolRequest.summary}`,
                nextAction: 'Observing the tool result and replanning the next step.',
                currentStep: `tool_${toolSteps}`,
                waitingReason: null,
            });
            run.checkpoints.push(createCheckpoint(`tool-${toolSteps}`, toolResult.summary));
            finalMessage = toolResult.summary;
            continue;
        }

        const finalText = await sendModelRequest(model, session.messages);
        const summary = summarize(finalText);
        session.messages.push(vscode.LanguageModelChatMessage.Assistant(finalText));
        session.lastResponseSummary = summary;
        trimSessionMessages(session);

        if (run.reversibleArtifacts.length > 0 && run.resultDisposition === 'pending') {
            applyAgentRunTransition(run, {
                status: 'waiting_user',
                currentAction: 'Waiting for result disposition after applying workspace changes',
                nextAction: 'Keep the result, revert the applied changes, or abandon this run result.',
                currentStep: 'waiting_result_disposition',
                summary,
                waitingReason: 'result_disposition',
                pendingUserDecision: createResultDispositionPendingDecision(run.runId),
                checkpoint: {
                    label: `result-disposition-${iteration}`,
                    statusSummary: summary,
                },
            });
            persistAgentRunSession(bridge, sessionId, session, run);

            return {
                message: finalText,
                run,
            };
        }

        applyAgentRunTransition(run, {
            status: 'completed',
            currentAction: `Completed the ${config.mode} runtime loop`,
            nextAction: config.mode === 'ask'
                ? 'You can ask a follow-up question or continue the same task with a narrower instruction.'
                : 'You can continue the run with a follow-up request, or start a new agent task.',
            currentStep: 'completed',
            summary,
            waitingReason: null,
            checkpoint: {
                label: `completed-${iteration}`,
                statusSummary: summary,
            },
        });
        persistAgentRunSession(bridge, sessionId, session, run);

        return {
            message: finalText,
            run,
        };
    }

    applyAgentRunTransition(run, {
        status: 'waiting_user',
        currentAction: 'Reached the current loop budget',
        nextAction: 'Approve the pacing decision to continue, or cancel the run.',
        currentStep: 'waiting_user',
        summary: 'The agent paused after exhausting the current planning, tool, or write budget.',
        waitingReason: 'pacing',
        pendingUserDecision: createPacingPendingDecision(run.runId),
        checkpoint: {
            label: 'waiting-user',
            statusSummary: 'The agent paused after exhausting the current planning, tool, or write budget.',
        },
    });
    persistAgentRunSession(bridge, sessionId, session, run);

    return {
        message: finalMessage || run.summary,
        run,
    };
}

function buildAgentRuntimeUserPrompt(goal: string, session: SessionState, config: AgentRuntimeConfig): string {
    const sessionSummary = buildSessionSummary(session);
    return [
        `You are running the ${config.mode} mode of an autonomous coding loop for a Feishu remote agent bridge.`,
        'Stay grounded in repository context. Use the planning/tool loop to inspect code before answering when needed.',
        'Once a search has already found the likely file, stop broad searching and read that file directly.',
        config.allowWriteTools
            ? 'You may request read_file, search_text, run_tests, write_file, or apply_patch. Write actions must be narrowly scoped and will require explicit approval before execution.'
            : config.allowTestTool
                ? 'You may request read_file, search_text, and run_tests. Do not request write_file or apply_patch in this mode.'
                : 'You may request read_file and search_text only. Do not request run_tests, write_file, or apply_patch in this mode.',
        sessionSummary || '',
        'Current autonomous task:',
        goal,
    ]
        .filter((value) => value && value.trim().length > 0)
        .join('\n\n');
}

async function executeAgentRuntimeTool(toolRequest: AgentToolCall): Promise<ToolResultPayload> {
    if (toolRequest.name === 'read_file') {
        return executeReadFileTool(toolRequest);
    }

    if (toolRequest.name === 'search_text') {
        return executeSearchTextTool(toolRequest);
    }

    if (toolRequest.name === 'run_tests') {
        return executeRunTestsTool(toolRequest);
    }

    if (toolRequest.name === 'write_file') {
        return executeWriteFileTool(toolRequest);
    }

    return executeApplyPatchTool(toolRequest);
}

function requiresAuthorizationForTool(run: AgentRunState, toolRequest: AgentToolCall, config: AgentRuntimeConfig): boolean {
    const policy = run.authorizationPolicy;
    if (!policy) {
        return false;
    }

    if (!config.allowWriteTools && (toolRequest.name === 'write_file' || toolRequest.name === 'apply_patch')) {
        return true;
    }

    if ((toolRequest.name === 'write_file' || toolRequest.name === 'apply_patch') && policy.requireWriteApproval) {
        return true;
    }

    if (toolRequest.name === 'run_tests' && policy.requireShellApproval) {
        return false;
    }

    return false;
}

async function executeReadFileTool(toolRequest: AgentToolCall): Promise<ToolResultPayload> {
    const path = typeof toolRequest.args.path === 'string' ? toolRequest.args.path.trim() : '';
    if (!path) {
        return {
            success: false,
            output: 'read_file tool requires a path',
            summary: 'Missing file path for read_file tool.',
            relatedFiles: [],
        };
    }

    const target = await resolveWorkspaceFile(path);
    if (!target) {
        return {
            success: false,
            output: `Unable to resolve file path: ${path}`,
            summary: `Could not resolve file path ${path}.`,
            relatedFiles: [],
        };
    }

    try {
        const raw = await vscode.workspace.fs.readFile(target);
        const text = Buffer.from(raw).toString('utf8');
        const lines = text.split(/\r?\n/);
        const lineRange = normalizeReadFileRange(toolRequest.args);
        const startLine = lineRange?.startLine ?? 1;
        const endLine = Math.min(lineRange?.endLine ?? Math.min(lines.length, DEFAULT_READ_FILE_LINE_SPAN), lines.length);
        const excerpt = lines
            .slice(startLine - 1, endLine)
            .map((line, index) => `${startLine + index}: ${line}`)
            .join('\n');
        const relativePath = vscode.workspace.asRelativePath(target, false);

        return {
            success: true,
            output: [`File: ${target.fsPath}`, `Lines: ${startLine}-${endLine} / ${lines.length}`, '', excerpt].join('\n'),
            summary: `Read ${relativePath} lines ${startLine}-${endLine}.`,
            relatedFiles: [relativePath],
        };
    } catch (error) {
        return {
            success: false,
            output: error instanceof Error ? error.message : String(error),
            summary: `Failed to read ${path}.`,
            relatedFiles: [],
        };
    }
}

async function executeSearchTextTool(toolRequest: AgentToolCall): Promise<ToolResultPayload> {
    const query = typeof toolRequest.args.query === 'string' ? toolRequest.args.query.trim() : '';
    const pathScope = typeof toolRequest.args.path === 'string' ? toolRequest.args.path.trim() : '';
    if (!query) {
        return {
            success: false,
            output: 'search_text tool requires a query',
            summary: 'Missing query for search_text tool.',
            relatedFiles: [],
        };
    }

    const files = await vscode.workspace.findFiles(WORKSPACE_FILE_GLOB, WORKSPACE_EXCLUDE_GLOB, 200);
    const matches: Array<{ relativePath: string; lineNumber: number; line: string; score: number }> = [];
    const regex = toolRequest.args.isRegex === true ? new RegExp(query, 'i') : null;
    const lowered = query.toLowerCase();

    for (const file of files) {
        const relativePath = vscode.workspace.asRelativePath(file, false);
        if (pathScope && !relativePath.replace(/\\/g, '/').includes(pathScope.replace(/\\/g, '/'))) {
            continue;
        }

        try {
            const stat = await vscode.workspace.fs.stat(file);
            if (stat.size > MAX_FILE_BYTES) {
                continue;
            }

            const raw = await vscode.workspace.fs.readFile(file);
            const text = Buffer.from(raw).toString('utf8');
            const lines = text.split(/\r?\n/);

            for (let index = 0; index < lines.length; index += 1) {
                const line = lines[index] ?? '';
                const matched = regex ? regex.test(line) : line.toLowerCase().includes(lowered);
                if (!matched) {
                    continue;
                }

                matches.push({
                    relativePath,
                    lineNumber: index + 1,
                    line,
                    score: scoreSearchMatch(relativePath, line, query, toolRequest.args.isRegex === true),
                });
            }
        } catch {
            // Best-effort search; unreadable files are skipped.
        }
    }

    if (matches.length === 0) {
        return {
            success: true,
            output: `No matches found for ${query}.`,
            summary: `No matches found for ${query}.`,
            relatedFiles: [],
        };
    }

    matches.sort((left, right) => {
        if (right.score !== left.score) {
            return right.score - left.score;
        }
        if (left.relativePath !== right.relativePath) {
            return left.relativePath.localeCompare(right.relativePath);
        }
        return left.lineNumber - right.lineNumber;
    });

    const topMatches = matches.slice(0, 20);
    const relatedFiles = dedupeStrings(topMatches.map((match) => match.relativePath));

    return {
        success: true,
        output: topMatches.map((match) => `${match.relativePath}:${match.lineNumber}: ${match.line}`).join('\n'),
        summary: `Found ${matches.length} matches for ${query}.`,
        relatedFiles,
    };
}

function scoreSearchMatch(relativePath: string, matchedLine: string, query: string, isRegex: boolean): number {
    const normalizedPath = relativePath.replace(/\\/g, '/').toLowerCase();
    const lowerLine = matchedLine.toLowerCase();
    const lowerQuery = query.toLowerCase();
    let score = 0;

    if (normalizedPath.startsWith('src/')) {
        score += 100;
    }
    if (normalizedPath.endsWith('.rs')) {
        score += 40;
    }
    if (normalizedPath.includes('/tests/') || normalizedPath.includes('/test/') || normalizedPath.includes('/__tests__/')) {
        score -= 120;
    }

    if (/\b(pub\s+)?fn\s+/i.test(matchedLine) || /\bfunction\s+/i.test(matchedLine)) {
        score += 140;
    }
    if (/\b(enum|struct|type|class|interface|const|let)\s+/i.test(matchedLine)) {
        score += 80;
    }

    if (!isRegex && new RegExp(`\\b${escapeForRegExp(query)}\\b`, 'i').test(matchedLine)) {
        score += 30;
    }
    if (!isRegex && lowerLine.includes(`${lowerQuery}(`)) {
        score += 20;
    }
    if (!isRegex && new RegExp(`\\b(pub\\s+)?fn\\s+${escapeForRegExp(query)}\\b`, 'i').test(matchedLine)) {
        score += 200;
    }
    if (!isRegex && new RegExp(`\\bfunction\\s+${escapeForRegExp(query)}\\b`, 'i').test(matchedLine)) {
        score += 200;
    }

    return score;
}

async function resolveWorkspaceFile(path: string): Promise<vscode.Uri | null> {
    if (!path.trim()) {
        return null;
    }

    if (/^[A-Za-z]:[\\/]/.test(path) || path.startsWith('/')) {
        return vscode.Uri.file(path);
    }

    const folders = vscode.workspace.workspaceFolders ?? [];
    for (const folder of folders) {
        const candidate = vscode.Uri.joinPath(folder.uri, ...path.replace(/\\/g, '/').split('/').filter(Boolean));
        try {
            await vscode.workspace.fs.stat(candidate);
            return candidate;
        } catch {
            // Try next folder.
        }
    }

    const basename = nodePath.basename(path.replace(/\\/g, '/'));
    if (!basename) {
        return null;
    }

    const matches = await vscode.workspace.findFiles(`**/${basename}`, WORKSPACE_EXCLUDE_GLOB, 5);
    return matches[0] ?? null;
}

async function executeRunTestsTool(toolRequest: AgentToolCall): Promise<ToolResultPayload> {
    const folder = vscode.workspace.workspaceFolders?.[0];
    if (!folder) {
        return {
            success: false,
            output: 'run_tests requires an open workspace folder',
            summary: 'No workspace folder is available for run_tests.',
            relatedFiles: [],
        };
    }

    const command = typeof toolRequest.args.command === 'string' && toolRequest.args.command.trim()
        ? toolRequest.args.command.trim()
        : await detectDefaultTestCommand(folder.uri);
    if (!command) {
        return {
            success: false,
            output: 'Unable to infer a default test command. Provide args.command explicitly.',
            summary: 'Could not determine how to run tests for this workspace.',
            relatedFiles: [],
        };
    }

    try {
        const { stdout, stderr } = await execAsync(command, {
            cwd: folder.uri.fsPath,
            windowsHide: true,
            timeout: 120000,
            maxBuffer: 1024 * 1024,
        });
        const output = [stdout.trim(), stderr.trim()].filter((value) => value.length > 0).join('\n\n');
        return {
            success: true,
            output: output || `(Command completed successfully: ${command})`,
            summary: `Ran tests with \"${command}\" successfully.`,
            relatedFiles: [],
        };
    } catch (error) {
        const execError = error as { stdout?: string; stderr?: string; message?: string };
        const output = [execError.stdout?.trim(), execError.stderr?.trim(), execError.message?.trim()]
            .filter((value): value is string => Boolean(value && value.length > 0))
            .join('\n\n');
        return {
            success: false,
            output: output || `Test command failed: ${command}`,
            summary: `Running tests with \"${command}\" failed.`,
            relatedFiles: [],
        };
    }
}

async function executeWriteFileTool(toolRequest: AgentToolCall): Promise<ToolResultPayload> {
    const targetPath = typeof toolRequest.args.path === 'string' ? toolRequest.args.path.trim() : '';
    const content = typeof toolRequest.args.content === 'string' ? toolRequest.args.content : null;
    if (!targetPath || content === null) {
        return {
            success: false,
            output: 'write_file requires args.path and args.content',
            summary: 'Missing path or content for write_file.',
            relatedFiles: [],
        };
    }

    const target = await resolveWritableWorkspaceFile(targetPath);
    if (!target) {
        return {
            success: false,
            output: `Unable to resolve writable file path: ${targetPath}`,
            summary: `Could not resolve writable file path ${targetPath}.`,
            relatedFiles: [],
        };
    }

    let previousContent = '';
    let existed = false;
    try {
        const existing = await vscode.workspace.fs.readFile(target);
        previousContent = Buffer.from(existing).toString('utf8');
        existed = true;
    } catch {
        existed = false;
    }

    await vscode.workspace.fs.writeFile(target, Buffer.from(content, 'utf8'));
    const relativePath = vscode.workspace.asRelativePath(target, false);
    return {
        success: true,
        output: `Wrote ${content.length} characters to ${relativePath}.`,
        summary: `Updated ${relativePath}.`,
        relatedFiles: [relativePath],
        artifactSnapshots: [
            {
                path: relativePath,
                existed,
                previousContent,
            },
        ],
    };
}

async function executeApplyPatchTool(toolRequest: AgentToolCall): Promise<ToolResultPayload> {
    const targetPath = typeof toolRequest.args.path === 'string' ? toolRequest.args.path.trim() : '';
    const search = typeof toolRequest.args.search === 'string' ? toolRequest.args.search : null;
    const replace = typeof toolRequest.args.replace === 'string' ? toolRequest.args.replace : null;
    if (!targetPath || search === null || replace === null) {
        return {
            success: false,
            output: 'apply_patch requires args.path, args.search, and args.replace',
            summary: 'Missing patch arguments for apply_patch.',
            relatedFiles: [],
        };
    }

    const target = await resolveWorkspaceFile(targetPath);
    if (!target) {
        return {
            success: false,
            output: `Unable to resolve file path: ${targetPath}`,
            summary: `Could not resolve file path ${targetPath}.`,
            relatedFiles: [],
        };
    }

    const raw = await vscode.workspace.fs.readFile(target);
    const current = Buffer.from(raw).toString('utf8');
    if (!current.includes(search)) {
        return {
            success: false,
            output: 'apply_patch search text was not found in the target file',
            summary: `Patch anchor was not found in ${targetPath}.`,
            relatedFiles: [],
        };
    }

    const updated = current.replace(search, replace);
    await vscode.workspace.fs.writeFile(target, Buffer.from(updated, 'utf8'));
    const relativePath = vscode.workspace.asRelativePath(target, false);
    return {
        success: true,
        output: `Applied a targeted replacement in ${relativePath}.`,
        summary: `Patched ${relativePath}.`,
        relatedFiles: [relativePath],
        artifactSnapshots: [
            {
                path: relativePath,
                existed: true,
                previousContent: current,
            },
        ],
    };
}

async function revertRuntimeArtifacts(
    session: SessionState,
    run: AgentRunState,
): Promise<{ success: boolean; summary: string }> {
    const snapshots = run.reversibleArtifacts.flatMap((artifact) => session.runtimeArtifactSnapshots[artifact.artifactId] ?? []);
    if (snapshots.length === 0) {
        return {
            success: false,
            summary: 'No local artifact snapshots were available to revert this runtime result.',
        };
    }

    for (const snapshot of snapshots.reverse()) {
        const target = await resolveWritableWorkspaceFile(snapshot.path);
        if (!target) {
            return {
                success: false,
                summary: `Failed to resolve ${snapshot.path} while reverting the runtime result.`,
            };
        }

        if (!snapshot.existed) {
            try {
                await vscode.workspace.fs.delete(target);
            } catch {
                // Ignore missing file during cleanup.
            }
            continue;
        }

        await vscode.workspace.fs.writeFile(target, Buffer.from(snapshot.previousContent, 'utf8'));
    }

    for (const artifact of run.reversibleArtifacts) {
        delete session.runtimeArtifactSnapshots[artifact.artifactId];
    }

    return {
        success: true,
        summary: `Reverted ${snapshots.length} recorded file snapshot(s) from the current runtime result.`,
    };
}

async function resolveWritableWorkspaceFile(targetPath: string): Promise<vscode.Uri | null> {
    const resolved = await resolveWorkspaceFile(targetPath);
    if (resolved) {
        return resolved;
    }

    if (/^[A-Za-z]:[\\/]/.test(targetPath) || targetPath.startsWith('/')) {
        return vscode.Uri.file(targetPath);
    }

    const folder = vscode.workspace.workspaceFolders?.[0];
    if (!folder) {
        return null;
    }

    const parts = targetPath.replace(/\\/g, '/').split('/').filter(Boolean);
    return vscode.Uri.joinPath(folder.uri, ...parts);
}

async function detectDefaultTestCommand(folder: vscode.Uri): Promise<string | null> {
    const packageJson = vscode.Uri.joinPath(folder, 'package.json');
    const cargoToml = vscode.Uri.joinPath(folder, 'Cargo.toml');
    const pyprojectToml = vscode.Uri.joinPath(folder, 'pyproject.toml');
    const pytestIni = vscode.Uri.joinPath(folder, 'pytest.ini');

    if (await pathExists(cargoToml)) {
        return 'cargo test';
    }

    if (await pathExists(packageJson)) {
        return 'npm test';
    }

    if (await pathExists(pyprojectToml) || await pathExists(pytestIni)) {
        return 'pytest';
    }

    return null;
}

async function pathExists(target: vscode.Uri): Promise<boolean> {
    try {
        await vscode.workspace.fs.stat(target);
        return true;
    } catch {
        return false;
    }
}
