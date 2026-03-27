# Work Log

## 2026-03-27

### Summary

- Implemented full VS Code CLI bridge: users can now control VS Code through Feishu messages
- Added 12 intent types for comprehensive command coverage (file ops, extensions, Git, shell)
- Created real shell command executor with output capture and timeout support
- Created VS Code CLI operations module wrapping `code` CLI and `git` commands
- Added message deduplication (TTL-based) to prevent duplicate event processing
- Rewrote main.rs handler to dispatch all intent types to corresponding VS Code operations
- All 8 unit tests passing; build clean with no warnings

### New Intent Types

| Intent | Example Commands |
|---|---|
| OpenFile | `打开 src/main.rs:42`, `open src/lib.rs` |
| OpenFolder | `打开目录 /home/user/project` |
| InstallExtension | `安装扩展 rust-analyzer` |
| UninstallExtension | `卸载扩展 some.extension` |
| ListExtensions | `已安装扩展`, `list extensions` |
| DiffFiles | `diff a.rs b.rs` |
| GitStatus | `git status` |
| GitPull | `git pull` |
| GitPushAll | `git push 提交消息` |
| RunShell | `$ echo hello`, `shell dir` |
| Help | `帮助`, `help` |
| Unknown | (fallback with hint) |

### Files Added

- `src/executor.rs` — Shell command executor (`CmdResult`, `run_cmd()`, `to_reply()`)
- `src/vscode.rs` — VS Code CLI operations (10 functions wrapping `code` and `git`)

### Files Updated

- `src/lib.rs` — Expanded Intent enum (5→12 variants), added `parse_intent()` with Chinese+English prefix matching, `MessageDedup`, `help_text()`, 8 unit tests
- `src/main.rs` — Rewrote handler to dispatch all 12 intent types, added dedup via `Mutex<Option<MessageDedup>>`

### Commits

- `7e1df83` — feat: integrate Feishu API for message sending
- `1d2c48e` — feat: WebSocket long connection for Feishu event-driven messaging
- `3288234` — feat: VS Code CLI bridge — 12 intent types, shell executor, dedup

### Verification

- `cargo build` — clean, no warnings
- `cargo test` — 8/8 tests passed
- `cargo fmt --check` — no formatting issues
- End-to-end verified: Feishu message → WebSocket → intent parse → VS Code CLI → reply

### Architecture

```
Feishu (用户消息)
  └─ WebSocket 长连接 (protobuf pbbp2)
       └─ main.rs: handle_message()
            ├─ MessageDedup (去重)
            ├─ parse_intent() → Intent enum
            ├─ vscode::* / executor::run_cmd()
            └─ FeishuClient::reply()
```

### Next Candidates

- Add session/context management for multi-step conversations
- Add configurable workspace path for Git operations
- Add more VS Code operations (search, terminal management)
- Support rich card messages instead of plain text replies

## 2026-03-26

### Summary

- Added a native desktop setup wizard for `feishu-vscode-bridge`
- Implemented VS Code detection before Feishu configuration
- Added guided installation flow when VS Code is missing
- Added actions to open the current workspace in VS Code or open the project directory
- Improved cross-platform support for Windows, macOS, and Linux
- Added CI checks for `setup-gui` on Ubuntu, Windows, and macOS

### Files Updated

- `Cargo.toml`
- `README.md`
- `.github/workflows/ci.yml`
- `src/bin/setup_gui.rs`

### Verification

- Local Windows compile check passed: `cargo check --bin setup-gui`
- Local Windows launch smoke test passed: `cargo run --bin setup-gui`
- GitHub Actions now includes multi-platform compile validation for the setup GUI

### Notes

- macOS detection now covers both `/Applications/Visual Studio Code.app` and `~/Applications/Visual Studio Code.app`
- The setup wizard writes Feishu configuration to `.env` in the project root

### Next Candidates

- Preserve unrelated existing environment variables when updating `.env`
- Validate Feishu webhook format before saving
- Add screenshots for the setup wizard to the README