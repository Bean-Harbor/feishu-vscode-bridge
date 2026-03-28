# Work Log

## 2026-03-28

### Summary

- Added in-memory plan execution for multi-step commands
- Implemented `执行计划`, `继续`, and `执行全部` intents with safe pause-on-failure behavior
- Added approval gates for high-risk commands with `批准` / `拒绝` follow-up actions
- Made approval policy configurable through `BRIDGE_APPROVAL_REQUIRED`
- Added a reusable `plan.rs` state machine for step execution and retry/resume behavior
- Enabled `bridge-cli` to execute direct commands from the terminal, not only `listen`
- Fixed `code --goto` argument handling so file+line open works correctly
- Added persisted plan sessions so CLI can continue across multiple invocations
- Isolated persisted sessions by chat key to avoid cross-conversation plan collisions
- Added interactive Feishu plan cards with `继续` and `执行全部` buttons
- Added approval cards with `批准` and `拒绝` buttons for gated steps
- Added approval policy parsing for `default` / `none` / `all` and per-command-type overrides
- Added `BRIDGE_WORKSPACE_PATH` so Git commands can target a configured default workspace when no repo path is passed explicitly
- Extracted bridge runtime logic from `main.rs` into a reusable `bridge.rs` module for testable session and approval orchestration
- Added integration tests for approval card flows with a fake executor so tests do not run real `git pull` or shell commands
- Ignored the local `.feishu-vscode-bridge-session.json` runtime state file so transient approval/session data is not synced to GitHub
- Extended WebSocket event parsing to handle both text messages and card action callbacks
- Upgraded plan cards to show current step, remaining steps, failed step details, and completion status
- Added a dedicated `重新执行失败步骤` card action for paused failures
- Fixed live Feishu card callback replies to normalize callback targets to `chat_id` before calling `im/v1/messages`
- Completed end-to-end validation in Feishu: plan card creation, `继续` button callback, and follow-up card reply all succeeded
- Completed end-to-end validation in Feishu for `重新执行失败步骤`: callback delivery and follow-up card reply succeeded, and the persisted session correctly remained paused on the failing second step
- Completed end-to-end validation in Feishu for the success-after-retry path: a failing second step was retried after fixing its runtime precondition, and the persisted session advanced to the third step
- Completed end-to-end validation in Feishu for a non-default approval policy: with `BRIDGE_APPROVAL_REQUIRED=git_pull`, sending `git pull` produced a pending-approval card and clicking `批准` successfully triggered `card.action.trigger` and the follow-up card reply
- Completed end-to-end validation in Feishu for the default `git push` approval flow: sending `git push` produced a pending-approval card, clicking `批准` completed the auto commit, and the resulting commit was pushed to `origin/main`
- Confirmed local macOS `setup-gui` currently crashes at runtime, so `.env` was prepared manually for listener validation

### Files Added

- `src/plan.rs` — plan session, progress model, and execution state machine
- `src/bridge.rs` — reusable bridge runtime, persisted-session orchestration, and card rendering
- `tests/approval_card_flow.rs` — integration coverage for approve/reject approval-card flows

### Files Updated

- `src/lib.rs` — added plan intents, execution mode parsing, and unit tests
- `src/main.rs` — reduced to CLI/listener entrypoint and Feishu response dispatch
- `src/feishu.rs` — added card callback parsing, multiline/post message parsing, and `chat_id` normalization for card replies
- `src/vscode.rs` — fixed `open_file()` to pass `--goto` correctly
- `src/vscode.rs` — added default workspace-path resolution for Git operations and made `git push` path-safe by executing Git subcommands directly
- `README.md` — updated quick start, plan commands, and approval-flow test coverage
- `.gitignore` — ignore local persisted session state file

### Verification

- `cargo test`
- `cargo run --bin bridge-cli -- "执行计划 打开 Cargo.toml; git status"`
- `cargo run --bin bridge-cli -- "继续"`
- `cargo run --bin bridge-cli -- '$ pwd'` then `cargo run --bin bridge-cli -- "批准"`
- `BRIDGE_APPROVAL_REQUIRED=git_pull cargo run --bin bridge-cli -- "git pull"`
- `BRIDGE_WORKSPACE_PATH=/Users/Bean/Documents/trae_projects/feishu-vscode-bridge cargo test`
- `cargo test --test approval_card_flow`
- Live Feishu validation: set `BRIDGE_APPROVAL_REQUIRED=git_pull`, send `git pull`, then click card button `批准`
- Live Feishu validation: send `执行计划 git status; $ pwd`, then click card button `继续`
- Live Feishu validation: send `执行计划 git status; $ false; $ pwd`, then click card button `重新执行失败步骤`
- Live Feishu validation: send `执行计划 git status; $ test -f /tmp/...flag; $ pwd`, let step 2 fail once, create the missing flag file, then click `重新执行失败步骤`
- Live Feishu validation: send `git push`, then click card button `批准`, and verify that the generated `auto commit via feishu-bridge` commit reaches `origin/main`

### Live Debugging Notes

- Initial button failure showed Feishu client-side error `200340`, which pointed first to missing or incomplete card callback configuration on the Feishu app side
- After callback delivery was enabled, the local listener successfully received `card.action.trigger`
- The remaining code-side root cause was an incorrect reply target type: callback replies were sent with `receive_id_type=open_chat_id`
- The fix was to normalize card callback reply targets to `chat_id` before sending follow-up text or cards
- Fresh listener logs then confirmed the full happy path:
     - received `im.message.receive_v1`
     - sent the initial plan card
     - received `card.action.trigger`
     - sent the follow-up card successfully
- Retry validation also confirmed the failure-loop behavior:
     - the failed-step retry button delivered `card.action.trigger`
     - the bridge sent the follow-up card successfully
     - `.feishu-vscode-bridge-session.json` remained at `next_step: 1`, meaning the second step retried and failed again as expected
- Retry validation also confirmed the success-after-retry behavior:
     - after the missing runtime condition was restored, the failed-step retry button delivered `card.action.trigger`
     - the bridge sent the follow-up card successfully
     - `.feishu-vscode-bridge-session.json` advanced to `next_step: 2`, meaning the second step succeeded on retry and the plan moved on to step 3
- Non-default approval-policy validation also confirmed that:
     - `BRIDGE_APPROVAL_REQUIRED=git_pull` changed approval scope without code changes
     - a plain `git pull` message entered the approval flow in live Feishu
     - clicking `批准` delivered `card.action.trigger` and the bridge sent the follow-up card successfully
- Default `git push` approval validation also confirmed that:
     - with default approval policy, a plain `git push` message entered the approval flow in live Feishu
     - clicking `批准` delivered `card.action.trigger` and the bridge sent the follow-up card successfully
     - the bridge created commit `047bce9` with message `auto commit via feishu-bridge`
     - the generated commit reached `origin/main`, proving the default approval path completes commit plus push end to end

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

- Add configurable workspace path for Git operations
- Add more VS Code operations (search, terminal management)
- Add live Feishu validation for non-default `BRIDGE_APPROVAL_REQUIRED` combinations

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