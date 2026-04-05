# Feishu Live Regression Checklist

这份清单用于每次重要改动后，快速回归真实飞书链路，确认 bridge 在真实环境里仍然是“可继续推进的 agent 任务”，而不是只在本地 CLI 里可用。

目标不是覆盖所有功能，而是优先覆盖最容易在真实飞书环境里退化的几条链路：extension bootstrap、agent tool loop、同 session continuation、失败恢复。

## 准备

先确认 companion extension 已按仓库主路径启动；不要在 `/health` 未恢复前反复排查监听器或飞书鉴权。

推荐开发态启动顺序：

1. 在仓库里按 `F5`，使用 `Run Feishu Agent Bridge Extension`
2. 确认输出通道 `Feishu Agent Bridge` 已监听 `http://127.0.0.1:8765`
3. 再启动飞书监听器

- 本地已存在有效 `.env`
- 已编译监听器：`cargo build`
- 建议关闭审批：`BRIDGE_APPROVAL_REQUIRED=none`
- 建议固定工作区：`BRIDGE_WORKSPACE_PATH=/absolute/path/to/repo`
- 建议优先使用仓库内启动脚本，它默认走 `target/bridge-live-runner`，可避开 Windows 上长期运行的 `target/debug/bridge-cli.exe` 锁文件问题

推荐启动命令：

```bash
cd /Users/Bean/Documents/trae_projects/feishu-vscode-bridge
./scripts/start-live-listener.sh
```

Windows PowerShell：

```powershell
.\scripts\start-live-listener.ps1
```

启动成功的最低标志：

- `http://127.0.0.1:8765/health` 返回 `{"status":"ok",...}`
- `✅ 飞书认证成功`
- `✅ WebSocket 已连接，等待飞书消息...`

## 回归原则

- 每轮至少验证 1 条 agent 文本链路和 1 条卡片回调链路
- 每轮至少验证 1 条依赖持久化上下文的 agent continuation 链路
- 如果收到的是 `post` 消息，仍应能被正确解析成纯文本命令
- 如果本轮触及 extension / protocol / session 逻辑，优先验证 agent MVP 主链，而不是只验证计划卡片链路

## 核心回归清单

### 1. Extension 健康检查

在本机确认：

```text
GET http://127.0.0.1:8765/health
```

预期：

- 返回 `200`
- JSON 中包含 `status: ok`
- 如果这里失败，本轮不继续做飞书 smoke，先回到 `F5` extension bootstrap 路径

### 2. Agent tool loop 主链

在飞书发送：

```text
问 Copilot 分析 parse_intent 这个函数是干什么的，如果不够就读取代码后回答
```

预期：

- 收到 `🧭 Agent 任务更新` 风格回复
- 回复中包含 `session` id
- 回复中包含 `当前动作`、`最近状态`、`结果摘要`
- 如果问题确实需要更多上下文，本轮应至少出现一次 read-only tool loop 迹象，例如 `read_file(...)` 或工具结果摘要，而不是始终只给无依据的直接答案

### 3. Agent continuation 链路

在上一条 agent 回复之后发送：

```text
继续，给我最小修复建议
```

预期：

- 回复继续沿用同一个 agent 任务，而不是重新开始一次 ask
- 回复中的上次动作应体现为 `继续 Agent 任务`
- 结果应基于上一轮结论推进，而不是重复首轮总结

### 4. 建议动作复用链路

如果上一轮 agent 回复里带有 `➡️ 下一步建议`，直接发送：

```text
按建议继续
```

预期：

- bridge 复用上一轮持久化的 `nextAction`
- 返回的 agent 回复仍处于同一任务连续上下文中
- 不应提示“没有可继续的 agent 任务”或丢失建议动作

### 5. 失败追问链路

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

### 6. 上一步结果回放

在失败链路之后直接发送：

```text
看上一步
```

预期：

- 能回放最近一步结果
- 回复内容与刚才失败的步骤一致，不应串到别的任务

### 7. diff 追问链路

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

### 8. 文件上下文追问链路

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

### 9. 任务连续性链路

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

### 10. 卡片回调链路

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

### 11. Reset 链路

在至少完成一轮 agent 对话后发送：

```text
重置 Copilot 会话
```

然后重新发送：

```text
问 Copilot parse_intent 这个函数是干什么的
```

预期：

- reset 命令返回成功提示
- 新一轮 ask 不应错误复用刚才 continuation 的内部上下文

### 12. 文件列表追问

在产生 diff 或补丁后发送：

```text
看文件列表
```

预期：

- 返回最近一次改动涉及的文件列表

### 13. 补丁撤回链路

在最近一次 `应用补丁` 成功后发送：

```text
撤回补丁
```

预期：

- 最近补丁能被反向应用
- 撤回后 `查看 diff` 结果符合预期

## 判定标准

满足下面 6 条即可认为这轮真实飞书回归通过：

- extension `/health` 正常
- 监听器成功鉴权并建立 WebSocket
- 至少 1 条 `问 Copilot ...` agent 文本链路通过
- 至少 1 次 read-only tool loop 或明确工具摘要通过
- 至少 1 条 agent continuation 链路通过
- 至少 1 条 `card.action.trigger` 卡片回调链路通过
- 至少 1 条依赖持久化会话的失败追问或结果回放链路通过

## 常见失败信号

- `code=10014`：App Secret 无效
- `/health` 不通：优先归类为 extension bootstrap / activation 问题，不要先怀疑飞书鉴权
- 收到消息但日志提示未识别 payload：优先检查 `text` / `post` 消息解析分支
- 卡片点击无响应：优先检查飞书事件订阅和 `chat_id` 回复目标
- `继续，...` 新起一轮 ask：优先检查 session kind、agent session persistence 和 continuation routing
- `按建议继续` 失效：优先检查 `nextAction` 是否已落盘，以及上一轮是否误被 plan/direct 会话覆盖
- 追问串线：优先检查最近结果、最近文件、最近 diff 的持久化写入是否被覆盖

## 建议记录格式

每轮回归完成后，在 `docs/work_log.md` 追加：

- 回归日期
- extension 健康检查结果
- 本轮验证命令
- 实际通过的 agent / continuation / card / follow-up 链路
- 是否观察到真实 tool loop（`read_file` / `search_text`）
- 暴露出的真实飞书环境差异
- 是否需要补 parser / transport / session 持久化兼容