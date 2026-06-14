# AI 编程工具仓库研究与 Lan Code 方向

## 研究范围

本地 `research/repos/` 已浅克隆 18 个公开官方仓库：

| 项目 | 主要形态 | 主要语言 | 最值得研究的部分 |
| --- | --- | --- | --- |
| Kilo Code | CLI / IDE / 平台 | TypeScript | 多端产品与 OpenCode 系谱 |
| Cline | SDK / CLI / VS Code | TypeScript | IDE agent、checkpoint、审批体验 |
| Qwen Code | CLI / channels | TypeScript | Gemini CLI 系谱、多渠道接入 |
| Gemini CLI | CLI / core | TypeScript | scheduler、policy、sandbox、确认总线 |
| Tabby | 自托管服务 / IDE 客户端 | Rust | 推理、索引、服务端部署 |
| Void | 编辑器发行版 | TypeScript | VS Code fork 级整合 |
| Crush | TUI CLI | Go | 小而清晰的终端产品结构 |
| OpenCode | CLI / desktop / server / SDK | TypeScript | session/event、server、插件化 |
| Claude Code 官方仓库 | CLI 配套公开内容 | 混合 | 插件、hooks、SDK 示例；非完整核心源码 |
| Continue | CLI / IDE / CI | TypeScript | 配置化模型与上下文、AI checks |
| Roo Code | VS Code / 多包 | TypeScript | modes、auto-approval、provider 广度 |
| Aider | CLI | Python | repo map、编辑格式、git 原生工作流 |
| OpenHands | Web / runtime / agent | Python | 容器化 runtime、远程执行 |
| Goose | CLI / desktop / server / SDK | Rust | ACP、MCP、扩展、跨语言 SDK |
| Plandex | CLI / server | Go | 大任务 plan、上下文与变更审阅 |
| OpenAI Codex | CLI / app server / SDK | Rust | app-server 协议、sandbox、权限、事件 |
| Trae Agent | agent / benchmark | Python | 精简 agent loop 与评测 |
| Kun | desktop / local runtime / CLI | TypeScript | 需求先行工作流、HTTP/SSE runtime、goal/todo/plan/review、缓存与历史卫生 |

说明：Anthropic 官方 `anthropics/claude-code` 仓库不是 Claude Code 完整核心源码。
2026 年 3 月出现的所谓“源码发布”是 source map 泄漏后的非授权镜像，不纳入克隆、
引用或实现依据。

## 共同架构

几乎所有成熟工具都收敛到同一条 agent loop：

1. 收集系统指令、仓库规则、用户消息与上下文。
2. 选择模型并构造 provider 请求。
3. 流式接收文本、推理和 tool call。
4. 对 tool call 做解析、校验、权限判断和执行。
5. 将 tool result 写回历史，继续下一轮模型请求。
6. 达到完成、错误、中断或轮数上限后结算。

真正拉开差距的不是这个循环，而是循环周围的工程：

- 上下文选择是否精准、稳定、可压缩。
- 编辑与 patch 是否可靠，失败后是否容易恢复。
- shell、文件、网络权限是否真正执行层隔离。
- 会话与事件是否可恢复、可分叉、可跨客户端继续。
- 模型、MCP、工具、skills、hooks 是否有清晰扩展边界。
- 用户能否看懂 agent 正在做什么，并在关键点控制它。

## 共同优点

### 多模型与工具生态

开源项目普遍支持 OpenAI-compatible、Anthropic、Gemini、本地模型和 MCP。用户不会
被单一模型绑定，工具能力也可快速扩展。

### Agent 已从聊天升级为执行系统

现代项目都将 shell、文件编辑、搜索、Git、浏览器等能力纳入循环，并逐步加入
plan/build、子代理、后台任务和长任务恢复。

### Core 与客户端开始分离

Codex 的 app-server、Goose 的 SDK/server/ACP、OpenCode 的 server/SDK/spec 都证明：
一个独立运行时服务比“把所有逻辑塞在 VS Code extension host”更适合多客户端产品。

## 共同缺陷

### 权限提示不等于安全

不少工具只有 UI 级确认，底层进程仍拥有宿主机完整权限。真正安全需要把策略判断和
OS sandbox 执行分开，并对文件、网络、进程、密钥分别建模。

### TypeScript IDE 系项目容易过度耦合

Cline、Roo、Kilo 等能力很强，但扩展生命周期、webview 状态、provider、agent loop
容易互相引用。长期会导致测试慢、复用难、CLI/desktop 行为不一致。

### 会话可靠性经常晚于功能数量

流式 delta、工具执行、持久化和 UI 状态如果没有统一事件模型，崩溃恢复和多客户端
重连会出现重复执行、丢消息、状态漂移。

### “支持很多模型”带来最低公分母问题

不同 provider 对 tool calling、reasoning、缓存、并行调用、上下文长度和错误恢复的
语义并不一致。把它们强行压成一个简单接口，会损失能力或产生隐性 bug。

### 大上下文不是好上下文

仓库全量塞入 prompt 成本高且噪声大。Aider 的 repo map、Tabby 的索引、现代工具的
rules/skills/context provider 都在解决同一问题，但仍缺少统一、可解释的选择机制。

## 各路线的取舍

### CLI-first

代表：Codex、Gemini CLI、Qwen Code、Aider、Crush、Plandex。

优点是启动路径短、容易组合 shell/Git、自动化友好。缺点是 diff、审批、长任务与
多会话展示空间有限。适合作为 core 的第一个真实客户端，但不应成为 core 本身。

### IDE-first

代表：Cline、Roo Code、Continue。

上下文和编辑体验天然强，用户也容易审阅变更。缺点是高度依赖编辑器 API，后台执行、
远程运行和多客户端一致性较难。

### Editor fork

代表：Void。

可以控制整个体验，但维护 VS Code 上游成本巨大。Lan Code 当前不应走这条路线。

### Service/runtime-first

代表：OpenHands、Tabby、Goose、Codex app-server、OpenCode server。

最适合多客户端与团队场景，但需要认真处理认证、隔离、持久化、协议兼容和运维。
这是 Lan Code core 应采用的主路线。

## 项目级判断

| 项目/家族 | 应借鉴 | 应避免 |
| --- | --- | --- |
| Kun | 需求先行工作流、HTTP/SSE 单运行时、goal/todo/plan/review/fork、缓存与历史卫生 | Electron 大客户端、过宽产品面、非商业许可证源码复用风险 |
| Codex | app-server、Thread/Turn/Item、协议生成、细粒度 sandbox 与 approval 分层 | 协议面迅速膨胀后给客户端带来的兼容压力 |
| OpenCode | core/server/SDK/多客户端、durable inbox、context epoch、工具注册作用域 | 过早引入过于抽象的 effect/event 模型，增加贡献门槛 |
| Goose | ACP、MCP、扩展、server、Rust SDK 与跨语言绑定 | 扩展、recipe、server API 多套概念并存导致的认知成本 |
| Gemini CLI / Qwen Code | scheduler、policy、确认总线、sandbox 扩权恢复 | TypeScript core 中配置对象和运行时服务逐渐变成大依赖中心 |
| Cline / Roo / Kilo | 审批体验、checkpoint、多 provider、成熟 IDE 交互 | core、extension host、webview、产品后端互相耦合 |
| Aider | repo map、编辑格式、Git 原生、简单可靠的 CLI 心智模型 | CLI 交互模型限制多客户端和后台长任务 |
| OpenHands | 隔离 runtime、远程任务、Web 产品化 | 容器与部署复杂度成为个人本地使用门槛 |
| Tabby | 自托管、索引、推理服务、IDE 客户端薄层 | 把补全/索引服务和 agent runtime 过早揉成一个大服务 |
| Continue | 配置化上下文、模型、IDE/CI 多入口 | 产品方向变化让核心抽象和历史能力同时存在 |
| Void | 完整编辑器体验控制权 | 长期维护 VS Code fork 的巨大成本 |
| Crush | 清晰、轻量、好看的 TUI | 小型 CLI 架构在复杂持久化和远程客户端下扩展不足 |
| Plandex | 大任务规划、变更审阅、上下文管理 | plan 过程过重，简单任务延迟高 |
| Trae Agent | 小而清楚、适合研究和评测 | 生产级权限、持久化、多客户端能力不足 |

## Kun 专项研究

本地快照位于 `research/repos/kun`，提交为
`a4c91b31d4f7b777769f1dd3e1ea8d13a2c17941`。Kun 的核心不是 Electron GUI，
而是独立的 `kun serve` TypeScript 运行时。桌面端通过本地 HTTP 请求和 SSE
事件连接运行时，Agent loop、线程、审批、工具和持久化不落在 renderer 中。

### 最值得 Lan Code 吸收的设计

1. **长期任务是一等领域对象。** Kun 将目标、Todo、计划、审查和分叉写入
   thread 状态，而不是只把它们当成聊天文本。目标可以跨 turn 继续，并有明确的
   complete / blocked 审计规则。
2. **协议覆盖完整工作流。** 它提供 thread list/create/fork/resume、turn
   start/steer/interrupt/compact、approval、user input、usage 和 workspace
   status。GUI 只是协议客户端。
3. **工具目录按上下文裁剪。** CapabilityRegistry 统一 built-in、MCP、web、
   skill、memory、delegation 和媒体工具，并根据 sandbox、plan mode、provider
   和 allow-list 同时控制“是否展示给模型”和“是否允许执行”。
4. **Agent loop 有运行卫生。** 包括不可变 prompt 前缀、工具 schema 指纹、
   history repair、tool-call repair、tool storm breaker、read-before-edit、
   并行只读工具、上下文压缩和 token economy。
5. **追加式事实与可重建索引分离。** JSONL 保存可回放事实，SQLite 只承担可重建
   的列表与搜索索引。这比只保存 session snapshot 更适合恢复、审计和多客户端。
6. **计划与 Todo 双向关联。** Markdown 计划中的 checkbox 可以同步到结构化
   Todo，Todo 状态变化也可以回写计划文件，让计划不只是一次性模型输出。

### Kun 的缺点与风险

- 产品同时覆盖 Code、Write、媒体生成、IM、自动化和插件，能力很丰富，但主路径
  容易变宽，客户端复杂度和测试成本较高。
- Agent loop 单文件和配置面已经较大。Lan Code 应吸收机制，但继续保持 Rust
  crate 边界，避免形成新的大依赖中心。
- 当前 sandbox policy 仍主要是工具展示与执行前阻断；真正的宿主机隔离仍需要
  OS sandbox executor。
- HTTP/SSE 很适合桌面和 Web，但本地 CLI/IDE 还需要考虑 socket、背压、重连和
  协议版本协商。
- Kun 使用 PolyForm Noncommercial License 1.0.0。Lan Code 只能研究其思想，
  不应复制源码、提示词或实现细节进入可能商业化的发行版。

### 对 Lan Code 路线图的直接影响

Kun 证明 Lan Code 下一阶段不应继续堆桌面按钮，而应先扩展 core 的领域模型和协议：

1. 将 `Session` 演进为 `Thread -> Turn -> Item`，给 tool call、approval、
   message、review、compaction 稳定身份。
2. 增加 `turn/steer`、`thread/fork`、`thread/archive`、`turn/compact` 和
   `user-input` gate。
3. 增加结构化 goal / todo，并让桌面端展示真实进度，而不是从事件文本猜测。
4. 将当前 snapshot + event 存储演进为追加式事实日志和可重建 projection。
5. 实现上下文预算、历史修复、重复工具调用熔断和 read-before-edit。
6. 在这些协议稳定后，再接入 MCP、skills、subagent 和更多客户端。

## 关键设计结论

### 1. 协议应围绕事实，而不是 UI 命令

客户端应接收 `session.created`、`turn.started`、`item.completed`、
`approval.requested` 这类领域事件，而不是 `showSpinner`、`appendChatBubble`。
这样 CLI、桌面端和 IDE 才能基于同一事实构建不同体验。

### 2. Durable event 与 live delta 必须分开

逐 token delta 适合实时展示，但不适合作为恢复依据。完整用户消息、模型消息、工具调用、
工具结算、审批结论必须持久化并具备稳定顺序。重连客户端先 replay durable event，再
订阅 live delta。

### 3. 工具调用必须具备不可混淆的身份

至少需要 `session_id + turn_id + assistant_item_id + tool_call_id`。模型供应商返回的
call id 不一定跨 turn 唯一，不能直接作为内部全局身份。工具定义在一个 provider turn
开始时应被冻结；执行时若注册项已变化，应拒绝 stale call。

### 4. 权限系统需要四层

1. 工具声明动作与资源，不自行决定是否安全。
2. Policy 根据用户模式、工作区、规则和风险做 Allow/Ask/Deny。
3. Approval 由客户端展示，但结论回到 core。
4. Sandbox/Executor 在 OS 层真正限制文件、网络与进程权限。

只实现第 2、3 层的产品仍然可能在底层拥有完整权限。

### 5. Provider 接口要保存能力差异

统一接口应覆盖共同生命周期，但需要 capability negotiation 保存 provider-native
reasoning、prompt cache、parallel tool calls、hosted tools、structured output 等能力。
历史中保存 provider metadata 时还要考虑切换模型后的重放兼容性。

### 6. 多客户端意味着单一执行所有权

同一 session 可以被多个客户端观察，但同一时刻只能有一个明确的 turn 执行所有者。
steer、queue、interrupt、approval resolve 都必须通过 core 串行协调，不能由各客户端
直接改状态。

## 许可与复用边界

研究仓库用于学习架构，不代表可以任意复制源码。MIT、Apache-2.0 等宽松许可证也要求
保留相应声明；没有明确开源许可证或仅公开配套内容的仓库，应视为不可复用核心源码。
Lan Code 当前原型是独立实现，没有复制研究仓库代码。

## Lan Code 建议

### 核心定位

Lan Code 应是“本地优先、可远程部署、客户端无关的 agent runtime”。桌面端、CLI、
VS Code、JetBrains、Web 都是协议客户端，不拥有 agent 真相。

### 第一优先级

1. 稳定协议：initialize、session、turn、item/event、approval、interrupt。
2. 事件存储：追加式 durable event + read model，避免 UI 状态成为事实来源。
3. 权限系统：工具风险声明、策略判断、用户审批、sandbox 执行四层分离。
4. Provider capability matrix：保留各模型特性，避免最低公分母接口。
5. 工具注册表：内建工具、MCP、插件使用同一调用和结算边界。
6. 可恢复执行：明确哪些动作可重试，任何有副作用调用不得静默重放。

### 暂时不要做

- 不要先做一个完整 IDE fork。
- 不要让 web 客户端直接拿宿主机 shell 权限。
- 不要把 MCP 当作 core 内部所有模块的接口；内部接口应更强类型、更可控。
- 不要在没有 sandbox enforcement 时宣传“安全模式”。
- 不要一开始就追求几十个 provider；先把 capability 和错误语义做对。

## 分阶段路线图

### M0：架构验证（当前）

- Rust workspace、协议/core/daemon 分层。
- session、event、tool registry、基础 permission policy。
- JSONL stdio transport 与 smoke test。

### M1：可用 CLI agent

- 已完成：`turn/start`、OpenAI-compatible provider、DeepSeek V4 Pro 实测。
- 已完成：list/read/search 与精确 `replace_text` 工具、只读默认策略。
- 已完成：`turn/interrupt`、approval 暂停/resolve、并发 daemon 请求处理。
- 已完成：SQLite session/message snapshot、durable event 与崩溃后 interrupted 恢复。
- 已完成：无 shell 字符串解析的 `run_command`，严格标记为 FullAccess。
- 已完成：同一 JSONL 连接上的实时 `event` notification 与 durable event replay。
- 待完成：Anthropic adapter、apply-patch/git 工具、跨进程审批继续。
- 工作区路径 containment；高风险工具默认 Ask。

### M2：安全与扩展

- Windows/Linux/macOS sandbox executor。
- MCP client、skills、hooks、插件 manifest。
- 上下文发现、repo map、token budget 与 compaction。
- provider capability matrix、重试与副作用去重策略。

### M3：多客户端

- TypeScript/Kotlin client SDK。
- VS Code 与 JetBrains 薄客户端。
- 桌面端通过本地 socket 连接 daemon。
- Web 端仅通过认证 gateway 访问受限远程 runtime。

### M4：团队与远程运行

- 多用户认证、审计、策略配置。
- 容器/VM runtime、远程 workspace。
- session 分享、任务调度、评测与可观测性。

## 当前原型

本仓库已创建 Rust workspace：

- `lan-protocol`：跨客户端可序列化类型。
- `lan-core`：会话、事件广播、工具注册、风险与权限决策。
- `lan-daemon`：JSONL/stdin-stdout transport。

这不是完整 agent，当前目的是验证最重要的架构边界。下一阶段应实现真正的 turn loop、
事件持久化、approval resolve、模型 adapter、文件/patch/shell 工具和 sandbox。
