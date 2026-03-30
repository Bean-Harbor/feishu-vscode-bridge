# HarborBeacon Runtime Migration Plan

## Goal

把当前 `feishu-vscode-bridge` 从“命令桥 + ask bridge”推进到更接近 HarborBeacon 的 IM agent runtime。

本轮只先做第一阶段：

- 记录可执行的迁移计划
- 把 `bridge.rs` 里的 session / reply 职责拆出来
- 把 `plan.rs` 的审批从布尔暂停升级成带上下文的审批请求

## Phase 1

### 1. Runtime Split

目标：让 `bridge.rs` 不再同时承担状态存储、状态回放、agent 回复格式化。

拆分方向：

- `src/session.rs`
  - 持久化会话结构
  - 最近文件 / diff / patch / last result 聚合
  - plan progress -> stored session 的转换
- `src/reply.rs`
  - agent reply 格式化
  - 最近结果 / 最近 diff / 最近文件回放
  - 通用 `describe_intent` 文本化

### 2. Approval Context

目标：审批时不再只知道“第几步卡住了”，而是知道“为什么卡住、风险是什么、批准后会发生什么”。

落点：

- `src/plan.rs`
  - 新增 `ApprovalRequest`
  - `PlanProgress` 改为携带结构化审批信息
- `src/bridge.rs`
  - 生成审批理由和风险摘要
  - 卡片和文本回复展示审批上下文

## Phase 2

后续再做：

- 群聊触发规则
- thinking 占位回复
- autonomy / risk 语义层
- agent task state 与 tool loop

## Current Status

- 本文档已记录第一阶段迁移目标
- 当前代码会先完成 Runtime Split + Approval Context