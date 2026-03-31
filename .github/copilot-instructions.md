# Project Scope

- This repository is a standalone Feishu <-> VS Code Copilot bridge.
- Its purpose is to publish a Feishu remote bridge for VS Code Copilot that users can try independently.
- Default development direction: improve the Feishu <-> VS Code Copilot connection, setup flow, execution reliability, and standalone publishability.
- Do not proactively steer this repository toward device control, local-agent control, or external platform feature expansion unless the user explicitly asks for that scope change.
- Startup discipline for future sessions:
	- Do not improvise extension startup from raw shell commands before checking the verified path in `docs/work_log.md` and `vscode-agent-bridge/README.md`.
	- The default verified extension startup path is VS Code `F5` using the workspace launch config `Run Feishu Agent Bridge Extension`, which triggers the workspace task `build-feishu-agent-bridge-extension` first.
	- The default verified Feishu listener path is `./scripts/start-live-listener.sh` or `bash ./scripts/start-live-listener.sh` on POSIX hosts, instead of ad hoc `cargo run` or long-lived `target/debug` binaries.
	- When `问 Copilot` or `/health` is broken, first classify the failure as one of: extension bootstrap, extension build prerequisites (`node` / `npm`), VS Code activation, or listener auth; do not repeat blind startup experiments without that classification.