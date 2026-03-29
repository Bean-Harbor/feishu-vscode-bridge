# Feishu VS Code Bridge

A standalone open-source Rust project for bridging Feishu commands to local VS Code development actions.

This repository focuses on:

- Shipping a standalone Feishu <-> VS Code Copilot bridge that users can try independently
- Enabling remote Copilot-assisted development from Feishu
- Keeping scope centered on chat-to-editor workflow reliability, setup, and publishability

It is not intended to expand into unrelated device-control or local-agent-control scope inside this repository unless that direction is explicitly revisited later.

- Intent parsing for chat commands
- Plan execution with step-by-step mode
- Continue-all mode (`执行全部`) for one-shot execution
- Safe pause-on-failure behavior

## Product Positioning

- Current goal: publish a standalone bridge so users can experience Feishu remote connection to VS Code Copilot first
- Current non-goal: adding unrelated device-control capabilities into this repository


## Quick Start

```bash
cargo test
cargo run --bin bridge-cli -- "执行计划 打开 Cargo.toml; git status"
cargo run --bin bridge-cli -- "执行全部 打开 Cargo.toml; git status"
```

## GUI Setup Wizard

This repository also includes a desktop setup wizard for guiding users through VS Code detection and Feishu bot configuration.

Run it with:

```bash
cargo run --bin setup-gui
```

The setup wizard currently supports:

- Windows
- macOS
- Linux

What it does:

- Detects whether VS Code is installed before continuing
- Guides the user to install VS Code if it is missing
- Collects Feishu App ID and App Secret and writes them to `.env`
- Preserves unrelated existing `.env` entries when updating Feishu settings

Compatibility status:

- Windows: locally compiled and launched
- macOS: uses native macOS dialog windows via `osascript`, including download-and-retry guidance for missing VS Code and terminal fallback when native dialogs are unavailable
- Linux: code path implemented and checked in CI with `cargo check --bin setup-gui`

## Current Scope

- Core command parsing, plan execution, and bridge runtime engine (Rust)
- Direct commands plus multi-step plans with `继续`, `继续刚才的任务`, and `执行全部`
- Local persisted session state for both plans and one-off commands, including task summary, latest result, recent files, latest diff, and reversible patch context
- Feishu session isolation keyed by chat plus sender, so concurrent users in the same group chat do not overwrite each other's persisted context
- Conversational follow-up actions such as `刚才为什么失败`, `把上一步结果发我`, `继续改刚才那个文件`, `把刚才的 diff 发我`, `把刚才改动的文件列表发我`, and `撤回刚才的补丁`
- Interactive Feishu cards for pause / failure / approval states, with primary actions and follow-up actions grouped separately
- Configurable approval gates for selected command types before execution, including approval handling for patch application
- Configurable default workspace path for Git operations
- JSONL audit logging for Feishu inbound messages and card callbacks, including session key, sender, command, reply kind, and send outcome
- Workspace read/search/test/change tools: `读取`, `列出`, `搜索`, `运行测试`, `查看 diff`, `应用补丁`
- Patch rollback support via reverse apply of the latest remembered patch
- Minimal CLI demo executor
- Native desktop setup GUI for initial configuration

## Plan Commands

Practical Feishu chat examples: see `docs/feishu_chat_templates.md` for copy-paste conversation templates.

One-page quick ref: see `docs/feishu_quick_ref.md` for a condensed cheat sheet suitable for a pinned Feishu doc or group notice.

Ultra-short group notice: see `docs/feishu_group_notice.md` for a minimal pinned-message version.

Live regression checklist: see `docs/feishu_live_regression_checklist.md` for a repeatable real-Feishu validation pass before or after shipping bridge changes.

- `执行计划 <命令1>; <命令2>`: execute exactly one step, then pause
- `继续`: execute the next pending step, or retry the failed step
- `继续刚才的任务`: resume the current plan, or summarize the last persisted task when no active plan remains
- `刚才为什么失败`: explain the latest failure using the stored step result
- `把上一步结果发我`: return the latest stored step result verbatim
- `继续改刚才那个文件`: reopen context by reading the most recently touched file, including files inferred from `apply_patch` diff headers
- `把刚才的 diff 发我`: return the latest stored diff or patch content
- `把刚才改动的文件列表发我`: return the latest remembered file list from a direct command or plan step
- `撤回刚才的补丁`: reverse-apply the latest remembered patch
- Short chat aliases also work for Feishu follow-ups, for example: `为什么失败了`, `看上一步`, `继续这个文件`, `看 diff`, `看文件列表`, `撤回补丁`
- `重新执行失败步骤`: retry only the currently failed step
- `批准`: execute the current approval-gated step
- `拒绝`: cancel the current approval-gated plan
- `执行全部 <命令1>; <命令2>`: execute remaining steps continuously until completion or the first failure

Example:

```text
执行计划 打开 Cargo.toml; git status; $ pwd
继续
执行全部
```

Workspace examples:

```text
读取 src/lib.rs 1-120
列出 src
搜索 parse_intent 在 src
运行测试
运行测试 cargo test --lib
查看 diff
查看 diff src/lib.rs
应用补丁
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
```

Session notes:

- CLI 会话保存在当前目录下的 `.feishu-vscode-bridge-session.json`
- 飞书计划按 chat 隔离，不同会话不会共用同一个待执行计划
- 飞书计划现在按 `chat_id + sender_id` 隔离，群聊里不同发送者不会共享同一份上下文
- 会话会持久化 `current_task`、`pending_steps`、`last_result`、`last_action`
- 会话还会持久化最近一步的原始结果、最近一次明确操作到的主文件、最近文件列表、最近一次 diff / patch 内容，以及最近一次可撤回的补丁
- 飞书监听还会默认追加写入 `.feishu-vscode-bridge-audit.jsonl`，用于审计消息、卡片回调和回复结果；可通过 `BRIDGE_AUDIT_LOG_PATH` 覆盖路径
- 飞书当前只自动处理纯文本 `text` 和纯文本富文本 `post`；如果消息是图片、附件、语音、媒体，或富文本里夹带图片/附件，机器人会直接回复降级提示，要求把命令、日志、补丁或截图关键信息转成文字后再发
- `apply_patch` 会从 unified diff 头里推断一个最近文件列表，所以多文件补丁后也能继续追问文件上下文
- 这些上下文现在不仅在计划执行里持久化，直接命令执行后也会落盘，所以后续追问不再依赖必须先走 `执行计划`
- 即使计划已完成或被拒绝，后续发送 `继续刚才的任务` 仍可看到上次任务摘要
- 在失败暂停或直接执行完成后，可直接发送 `刚才为什么失败`、`把上一步结果发我`、`继续改刚才那个文件`、`把刚才的 diff 发我`、`把刚才改动的文件列表发我`、`撤回刚才的补丁`

Attachment and multimodal constraints:

- 当前支持：`text` 消息、只包含文字/链接/@ 的 `post` 消息
- 当前不自动解析：图片、附件、语音、媒体，以及夹带这些元素的富文本消息
- 推荐降级方式：把命令、报错、日志、补丁片段、文件名和路径直接粘贴成文本；如果来自截图，先把关键内容转成文字
- 这样做是为了避免把截图或附件误读成命令，先保证飞书远程执行链路稳定，再逐步补多模态能力

## Feishu Card Actions

- 当计划未完成时，机器人会回复互动卡片，而不是纯文本
- 普通暂停卡片提供 `继续` 和 `执行全部` 两个按钮
- 失败暂停卡片提供 `重新执行失败步骤` 和 `执行全部` 两个按钮
- 待审批卡片提供 `批准` 和 `拒绝` 两个按钮
- 当会话里已有上下文时，卡片会把主操作和追问操作分成两组，并用更短的对话文案展示按钮，例如 `看上一步`、`继续这个文件`、`看 diff`、`看文件列表`、`撤回补丁`
- 按钮点击后会直接触发同名命令，不需要手动再发文本
- 卡片会显示当前状态、当前任务、最近结果、最近文件、已完成步数、当前步骤、剩余步骤，以及失败步骤或完成状态
- 默认审批策略会拦截 `run` / `$` shell 命令和 `git push`
- 默认审批策略也会拦截 `应用补丁`
- 可通过 `BRIDGE_APPROVAL_REQUIRED` 配置审批范围
- 已在真实飞书环境验证：`执行计划 git status; $ pwd` -> 点击 `继续` 可成功触发 `card.action.trigger` 并正常回卡片回复
- 已在真实飞书环境验证：`执行计划 git status; $ false; $ pwd` -> 点击 `重新执行失败步骤` 可成功触发 `card.action.trigger` 并正常回卡片回复；因失败步骤仍为 `false`，计划会继续停在第 2 步
- 已在真实飞书环境验证：`执行计划 git status; $ test -f /tmp/...flag; $ pwd` -> 第 2 步首次失败后，补齐条件再点击 `重新执行失败步骤`，计划会推进到第 3 步前的安全暂停点
- 已在真实飞书环境验证：设置 `BRIDGE_APPROVAL_REQUIRED=git_pull` 后，发送 `git pull` 会先进入待审批卡片；点击 `批准` 可成功触发 `card.action.trigger` 并正常回卡片回复

If button clicks do not work, check the following first:

- 飞书应用已开启消息卡片回调事件，并完成发布/生效
- 卡片回调事件能到达本地监听器（日志中应看到 `card.action.trigger`）
- 回复目标必须使用 `chat_id`；如果错误地用 `open_chat_id` 调用 `im/v1/messages`，飞书会返回 400

For local validation, a practical end-to-end check is:

```text
执行计划 git status; $ pwd
点击卡片里的「继续」
```

Expected listener log:

```text
📨 收到飞书事件类型: card.action.trigger
🖱️ 收到卡片点击 [...]: 继续
✅ 卡片回复已发送 [...]: 卡片
```

Note: on this macOS host, the native `eframe/winit` window path crashes at runtime, so macOS `setup-gui` now uses native `osascript` dialog windows instead of the `eframe` window path. If `osascript` is unavailable, it falls back to terminal-guided setup instead of aborting.

## Approval Policy Configuration

Approval gating is configured through the environment variable below:

```bash
BRIDGE_APPROVAL_REQUIRED=shell,git_push
```

Supported values:

- `default`: same as `shell,git_push,apply_patch`
- `none`: disable approval gating entirely
- `all`: require approval for all supported gated command types
- `shell`: gate `run ...` and `$ ...`
- `git_push`: gate `git push`
- `git_pull`: gate `git pull`
- `apply_patch`: gate `应用补丁`
- `install_extension`: gate extension install
- `uninstall_extension`: gate extension uninstall
- `extensions`: gate both install and uninstall extension actions
- `git`: gate both `git pull` and `git push`

Examples:

```bash
# default behavior
BRIDGE_APPROVAL_REQUIRED=default

# only gate git write operations
BRIDGE_APPROVAL_REQUIRED=git_push

# gate patch application while leaving shell disabled
BRIDGE_APPROVAL_REQUIRED=apply_patch

# gate shell and all git actions
BRIDGE_APPROVAL_REQUIRED=shell,git

# disable approvals for local testing
BRIDGE_APPROVAL_REQUIRED=none
```

If `.env` is used, add the variable there and restart `bridge-cli listen`.

## Workspace Path Configuration

Git commands can use a default workspace path when the incoming command does not explicitly pass a repository path.

Configure it with:

```bash
BRIDGE_WORKSPACE_PATH=/absolute/path/to/your/repo
```

Behavior:

- `git status`, `git pull`, and `git push` will use `BRIDGE_WORKSPACE_PATH` when no repo path is included in the message
- If the message includes an explicit repo path, that path takes precedence over `BRIDGE_WORKSPACE_PATH`
- This is useful when the Feishu listener runs outside the target repository, or when you want all Git operations pinned to one workspace

Examples:

```bash
# .env
BRIDGE_WORKSPACE_PATH=/Users/Bean/Documents/trae_projects/feishu-vscode-bridge
```

```text
git status
git pull
git push
```

These commands will operate on `BRIDGE_WORKSPACE_PATH` by default.

## Test Command Configuration

`运行测试` supports either a default workspace test command or an explicit command sent from Feishu.

Configure the default command with:

```bash
BRIDGE_TEST_COMMAND="cargo test"
```

Behavior:

- `运行测试` will use `BRIDGE_TEST_COMMAND` when set
- If `BRIDGE_TEST_COMMAND` is not set, it defaults to `cargo test`
- `运行测试 cargo test --lib` or another explicit command overrides the default for that single request

Examples:

```text
运行测试
运行测试 cargo test --lib
运行测试 cargo test --test approval_card_flow
```

## Automated Approval Flow Tests

Approval card behavior is now covered by integration tests that exercise persisted session state without calling real `git` or shell commands.

Run them with:

```bash
cargo test --test approval_card_flow
```

Covered flows:

- `执行全部 git pull; git status` -> returns approval card -> `批准` -> executes the gated step and remaining steps -> clears persisted session state
- `执行计划 git pull; git status` -> returns approval card -> `拒绝` -> cancels the pending plan and clears persisted session state

## Local Debugging

This section describes a practical local debugging flow for Feishu message delivery and card button callbacks.

### 1. Prepare `.env`

Create `.env` in the repository root:

```bash
cat > .env <<'EOF'
FEISHU_APP_ID=your_app_id
FEISHU_APP_SECRET=your_app_secret
EOF
```

You can use `setup-gui` to generate or update the file. Existing non-Feishu environment variables in `.env` are preserved when the wizard updates `FEISHU_APP_ID` and `FEISHU_APP_SECRET`.

### 2. Start the Listener

Run the listener from the repository root so persisted session state is written to the correct working directory:

```bash
killall bridge-cli 2>/dev/null || true
cargo build --bin bridge-cli
./target/debug/bridge-cli listen
```

Expected startup logs:

```text
✅ 飞书认证成功
🔗 正在获取 WebSocket 连接地址...
🔗 连接到飞书 WebSocket...
✅ WebSocket 已连接，等待飞书消息...
```

### 3. Capture Logs to a File

For repeatable debugging, write a fresh log file before each validation round:

```bash
killall bridge-cli 2>/dev/null || true
log=/tmp/feishu-vscode-bridge-listen-fresh.log
: > "$log"
echo "=== restarted $(date '+%Y-%m-%d %H:%M:%S') ===" >> "$log"
set -a && source .env && set +a
cargo build --bin bridge-cli
./target/debug/bridge-cli listen >> "$log" 2>&1
```

Inspect logs with:

```bash
tail -n 80 /tmp/feishu-vscode-bridge-listen-fresh.log
```

### 4. Validate Basic Message Delivery

Send this message from Feishu:

```text
执行计划 git status; $ pwd
```

Expected log pattern:

```text
📨 收到飞书事件类型: im.message.receive_v1
📩 收到消息 [...]: 执行计划 git status; $ pwd
↩️ 准备回复 [...]: 卡片
✅ 回复已发送 [...]: 卡片
```

### 5. Validate Card Button Callback

Click `继续` in the returned Feishu card.

Expected log pattern:

```text
📨 收到飞书事件类型: card.action.trigger
🖱️ 收到卡片点击 [...]: 继续
↩️ 准备卡片回复 [...]: 卡片
✅ 卡片回复已发送 [...]: 卡片
```

This confirms the full callback path:

- Feishu delivered the card action event
- the listener resumed the persisted plan session
- the bridge replied successfully to the same chat

### 6. Validate Failed-Step Retry

Use a command sequence where the second step fails, for example:

```text
执行计划 git status; $ false; $ pwd
```

Expected behavior:

- the first step succeeds
- the second step fails
- the returned card shows a paused failure state
- the card includes `重新执行失败步骤` and `执行全部`

Expected log pattern when the failure is first produced:

```text
📨 收到飞书事件类型: im.message.receive_v1
📩 收到消息 [...]: 执行计划 git status; $ false; $ pwd
↩️ 准备回复 [...]: 卡片
✅ 回复已发送 [...]: 卡片
```

Then click `重新执行失败步骤` in the Feishu card.

Expected retry log pattern:

```text
📨 收到飞书事件类型: card.action.trigger
🖱️ 收到卡片点击 [...]: 重新执行失败步骤
↩️ 准备卡片回复 [...]: 卡片
✅ 卡片回复已发送 [...]: 卡片
```

Expected card behavior after retry:

- if the failed step still fails, the plan remains paused on the same step
- if the failed step succeeds, the plan advances to the next remaining step

Validated live result on this machine:

- sending `执行计划 git status; $ false; $ pwd` created the failure card successfully
- clicking `重新执行失败步骤` produced `card.action.trigger` and a successful follow-up card reply
- persisted session state stayed at `next_step: 1`, which confirms the bridge retried the failing second step and kept the plan paused when `false` failed again
- sending `执行计划 git status; $ test -f /tmp/...flag; $ pwd` and then clicking `继续` produced the expected failure on step 2
- after creating the missing `/tmp/...flag` file, clicking `重新执行失败步骤` advanced persisted session state to `next_step: 2`, which confirms the failed second step retried successfully and the plan advanced to step 3

### 7. Common Failure Cases

- 高风险命令没有进入审批卡片，直接执行了：
	确认当前命令是否属于 `BRIDGE_APPROVAL_REQUIRED` 覆盖范围，并检查监听进程是否已重启加载新配置。

- No `card.action.trigger` log appears:
	Feishu app callback configuration is still incomplete, disabled, or not yet published.

- `im/v1/messages` returns HTTP 400 with `receive_id_type=open_chat_id`:
	The callback reply target is wrong. Card replies must be sent with `chat_id`.

- `⚠️ 当前没有待继续的计划`:
	The listener and the command that created the plan are not using the same session key or working directory.

- `.feishu-vscode-bridge-session.json` is not created:
	The plan may have completed immediately, or the listener was started outside the repository root.

### 8. Persisted Session File

When a step-by-step plan pauses, the listener writes session state to:

```text
.feishu-vscode-bridge-session.json
```

This file is created in the current working directory of the `bridge-cli listen` process, so always start the listener from the repository root.

## Next Milestones

- Feishu adapter as independent module
- VS Code bridge action adapter
- Configurable approval policy and workspace scope
- End-to-end integration tests

## Why This Repo

This project isolates the Feishu <-> VS Code bridge capability for standalone adoption and easier contribution.

## License

MIT
