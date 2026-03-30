# Copilot Bridge Porting Plan

## Goal

把 `feishu-vscode-bridge` 从“飞书 -> 本地开发命令桥”推进成“飞书 -> VS Code remote agent bridge”：用户在飞书窗口里发起任务后，桥接器不只是回一条建议或执行单条命令，而是像一个持续工作的 agent，会复用 VS Code 工作区上下文、维护会话状态、调用模型、编排工具动作，并把执行进度与结果持续回传到飞书。

## Product Target

- 用户在飞书里获得接近 VS Code Copilot Chat agent 的连续工作体验，而不是只得到一次性问答
- 优先打通“远程发问 -> VS Code 侧建立 agent 会话 -> 获取编辑器上下文 -> 模型回复 -> 需要时调用本地工具 -> 回飞书”的主链路
- 继续复用当前仓库已经完成的 Feishu 卡片、审批、计划执行、审计、会话持久化与本地工具执行能力
- 明确区分两层能力：
  - `ask bridge`：一次性提问与回答
  - `agent bridge`：可持续推进任务、读写工作区、调用工具、恢复上下文

## Implementation Base

- 当前 Rust 桥接层已经具备：Feishu 消息收发、卡片回调、审批、会话持久化、审计、以及工作区读写/搜索/测试/Git 能力
- 当前缺失的是 VS Code 侧的 companion extension，它负责接入 Copilot / Language Model API、读取编辑器上下文、维护 agent session，并暴露本地 bridge protocol 给 Rust 监听器调用
- 因此后续工作不再只是继续给 Rust 增加命令，而是引入一个新的 VS Code extension 组件，并把 Rust bridge 与 extension bridge 串起来

## Delivery Principles

1. 先把 remote agent 的最小闭环跑通，再做复杂工具编排
2. 不以“控制现有内置 Copilot Chat 面板会话”为前提，优先实现由 companion extension 自己维护的 bridge session
3. Rust 继续承担 Feishu 入口、审批、安全边界、审计和本地工具执行；VS Code extension 负责模型、编辑器上下文和 agent 会话
4. 每一批能力都要同时包含：
   - 飞书侧触发方式
   - VS Code / extension 侧状态变化
   - 失败路径与回退策略
   - 真实飞书验证路径

## Current Status

已完成的“命令桥”基础：

- 飞书文本消息、卡片回调、审批、失败重试、会话持久化、审计
- 工作区读写、搜索、测试、diff、patch、Git 历史与 blame
- 真实飞书回归：文本链路、卡片继续链路、失败重试链路、以及 Windows listener 启动标准化

仍未完成的“agent bridge”关键缺口：

- 尚无 VS Code companion extension
- 尚未建立 Feishu session -> VS Code agent session 的映射
- 尚未通过 Copilot / Language Model API 发起会话式请求
- 尚未把编辑器上下文（active editor、selection、diagnostics 等）注入会话
- 尚未形成“模型决定 -> 工具编排 -> 结果再喂回模型”的 agent 回路

## A0 Foundation

### Objective

先建立 remote agent bridge 的基础连接能力，让 VS Code 侧 companion extension 能作为本地 agent runtime 存在，并让 Rust bridge 能向它发起一次会话式提问。

### Scope

- 新增 VS Code companion extension 工程
- 提供本地 bridge server
  - 接收来自 Rust / Feishu bridge 的会话请求
  - 暴露健康检查与基础 ask 接口
- 使用 VS Code Language Model API / Copilot model 发起请求
- 维护最小 session store
  - `session_id`
  - `message history`
  - 最近一次模型结果摘要
- 注入最小编辑器上下文
  - 当前 workspace
  - active file
  - selected text

### Why First

- 没有 companion extension，就没有真正的 VS Code-side agent session
- 这一步能快速验证最关键的前提：Copilot model 是否能被 extension 调用，并稳定返回结果给 Feishu

### Expected User Flows

- 飞书发送：`问 Copilot parse_intent 这个函数是干什么的`
- Rust bridge 把消息转给 extension
- extension 建立 / 复用 `session_id`
- extension 读取 active editor context
- extension 调用 Copilot model，返回文本答案
- Rust bridge 把回复回给飞书

### Acceptance Criteria

- 本地 extension 可被激活并启动 bridge server
- 能处理至少 1 条 ask-style 会话请求
- 能返回来自 Copilot / LM API 的模型回答
- 会话 ID 在多次请求之间可复用
- `cargo test` 仍通过；extension 能完成本地 smoke

## A1 Session Bridge

### Objective

把 ask-style 单次调用升级成真正的 bridge session，让飞书里的连续追问能映射到 VS Code extension 侧的同一会话历史。

### Scope

- Feishu session key -> extension session key 映射
- extension 内部会话历史持久化 / 生命周期管理
- 会话级上下文汇总
  - 最近消息
  - 最近模型回复
  - 最近文件上下文
- 支持飞书连续追问复用同一 session

### Why Second

- 这是从 `ask bridge` 迈向 `agent bridge` 的分水岭
- 没有稳定 session，后续的 agent 编排只能退化成一问一答

### Expected User Flows

- 飞书发送：`这个报错先别改，告诉我根因`
- 接着发送：`那最小修复方案呢`
- 接着发送：`先只改测试，不动主逻辑`
- extension 会在同一 session 下持续理解上下文，而不是每次当成新问题

### Acceptance Criteria

- 连续三轮追问共享同一 session history
- 切换到不同 Feishu sender / chat 后不会串线
- session 可被显式重置或自然过期

## A2 Context Bridge

### Objective

让 bridge session 不只知道“聊天历史”，还知道当前 VS Code 工作现场，向真正的 agent 体验靠近。

### Scope

- 采集并注入：
  - active editor URI
  - selection / visible range
  - diagnostics / Problems 摘要
  - workspace folders
  - 可选：最近打开文件
- 提供明确的上下文边界与截断策略
- 将上下文摘要纳入会话请求与回显

### Why Third

- 用户之所以觉得 Copilot Chat 像 agent，不只是因为模型，而是因为它看得到当前编辑器上下文

### Acceptance Criteria

- 模型回答能引用当前文件或选区
- 上下文过大时能被可控截断
- 用户可感知当前上下文来源，而不是黑箱行为

## A3 Tool Loop

### Objective

把 extension 侧模型会话和当前 Rust 工具桥接起来，形成真正的 agent loop：模型可以决定读文件、搜代码、跑测试、看 diff，再根据结果继续回复。

### Scope

- 设计 extension <-> Rust bridge tool protocol
- 由 extension 发起工具调用请求
- 由 Rust bridge 执行现有工具能力
- 把工具结果回注入模型上下文
- 首批接入工具：
  - `read_file`
  - `search_text`
  - `search_symbol`
  - `find_references`
  - `run_tests`
  - `git_diff`

### Why Fourth

- 只有会话桥和上下文桥还不够；真正的 agent 必须能自主调用工具
- 当前 Rust 侧已经有大量可靠工具能力，应该复用而不是在 extension 重造一套

### Acceptance Criteria

- 能完成 1 条“模型 -> 工具 -> 模型 -> 飞书”的闭环
- 工具调用在飞书侧可见且可审计
- 失败的工具调用不会破坏会话状态

## A4 Controlled Write Path

### Objective

在 agent loop 稳定后，再把补丁、写文件、测试回归、审批接入到 agent 编排里，让 agent 真正具备受控改代码能力。

### Scope

- 接入 `apply_patch` / `write_file` / `run_test_file`
- 保持审批边界
- 把 patch / diff / test result 重新喂给 session
- 让飞书能看到“方案 -> 修改 -> 验证”的连续链路

### Acceptance Criteria

- 至少完成 1 条真实飞书 `分析 -> 改代码 -> 查看 diff -> 跑测试 -> 汇报结果` agent 链路
- 写操作全部仍走现有审批边界

## A5 Publishability

### Objective

把 remote agent bridge 从可跑的开发原型推进成可安装、可配置、可复现的独立发布物。

### Scope

- 完整 extension 安装与启动说明
- Rust bridge 与 extension 的启动顺序与健康检查
- 配置向导补充 extension 依赖检查
- 日志、诊断、常见失败处理文档

### Acceptance Criteria

- 新用户按文档可在一台机器上完成 setup
- 能完成 1 条 ask-style smoke 和 1 条 agent-style smoke

## Suggested Build Order

1. M0 已完成：Feishu 命令桥、会话、卡片、审批、审计、工作区工具
2. A0.1 新增 companion extension 工程骨架
3. A0.2 在 extension 内启动本地 bridge server
4. A0.3 用 Copilot / LM API 跑通单次 ask 请求
5. A1.1 建立 session 映射与历史复用
6. A2.1 注入 active editor / selection / diagnostics
7. A3.1 定义 extension <-> Rust tool protocol
8. A3.2 接入只读工具闭环
9. A4.1 接入受控写路径与审批
10. A5.1 文档、setup、发布收尾

## Non-Goals For Now

- 尝试直接控制现有内置 GitHub Copilot Chat 面板的私有会话状态
- 在 companion extension 里重写一整套本地工具执行系统
- 在没有 session / context / tool loop 基础前，直接追求复杂多 agent 编排
- 为了“像 agent”而绕过现有审批与审计边界

## How To Resume Later

后续如果上下文丢失，直接把当前仓库视为“命令桥已完成，agent bridge 未开始”的状态继续推进。第一优先级不是再给 Rust 增加更多零散命令，而是完成 A0：落 companion extension、建立本地 bridge server、打通一条 ask-style Copilot / LM 请求。A0 完成后，再继续 A1 session、A2 上下文、A3 工具回路。