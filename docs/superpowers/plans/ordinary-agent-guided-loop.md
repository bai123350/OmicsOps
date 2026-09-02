# 普通 Agent Guided Loop 实施计划

> 日期：2026-09-02
> 状态：实现计划；完成状态必须以实际测试和一次性真实验收记录为准。

## 1. 范围闸门

本计划只实现 direct ordinary Agent（`agent_v4_start_direct`、
`RunExecutionKindV4::OrdinaryAgent`）的 `fast`/`multi_step` guided loop。实现、
review 和验收期间必须保持以下不变量：

- 不修改 Plan 会话模式、`ExecutionPlanV4`、Plan revision/hash、
  `agent_v4_start_planning`、批准/请求修改/取消或批准后的 `approved_plan` 执行；
- 不把动态 task list 当作 Plan，不引入 Plan approval、Plan lock、Plan capability
  freeze 或浏览器授权；
- Host 只允许 task shape 的初始选择以及单调 `fast → multi_step`，拒绝
  `multi_step → fast`、ordinary/Plan 互换和通过模型文字改权限；
- 所有恢复复用原 `run_id`、project、conversation、spec 和事件链；不创建第二个
  run，不把等待/失败说成完成，不盲目重放不确定副作用；
- 自动化测试不要求真实 SSH、GPU、SLURM、WSL、MCP server、浏览器、API key、
  模型网络或网页；真实验收另行执行并明确写“未执行/阻塞”。

## 2. 协议、事件和持久化契约

### Step 0 — 基线与兼容性

保存现有 Agent/Plan 的 descriptor、prompt、事件序列和 completion gate 快照，
作为回归基线。确认新增字段和事件是 additive、可被旧事件读取，旧的批准计划
缺少 execution kind 时仍默认为 `approved_plan`。检查事件 hash、项目/会话归属和
序列号校验不会被 UI 任务列表或迟到 worker 绕过。

### Step 1 — task shape 与 Host 状态机

1. 使用 `AgentTaskShapeV4::{Fast, MultiStep}` 和
   `AgentTaskShapeSourceV4::{Model, Host}` 记录 shape 和脱敏原因。
2. 在 ordinary direct run 的入口建立默认/初始 shape；让 Host 最终裁决，并只
   接受 `fast → multi_step` 的提升。研究请求若需要固定证据流程，在进入 discovery
   前必须为 `multi_step`。
3. 使用 `AgentPhaseV4` 记录 routing、discovery、clarification、organizing、
   executing、verifying；阶段变化、cycle/model round 和批次边界都通过 durable
   事件，而不是内存变量或 UI label 表示。
4. 将 shape、phase、task revision/tasks、cycle 写入
   `ContextCheckpointV4` 的可选字段；不得把私有推理放进 checkpoint。

完成标准：模型提出非法 shape 或逆向迁移时 fail closed；Plan/approved-plan 的
 现有事件和 descriptor 快照无行为变化。

## 3. Guided loop 与 discovery

### Step 2 — 普通 multi-step 循环

实现以 phase → model cycle → tool batch → durable outcomes → next phase 为边界
的循环。成功返回的模型轮次以可关联的 `CycleFinished` 收口；崩溃、取消或模型
错误留下的开放 `CycleStarted` 是恢复时可识别的中断边界。每个
`ToolBatchStarted` 必须绑定相同 run/cycle/phase；正常收口时由
`ToolBatchFinished` 汇总工具名、调用 ID、耗时和成功/失败数，缺少 finished 的
开放批次在恢复时保持中断或 uncertain，而不是猜测成功。成功推进只读取
同一 run 中的 `ToolFinished(succeeded=true)` 或精确安全复用。

fast 不强制创建 task list，但仍执行 route、工具校验、审批、科学状态和完整
completion gate；需要拆解或研究证据时只能由 Host 提升到 multi_step。

### Step 3 — adaptive discovery

在成功 `agent.route_request(adaptive, task_shape=multi_step)` 后进入 discovery，一批可并行执行相互
独立的只读调用：

```text
project.list + search_memory + search_skills
```

`project.list` 从项目根开始；若确有必要，只能在 discovery 完成后提出 material
scope 的 `agent.request_input`，再用 `agent.update_tasks` 创建 2–12 项只读 live
task list。`search_skills` 返回匹配时，至少一次成功 `use_skill` 后才能继续；无
匹配时不虚构调用。把结果作为不可信输入交给 organizing/executing，不让 discovery
结果扩大 capability 或跳过 approval。

### Step 4 — research discovery 与浏览器顺序

research 必须保持：

```text
search_mcp_tools
→ project.list + search_memory + search_skills（可并行）
→ use_skill（有匹配时）
→ [必要时] agent.request_input（material scope）
→ agent.update_tasks（2–12 项，只读）
→ use_mcp_tool 或 agent.record_mcp_unavailable
→ browser_setup → web_search → web_scan(search_results)
→ 独立来源 web_open_tab → web_scan(source)
```

`search_mcp_tools` 是第一个 discovery 门；不启动 server。MCP 必须先于真实浏览器，
没有可调用专业 MCP 时的 unavailable 记录要含 query、candidate_count 和具体
原因。明确结果数为零时可以跳过独立来源，否则独立来源必须打开并扫描；导航后
重新扫描。所有 browser/MCP action 仍由各自 exact approval、session、target、
schema/catalog、protocol 和 run ownership 复核。

完成标准：顺序错误、并发跨越依赖、失败 outcome、模型口头成功、跨 run 事件和
旧 approval 都不能推进阶段；adaptive 不会意外执行 research-only 浏览器门。

## 4. Task list、公开摘要与用户交互

### Step 5 — 动态只读 task list

实现 `agent.update_tasks` 的 2–12 项有界 `AgentTaskV4` 列表和 `TaskListUpdated`
revision/expected_revision 保护。
列表更新只追加只读状态事件，可在发现、澄清、组织和修复阶段动态调整；不能执行
外部动作、授予工具、改 RunMode、创建 Plan revision 或替代 completion criteria。
并发 revision 冲突应要求读取最新状态后再提交。

### Step 6 — 公开思考摘要

保留 `ModelText`/`public_text` 作为简短用户进度和公开思考摘要；Provider 适配层
不把 reasoning/thinking 专用字段映射成公开文本，提示词禁止模型主动输出凭据、
隐藏提示词、工具 hash、内部调度或私有 CoT。Host 不声称能对模型主动写入公开
文本的任意语义泄漏做可靠自动识别，因此这些摘要仍按模型输出处理，而不是秘密
存储通道或事实证据。模型摘要只能改善可见性，不能成为阶段/完成证据。

### Step 7 — 输入等待与同 run 恢复

为 `InputRequested` 保存 `AgentInputReasonV4`，绑定 question/run，并在用户回答
后恢复相同 `run_id`、phase、shape、task revision、cycle、spec 和事件链。回答是
不可信数据，不得改变 capability、审批或浏览器目标；不足时再次等待。

完成标准：事件数、RunCreated 数、project/conversation/spec、已成功阶段和
task-list revision 在恢复前后连续；不会重复成功只读副作用或静默新建 run。

## 5. 崩溃、审批和完成门禁

### Step 8 — 崩溃与 uncertain dispatch

从 durable event chain、run record 和 checkpoint 恢复当前阶段、批次和列表。已
成功 outcome 只复用；dispatch 前未执行的调用需重新授权后才能派发；dispatch 后
无 outcome 的副作用调用写入 `ToolDispatchUncertain`，要求用户核验并记录
`ToolDispatchResolved`。断线、CAPTCHA、权限不足和 approval wait 保持原 run，
不切换模拟浏览器、普通 HTTP 或放宽权限。

### Step 9 — 完成门禁

在 `agent.complete` 入口集中验证：

1. ordinary execution kind、route、shape、phase 和事件链完整；
2. 当前 shape 的任务完成；multi_step 的 discovery/Skill/MCP/浏览器/独立来源
   成功证据完整；
3. task list 的所有项为 `completed`，没有活动 batch、未回答输入、未决定 approval
   或 unresolved dispatch；
4. completion proposal、用户可见 Markdown、criteria evidence、deterministic
   verification 和独立 Reviewer 规则通过。

不满足条件时返回可恢复的 Host rejection 或 waiting/needs-attention，并追加
审计事件；禁止 `RunCompleted` 写入空答案、不完整 evidence 或仅凭模型文本。

## 6. 安全、平台和浏览器检查

### Step 10 — 安全回归

- 每个工具按 effect、参数、项目路径、目标、scope 和精确审批重新校验；task list、
  public summary、Full Access 和模型选择不能扩大权限。
- 凭据只走现有 keyring/Windows Credential Manager/安全 vault；SQLite、事件、
  日志、checkpoint、模型上下文、task list、导出和 Git 只保存脱敏引用/metadata。
- 浏览器只允许 ordinary research 的受控 loopback bridge、固定 session/protocol、
  当前 run tab ownership 和独立授权；禁止任意 HTTP、proxy、eval、扩展安装、网页
  AI prompt、凭据读取及 CAPTCHA/MFA 自动绕过；截图/下载只保留相对路径、大小、
  SHA-256。
- Windows 继续使用 `CREATE_NO_WINDOW` 启动控制台型子进程；macOS 遵循辅助功能
  授权；两平台 fail closed，不提权、不杀用户浏览器、不关闭非 run-owned tabs。

### Step 11 — Plan 与旧行为 smoke

用现有 Plan 测试确认：Plan descriptor、read-only MCP/schema/catalog approval、
revision/hash、批准/修改/取消、approved-plan execution 和恢复保持不变；Plan
不会生成 guided task list/phase/cycle/batch 事件或启动浏览器。ordinary fast 的
最小请求、ordinary research 和 ordinary multi-step 的基线分别跑通。

## 7. 验证与真实验收分层

### Step 12 — 确定性检查

使用 fake model/ToolPort、内存 EventStore、固定时钟和临时目录，覆盖 shape 单调
迁移、phase/cycle/batch 事件、adaptive/research discovery、并行和依赖、Skill
命中、MCP unavailable、浏览器独立来源、task-list revision、公开摘要/CoT 隔离、
同 run 输入、崩溃/uncertain、完成门禁、Plan 隔离和凭据脱敏。先运行近邻包测试，
交付前运行：

```powershell
cargo test --workspace
npm test
npm run build
```

### Step 13 — 一次性真实验收

在 `acceptance/README.md` 指定的一次性环境中单独执行真实模型、MCP、浏览器和
Windows/macOS 安装/恢复/安全负面案例。每次记录 OS、浏览器/扩展版本、精确模型
ID、命令、日志、run IDs、来源和 artifact SHA；ignored 测试未显式执行时保持
“未执行/阻塞”。不得把编译、fake tool、静态检查、模拟事件或本实施计划描述为
生产 E2E 验收。

## 8. 回滚与已知限制

- 任一 event gate、shape 隔离、Plan 回归、审批、凭据脱敏或浏览器安全回归时，
  立即禁用 ordinary guided loop capability；保留历史事件和已校验资产，禁止
  destructive reset。
- 回滚只停止当前 run-owned work/browser tabs，不删除历史事件、授权或项目文件；
  未解决的 dispatch 和 human intervention 保持可见并可复核。
- 当前计划不声称真实模型、MCP、浏览器、Windows/macOS 安装或跨平台人工交接已
  完成；这些均须有实际命令和可复核记录后才能更新状态。
