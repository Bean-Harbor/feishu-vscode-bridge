# Copilot Modes And Autonomous Agent Plan

## Goal

把当前 `feishu-vscode-bridge` 从“飞书里可调用一些 VS Code / Copilot 能力”推进成一套明确分层的远程工作模式：

- `/ask`：快速问答 / 解释 / 只读分析
- `/plan`：任务规划 / 方案比较 / 风险确认
- `/agent`：真正的 autonomous agent，多轮工具调用、可暂停、可恢复、可审批

同时，飞书侧不仅支持 slash 形式，也支持等价中文命令，让用户可以直接在飞书里自然使用：

- `/ask <任务>`
- `/plan <任务>`
- `/agent <任务>`
- `问 Copilot <任务>`
- `规划 <任务>` / `给我计划 <任务>`
- `让 agent 做 <任务>` / `自动完成 <任务>`

## Why This Plan Exists

当前仓库里已经有：

- 一个可用的 Feishu 入口
- 一个 Rust bridge runtime
- 一个 VS Code companion extension
- 一个最小 ask 路径
- 一个最小 semantic planner 路径
- 最小 session continuity

但当前缺口也很明确：

- `/ask` 现在本质还是单轮问答 + 最多一轮只读工具
- `/plan` 现在更像“自然语言转执行/确认/澄清”的路由层，而不是完整任务规划器
- `/agent` 还没有独立模式，更不是 autonomous agent

因此，后续不该继续把能力都塞进 `问 Copilot` 这一条路径里，而应该把三种模式拆开建模。

## Product Definition

### Ask Mode

适用场景：

- 解释代码
- 阅读当前文件 / 当前报错
- 快速给出根因判断
- 在少量补充上下文后回答

边界：

- 默认只读
- 不做长时间任务推进
- 不承担自动写代码主路径
- 可以保留极少数只读工具调用

用户感知：

- 快
- 轻量
- 问完即收敛

### Plan Mode

适用场景：

- 让我先看看应该怎么做
- 帮我拆步骤
- 给出候选方案
- 帮我判断风险和确认点

边界：

- 默认不直接长跑执行
- 输出计划、候选方案、确认卡、审批节点
- 可把低风险高置信动作落成 `execute`
- 中高风险或歧义动作落成 `confirm`
- 信息不足时落成 `clarify`

用户感知：

- 先想清楚再动
- 能看见风险与分支

### Agent Mode

适用场景：

- 帮我持续推进这个任务
- 自己分析、搜索、读文件、改代码、跑测试
- 遇到风险动作停下来让我批
- 中途我还能查看状态、继续、取消、批准
- 改完代码后，我可以对本轮结果做处置决策
- 对需要人工门禁的步骤，我可以对授权策略做决策，而不是只能被动逐步点批准

边界：

- 允许多轮工具调用
- 允许读写能力，但高风险写操作必须审批
- 目标是任务完成、失败、阻塞、等待用户，而不是单轮回答

最低标准：

- autonomous agent 不能只是“分析后给建议”
- 它必须真的能落地修改工作区、执行验证，并把结果呈现给用户
- 用户随后必须能在正确的干预点上做控制决策，而不只是收到一段完成文本

这里要抽象看待两类控制，而不是把按钮名字写死：

- 执行前/执行中控制：是否授权、如何授权、授权范围多大、是单步批准还是放宽当前 run 的门禁
- 执行后结果控制：是否保留本轮结果、是否撤销、是否接受部分结果并继续下一轮

`keep / undo / bypass approvals` 只是当前最接近 Copilot Chat 体验的几个例子，不是最终协议必须只长成这几个枚举。

用户感知：

- 一个真的在推进任务的 agent
- 不是问答工具

## Current State Mapping

## Backend Strategy Decision

当前决定不是改成 `Feishu -> Copilot CLI` 的单层架构，而是保持：

- `Feishu -> Bridge Runtime -> Backend`

其中 backend 可以逐步扩展为：

- `VS Code companion`：负责编辑器上下文、当前 ask / plan、workspace 感知
- `Copilot CLI`：负责未来的通用 agent 执行 backend

这意味着：

- `/ask /plan /agent` 的模式语义、session、审批、结果处置、飞书卡片，继续留在 bridge/runtime 层
- backend 只负责承接模型调用和执行能力，不反向决定产品协议
- 后续接 Copilot CLI 时，优先做 adapter，而不是推翻现有 runtime 设计

### Already Present

- Ask baseline:
  - `问 Copilot <问题>`
  - Rust -> extension -> language model
  - 最多一轮只读工具调用
  - 同 session continuation
- Plan baseline:
  - semantic planner 已支持 `execute / confirm / clarify`
  - 可以把歧义 Git 意图转成确认卡
- Session baseline:
  - direct / plan / agent 三类 session 持久化
  - 当前项目、最近结果、最近文件、nextAction 已能保存

### Missing

- 明确的 `/ask /plan /agent` 模式入口
- 独立的 agent runtime 状态机
- 多轮工具循环
- write-capable autonomous flow
- agent run 状态查询 / 审批 / 取消协议
- 用户干预控制面还没有抽象出来：当前系统还缺“授权策略”和“结果处置策略”这两个运行时概念
- 与 Feishu 卡片/命令一致的 agent 操作语义

## Target Architecture

## Implementation Strategy Choice

这三种模式可以一起做，但工程上最合理的顺序不是先补齐 `/ask /plan` 的表面入口，而是先单独起 `/agent runtime`。

原因：

- `/agent` 是能力上限，决定整套系统最终是不是“真正会持续推进任务的 agent”
- 如果先把 `/ask /plan` 做成稳定表面，再回头重构 agent runtime，很容易把轻量问答路径、planner 路径、session 协议各做一套，最后反而更难收敛
- 如果先把 `/agent runtime` 这根主干搭起来，那么 `/ask` 和 `/plan` 都可以退化成它上面的受限模式：
  - `/ask` = 小预算、只读、快速收敛的 agent run
  - `/plan` = 禁止写动作、偏向规划输出与确认卡的 agent run

因此，推荐采用：

1. 先以 `/agent runtime` 为主线，而不是先补 ask/plan surface
2. 但在真正写 runtime 骨架前，先定义 runtime domain model，也就是运行时对象、控制面和状态契约
3. 再按这个契约实现独立 `/agent runtime`
4. 再让 `/ask` 和 `/plan` 映射成 agent runtime 的两种受限运行配置
5. 最后补 slash surface 和中文 alias

### Priority Decision

当前“最优先”的事情，需要分成两层来看：

- 产品/架构主线优先级：`/agent runtime` 最高
- 立即开工顺序优先级：先定义对象和契约，再实现 runtime

也就是说：

- 如果问整个项目现在先围绕哪条主线推进，答案是 `runtime first`
- 如果问下一刀代码最先该落什么，答案是“先定义 runtime domain model”

原因很直接：

- 现在你已经明确指出，`keep / undo / bypass approvals` 只是例子，真正缺的是更高层的抽象
- 如果在这些抽象没定义好之前就直接起 runtime，很容易把例子硬编码成状态和按钮，后面每次提醒都要返工
- 反过来，如果先把对象、控制面、状态流定义好，runtime 一旦开写，后续 `/ask /plan /agent` 都能复用同一套骨架

所以，计划已经更新为：

- 不是“定义对象”与“做 runtime”二选一
- 而是“以 runtime 为最高目标，但先做 runtime 的对象定义”

### Shared Entry Layer

Feishu 入口继续复用当前 Rust bridge：

- 接收文本消息
- 接收卡片按钮回调
- 做 sender/chat 级 session 隔离
- 写审计日志

但在 `BridgeApp::dispatch(...)` 这一层明确分出三种模式：

- `AskMode`
- `PlanMode`
- `AgentMode`

### Ask Runtime

保留当前 extension-hosted ask 模型，但限制职责：

- 快速问答
- 可做只读 grounded analysis
- 工具预算低
- 不负责长任务推进

### Plan Runtime

扩展当前 semantic planner：

- 接收用户目标
- 返回结构化计划结果
- 输出步骤、风险、置信度、候选方案、确认点
- 可以把计划交给现有 `plan_dispatch.rs`

### Agent Runtime

新增真正的 agent loop：

1. 读取目标与上下文
2. 规划下一步
3. 选择一个工具动作或直接完成
4. 执行工具
5. 观察结果
6. 再规划
7. 直到 `completed / failed / waiting_user / needs_approval / cancelled`

## Protocol Design

## Runtime Domain Model

这里是整个后续计划的真正起点。没有这一层，runtime 很容易退化成“把若干例子写成枚举和按钮”。

### Core Runtime Objects

建议先定义这些对象，再开始实现 endpoint 和状态机：

- `AgentRun`
  - 一次独立任务推进实例
- `ControlPoint`
  - runtime 在什么时刻必须停下来，等待用户控制
- `AuthorizationPolicy`
  - 当前 run 对执行授权和审批门禁的策略
- `ResultDisposition`
  - 当前 run 产物处于什么处置状态
- `PendingUserDecision`
  - 当前究竟在等用户决定哪一类事情
- `ReversibleArtifact`
  - 哪些产物可以撤销、回滚、重放
- `RunBudget`
  - 当前 run 的工具/轮次/写入预算
- `RunCheckpoint`
  - 当前可恢复的阶段点

### Control Categories

用户干预点先按类别定义，不按按钮文案定义：

- 授权控制
- 结果处置控制
- 目标修正控制
- 节奏控制

后续第一版 Feishu 卡片/命令只是这些控制类别在聊天界面里的投影。

### Why This Comes Before Runtime Code

- 它决定 extension 和 Rust 之间的协议长什么样
- 它决定 session store 里到底要存哪些字段
- 它决定 `/agent` 完成后为什么会进入 `waiting_user`
- 它决定后续 keep/undo/审批/继续是不是同一套控制面的一部分

如果这一层没先定义，runtime 代码大概率会被当前几个例子绑死。

### New User-Facing Commands

建议新增下面这组显式模式命令：

- `/ask <prompt>`
- `/plan <prompt>`
- `/agent <prompt>`
- `规划 <prompt>`
- `让 agent 做 <prompt>`
- `查看 agent 状态`
- `继续 agent`
- `批准 agent 当前步骤`
- `取消 agent`

兼容旧命令：

- `问 Copilot <prompt>` == `/ask <prompt>`
- `继续，...` 在 ask / agent 下都可以复用，但语义要基于 session kind 分流

说明：

- 上面这一组命令只是第一版 surface 示例，不应反向约束底层 runtime 模型
- 真正需要先定义的是“用户可以在什么干预点，对哪一类控制面做决策”
- 现有 `撤回刚才的补丁` 能作为 agent 结果处置里的一个底层回退能力，但还没有提升成 agent run 级语义

### New Extension Endpoints

建议新增或重组为：

- `POST /v1/chat/ask`
- `POST /v1/chat/plan`
- `POST /v1/chat/agent/start`
- `POST /v1/chat/agent/continue`
- `POST /v1/chat/agent/status`
- `POST /v1/chat/agent/approve`
- `POST /v1/chat/agent/keep`
- `POST /v1/chat/agent/undo`
- `POST /v1/chat/agent/bypass-approvals`
- `POST /v1/chat/agent/cancel`
- `POST /v1/chat/tool-result`

说明：

- `ask` 与 `plan` 保持轻量
- `agent/*` 使用独立 run state，不再复用 ask 的单 pendingToolRequest 结构

### New Agent Status Model

建议统一成：

- `running`
- `needs_tool`
- `needs_approval`
- `waiting_user`
- `completed`
- `failed`
- `cancelled`

补充说明：

- `waiting_user` 不只是信息不足，还包括“等待用户做控制决策”
- 这里的控制决策可能属于不同类别：
  - 授权
  - 结果处置
  - 目标修正
  - 范围收缩/放宽

### New Agent Session Schema

建议在当前 `StoredAgentState` 基础上扩展：

- `run_id`
- `mode`
- `goal`
- `status`
- `iteration`
- `budget`
- `current_action`
- `next_action`
- `tool_history`
- `artifacts`
- `pending_approval`
- `approval_mode`
- `latest_patch`
- `latest_write_set`
- `awaiting_user_decision`
- `can_keep`
- `can_undo`
- `waiting_reason`
- `completion_summary`

这里还需要再抽象一层，避免 schema 只围着当前例子打转：

- `control_points`: 当前 run 里有哪些用户干预点
- `authorization_policy`: 当前 run 的授权策略
- `result_disposition`: 当前结果处于什么处置状态
- `reversible_artifacts`: 当前哪些结果是可逆的
- `pending_user_decision`: 当前正在等用户决定的是哪一类控制

也就是说，`approval_mode`、`can_keep`、`can_undo` 可以是第一版字段，但更上层要归入这几个抽象概念。

## Tooling Model

### Ask Tool Policy

- 只读工具
- 小预算
- 重点是回答质量

### Plan Tool Policy

- 默认不执行写工具
- 重点是形成计划和确认

### Agent Tool Policy

分层：

- Level 1: read-only
  - `read_file`
  - `search_text`
  - `search_symbol`
  - `find_references`
  - `find_implementations`
  - `git_status`
  - `git_diff`
- Level 2: low-risk execution
  - `run_tests`
  - `run_specific_test`
  - `run_test_file`
- Level 3: write-capable, approval required
  - `apply_patch`
  - `write_file`
  - `git_pull` when configured as gated
  - `git_push_all`
  - `run_shell`

### Agent Completion / Confirmation Policy

这部分是 autonomous agent 是否成立的关键，不是附属 UX：

- agent 完成一轮写操作后，必须把本轮变更作为一个可回看的结果呈现出来
- 用户必须能在至少两类控制面上做明确决策：
  - 授权控制：当前 run 如何通过审批/门禁
  - 结果处置：当前 run 产生的变更如何保留、撤销、确认、继续

这意味着 agent runtime 不能只记录“最后一段文本回复”，还必须记录：

- 本轮产生的 patch / write set
- 是否已经应用到工作区
- 是否可逆
- 用户当前还拥有哪些控制权
- 当前等待的控制决策属于哪一类

没有这一层，就还不是你要的那种 Copilot Chat 风格 autonomous agent。

## Current Runtime Baseline

当前仓库不是从零开始进入 runtime 主线，而是已经有一层可运行但还不完整的 runtime 骨架：

- Rust 侧已有 runtime domain structs：`AgentRunMode`、`AgentRunStatus`、`PendingUserDecision`、`RunBudget`、`RunCheckpoint`、`AgentRunState`
- extension 侧已有独立 endpoint：
  - `POST /v1/chat/agent/start`
  - `POST /v1/chat/agent/continue`
  - `POST /v1/chat/agent/status`
  - `POST /v1/chat/agent/approve`
  - `POST /v1/chat/agent/cancel`
- bridge/parser/session/reply 已有第一版 runtime surface：
  - `/agent <任务>`
  - `继续 agent`
  - `agent 状态`
  - `批准 agent`
  - `取消 agent`
- extension runtime 已能做多轮只读推进和 checkpoint 增长
- Feishu 侧已能展示 waiting-state、待决策选项和下一步

但当前还不能把它视为“runtime 主线已经完成”，因为还缺下面这些关键条件：

- ask / plan / agent 仍是并行能力，不是统一 runtime 上的三种受限模式
- 当前 runtime 还偏向 endpoint-level continuation，不是完整的 run state machine
- 控制面还没有抽象完整：授权策略、结果处置、节奏控制、目标修正仍未统一进同一套协议
- 多轮 loop 仍以只读路径为主，没有受控写入和写后处置闭环
- waiting-state 虽然已有文案和按钮，但其上层 runtime 语义仍不够稳定

## Phased Implementation

### Execution Status

- Phase A. Runtime Domain Model — started, first contract landed
- Phase B. Agent Runtime Skeleton — started, first runnable slice landed
- Phase C. Multi-Hop Agent Loop — not started
- Phase D. Ask / Plan As Restricted Agent Modes — not started
- Phase E. Mode Surface — not started
- Phase F. Controlled Write Agent — not started

### Phase A. Runtime Domain Model

目标：先定义 autonomous agent 的运行时对象、控制面和状态契约。

需要做：

- 定义 `AgentRun`
- 定义 `ControlPoint`
- 定义 `AuthorizationPolicy`
- 定义 `ResultDisposition`
- 定义 `PendingUserDecision`
- 定义 `ReversibleArtifact`
- 定义 `RunBudget` / `RunCheckpoint`
- 明确 extension <-> Rust 的共享 JSON contract

验收：

- 能清楚表达“当前 run 在哪里、等什么、可以控制什么”
- `keep / undo / approvals` 这些例子都能自然落到抽象对象里，而不是靠硬编码特判
- 后续 runtime endpoint 可以围绕这些对象直接展开

当前执行清单：

- [x] Rust 侧新增 runtime domain structs
- [x] extension 侧新增对应 runtime types
- [x] session schema 为 agent runtime 预留字段
- [x] 定义 Rust <-> extension 的 agent runtime JSON contract
- [ ] 把 waiting-state 和 approval-state 收敛到统一 control-point schema
- [ ] 把 result disposition 从文案能力提升成 run-level state
- [ ] 把 authorization policy 从布尔字段整理成 runtime policy object

### Phase B. Agent Runtime Skeleton

目标：先把 `/agent` 从当前 ask bootstrap 里独立出来，建立真正的 agent run 主干。

需要做：

- 新增 agent 专用 endpoint：
  - `POST /v1/chat/agent/start`
  - `POST /v1/chat/agent/continue`
  - `POST /v1/chat/agent/status`
  - `POST /v1/chat/agent/approve`
  - `POST /v1/chat/agent/cancel`
- extension 侧新增独立 agent run store
- Rust 侧新增独立 agent run/session schema
- 定义统一 agent status：
  - `running`
  - `needs_tool`
  - `needs_approval`
  - `waiting_user`
  - `completed`
  - `failed`
  - `cancelled`
- 先只支持多轮只读工具，不急着接写路径

说明：

- 这一步建立在 Phase A 的对象定义之上，不再允许直接围着示例按钮造状态
- 这一步可以先从只读 loop 起骨架，但文档目标不再把“只读 agent”视为终点
- 真正的完成标准仍然是后续接入 write-capable flow 与 keep/undo/bypass approvals

验收：

- 飞书里可以发起一个独立 agent 任务
- agent 任务可以查看状态、继续、取消
- agent 不再复用 ask 的单 pendingToolRequest 模型

当前执行清单：

- [x] extension 注册 `/v1/chat/agent/start`
- [x] extension 注册 `/v1/chat/agent/continue`
- [x] extension 注册 `/v1/chat/agent/status`
- [x] extension 注册 `/v1/chat/agent/approve`
- [x] extension 注册 `/v1/chat/agent/cancel`
- [x] Rust 侧新增 agent runtime client 占位
- [x] Rust 侧新增 session 持久化骨架
- [x] Rust bridge 已接入 runtime direct-command surface
- [ ] 把 extension pending state 收敛成独立 run store，而不是 ask 风格会话结构的扩展
- [ ] 让 approve / cancel / continue 基于统一 run reducer 更新状态，而不是各 endpoint 各自推进
- [ ] 把 runtime checkpoint、tool history、waiting reason 统一成稳定持久化字段

补充进展：

- Rust bridge 已新增显式 runtime 命令入口：`/agent`、`agent 状态`、`继续 agent`、`批准 agent`、`取消 agent`
- `follow_up` 已能在检测到 runtime session 时优先走 `ContinueAgentRun`
- session / reply 层已能展示 runtime run id、状态、待决策选项和下一步

### Phase C. Multi-Hop Agent Loop

目标：让 `/agent` 具备真正 autonomous loop。

需要做：

- planner 输出从单轮 `answered / needs_tool` 升级成 agent 状态机
- 支持多轮 `plan -> tool -> observe -> replan`
- 增加 iteration budget、停止条件、错误恢复
- 持续把阶段状态同步回飞书

验收：

- 至少跑通 3 轮以上只读 autonomous task
- 中途状态、当前动作、下一步建议都可见

当前执行清单：

- [ ] planner 输出升级为 agent loop decision
- [ ] 把单次 continuation 串成稳定的 `plan -> tool -> observe -> replan` reducer
- [ ] 增加 iteration budget、stop reason、recoverable failure 分类
- [ ] 把阶段状态持续同步回飞书
- [ ] 让工具历史参与 replanning，而不是只作为附加上下文

### Phase D. Ask / Plan As Restricted Agent Modes

目标：把 `/ask` 和 `/plan` 改造成 `/agent runtime` 上面的两种受限配置，而不是并行实现三套运行时。

需要做：

- `/ask` 映射成：
  - 只读
  - 小预算
  - 快速完成
- `/plan` 映射成：
  - 偏规划输出
  - 默认不执行写动作
  - 更容易输出 `confirm / clarify`
- 把当前 `问 Copilot` 和 `semantic_planner` 逐步迁到统一 runtime contract

验收：

- ask / plan / agent 三者共享一套底层 run state
- 只是 budget、tool policy、stop policy 不同

当前执行清单：

- [ ] ask policy 映射到 agent runtime
- [ ] plan policy 映射到 agent runtime
- [ ] 旧 `问 Copilot` 和 semantic planner 渐进迁移

### Phase E. Mode Surface

目标：在 runtime 稳定后，再补正式 surface，让用户能直接使用 `/ask /plan /agent` 与中文命令。

需要做：

- parser 增加 slash mode 和中文 alias
- `BridgeApp` 明确分流三种模式
- reply 文案里明确当前 mode

验收：

- 三种模式都能从飞书文本触发
- slash 和中文命令映射一致

当前执行清单：

- [ ] 新增 `/ask /plan /agent` parser
- [ ] 新增中文 alias
- [ ] reply 中明确 mode 和 run 状态

### Phase F. Controlled Write Agent

目标：接入真实改代码能力。

需要做：

- 把 `apply_patch` / `write_file` / 测试验证接到 agent loop
- 写操作统一走审批边界
- 结果写入 session + audit
- 写完后明确进入用户控制决策点，而不是直接把 run 标成完成
- 把现有 `撤回刚才的补丁` 提升成 agent run 级结果处置能力之一，而不是普通 follow-up 小命令
- 抽象出授权控制与结果处置控制，再决定第一版 Feishu surface 具体长成哪些按钮/命令

验收：

- 至少能跑通一次真实 `分析 -> 改代码 -> 跑测试 -> 汇报结果 -> 用户控制决策` 链路
- 用户可以在审批门禁场景下改变授权策略
- 用户可以在写入完成后对结果做保留/撤销/确认类处置

当前执行清单：

- [ ] agent loop 接入 `apply_patch`
- [ ] agent loop 接入 `write_file`
- [ ] 写后进入结果处置控制点
- [ ] 把补丁撤回提升成 agent run 级能力

## Immediate Next Slice

现在已经进入 runtime 主线，下一段不该再做“是否起 runtime”的讨论，而是把现有骨架推进成真正可扩展的 runtime 主干。

建议按下面顺序推进：

1. 统一 runtime 状态归约层

- 给 extension 侧 agent run 引入单一 reducer / transition 入口
- 把 `start / continue / approve / cancel / tool-result` 都改成对同一 run state 的状态变换
- 明确每次状态变换输出：`status`、`waitingReason`、`pendingUserDecision`、`checkpoints`、`toolHistory`

2. 收敛 control-plane schema

- 把现有 waiting-state 文案、批准选项、暂停语义收敛到 `ControlPoint` / `PendingUserDecision`
- 区分四类控制：授权、结果处置、目标修正、节奏控制
- 避免继续把“批准本次写入”“先停在这里”这类 surface 文案写死成底层状态

3. 跑通真正的 multi-hop read-only runtime

- 让 `/agent` 在同一个 run 内稳定执行多轮 `plan -> tool -> observe -> replan`
- 让 run budget、stop reason、recoverable failure 成为一等字段
- 让 Feishu 回复展示 run 在第几轮、当前因为什么暂停、下一步在等什么

4. 再折叠 ask / plan

- `/ask` 变成小预算、只读、快速收敛的 runtime policy
- `/plan` 变成偏规划、偏确认、默认不写的 runtime policy
- 这一步之前，不再继续增强 ask-only 和 planner-only 的平行协议

这样推进的直接好处是：

- 保留已经落地的 endpoint 和 parser 成果，不回退重来
- 把真正的工程重心放在 runtime state machine，而不是表层命令扩展
- 为后续 controlled write、keep / undo / bypass approvals 留出正确的抽象位置

## Acceptance Standard

这个计划完成后，应该能明确回答下面三个问题：

1. 飞书里一句话发 `/ask`、`/plan`、`/agent` 分别会发生什么
2. 当前系统在哪一层做模式分流、在哪一层做工具编排、在哪一层做审批
3. autonomous agent 与当前 ask bootstrap 的差别到底在哪里

如果仍然回答不清，就说明文档或架构拆分还不够清楚。