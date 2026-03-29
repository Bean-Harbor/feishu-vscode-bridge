# Feishu Live Regression Checklist

这份清单用于每次重要改动后，快速回归真实飞书链路，确认桥接器仍然像一段连续对话，而不是只在本地 CLI 里可用。

目标不是覆盖所有功能，而是优先覆盖最容易在真实飞书环境里退化的几条链路。

## 准备

- 本地已存在有效 `.env`
- 已编译监听器：`cargo build`
- 建议关闭审批：`BRIDGE_APPROVAL_REQUIRED=none`
- 建议固定工作区：`BRIDGE_WORKSPACE_PATH=/absolute/path/to/repo`

推荐启动命令：

```bash
cd /Users/Bean/Documents/trae_projects/feishu-vscode-bridge
set -a
source .env
env BRIDGE_WORKSPACE_PATH=/Users/Bean/Documents/trae_projects/feishu-vscode-bridge BRIDGE_APPROVAL_REQUIRED=none ./target/debug/bridge-cli listen
```

启动成功的最低标志：

- `✅ 飞书认证成功`
- `✅ WebSocket 已连接，等待飞书消息...`

## 回归原则

- 每轮至少验证 1 条文本消息链路和 1 条卡片回调链路
- 每轮至少验证 1 条依赖持久化上下文的追问链路
- 如果收到的是 `post` 消息，仍应能被正确解析成纯文本命令

## 核心回归清单

### 1. 失败追问链路

在飞书发送：

```text
执行全部 读取 src/lib.rs 1-20; $ false
```

预期：

- 机器人返回失败暂停卡片
- 卡片中能看到当前任务和失败状态

接着点击卡片中的：

```text
刚才为什么失败
```

或直接发送：

```text
为什么失败了
```

预期：

- 能收到基于上次失败结果的解释性回复
- 回复里包含失败步骤或关键错误，而不是泛化空话

### 2. 上一步结果回放

在失败链路之后直接发送：

```text
看上一步
```

预期：

- 能回放最近一步结果
- 回复内容与刚才失败的步骤一致，不应串到别的任务

### 3. diff 追问链路

先在本地确保仓库存在未提交改动，然后在飞书发送：

```text
查看 diff
```

预期：

- 直接返回当前 diff 文本或摘要

接着发送：

```text
把刚才的 diff 发我
```

预期：

- 能回放刚才保存的 diff 内容
- 内容应与上一条 diff 语义一致

### 4. 文件上下文追问链路

发送：

```text
读取 src/bridge.rs 1-40
```

接着发送：

```text
继续这个文件
```

预期：

- 回复应继续围绕 `src/bridge.rs`
- 不应跳到历史任务里更早的文件

### 5. 任务连续性链路

发送：

```text
执行计划 读取 src/bridge.rs 1-40; 搜索 follow_up 在 src; 查看 diff
```

然后发送：

```text
继续刚才的任务
```

预期：

- 回复里包含当前任务目标
- 回复里包含最近步骤或剩余步骤信息
- 如果没有活动计划，也应返回最近任务摘要，而不是说上下文丢失

### 6. 卡片回调链路

发送：

```text
执行计划 git status; $ pwd
```

点击卡片中的：

```text
继续
```

预期监听日志至少出现：

- `📨 收到飞书事件类型: card.action.trigger`
- `🖱️ 收到卡片点击`
- `✅ 卡片回复已发送`

## 建议补充项

如果本轮改动触及补丁或审批逻辑，再补下面两项。

### 7. 文件列表追问

在产生 diff 或补丁后发送：

```text
看文件列表
```

预期：

- 返回最近一次改动涉及的文件列表

### 8. 补丁撤回链路

在最近一次 `应用补丁` 成功后发送：

```text
撤回补丁
```

预期：

- 最近补丁能被反向应用
- 撤回后 `查看 diff` 结果符合预期

## 判定标准

满足下面 4 条即可认为这轮真实飞书回归通过：

- 监听器成功鉴权并建立 WebSocket
- 至少 1 条 `im.message.receive_v1` 文本消息链路通过
- 至少 1 条 `card.action.trigger` 卡片回调链路通过
- 至少 1 条依赖持久化会话的追问链路通过

## 常见失败信号

- `code=10014`：App Secret 无效
- 收到消息但日志提示未识别 payload：优先检查 `text` / `post` 消息解析分支
- 卡片点击无响应：优先检查飞书事件订阅和 `chat_id` 回复目标
- 追问串线：优先检查最近结果、最近文件、最近 diff 的持久化写入是否被覆盖

## 建议记录格式

每轮回归完成后，在 `docs/work_log.md` 追加：

- 回归日期
- 本轮验证命令
- 实际通过的链路
- 暴露出的真实飞书环境差异
- 是否需要补 parser / transport / session 持久化兼容