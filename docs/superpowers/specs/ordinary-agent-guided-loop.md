# OmicsOps 普通 Agent Guided Loop 设计

> 日期：2026-09-02
> 状态：普通 Agent 的实现设计基线；真实模型、浏览器、跨平台安装和生产验收仍需按验收清单单独记录。

## 1. 摘要

本设计为 OmicsOps 的普通 Agent 增加一个可持久、可恢复的 guided loop。它只
覆盖由 `agent_v4_start_direct` 直接创建的
`RunExecutionKindV4::OrdinaryAgent` Execute run。它让 Host 能够记录任务形态、
阶段、模型轮次、工具批次和动态只读 task list，并在用户输入、审批、浏览器交接
或进程崩溃后继续同一个 run。

这不是新的会话模式，也不是把普通 Agent 变成 Plan。`fast` 和 `multi_step` 是
ordinary run 内的 `task_shape`；Host 只允许 `fast → multi_step` 的单调提升。
Plan 生成、Plan revision/approval/revise/cancel、批准后的冻结执行和现有 Plan
只读 MCP 门禁保持不变。

## 2. 范围、非目标与术语

### 2.1 范围

- 直接普通 Agent 的 `fast` 和 `multi_step` task shape；
- 可审计的 `TaskShapeSelected`、`PhaseChanged`、`CycleStarted`、
  `CycleFinished`、`TaskListUpdated`、`ToolBatchStarted` 和
  `ToolBatchFinished` 事件；
- `routing`、`discovery`、`clarification`、`organizing`、`executing`、
  `verifying` 阶段游标，以及 checkpoint 中的 shape、phase、task revision、
  tasks 和 cycle；
- adaptive 与 research retrieval 的固定 discovery、条件性 Skill/MCP/浏览器
  证据门禁；
- 公开思考摘要、输入等待、同 run 恢复、崩溃恢复和完成门禁。

### 2.2 非目标

- 不为 Plan 增加 task list、guided loop、真实浏览器、Network 或写入能力；
- 不把 task list 当作 `ExecutionPlanV4`，不引入 Plan hash、Plan approval 或
  Plan lock；
- 不持久化或展示私有 Chain of Thought，不实现任意多代理、递归委派、任意
  HTTP、任意脚本或凭据读取；
- 不以自动化测试、模拟浏览器、静态检查或 ignored 测试替代真实浏览器/模型/跨
  平台生产验收。

### 2.3 术语与权威性

- **普通 Agent**：`agent_v4_start_direct` 直接创建且执行来源完整性校验为
  `ordinary_agent` 的 run；Plan 和批准后的计划即使 `RunModeV4::Execute` 也
  不是普通 Agent。
- **task shape**：`AgentTaskShapeV4::Fast` 或 `MultiStep`。它是 run 内的编排
  形态，不是会话模式、权限级别或批准状态。
- **phase**：`AgentPhaseV4` 的当前编排阶段；phase 事件只描述游标，不替代
  工具 outcome。
- **cycle/model round**：一次模型请求及其公开摘要和随后的工具批次，使用
  `cycle_id` 关联；模型输出本身不是成功证据。
- **tool batch**：同一 cycle/phase 中由 Host 统一授权、调度和汇总的一组工具
  调用；可并行只限相互独立的只读调用。
- **task list**：`AgentTaskV4` 的有限只读进度投影，由
  `agent.update_tasks` 创建/更新，保持 2–12 项并由 `TaskListUpdated.revision`
  版本化；它不会执行任务或授予能力。
- **成功事件**：同一 run 中持久化且 `succeeded = true` 的 `ToolFinished`，或
  经过精确幂等检查的成功 `ToolOutcomeReused`。请求、dispatch、失败、拒绝、
  过期事件和模型文字都不算成功。

## 3. 运行形态和单调状态机

### 3.1 两种 task shape

`fast` 用于短、边界明确的请求。它仍然必须先成功完成
`agent.route_request({route, task_shape, reason})`；不要求创建 task list，也不能
因为省略 task list 而绕过工具验证、审批、科学状态或 completion gate。

`multi_step` 用于需要发现、拆解、澄清、执行和验证的请求。Host 对每个周期写入
当前 phase、cycle 和 tool batch，并用 task list 向用户公开有限的进度状态。若
研究请求需要多阶段来源证据，必须在进入研究 discovery 前处于 `multi_step`；
`fast` 不能成为跳过 MCP/浏览器顺序的旁路。

`TaskShapeSelected` 的 `source` 区分模型建议与 Host 决策，但模型只能提出建议。
Host 对持久事件和迁移执行以下关系：

```text
未选择 ──→ fast
未选择 ──→ multi_step
fast ────→ multi_step
fast ────→ completed/failed/cancelled/needs_attention
multi_step ──→ completed/failed/cancelled/needs_attention
```

不存在 `multi_step → fast`、ordinary → Plan 或 Plan → ordinary 的隐式迁移。
task shape 不能扩展冻结 capability、把只读调用变为副作用调用或替代审批。

### 3.2 阶段、轮次和批次事件

Host 在普通 Agent run 中持久化：

| 事件 | 作用 | 最小绑定 |
| --- | --- | --- |
| `TaskShapeSelected` | 记录 shape、来源和脱敏原因 | run、shape、source |
| `PhaseChanged` | 记录当前 `AgentPhaseV4` | run、phase |
| `CycleStarted` / `CycleFinished` | 标记一个模型轮次及其边界 | run、`cycle_id` |
| `ToolBatchStarted` | 记录统一授权/调度的工具批次 | run、batch、cycle、phase、tool/call IDs |
| `ToolBatchFinished` | 记录批次汇总 | 上述绑定、duration、成功/失败数 |
| `TaskListUpdated` | 追加 task list 新 revision | run、revision、有限 tasks |

事件必须沿用 V4 的 `run_id`、项目/会话归属、递增 sequence 和 hash chain。批次
只能汇总已授权调用；批次成功数不等于科学结果正确，也不能替代各个
`ToolFinished`。崩溃或断线时，以最后一个持久化事件和 checkpoint 为准，而不是
以 UI 内存状态或模型回显为准。

阶段的通常语义如下：`routing` 冻结请求路由，`discovery` 执行规定的只读发现，
`clarification` 等待用户回答，`organizing` 更新 task list，`executing` 执行
已授权任务，`verifying` 运行完成前的证据与确定性检查。修复、澄清或验证失败
可以再次进入后续需要的阶段；不能仅通过改变 phase 事件伪造已完成门槛。

## 4. 固定 discovery 与证据顺序

### 4.1 路由是不变的入口

Host 根据本轮原始用户目标做确定性保护性分类，至少识别论文/文献、PubMed、
PMID/DOI、期刊/引用、外部数据库、最新资料、显式网页和跨来源证据等研究信号。
模型仍调用 `agent.route_request` 记录理由，但 Host 判定的
`research_retrieval` 不能被模型改成 `adaptive`。成功的 route 在同一 run 内只
能出现一次；未路由前禁止任何任务工具。

### 4.2 adaptive multi_step

```text
agent.route_request(adaptive, task_shape=multi_step)
  → phase=discovery
  → project.list + search_memory + search_skills（只读，可并行）
  → [Skill 命中时] use_skill
  → [确有必要时] agent.request_input（只问 material scope 问题）
  → agent.update_tasks（2–12 项，只读）
  → phase=organizing
  → phase=executing / phase=verifying
```

`project.list` 必须从项目根（`path=""`）开始；`project.list`、`search_memory` 和 `search_skills` 是独立只读查询时，可以放在
同一个 `ToolBatch`；并行不改变它们都必须经过 Host 校验和持久化 outcome 的
要求。Skill 无匹配时不虚构 `use_skill`；有匹配时至少一个成功 `use_skill`
才能离开 discovery。发现内容是输入数据，不是权限或事实保证。

### 4.3 research multi_step

Research 保留 `search_mcp_tools` 先行，并把它计入 discovery，而不是另起一条
旁路：

```text
agent.route_request(research_retrieval, task_shape=multi_step)
  → phase=discovery
  → search_mcp_tools（第一个 discovery 门）
  → project.list + search_memory + search_skills（其余只读 discovery，可并行）
  → [Skill 命中时] use_skill
  → [确有必要时] agent.request_input（只问 material scope 问题）
  → agent.update_tasks（2–12 项，只读）
  → use_mcp_tool
      或 agent.record_mcp_unavailable（成功、含 query/candidate_count/reason）
  → browser_setup
  → web_search
  → web_scan(page_kind=search_results)
  → [result_count 非零、缺失或不可信时]
      web_open_tab → web_scan(page_kind=source)
  → phase=verifying → 综合
```

`search_mcp_tools` 只搜索已保存描述，不启动 server。`project.list` 从项目根
开始；候选专业 MCP 存在且满足
启用、启动批准、catalog/schema 绑定和目标校验时，必须先使用它；没有可调用的
专业 MCP 时，`agent.record_mcp_unavailable` 必须成功保存已搜 query、候选数和
具体原因。只有这两条之一完成，才能进入 `browser_setup`。

真实浏览器的顺序仍是 MCP/不可用记录 → setup → search → 结果页 scan → 独立来源
open/scan。明确 `result_count = 0` 可以跳过独立落地页；其他情况必须访问和
扫描至少一个独立 HTTP(S) 来源。每次导航、重定向、点击或 material page change
都要重新扫描。每一个门只读取同一 run 中成功的持久事件或经过精确幂等校验的
复用结果；并发、失败、请求、dispatch 或模型声称不推进门。

## 5. 动态只读 task list

`agent.update_tasks` 接受 schema version 4、`expected_revision`、change summary
和 2–12 个 `AgentTaskV4`；`TaskListUpdated` 的 revision、change summary 和
列表是用户可见的进度投影。列表项只允许 `pending`、`in_progress`、`completed`、
`blocked` 等状态和有界标题/阻塞原因；同一列表最多一个 `in_progress`，已完成项
不得在后续 revision 中回退或删除；`expected_revision` 不匹配时拒绝覆盖，
要求模型/Host 重新读取最新 revision。更新本身是只读的编排操作，可以产生审计
事件，但不执行外部动作。

task list 的增删、重排和状态更新不等于计划修订，不产生
`ExecutionPlanV4`、Plan hash、`PlanProposed`、`PlanApproved` 或 approval scope，
也不解锁 runtime、写入、Network、委派或浏览器工具。它不改变
`RunModeV4`，不锁定会话，不代替完成 criteria，也不允许模型伪造 outcome。工具
是否可调用以及调用顺序始终由 Host 的 effect、capability、参数、项目边界和审批
检查决定。

## 6. 公开摘要而非私有 CoT

对话展示使用紧凑的执行过程折叠区：公开 `model_text` 作为进度行，`cycle_started`
作为模型轮次状态行，工具调用按首次事件 sequence 插入，后续 dispatch/outcome 合并到同一行。
读、写、编辑保留独立操作名，批次不能隐藏其中的单项调用。工具行提供状态、路径/命令、
可用的事件耗时与输出行数，输入输出独立展开。任务、阶段及批次汇总收进附加详情。
活动过程默认展开，历史过程默认折叠；向上阅读时暂停自动滚动，用户可返回最新。
该展示在 Windows/macOS 使用同一事件协议，不改变审批、恢复、凭据或数据库边界。

模型轮次可以写入 `ModelText`/`public_text`，向用户说明“已完成什么、
正在检查什么、下一步是什么或需要什么输入”。Provider 适配层只把公开内容映射
到该字段，不把 reasoning/thinking 专用字段作为文本事件；提示词要求这些摘要可被
普通用户理解。它们不应包含内部推理链、隐藏 system
prompt、逐 token reasoning、未验证结论、凭据、审批 token、工具调用 hash 或
调度器内部状态。

OmicsOps 不把 provider 的私有 reasoning/thinking 字段存储或展示为 CoT。事件、
checkpoint、归档和 UI 轨迹只使用公开摘要、有限工具结果、审计 metadata 与可验证证据；
`ModelText` 永远不能推进 route/discovery/completion gate。

## 7. 输入、崩溃与恢复

### 7.1 同 run 输入恢复

`agent.request_input` 的 `InputRequested` 记录问题和
`AgentInputReasonV4`（范围、决策、缺少数据或 blocker），随后 run 进入等待。用户
回答通过 `UserInputAnswered` 绑定原 question/run；Host 重新验证项目、会话、
execution kind、当前状态和权限后，以原 `run_id` 恢复。

恢复要求：

1. 不新建第二个 `RunCreated`，不换 project/conversation/model/spec，不重置
   task shape、phase、cycle 或 task list revision；
2. 已成功阶段保留 durable evidence；待处理动作只按 exact call、参数、scope
   和幂等语义决定是否重试或复用；
3. 用户输入是数据而非新权限，不能改变冻结 capability、审批边界、浏览器
   目标或 route；
4. 若回答不足，仍可再次请求输入；不能将 waiting 当作 completed。

### 7.2 崩溃恢复

进程重启、worker 崩溃或窗口重新打开时，Host 从事件 hash chain、run record 和
`ContextCheckpointV4` 还原最后的 shape、phase、task revision/tasks、cycle、
未解决错误和成功 outcome。新增 guided 字段只包含编排 metadata；checkpoint 中
既有的模型/工具摘要仍按原有模型输出和脱敏边界处理，不能被当作秘密存储通道。

- 已写入 `ToolFinished(succeeded=true)` 或精确匹配的
  `ToolOutcomeReused`：只复用，不为“重新开始”而重跑；
- dispatch 前崩溃：Host 可在重新授权后调度未开始的调用；
- dispatch 后没有 outcome：不能猜测成功；副作用调用进入
  `ToolDispatchUncertain`，等待人工 `ToolDispatchResolved`；
- 断线、CAPTCHA、权限不足、审批等待和 uncertain dispatch：保持原 run 的等待
  状态，修复后显式恢复；禁止新 run、模拟网页、普通 HTTP 或隐式放宽权限。

恢复期间，唯一权威状态是同一 run 的持久事件。迟到的旧 worker 事件、另一个
run/project 的事件、UI task list 或模型回显均被拒绝。

## 8. 完成门禁与失败语义

Host 只有在以下条件同时满足时才接受 `agent.complete`：

1. run 是完整性通过的 ordinary Agent Execute run，route、shape、当前 phase 和
   event chain 归属一致；
2. 当前 shape 的适用工作已完成；multi_step 的 discovery、Skill（有匹配时）、
   MCP/不可用记录以及研究浏览器的 search/result scan/independent source scan
   均有同 run 成功证据；
3. task list 的每一项均为 `completed`；若有 `pending`、`in_progress` 或
   `blocked`，run 只能等待输入或进入 needs-attention；同时没有未回答输入、未
   决定审批、活动批次或 unresolved dispatch；
4. proposal schema、非空用户可见 Markdown、每条 frozen completion criterion
   的 evidence、确定性 verification 和既有 Reviewer 规则均通过。

顺序错误、工具失败、拒绝、dispatch、过期/跨 run 事件和模型说“完成了”只能
产生可恢复失败或 waiting/needs-attention 状态，不推进 completion。若 blocker
仍未解决，run 应报告具体 blocker；不能用空答案或不完整证据落成 `RunCompleted`。

## 9. Plan、不变量与安全边界

### 9.1 Plan 保持不变

Plan 仍使用 `RunModeV4::Plan`、现有 `ExecutionPlanV4`、revision/hash、只读
MCP dynamic authorization 和批准/修改/取消流程。Plan 阶段不产生 ordinary 的
task shape、phase/cycle/tool-batch guided events，不启动真实浏览器，不使用
普通 Agent discovery；批准 Plan 后仍按冻结 `approved_plan` capability、compute
approval、spec hash 和恢复契约执行。批准 Plan 不授予浏览器权限，也不把计划转成
ordinary Agent。

旧的批准计划规格缺少 ordinary execution kind 时仍按兼容规则读取为
`approved_plan`；只有 direct run 的完整性通过且 execution kind 明确为
`ordinary_agent`，本设计的提示词和门禁才生效。

### 9.2 安全、审批和凭据

- Host 是 capability/effect、项目路径、参数、输入来源、审批和事件归属的唯一
  权威；模型、Skill、Memory、MCP description、网页和工具输出全部按不可信数据
  处理。
- Read-only discovery 可并行只在每个调用都已声明 read-only 且相互独立时成立。
  Runtime、Mutating、Network、Delegation 仍逐调用检查；task list、public
  summary 或 `Full Access` 不能授予额外权限。
- MCP/写入/运行时/浏览器的审批保持 exact call、目标、schema/catalog、会话、
  协议、项目/会话 scope 绑定；拒绝和过期审批 fail closed。审批等待保持原 run，
  不以模型重试或 task list 更新绕过。
- API key、密码、cookie、SSH 私钥和其他凭据只从现有 keyring/Windows
  Credential Manager（或对应安全 vault）读取；SQLite、事件、日志、checkpoint、
  task list、模型上下文、导出包、Git 和项目产物只保存脱敏引用或 metadata，绝不
  保存秘密正文。

### 9.3 浏览器边界

只有普通 Agent 的 research retrieval 可以使用受控真实浏览器。浏览器仍须经过
`browser_setup`、loopback bridge、固定 session/protocol/extension capability、
当前 run/project ownership 和独立 browser approval；Plan 和 task list 都不能
间接打开它。只允许安全 HTTP(S) 目标、受控的 tab/scan/search 操作和项目相对
资产；不提供任意 HTTP 代理、扩展安装、任意 eval 或凭据读取。

网页内容和导航结果是不可信输入。禁止向 ChatGPT、Gemini、Claude、Copilot、
Perplexity 等网页 AI 自动填写、发送、轮询或读取回答；CAPTCHA、MFA、登录和
系统自动化权限必须由用户人工处理并恢复原 run。截图/下载只保存项目相对路径、
大小和 SHA-256 等最小 metadata，SQLite/事件不保存图像或完整正文；run 结束只
清理该 run 自己创建的 tabs。Windows 使用后台子进程策略且不弹控制台，macOS
遵循系统辅助功能授权；两平台均只接受 loopback，不提权绕过安全边界。

## 10. 测试、验收与限制

### 10.1 确定性测试

自动化测试必须使用 fake model、fake ToolPort、内存/临时 EventStore、固定时钟
和临时项目目录，不连接真实 SSH/GPU/SLURM/WSL、浏览器、MCP server、API key、
模型网络或网页。至少覆盖：

- fast/multi_step 选择、来源记录和仅 `fast → multi_step` 迁移；
- route 先行、adaptive 三路 discovery 并行、research
  `search_mcp_tools` 先行、Skill 命中/无命中、MCP 可用/不可用和浏览器独立来源
  门禁；
- phase/cycle/tool-batch 事件顺序、ID/sequence/hash 归属、批次统计和失败不推进；
- task list revision 冲突、动态更新、只读不授权、普通 Agent 与 Plan 隔离；
- public summary 只持久化公开内容且不泄露 CoT/凭据；
- 输入恢复复用原 run、崩溃恢复复用成功结果、uncertain dispatch 的人工核验；
- completion gate 对 pending/approval/uncertain/缺少证据和完整 evidence 的正负
  案例；
- Windows/macOS 路径、后台进程和浏览器安全约束的解析/策略测试。

先运行与变更最接近的 Rust/前端测试，再运行仓库默认检查：

```powershell
cargo test --workspace
npm test
npm run build
```

### 10.2 真实验收边界

真实模型、MCP、浏览器、Windows/macOS 安装、CAPTCHA/断线恢复和真实科研证据必须
按一次性 acceptance 环境执行，并记录 OS、浏览器/扩展版本、模型精确 ID、实际
命令/日志、project/conversation/run ID 和可复核 artifact/source。对应测试若为
`#[ignore]`，未显式执行时状态只能写“未执行”或“阻塞”；fake 工具通过、静态
检查、编译成功和模拟事件不能描述为生产端到端验收通过。

本文只规定边界，不声称当前工作树已完成真实模型或浏览器验收。
