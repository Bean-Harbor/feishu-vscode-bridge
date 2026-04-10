export const DEFAULT_READ_FILE_LINE_SPAN = 120;
export const MAX_READ_FILE_LINE_SPAN = 200;

export type RuntimeToolName = 'read_file' | 'search_text' | 'run_tests' | 'write_file' | 'apply_patch';

export type RuntimeToolCallLike = {
    name: RuntimeToolName;
    args: Record<string, unknown>;
};

export function asPositiveInteger(value: unknown): number | undefined {
    if (typeof value === 'number' && Number.isInteger(value) && value > 0) {
        return value;
    }

    if (typeof value === 'string' && /^\d+$/.test(value)) {
        const parsed = Number.parseInt(value, 10);
        return parsed > 0 ? parsed : undefined;
    }

    return undefined;
}

export function normalizeReadFileRange(args: Record<string, unknown>): { startLine: number; endLine: number } | null {
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

export function formatToolRequest(toolRequest: RuntimeToolCallLike): string {
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

export function areEquivalentToolRequests(left: RuntimeToolCallLike, right: RuntimeToolCallLike): boolean {
    return left.name === right.name && formatToolRequest(left) === formatToolRequest(right);
}

export function extractPrimarySymbolCandidate(goal: string): string | null {
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

export function isDefinitionGroundingTask(goal: string): boolean {
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

export function findDefinitionLine(lines: string[], term: string): number {
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

export function isProbableCodeDefinitionLine(line: string): boolean {
    const trimmed = line.trimStart();
    if (!trimmed) {
        return false;
    }

    return !trimmed.startsWith('"')
        && !trimmed.startsWith("'")
        && !trimmed.startsWith('`');
}

export function selectPreferredRecentWorkspaceFileForSearch(
    recentWorkspaceFiles: string[],
    toolRequest: RuntimeToolCallLike,
): string | null {
    const recentFiles = recentWorkspaceFiles.filter((file) => file.trim().length > 0);
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

export function scoreWorkspaceSnippet(relativePath: string, matchedLine: string, term: string): number {
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

function escapeForRegExp(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
