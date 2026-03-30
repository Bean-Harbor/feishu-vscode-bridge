import * as http from 'http';
import * as vscode from 'vscode';

type AskRequest = {
    sessionId: string;
    prompt: string;
};

type ResetRequest = {
    sessionId: string;
};

type SessionState = {
    messages: vscode.LanguageModelChatMessage[];
    lastResponseSummary?: string;
    recentUserPrompts: string[];
    recentWorkspaceFiles: string[];
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
const MAX_SUMMARY_LENGTH = 200;
const MAX_WORKSPACE_SNIPPETS = 3;
const MAX_FILE_BYTES = 512 * 1024;
const MAX_SURROUNDING_LINES = 4;
const MAX_SESSION_MESSAGES = 12;
const MAX_RECENT_USER_PROMPTS = 3;
const MAX_RECENT_WORKSPACE_FILES = 3;
const SESSION_TTL_MS = 30 * 60 * 1000;
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
        await startBridgeServer(bridge);
    }
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

    const [model] = await vscode.lm.selectChatModels({
        vendor: vscode.workspace.getConfiguration('feishuVscodeBridge').get<string>('agentBridge.vendor', 'copilot'),
    });

    if (!model) {
        throw new Error('No compatible chat model is available. Ensure GitHub Copilot Chat or another LM provider is active in VS Code.');
    }

    const tokenSource = new vscode.CancellationTokenSource();
    try {
        const response = await model.sendRequest(session.messages, {}, tokenSource.token);
        let text = '';
        for await (const fragment of response.text) {
            text += fragment;
        }

        if (!text.trim()) {
            throw new Error('Model returned an empty response.');
        }

        const summary = summarize(text);
        const taskState = buildAnsweredTaskState(prompt, summary, workspaceContext.files);

        session.messages.push(vscode.LanguageModelChatMessage.Assistant(text));
        session.lastResponseSummary = summary;
        rememberRecentPrompt(session, prompt);
        rememberRecentWorkspaceFiles(session, workspaceContext.files);
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
            taskState,
            context: [sessionSummary, contextSummary, workspaceContext.summary]
                .filter((value) => value && value.trim().length > 0)
                .join('\n\n'),
        };
    } finally {
        tokenSource.dispose();
    }
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