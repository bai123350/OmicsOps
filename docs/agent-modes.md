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

在输入框左侧的 `+` 菜单选择 Plan，再发送研究目标。规划阶段只允许宿主标记为只读的内置工具、规划协议工具，以及通过 Plan 门禁的只读 MCP 目标；普通 runtime、写入、network-effect 和委派工具都会被拒绝。获准的只读 MCP 仍可能连接所配置的第三方 server，因此需要单独信任和审批。

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
