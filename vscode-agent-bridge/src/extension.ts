import * as http from 'http';
import * as vscode from 'vscode';

type AskRequest = {
    sessionId: string;
    prompt: string;
};

type ResetRequest = {
    sessionId: string;
};

type AgentToolName = 'read_file' | 'search_text';

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

type SessionState = {
    messages: vscode.LanguageModelChatMessage[];
    lastResponseSummary?: string;
    recentUserPrompts: string[];
    recentWorkspaceFiles: string[];
    currentTask?: string;
    pendingToolRequest?: PendingToolRequest;
    updatedAt: number;
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
const MAX_SUMMARY_LENGTH = 200;
const MAX_WORKSPACE_SNIPPETS = 3;
const MAX_FILE_BYTES = 512 * 1024;
const MAX_SURROUNDING_LINES = 4;
const MAX_SESSION_MESSAGES = 12;
const MAX_RECENT_USER_PROMPTS = 3;
const MAX_RECENT_WORKSPACE_FILES = 3;
const SESSION_TTL_MS = 30 * 60 * 1000;
const MAX_TOOL_RESULT_CHARS = 12_000;
const WORKSPACE_FILE_GLOB = '**/*.{rs,ts,tsx,js,jsx,py,toml}';
const WORKSPACE_EXCLUDE_GLOB = '**/{.git,node_modules,target,out,dist,build,.next,coverage}/**';
const BOOTSTRAP_WORKSPACE_ENV = 'BRIDGE_AGENT_BOOTSTRAP_WORKSPACE';

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
    const contextSummary = collectEditorContext();
    const workspaceContext = await collectWorkspaceContext(prompt);
    const sessionSummary = buildSessionSummary(session);
    const promptContext = [sessionSummary, contextSummary, workspaceContext.summary]
        .filter((value) => value && value.trim().length > 0)
        .join('\n\n');
    const userPrompt = promptContext
        ? `${promptContext}\n\nUser request:\n${prompt}`
        : prompt;

    session.messages.push(vscode.LanguageModelChatMessage.User(userPrompt));
    session.currentTask = prompt;

    const model = await selectChatModel();
    const plannerDecision = await decideAgentAction(model, prompt, promptContext, workspaceContext.files);

    rememberRecentPrompt(session, prompt);
    rememberRecentWorkspaceFiles(session, workspaceContext.files);
    session.updatedAt = Date.now();

    if (plannerDecision?.status === 'needs_tool' && plannerDecision.toolRequest) {
        session.pendingToolRequest = {
            prompt,
            toolRequest: plannerDecision.toolRequest,
            requestedAt: Date.now(),
        };
        trimSessionMessages(session);
        bridge.sessions.set(sessionId, session);

        const relatedFiles = dedupeStrings([
            ...plannerDecision.relatedFiles,
            ...workspaceContext.files,
            ...extractRelatedFilesFromToolRequest(plannerDecision.toolRequest),
        ]);
        const taskState = buildToolRequestTaskState(plannerDecision, relatedFiles);

        return {
            sessionId,
            status: taskState.status,
            message: plannerDecision.message,
            summary: plannerDecision.summary,
            currentAction: taskState.currentAction,
            nextAction: taskState.nextAction,
            relatedFiles: taskState.relatedFiles,
            toolCall: taskState.toolCall,
            toolResultSummary: taskState.toolResultSummary,
            toolRequest: plannerDecision.toolRequest,
            taskState,
            context: [sessionSummary, contextSummary, workspaceContext.summary]
                .filter((value) => value && value.trim().length > 0)
                .join('\n\n'),
        };
    }

    const text = await sendModelRequest(model, session.messages);
    const summary = summarize(text);
    const taskState = buildAnsweredTaskState(prompt, summary, workspaceContext.files);

    session.pendingToolRequest = undefined;
    session.messages.push(vscode.LanguageModelChatMessage.Assistant(text));
    session.lastResponseSummary = summary;
    trimSessionMessages(session);
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
        toolRequest: null,
        taskState,
        context: [sessionSummary, contextSummary, workspaceContext.summary]
            .filter((value) => value && value.trim().length > 0)
            .join('\n\n'),
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

    const toolRequest = sanitizeToolRequest(payload.toolRequest) ?? pending.toolRequest;
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
        updatedAt: Date.now(),
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

async function decideAgentAction(
    model: vscode.LanguageModelChat,
    prompt: string,
    promptContext: string,
    relatedFiles: string[],
): Promise<AgentPlannerDecision | null> {
    const plannerPrompt = [
        'You are planning the next step for a coding agent that can perform at most one read-only tool call before answering.',
        'Return JSON only with this shape: {"status":"answered"|"needs_tool","message":"...","summary":"...","currentAction":"...","nextAction":"...","relatedFiles":["..."],"toolRequest":null|{"name":"read_file"|"search_text","args":{...},"summary":"..."}}.',
        'Only choose needs_tool when the answer depends on repository code that is still missing from the current context.',
        'Prefer search_text before read_file unless you already know the exact file to inspect.',
        'Use read_file args: {"path":"relative/or/absolute","startLine":number,"endLine":number}.',
        'Use search_text args: {"query":"text","path":"optional/path","isRegex":false}.',
        relatedFiles.length > 0 ? `Known related files: ${relatedFiles.join(', ')}` : 'Known related files: none yet.',
        'Current context:',
        promptContext || '(no extra context)',
        'User request:',
        prompt,
    ].join('\n\n');

    try {
        const raw = await sendModelRequest(model, [vscode.LanguageModelChatMessage.User(plannerPrompt)]);
        return parsePlannerDecision(raw);
    } catch {
        return null;
    }
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
        return null;
    }
}

function sanitizeToolRequest(value: unknown): AgentToolCall | null {
    if (!value || typeof value !== 'object') {
        return null;
    }

    const record = value as Record<string, unknown>;
    const name = record.name;
    if (name !== 'read_file' && name !== 'search_text') {
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

        const startLine = asPositiveInteger(args.startLine);
        const endLine = asPositiveInteger(args.endLine);
        const sanitizedArgs: Record<string, unknown> = { path };
        if (startLine !== undefined) {
            sanitizedArgs.startLine = startLine;
        }
        if (endLine !== undefined) {
            sanitizedArgs.endLine = endLine;
        }

        return {
            name,
            args: sanitizedArgs,
            summary: typeof record.summary === 'string' && record.summary.trim()
                ? record.summary.trim()
                : `读取文件 ${path}`,
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
        'Continue the current Feishu coding-agent task using the read-only tool result below.',
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

    const query = typeof toolRequest.args.query === 'string' ? toolRequest.args.query : '(empty query)';
    const path = typeof toolRequest.args.path === 'string' && toolRequest.args.path.trim()
        ? `, path=${toolRequest.args.path}`
        : '';
    const regexFlag = toolRequest.args.isRegex === true ? ', regex=true' : '';
    return `search_text(${query}${path}${regexFlag})`;
}

function extractRelatedFilesFromToolRequest(toolRequest: AgentToolCall): string[] {
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