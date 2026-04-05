# Agent MVP Execution Plan

## Objective

把当前“能问、能连续追问”的 agent bootstrap，推进成一个最小可发布的 `agent MVP`：

- 用户在飞书里发的不是一次性问题，而是一个任务
- 系统能维护任务状态
- 系统能决定是否调用最小只读工具
- 系统能返回阶段性进展，而不是只回一段答案

## Current Baseline

当前已经具备：

- Feishu 入口
- Rust bridge runtime
- VS Code companion extension
- 基础 session 复用
- workspace context 注入
- `问 Copilot` / `重置 Copilot 会话`
- 现成的 Rust 只读/低风险工具能力
- 最小 `AgentTaskState` / 结构化 agent 响应
- 单轮只读 tool loop，当前支持 `read_file` 和 `search_text`
- Feishu 侧最小 agent 进度文本展示

当前还缺：

- 基于任务而不是基于问答的验证标准
- 一条更稳定、更可重复的 real Feishu 验证脚本化路径
- 发布视角下的安装/诊断收尾
- 面向真实用户的 installer / setup 收口

## MVP Definition

这版 agent MVP 不追求“完整自主编程 agent”，只追求下面这个最小闭环：

1. 用户发起一个开发任务
2. extension 建立任务会话并理解当前上下文
3. 模型决定是直接回答，还是先调用一个只读工具
4. Rust 执行工具并回传结果
5. 模型输出阶段结论和下一步建议
6. 用户继续推进、收敛或重置任务

## Non-Goals

- 不做多工具并发编排
- 不做自动写文件主路径
- 不做复杂计划树可视化
- 不做多 agent 协同
- 不做桌面端入口

## Packaging Constraint

- 虽然这版 MVP 仍以 VS Code extension 作为第一入口，但对用户交付必须按产品安装来设计
- Windows 和 macOS 需要共享一套安装职责定义，只在平台落地方式上分开
- 任何 agent 主链路改动，都要考虑它如何进入 Windows `Setup.exe` 和 macOS `.dmg` 的首装流程

## Workstreams

### W0. Packaging And Setup

目标：把“开发者可运行”收敛成“普通用户可安装”。

需要补的能力：

- Windows `Setup.exe` 产物路径
- macOS `.dmg` 产物路径
- runtime、setup UI、配置写入、日志目录、extension 安装的统一编排
- 首装健康检查
- 失败恢复入口

建议落点：

- `README.md`
- `docs/mvp_release_plan.md`
- `src/bin/setup_gui.rs`
- 未来新增 installer packaging 脚本

验收：

- Windows 和 macOS 都能由非开发者按说明完成安装
- 用户无需理解内部有哪些二进制或脚本

### W1. Agent State Model

目标：让系统知道当前处理的是一个 `任务`，而不只是一个 `prompt`。

需要补的能力：

- `task_id` / `session_id` 对应关系
- 当前任务目标
- 当前任务阶段
- 最近一次 agent decision
- 最近一次 tool result
- 当前是否等待用户确认

建议落点：

- `vscode-agent-bridge/src/extension.ts`
- `src/vscode.rs`
- `src/bridge.rs`

建议新增结构：

- extension 侧 `AgentTaskState`
- Rust 侧 `AgentTaskResponse`

验收：

- 同一飞书会话连续 3 轮任务推进不丢失任务目标
- agent 可以告诉用户“当前正在做什么”

### W2. Agent Response Protocol

目标：把现在的纯文本回答改成“可表达 agent 状态”的结构化响应。

当前问题：

- `ask_agent()` 返回的仍然主要是 `reply + summary`
- Rust 和 Feishu 看到的仍是问答式结果

目标响应结构建议：

- `sessionId`
- `taskState`
- `status`
- `message`
- `nextAction`
- `toolCall`
- `toolResultSummary`

状态建议枚举：

- `answered`
- `working`
- `needs_tool`
- `waiting_user`
- `blocked`
- `completed`

建议落点：

- `vscode-agent-bridge/src/extension.ts`
- `src/vscode.rs`

验收：

- Rust 侧能区分“这是一条最终回答”还是“agent 还在推进”
- Feishu 侧能展示阶段状态

### W3. First Tool Loop

目标：接入第一个真正的 agent tool loop。

首批只接一个最小集合：

- `read_file`
- `search_text`

为什么先选这两个：

- 覆盖大部分代码分析场景
- 风险低
- Rust 侧已具备成熟实现

最小执行逻辑：

1. 用户发任务
2. extension 组织上下文
3. 模型判断信息不足
4. extension 生成 tool request
5. Rust 执行 `read_file` 或 `search_text`
6. tool result 回注到 session
7. 模型输出阶段结论

建议落点：

- `vscode-agent-bridge/src/extension.ts`
- `src/vscode.rs`
- `src/bridge.rs`

需要新增的最小协议：

- extension -> Rust: `toolName + args`
- Rust -> extension: `success + output + summary`

验收：

- 至少跑通一条真实链路：
  - “分析 parse_intent 的作用，如果不够就去读代码再回答”

### W4. Feishu Agent UX

目标：让飞书里看到的是 agent 进展，而不是普通聊天回复。

需要补的能力：

- 当前任务状态展示
- 当前阶段展示
- 最近一次工具动作展示
- 下一步建议展示
- 用户可继续推进任务

建议第一版只做文本，不急着做复杂卡片。

文本结构建议：

- `任务`
- `状态`
- `当前动作`
- `结果摘要`
- `下一步`

建议落点：

- `src/bridge.rs`
- 如有必要，再扩到 Feishu card rendering

验收：

- 用户能从一条飞书回复里看懂 agent 当前在哪个阶段

### W4.5 Agent Continuation

目标：让 agent 从“问一次”升级成“继续做同一个任务”。

当前问题：

- `问 Copilot ...` 已能返回结构化状态，但后续仍主要依赖用户再次发送新的 `问 Copilot ...`
- Rust 侧虽然会把 agent 结果持久化到 session，但 `继续` / `继续刚才的任务` 仍主要服务于计划执行语义
- 这会让用户感知到的是“带一点状态的问答”，而不是“可继续推进的任务 agent`

需要补的能力：

- 区分“继续计划”与“继续 agent 任务”
- 当最近一次任务是 agent 且没有待执行 plan 时，把 `继续刚才的任务` 路由到 agent continuation
- 让 continuation 能复用当前 agent session，并把新的推进意图发回 extension
- 在 Feishu 回复里明确当前是在“继续上一个 agent 任务”而不是重新发起新问答

建议落点：

- `src/bridge.rs`
- `src/direct_command.rs`
- `src/session.rs`
- `src/lib.rs`
- `src/vscode.rs`

验收：

- 用户在一次 `问 Copilot ...` 之后，可以直接发送 `继续刚才的任务` 或 `继续，给我最小修复建议`
- 同一飞书会话内连续两到三轮推进仍复用同一个 agent session
- 当存在 plan 会话时，不破坏现有 `继续` 语义

当前状态：最小版本已完成。

- 已支持 `继续刚才的任务`
- 已支持自然追问 `继续，...`
- 已支持 `按建议继续`
- 已完成一次真实 Feishu continuation smoke

### W5. Validation Harness

目标：把验证标准从“ask 成功”升级成“agent loop 成功”。

必须补的验证：

- 本地 extension 健康检查
- Rust <-> extension tool protocol smoke
- 一条 real Feishu agent task smoke

建议测试路径：

1. `问 Copilot 分析 parse_intent，如果不够就读取代码后回答`
2. agent 触发 `read_file`
3. 返回阶段性结论
4. 用户继续：`继续，给我最小修复建议`

验收：

- 不是只返回直接答案，而是能证明中间发生过 tool loop

## Build Order

### Slice 0

先把安装约束明确，不让后续 agent 设计脱离真实分发形态。

- 明确 Windows `Setup.exe` 目标
- 明确 macOS `.dmg` 目标
- 明确 installer 对 extension 和 runtime 的职责边界

### Slice 1

先做协议，不做复杂行为。

- 定义 `AgentTaskState`
- 定义 `AgentTaskResponse`
- 让 Feishu 能展示 `状态 + 当前动作 + 结果摘要`

### Slice 2

接入第一个只读工具回路。

- 先接 `read_file`
- 再接 `search_text`
- 跑通一次真实 Feishu agent smoke

最小执行逻辑：

1. 用户发任务
2. extension 组织上下文
3. 模型判断信息不足
4. extension 生成 tool request
5. Rust 执行 `read_file` 或 `search_text`
6. tool result 回注到 session
7. 模型输出阶段结论

建议落点：

- `vscode-agent-bridge/src/extension.ts`
- `src/vscode.rs`
- `src/bridge.rs`

需要新增的最小协议：

- extension -> Rust: `toolName + args`
- Rust -> extension: `success + output + summary`

验收：

- 至少跑通一条真实链路：
  - “分析 parse_intent 的作用，如果不够就去读代码再回答”

### W4. Feishu Agent UX

目标：让飞书里看到的是 agent 进展，而不是普通聊天回复。

需要补的能力：

- 当前任务状态展示
- 当前阶段展示
- 最近一次工具动作展示
- 下一步建议展示
- 用户可继续推进任务

建议第一版只做文本，不急着做复杂卡片。

文本结构建议：

- `任务`
- `状态`
- `当前动作`
- `结果摘要`
- `下一步`

建议落点：

- `src/bridge.rs`
- 如有必要，再扩到 Feishu card rendering

验收：

- 用户能从一条飞书回复里看懂 agent 当前在哪个阶段

### W5. Validation Harness

目标：把验证标准从“ask 成功”升级成“agent loop 成功”。

必须补的验证：

- 本地 extension 健康检查
- Rust <-> extension tool protocol smoke
- 一条 real Feishu agent task smoke

建议测试路径：

1. `问 Copilot 分析 parse_intent，如果不够就读取代码后回答`
2. agent 触发 `read_file`
3. 返回阶段性结论
4. 用户继续：`继续，给我最小修复建议`

验收：

- 不是只返回直接答案，而是能证明中间发生过 tool loop

## Build Order

### Slice 1

已完成。

- 定义 `AgentTaskState`
- 定义 `AgentTaskResponse`
- Rust 能解析结构化 agent 响应

### Slice 2

已完成。

- extension 能产出 tool request
- Rust 能执行 `read_file`
- tool result 回注模型

### Slice 3

已完成最小版本。

- `search_text`
- 改善 agent 对“需要更多上下文”的处理

### Slice 4

已完成最小文本版。

- 任务状态
- 工具摘要
- 下一步建议

### Slice 5

优先拆成两段，先做“大功能”，再做回归与安装验证。

- 先补 agent continuation 主链路
- 再做真实回归和安装验证

### Slice 5A

已完成。

- 为 agent 增加“继续任务”语义
- 打通 `继续刚才的任务` 与最近 agent session 的复用
- 保证与 plan continuation 不冲突

### Slice 5B

做真实回归和安装验证。

- real Feishu smoke
- README 安装路径
- 最小诊断手册

### Slice 6

把 MVP 从“开发者可跑”推进到“用户可装”。

- 固化 Windows `Setup.exe` 路径
- 固化 macOS `.dmg` 路径
- 收口 extension 安装、health check、失败恢复说明
- 让 setup wizard / packaging 脚本和 README 对齐

## File-Level Change Plan

### Rust

- `src/vscode.rs`
  - 把 `ask_agent()` 升级成结构化 `agent bootstrap` 请求/响应处理
  - 新增 tool request 转发接口

- `src/bridge.rs`
  - 把 direct execution 的 `AskAgent` 从纯文本结果升级为任务状态结果
  - 持久化最近一次 agent state / tool result

- `src/lib.rs`
  - 暂时保留 `AskAgent` 名称即可，后续若需要再重命名为更中性的 task intent

### Extension

- `vscode-agent-bridge/src/extension.ts`
  - 新增 `AgentTaskState`
  - 新增 `AgentResponse`
  - 实现最小 tool-decision path
  - 实现 `read_file` 的 tool request
  - 维护 task-oriented session state

## First Coding Slice

下一步第一批工作不要继续扩命令面，而是只做下面三件：

1. 把 real Feishu 验证标准固定成可重复 checklist，而不是临场 smoke
2. 把 README、setup wizard、packaging 脚本的启动/诊断路径收成一个一致的安装故事
3. 优先补发布视角的缺口，而不是继续给 agent continuation 加更多细粒度命令

这三步完成后，产品才会从“本地已验证的 agent 原型”进入“可准备私测的 agent MVP”。

## Success Criteria For This Breakdown

如果下面四件事同时成立，就说明 agent MVP 拆解是正确的：

1. 你能明确说出当前在做的切片是什么
2. 每个切片都有落点文件
3. 每个切片都有可验证结果
4. 第一批切片做完后，产品行为会明显更像 agent 而不是问答