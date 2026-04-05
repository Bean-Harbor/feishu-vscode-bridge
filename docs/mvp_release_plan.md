# MVP Release Plan

## Goal

尽快发布一个可被真实用户安装、使用、反馈的 agent MVP，用最短路径验证下面这件事是否成立：

用户是否愿意在飞书里调用本地 agent，而不是来回切回 VS Code 手动操作。

## Product Definition

- 产品本体：本地 agent runtime / orchestration / skills / approvals / memory
- 第一入口：VS Code extension
- 第二入口：飞书
- 后续入口：桌面 app / 菜单栏 / 浏览器桥接 / 系统操作
- 首发分发形态：面向用户必须是安装包，而不是多个文件手工组装
- 首发平台要求：Windows 和 macOS 都要进入安装规划，因为核心用户会跨系统切换

结论：首发可以是 VS Code extension，但 VS Code extension 只是第一入口，不是最终产品定义。

## Why This Release Shape

- 当前最成熟的能力已经建立在 VS Code + Copilot + 本地工作区上下文之上
- 用 extension 作为第一入口，工程范围最小，最容易做出可安装、可演示、可反馈的闭环
- 飞书提供了一个非常清晰的远程使用场景，能直接验证需求是否真实存在
- 如果一开始就做完整平台、多入口桌面产品，交付周期会被显著拉长，反馈周期也会变慢
- 但“可安装”不能只对开发者成立，必须把 Windows 和 macOS 的真实安装路径一起收敛

## MVP Thesis

MVP 不是“发布一个插件看看有没有人下载”，而是验证下面三个假设：

1. 用户需要在飞书里远程调用本地开发助手
2. 用户愿意接受本地运行、可审批、可审计的工作方式
3. 用户需要的是持续推进任务的 agent，而不是只问一次问题的问答工具

## Target User

- 第一批用户：重度使用 VS Code 和 Copilot 的开发者
- 使用环境：本地开发、远程协作、经常在飞书里沟通代码问题
- 典型场景：
  - 在飞书里问当前仓库问题
  - 在飞书里追问错误根因和最小修复方案
  - 在飞书里触发读取、搜索、测试、diff、补丁审批等操作

## MVP Scope

### Must Have

- `问 Copilot <问题>` 作为 agent bootstrap 入口
- `重置 Copilot 会话`
- 基于当前 workspace 的上下文回答
- 会话连续性
- agent 任务状态的最小连续性
- 基础审批边界
- 审计日志
- 基础安装和诊断说明
- Windows 安装包方案
- macOS 安装包方案

### Should Have

- 读取 / 搜索 / 测试 / diff 这类高频只读或低风险工具
- 基础卡片交互
- 错误时的清晰降级提示
- 健康检查和最小自检路径
- companion extension 自动安装或半自动安装路径

### Not In MVP

- 多入口同时首发
- 完整桌面应用
- 泛化到非开发场景的自动化平台
- 复杂多 agent 编排
- 大规模 skills marketplace
- 系统级自动控制能力

## Agent Boundary

这里的 `问 Copilot` 不是产品定义，只是当前已经跑通的第一个 agent 入口命令。

当前已实现的是：

- agent bootstrap
- session 复用
- workspace context 注入
- reset

当前还未完整实现的是：

- 模型驱动的工具调用回路
- 任务级状态推进
- 分析 -> 调工具 -> 继续行动 -> 汇报结果 的完整闭环

所以这个 MVP 应该被表述为 `agent MVP`，而不是 `ask 产品`。

## Product Narrative

对外不把产品讲成“一个 VS Code 插件”。

更准确的说法是：

- 一个本地优先、可审批、可审计的开发者 agent
- 一个让你在飞书里调用本地 VS Code / Copilot 能力的远程工作流工具
- 一个正在从 VS Code extension 起步的本地 agent 产品

## Packaging Strategy

- Windows 对外产物：一个 `Setup.exe`
- macOS 对外产物：一个 `.dmg`
- 对用户隐藏内部复杂度：runtime、setup UI、配置文件、日志目录、extension 安装步骤都由安装流程统一承接
- 不接受首发把 `bridge-cli`、脚本、`.env` 模板、`.vsix` 手工交给用户拼装

硬决策：

- Windows 打包技术选 `NSIS`
- macOS 打包技术选 `dmg + setup app`
- extension 分发策略选 `bundled .vsix first, Marketplace fallback`

## Installer Scope

- 检测 VS Code 是否已安装
- 缺失时给出安装引导
- 安装或升级 companion extension
- 采集 Feishu 凭证并写入本地配置
- 执行最小健康检查
- 提供失败后的恢复入口

## Platform Plan

### Windows

- 首发安装器：`Setup.exe`
- 安装后结果：开始菜单入口、本地 runtime、配置目录、日志目录、已处理的 extension 安装流程
- 优先级：首个公开 beta 渠道

### macOS

- 同步规划安装器：`.dmg` + setup app
- 安装后结果：Applications 中的 setup app 或主应用入口、本地 runtime、配置目录、日志目录、已处理的 extension 安装流程
- 优先级：不晚于 Windows 私测验证，因为它是日常主力使用平台之一

## Release Sequence

### Phase 1: Private Beta

- 目标：10 到 20 个种子用户
- 方式：手把手安装，观察真实使用
- 核心任务：
  - 确认用户能在 5 分钟内跑通首个任务
  - 记录首个失败点
  - 记录前三大高频命令
  - 记录他们为什么切回 VS Code
  - 验证 Windows 和 macOS 两条安装路径都能独立完成首装

通过标准：

- 用户能成功完成至少 3 次真实任务
- 用户会主动发起连续追问
- 用户明确表达“这比我手动切回 VS Code 更快”
- Windows 和 macOS 各自的首装成功率达到可接受水平

### Phase 2: Public Beta

- 目标：公开发布 extension 入口
- 方式：文档化安装，自助使用
- 核心任务：
  - 提高安装成功率
  - 提高首个任务成功率
  - 稳定 agent bootstrap + session + context 主链路
  - 收集结构化反馈
  - 提供 Windows `Setup.exe`
  - 提供 macOS `.dmg`

通过标准：

- 用户无需人工陪跑即可完成安装
- 首次 agent 任务进入成功率足够高
- 用户能理解产品价值，不需要长篇解释
- 跨 Windows / macOS 的安装说明和恢复路径一致

### Phase 3: Full Product Expansion

- 将本地 runtime 进一步产品化
- 增加新的入口层，而不是复制一套新的逻辑
- 把 VS Code extension 从“产品表面”变成“产品入口之一”

## MVP Success Metrics

- 安装到首个成功任务的时间
- 首个 agent 任务进入成功率
- 七天内重复使用率
- 每个用户的连续追问次数
- 每个用户的任务推进深度
- 被使用最多的前三个命令
- 用户反馈中的高频失败原因
- 用户是否愿意继续保留本地常驻环境
- Windows 安装成功率
- macOS 安装成功率
- companion extension 安装成功率

## Current Engineering Priorities

1. 稳定 VS Code extension agent bootstrap/session/context 主链路
2. 保持 Feishu 入口可用、可追问、可审计
3. 把 Windows 和 macOS 的安装、诊断、恢复路径一起做短
4. 保持审批边界，不为了演示速度破坏安全模型
5. 尽快接入最小工具回路，不把产品停留在问答层
6. 收敛 installer 和 extension 的协同安装方案

## MVP Build Checklist

### Product

- 明确一句话定位
- 明确首批用户画像
- 明确首发不做什么

### Engineering

- agent bootstrap 主链路稳定
- session reset 可用
- workspace context 可用
- 基础工具链路可用
- 错误提示可读
- README 与安装说明可执行
- Windows 安装流程可走通
- macOS 安装流程可走通
- extension 安装流程可走通
- Windows 打包脚本存在且可跑通到产物目录
- macOS 打包脚本存在且可跑通到产物目录

### Feedback Loop

- 建立反馈收集表
- 记录失败日志与场景
- 每周整理高频问题
- 按真实使用频率排序后续需求

## Immediate Next Build Slice

MVP 的下一步不是继续泛化平台，也不是继续给 continuation 叠更多命令，而是先把已经跑通的 agent 链路收敛成“可验证、可安装、可讲清楚”的 MVP：

1. 用户在飞书发送 `问 Copilot ...`
2. 桥接到 VS Code extension
3. 复用会话并注入工作区上下文
4. 建立当前任务意图
5. 必要时继续只读工具调用
6. 返回清晰阶段性结果
7. 用户可以直接继续推进同一个任务，而不是重新发起一次 ask
8. 用户仍可随时重置会话

当前这条链路已经具备最小 continuation 能力，下一块优先要补的是两件事：

1. 把 real Feishu 验证标准和失败恢复路径固定下来
2. 把 Windows / macOS 的安装、诊断、extension 协同安装路径收口成真正可发布的故事

只要这条链路稳定，并且具备最小任务推进能力，就已经具备首发测试价值。

工程拆解见 `docs/agent_mvp_execution_plan.md`。