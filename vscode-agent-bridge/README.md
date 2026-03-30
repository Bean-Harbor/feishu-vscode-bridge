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