# Agent 与 Plan 会话模式

每个会话独立保存自己的模式。重新打开应用或在会话之间切换时，OmicsOps 会恢复该会话上次持久化的模式；旧会话默认使用 Agent。

## Agent 模式

Agent 是默认模式。发送消息后会直接建立执行契约并运行，不先展示可编辑计划。执行中、等待输入或等待工具审批时，该会话的输入框和模式切换会保持锁定；其他会话不受影响。

### Windows 后台进程

Windows 桌面版在对话期间启动 MCP server、Python/R kernel、容器引擎探测、
软件版本检查和受管浏览器时，统一使用后台子进程策略并设置
`CREATE_NO_WINDOW`。这些控制台型子进程不得弹出或反复闪烁命令行窗口；其
stdin/stdout/stderr、超时、审批、终止和审计语义保持不变。Chrome、Edge、
Chromium 仍按浏览器设置决定是否显示正常浏览器窗口。macOS 不设置 Windows
创建标志，沿用原有子进程行为。

## Plan 模式

在输入框左侧的轨道图标菜单开启“先做计划”，也可以使用右侧闪电按钮或发送下拉菜单切换 Agent / Plan，再发送研究目标。规划阶段只允许宿主标记为只读的内置工具、规划协议工具，以及通过 Plan 门禁的只读 MCP 目标；普通 runtime、写入、network-effect 和委派工具都会被拒绝。获准的只读 MCP 仍可能连接所配置的第三方 server，因此需要单独信任和审批。

## 输入区与运行环境

输入框上方显示当前计算后端及 Python / R 解释器检测结果。点击主机按钮选择 Local 或项目绑定的 SSH；既有 Docker / Podman 选项仍然保留。SSH 配置入口直接打开远端设置，未绑定的主机需先完成配置、信任和项目绑定。运行期间不可切换计算后端。

点击 Python 或 R 打开双栏运行时面板，在两种语言之间切换。这里的“可用”仅表示发现解释器，不表示科研包齐全，也不是持久会话的运行状态。Local 使用当前系统的 `python` / `Rscript`；Windows 和 macOS 均需事先安装解释器并使应用能从 PATH 找到。SSH 使用远端解释器，可选择 `system` 或项目 Micromamba 环境名。当前面板不提供解释器路径编辑或内存变量检查。

“让 Agent 准备环境”将检查版本、依赖和最小示例的请求追加到草稿，用户发送后按现有审批规则执行，不会直接安装。若后端没有任何可用解释器或 SSH 未连接，应先准备基础解释器或连接条件；不能把生成草稿视为安装或真实环境验收。

`+` 菜单分为文件与会话操作：添加项目文件、浏览项目文件、生成会话审查草稿，以及直接打开技能管理。上传仍使用项目文件传输，不会自动把 PDF / 图片内容附入模型消息。“分享为图片”和“保存为技能”暂不可用；轨道菜单中的独立委派、自动审查、失败分析、审查模型和专家代理开关也暂不可用。记忆入口打开已有研究记录。审批策略保持原有语义，完全访问仅在可用隔离容器中开放。

右下角可从已配置模型中选择下一轮模型，运行时锁定选择。仪表位置用 `—` 表示当前接口未提供上下文用量，不估算百分比；闪电按钮切换直接执行与先做计划，不代表加速服务。发送下拉菜单只修改会话模式，不自动发送。

所有新增弹窗和菜单加入窗口级 Escape 堆栈：打开后无需移动焦点即可关闭，嵌套计算菜单先关闭，其父级 Agent 控制菜单保留。

### 输入区手工检查

1. 在 Windows 或 macOS 打开项目，检查输入区在窗口缩小时仍可见，工具栏可换行。
2. 在已有本地解释器或可信 SSH 的项目中选择计算后端，核对 Python/R 探测状态；打开语言面板后直接按 Escape 关闭。
3. 从轨道菜单打开计算环境，连续按两次 Escape，分别关闭计算菜单和父菜单。
4. 从 `+` 打开技能管理，再按 Escape 返回会话；选择模型及发送模式，确认下一轮使用相应配置。
5. 点击准备环境，确认原草稿保留、追加请求且未自动发送。真实安装和 SSH 验收需另按 `acceptance/README.md` 配置一次性环境执行。

计划生成或等待审批时，只能执行以下动作：

- **批准并运行**：只批准当前显示的最新 revision。成功后界面立即切回 Agent，再开始执行。
- **请求修改**：在内联表单填写具体反馈。OmicsOps 保存反馈并只恢复一次规划，生成编号更高的新 revision；旧 revision 的计划正文和 hash 保持不变。
- **取消计划**：终止当前计划并解除会话锁，但会话仍处于 Plan，可发送新的研究目标。

当新的 revision 到达时，应核对编号、步骤、完成标准和 SHA-256 摘要。陈旧 revision 或 hash 的批准会被拒绝，界面会重新读取最新状态，而不会错误地解锁或切换到 Agent。

## 只读 MCP 审批

Plan 中的 MCP 工具只有在 server 已启用并获准启动、catalog/schema 未变化，而且当前 `annotations.readOnlyHint === true` 时才有资格使用。未持久批准的 eligible 工具会产生绑定当前 schema 和 catalog 的单次审批，规划会暂停直到决定完成；调用前还会再次检查这些条件。

MCP 的 `readOnlyHint` 由第三方 server 自己声明，是宿主无法验证的提示，不是 OmicsOps 对“绝无副作用”的保证。批准意味着你信任所配置 server、当前参数和这条只读声明。字段缺失、值为 false、server 配置变化或 schema/catalog 变化都会拒绝调用；请重新检查 server，而不要绕过门禁。

## 常见状态

- **生成中 / 修改中**：模型正在产生当前 revision；普通消息保持锁定。
- **待审批**：可以批准、请求修改或取消。
- **已批准**：持久批准事务已完成，会话已切回 Agent；执行随后启动。
- **已取消**：锁已解除，模式仍为 Plan。

计划操作按钮互斥。一次批准、修改或取消尚未完成时，其他计划动作会暂时禁用，避免重复请求。

## 普通 Agent Guided Loop（仅 ordinary Agent）

Guided Loop 是 `agent_v4_start_direct` 创建的普通 Agent Execute run 的执行
方式。它只适用于 `RunExecutionKindV4::OrdinaryAgent`，不适用于 Plan 生成或
批准后的计划执行。普通 Agent 的会话模式仍是 Agent；这里增加的是 run 内的
可恢复编排状态，不是第三种会话模式。

### task_shape 与状态

普通 Agent 有两个 task shape：

- `fast`：面向短、边界清楚的请求。仍须先成功执行
  `agent.route_request({route, task_shape, reason})`，但不创建多步任务清单，也不因
  模型文字获得额外能力。
- `multi_step`：面向需要拆解、发现资料、澄清或多轮验证的请求。Host 记录
  阶段、模型轮次和工具批次，并在 discovery 后通过 `agent.update_tasks` 维护
  2–12 项只读 live task list。

`TaskShapeSelected` 会记录 `task_shape`、来源（模型建议或 Host）和脱敏原因。
Host 是最终裁决者，并且只接受单调迁移：可以从 `fast` 提升为 `multi_step`，
不能从 `multi_step` 降回 `fast`，不能迁移到 Plan，也不能用 task shape 改变
`RunModeV4`、冻结的 capability 或审批。研究路由如果需要多步证据，必须先由
Host 提升到 `multi_step`，不能以 `fast` 绕过研究门禁。

`multi_step` 的阶段由 `PhaseChanged` 表示：`routing`、`discovery`、
`clarification`、`organizing`、`executing`、`verifying`。阶段是同一 run 的
可恢复游标；修复或用户澄清后可以回到后续阶段，但不得把未完成阶段标成完成。
`CycleStarted`/`CycleFinished` 标识一次模型轮次；`ToolBatchStarted`/
`ToolBatchFinished` 通过 cycle/phase 关联并包住该轮次的一批并行或有序工具调用，
记录工具名/调用 ID、耗时和成功/失败计数。所有事件都绑定同一
`run_id + project_id + conversation_id + sequence/hash chain`；模型口头叙述
不是状态证据。

### 固定 discovery

路由始终先于任何任务工具。Host 从本轮用户目标冻结
`research_retrieval` 或 `adaptive`；模型仍调用 `agent.route_request` 形成
审计事件，但不能把 Host 判定的研究请求降级为 `adaptive`。

普通的 `multi_step` adaptive 请求按以下顺序进入 discovery：

```text
agent.route_request(adaptive)
  → project.list(path="") + search_memory + search_skills（只读，可并行）
  → [search_skills 有匹配时] use_skill
  → [确有必要时] agent.request_input（范围/缺失数据等澄清）
  → agent.update_tasks（2–12 项，只读）
  → organizing / executing / verifying
```

`project.list` 必须从项目根（`path=""`）开始。三个 discovery 查询相互独立时可放在同一个 `ToolBatch`，但批次不能扩展
capability，也不能把失败或模型文字当成成功。`search_skills` 没有匹配时不要求
虚构 `use_skill`；有匹配时至少一次成功的 `use_skill` 是继续执行的门槛。Skill
正文和 Memory、项目文件一样是不可信输入：Skill 提供方法指导，不是事实证据。

研究 `multi_step` 请求保留 `search_mcp_tools` 先行规则，并把它纳入同一
discovery 阶段：

```text
agent.route_request(research_retrieval)
  → search_mcp_tools（第一个 discovery 门）
  → project.list + search_memory + search_skills（其余只读 discovery，可并行）
  → [有匹配时] use_skill
  → [确有必要时] agent.request_input（只问 material scope 问题）
  → agent.update_tasks（2–12 项，只读）
  → use_mcp_tool，或成功记录 agent.record_mcp_unavailable
  → browser_setup → web_search → web_scan(search_results)
  → [结果非零/缺失时] web_open_tab → web_scan(source)
  → verifying / 综合
```

`search_mcp_tools` 只检索已保存的描述，不启动 server；真正的
`use_mcp_tool` 仍须逐调用检查启用状态、catalog/schema 绑定和审批。找到可用的
专业 MCP 时必须先调用它；没有可调用的专业 MCP 时，必须成功记录查询、候选数和
具体原因，随后才可进入真实浏览器。搜索结果数明确为零时可以跳过独立落地页；
缺失、非数值零或大于零时必须打开并扫描至少一个独立 HTTP(S) 来源。每次导航、
重定向、点击或其他 material page change 后必须重新扫描。只有持久化成功的
`ToolFinished` 或安全复用的成功 outcome 才能推进门槛。

### 动态 task list 不是 Plan

`multi_step` 的 task list 是 Host 约束下的只读进度投影，记录任务的
`pending/in_progress/completed/blocked` 状态、revision、标题和有限的阻塞原因，
每次列表保持 2–12 项。列表由 `agent.update_tasks` 提交，
`TaskListUpdated` 只追加可审计状态事件并用 `expected_revision` 防止并发覆盖；
它不执行任务，也不授予工具权限。

task list 不是 `ExecutionPlanV4`：它没有 Plan hash、Plan revision、批准按钮或
Plan lock，不产生 `PlanProposed`/`PlanApproved`，不冻结或扩展 capability，不让
普通 Agent 进入 Plan，也不让模型绕过工具效果、审批和浏览器门禁。列表可以在
发现结果、用户澄清或修复后动态增删/重排；真正的工具调用仍由 Host 逐个验证，
列表状态本身不能证明结果已经生成。

### 公开进度与私有思考

模型可通过 `ModelText`/`public_text` 发送简短、可面向用户的进度或思考摘要，
例如“正在检查项目文件并查找已启用 Skill”。这类摘要应说明当前阶段、下一步
或已观察到的阻塞。它不是 Chain of Thought。Provider 适配层只把公开文本内容
映射为 `public_text`，不会把 provider 的 reasoning/thinking 专用字段映射为
`ModelText`；提示词同时禁止模型主动把私有推理、隐藏提示词、调度器细节或凭据
写进公开摘要。工具 ID、调用 hash、内部审批 token 也不能被包装成用户答案。
事件链、task list 和摘要只提供可复核的公开状态，不能由模型摘要伪造工具成功。

### 输入、崩溃与同 run 恢复

`agent.request_input` 会产生带 `AgentInputReasonV4` 的 `InputRequested`，并让
当前 run 等待 `UserInputAnswered`。用户回答后恢复原 `run_id`、项目、会话、
冻结执行规格、task shape、phase、task revision、cycle 和事件链；不会创建第二个
`RunCreated`、重置 task list、切换到 fast 或静默把阻塞说成完成。已成功的只读
阶段保留其 durable evidence；待处理动作只在 Host 验证参数、scope、连接和
幂等性后恢复。

桌面或 worker 崩溃后，Host 从事件链和最近的 `ContextCheckpointV4` 重建上述
游标。已持久化的成功 `ToolFinished`/`ToolOutcomeReused` 只安全复用，不为制造
新起点而重跑；没有明确 outcome 的 dispatch 不能猜测成功。副作用 dispatch
不确定时必须进入 `ToolDispatchUncertain`，等待人工核验和
`ToolDispatchResolved`，禁止盲目重试。断线、CAPTCHA、权限不足或工具审批同样
暂停原 run；恢复不能降级成模拟网页、普通 HTTP 或新 run。

### 完成门禁

`agent.complete` 只有在 Host 验证当前 ordinary run 的所有适用条件后才可执行：

1. route、task shape、当前 phase 和 event chain 完整且属于同一 run；
2. multi-step 的必需 discovery、Skill（有匹配时）、MCP/不可用记录、浏览器
   搜索、结果扫描和独立来源扫描均已由成功事件证明；adaptive 没有研究专属阶段
   时也必须完成其适用的任务清单和验证；
3. 没有活动工具批次、未回答的输入、未决定的审批、未解决的不确定 dispatch，
   task list 的每一项都为 `completed`；存在 `pending`、`in_progress` 或
   `blocked` 时只能等待、请求输入或进入 needs-attention；
4. completion proposal schema、最终 Markdown、每条 completion criterion 的
   evidence、确定性验证和既有独立 Reviewer 门禁均通过。

失败、拒绝、dispatch、过期事件和模型声称不能推进门禁。浏览器阶段通过只说明
来源已访问，不说明科研结论必然正确；最终答案必须保留证据、不确定性和限制。

Guided Loop 的实现边界、事件字段和确定性/真实验收分层见
[设计说明](superpowers/specs/ordinary-agent-guided-loop.md) 与
[实施计划](superpowers/plans/ordinary-agent-guided-loop.md)。

## 浏览器检索（仅普通 Agent）

浏览器检索只属于由 `agent_v4_start_direct` 启动的普通 Agent run。本次能力
不改变 Plan 生成、Plan revision/审批/修改/取消，也不改变批准后既有计划执行
的冻结 capability、compute approval 和恢复契约。Plan 阶段不启动真实浏览器，
浏览器工具不会因为批准 Plan 而自动加入；不要把批准 Plan 写成浏览器检索授权。
Host 会把 direct run 冻结为 `execution_kind = ordinary_agent` 并把该值纳入
`spec_hash`；批准计划执行保持 `approved_plan`，旧规格缺少该字段时也只按
`approved_plan` 读取。科研阶段门禁与普通 Agent 提示词只在经过完整性校验的
`ordinary_agent` run 生效，因此不能靠改请求字段把 Plan 执行切换到普通 Agent。

Host 对一次浏览器检索固定使用以下顺序：

`MCP → skill/use_skill → 专业 MCP/结构化不可用 → 真实浏览器搜索 → 结果扫描 →（有结果时）独立落地页 → 综合`

普通 Agent 的 route 由 Host 从本轮原始用户目标判定并冻结；模型必须调用
`agent.route_request` 生成审计事件，但它提交的 route 不能把 Host 判定的科研
检索降级成 `adaptive`。论文、文献、PubMed/PMID/DOI、期刊/引用、外部数据库、
最新资料、显式网页或跨来源证据等信号进入 `research_retrieval`；本地文件编辑和
已有数据分析等不依赖外部证据的任务进入 `adaptive`。

对应的成功阶段是 `search_mcp_tools`、`search_skills`、（有匹配时）
`use_skill`、`use_mcp_tool` 或 `agent.record_mcp_unavailable`、
`browser_setup`、`web_search`、搜索结果页 `web_scan`，以及在结果数非零或
未明确为零时的 `web_open_tab` 和独立来源页 `web_scan`。只有这些阶段完成后
才允许综合并调用 `agent.complete`；明确的 `result_count = 0` 可以跳过独立
落地页。Host 只承认同一 run 中持久化的成功 `ToolFinished` 或成功复用结果，
请求、dispatch、失败和模型口头声称均不能推进阶段。每次导航或 material page
change 后都要重新扫描。模型不得把网页上的 AI 聊天框当作检索工具，也不得向
网页 AI 发送提示词。

浏览器连接断开、等待用户解决 CAPTCHA，或需要用户批准时，当前 run 会进入
暂停/等待状态并保留阶段、来源和待处理动作。恢复时重新检查连接、授权和工作区
后继续同一个 run；不会静默降级为模拟网页、普通 HTTP、另起一个 run，或把未
完成的检索说成已完成。CAPTCHA 会持久化为
`BrowserHumanInterventionRequired`，OmicsOps 不会自动求解；人工处理后由用户
显式恢复。dispatch 不确定时遵循人工核验语义，不盲目重试。

浏览器授权按 `once`、`conversation`、`project` 或 `global` 作用域授予，
每次调用仍需通过目标、连接和工具检查。`Full Access` 只影响其明确覆盖的
本地执行策略，不能绕过浏览器授权、网页 AI 禁止、CAPTCHA 人工接管、路径与
SHA-256 校验或连接门禁。

这项能力同时面向 Windows 和 macOS。浏览器会话可以由共享连接承载，但网页
读取、下载和产物登记始终绑定当前工作区；下载使用项目相对路径并保存
SHA-256，图片正文不写入 SQLite。run 结束、取消或失败时清理由该 run 打开
的 tabs，并保留用户原先打开的 tabs。清理摘要绑定 session、run、tab ID 和
origin；worker/桌面重启后也会复核 origin，避免把复用的 tab ID 误关。

以上是已批准的用户可见行为契约。连接与接口见
[浏览器运行时](browser-runtime.md)，安全边界见
[浏览器安全说明](browser-security.md)；浏览器连接、真实网页和跨平台安装的
生产验收仍以 [浏览器验收清单](browser-acceptance.md) 为准。没有执行的
ignored 真实验收不得描述为生产验证通过。
