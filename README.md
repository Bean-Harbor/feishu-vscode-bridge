# Feishu VS Code Bridge

A standalone open-source Rust project for bridging Feishu commands to local VS Code development actions.

This repository extracts the Feishu <-> VS Code bridge flow from HarborNAS work and focuses on:

- Shipping a standalone Feishu <-> VS Code Copilot bridge that users can try independently
- Enabling remote Copilot-assisted development from Feishu without requiring HarborOS
- Keeping scope centered on chat-to-editor workflow reliability, setup, and publishability

It is not intended to expand back into HarborOS device or local-agent control inside this repository unless that direction is explicitly revisited later.

- Intent parsing for chat commands
- Plan execution with step-by-step mode
- Continue-all mode (`执行全部`) for one-shot execution
- Safe pause-on-failure behavior

## Product Positioning

- Origin: split out from HarborOS-related work
- Current goal: publish a standalone bridge so users can experience Feishu remote connection to VS Code Copilot first
- Current non-goal: adding HarborOS control capabilities into this repository


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
- Lets the user open the current project in VS Code or open the project directory
- Collects Feishu webhook settings and writes them to `.env`

Compatibility status:

- Windows: locally compiled and launched
- macOS: code path implemented and checked in CI with `cargo check --bin setup-gui`
- Linux: code path implemented and checked in CI with `cargo check --bin setup-gui`

## Current Scope

- Core command and plan execution engine (Rust)
- `继续` / `执行全部` plan intent support with local persisted session state
- Configurable approval gates for selected command types before execution
- Minimal CLI demo executor
- Native desktop setup GUI for initial configuration

## Plan Commands

- `执行计划 <命令1>; <命令2>`: execute exactly one step, then pause
- `继续`: execute the next pending step, or retry the failed step
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

Session notes:

- CLI 会话保存在当前目录下的 `.feishu-vscode-bridge-session.json`
- 飞书计划按 chat 隔离，不同会话不会共用同一个待执行计划

## Feishu Card Actions

- 当计划未完成时，机器人会回复互动卡片，而不是纯文本
- 普通暂停卡片提供 `继续` 和 `执行全部` 两个按钮
- 失败暂停卡片提供 `重新执行失败步骤` 和 `执行全部` 两个按钮
- 待审批卡片提供 `批准` 和 `拒绝` 两个按钮
- 按钮点击后会直接触发同名命令，不需要手动再发文本
- 卡片会显示当前状态、已完成步数、当前步骤、剩余步骤，以及失败步骤或完成状态
- 默认审批策略会拦截 `run` / `$` shell 命令和 `git push`
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

Note: on this macOS host, `setup-gui` currently crashes at runtime, so local Feishu validation was completed with a manually prepared `.env` instead.

## Approval Policy Configuration

Approval gating is configured through the environment variable below:

```bash
BRIDGE_APPROVAL_REQUIRED=shell,git_push
```

Supported values:

- `default`: same as `shell,git_push`
- `none`: disable approval gating entirely
- `all`: require approval for all supported gated command types
- `shell`: gate `run ...` and `$ ...`
- `git_push`: gate `git push`
- `git_pull`: gate `git pull`
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

# gate shell and all git actions
BRIDGE_APPROVAL_REQUIRED=shell,git

# disable approvals for local testing
BRIDGE_APPROVAL_REQUIRED=none
```

If `.env` is used, add the variable there and restart `bridge-cli listen`.

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

If `setup-gui` works on your machine, you can use it to generate the file. On the macOS machine used during validation, `setup-gui` crashed at runtime, so `.env` was prepared manually.

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

The original implementation lived in a larger HarborNAS workspace with nested repositories. This project isolates the bridge capability for open-source adoption and easier contribution.

## License

MIT
