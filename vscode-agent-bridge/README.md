# Feishu VS Code Agent Bridge Extension

This companion extension hosts the VS Code side of the remote agent bridge.

## Purpose

- Start a local HTTP server for the Rust / Feishu bridge
- Maintain per-session ask history inside VS Code
- Read minimal editor context such as the active file and selection
- Call a VS Code language model, preferably a Copilot chat model

## Current Endpoints

- `GET /health`
- `POST /v1/chat/ask`

## Local Prerequisites

- Node.js with `npm` available on `PATH`
- VS Code with a compatible chat model provider enabled
  - preferred: GitHub Copilot Chat

## Build

From this directory:

```powershell
npm install
npm run compile
```

## Run In Extension Host

1. Open this repository in VS Code.
2. Run `npm install` and `npm run compile`.
3. Use the workspace launch config `Run Feishu Agent Bridge Extension`.
4. Press `F5` to launch an Extension Development Host.
5. In the Extension Development Host, confirm the output channel `Feishu Agent Bridge` shows the local server port.

The repository now includes:

- `.vscode/launch.json` — launches the Extension Development Host against `vscode-agent-bridge/`
- `.vscode/tasks.json` — builds the companion extension before launch

## Quick Diagnostic Path

Use this shortest path before debugging Rust-side code:

1. Launch the Extension Development Host with `Run Feishu Agent Bridge Extension`.
  - On hosts without `npm` but with an existing compiled `out/extension.js`, you can also use `./scripts/start-extension-dev-host.sh --port 8766` to start an isolated dev host against the repository checkout.
2. Check the `Feishu Agent Bridge` output channel and confirm it logs the local server port.
3. Verify `http://127.0.0.1:8765/health` returns OK, or let `setup-gui` run the same health check after installing the extension.
4. Start the Rust listener with `./scripts/start-live-listener.sh`.
  - If the shell reports `permission denied`, run `bash ./scripts/start-live-listener.sh` or refresh the repository copy so the executable bit is preserved.
5. If the listener reaches Feishu authentication but `/health` is still unavailable, treat the current blocker as extension bootstrap or activation, not Feishu credentials.
6. If `/health` is up but `context` has no `Workspace:` line, assume the bound server belongs to a VS Code window without the repository opened. The extension now skips auto-start in no-workspace windows, so closing and reopening VS Code on the repository should let the correct window claim `8765`.
7. When invoking VS Code CLI from scripts or setup flows, prefer `--add <workspace>` over passing the folder as a positional argument. Opening a folder directly can replace the current window and terminate the active bridge/debug session.

## Rust Bridge Integration

The Rust bridge talks to this extension over a local HTTP endpoint.

Default endpoint:

```text
http://127.0.0.1:8765
```

Override with either:

- `BRIDGE_AGENT_BRIDGE_URL`
- `BRIDGE_AGENT_BRIDGE_PORT`

## First Ask-Style Smoke

1. Launch the Extension Development Host with `Run Feishu Agent Bridge Extension`.
2. Wait for the output channel `Feishu Agent Bridge` to show the local server port.
3. Start the Rust listener as usual.
4. Send this message from Feishu:

```text
问 Copilot parse_intent 这个函数是干什么的
```

Expected result:

- Rust bridge calls `POST /v1/chat/ask`
- the extension returns a model reply
- Feishu receives a text response containing the bridge `session` id and model answer

## Current Limitations

- The Rust bridge currently only uses the direct ask path: `问 Copilot <问题>`
- Plan execution and tool-calling loops are not wired into the extension yet
- The extension maintains its own bridge session; it does not attach to the built-in Copilot Chat panel session