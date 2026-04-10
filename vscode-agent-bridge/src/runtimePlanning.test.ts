import test from 'node:test';
import assert from 'node:assert/strict';

import {
    areEquivalentToolRequests,
    extractPrimarySymbolCandidate,
    findDefinitionLine,
    formatToolRequest,
    isDefinitionGroundingTask,
    normalizeReadFileRange,
    scoreWorkspaceSnippet,
    selectPreferredRecentWorkspaceFileForSearch,
} from './runtimePlanning';

test('extractPrimarySymbolCandidate prefers symbol-like identifiers from analysis goals', () => {
    assert.equal(
        extractPrimarySymbolCandidate('Analyze what the parse_intent function does in this repo'),
        'parse_intent',
    );
    assert.equal(
        extractPrimarySymbolCandidate('请解释 BridgeContext 在这里是做什么的'),
        'BridgeContext',
    );
});

test('findDefinitionLine ignores quoted examples and chooses the real definition', () => {
    const lines = [
        '"fn parse_intent() is only an example string"',
        '    // helper comment',
        'pub fn parse_intent(text: &str) -> Intent {',
        '    Intent::Help',
        '}',
    ];

    assert.equal(findDefinitionLine(lines, 'parse_intent'), 2);
});

test('scoreWorkspaceSnippet favors source definitions over test assertions', () => {
    const sourceScore = scoreWorkspaceSnippet('src/lib.rs', 'pub fn parse_intent(text: &str) -> Intent {', 'parse_intent');
    const testScore = scoreWorkspaceSnippet('tests/lib_test.rs', 'assert_eq!(parse_intent("help"), Intent::Help);', 'parse_intent');

    assert.ok(sourceScore > testScore);
});

test('selectPreferredRecentWorkspaceFileForSearch narrows to the scoped file when possible', () => {
    const toolRequest = {
        name: 'search_text' as const,
        args: {
            query: 'parse_intent',
            path: 'src/lib.rs',
        },
    };

    assert.equal(
        selectPreferredRecentWorkspaceFileForSearch(
            ['src/bridge.rs', 'src/lib.rs', 'README.md'],
            toolRequest,
        ),
        'src/lib.rs',
    );
});

test('formatToolRequest and equivalence checks detect repeated identical tool calls', () => {
    const left = {
        name: 'read_file' as const,
        args: { path: 'src/lib.rs', startLine: 10, endLine: 50 },
    };
    const right = {
        name: 'read_file' as const,
        args: { path: 'src/lib.rs', startLine: 10, endLine: 50 },
    };
    const different = {
        name: 'read_file' as const,
        args: { path: 'src/lib.rs', startLine: 20, endLine: 60 },
    };

    assert.equal(formatToolRequest(left), 'read_file(src/lib.rs:10-50)');
    assert.equal(areEquivalentToolRequests(left, right), true);
    assert.equal(areEquivalentToolRequests(left, different), false);
});

test('normalizeReadFileRange clamps wide ranges and infers missing start lines', () => {
    assert.deepEqual(
        normalizeReadFileRange({ startLine: 20, endLine: 500 }),
        { startLine: 20, endLine: 219 },
    );
    assert.deepEqual(
        normalizeReadFileRange({ endLine: 10 }),
        { startLine: 1, endLine: 10 },
    );
});

test('isDefinitionGroundingTask recognizes explanation-style prompts', () => {
    assert.equal(isDefinitionGroundingTask('分析 parse_intent 这个函数是干什么的'), true);
    assert.equal(isDefinitionGroundingTask('run cargo test'), false);
});
