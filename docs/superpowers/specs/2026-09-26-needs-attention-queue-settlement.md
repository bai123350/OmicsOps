# NeedsAttention 后恢复原会话队列

## 问题与范围

上下文预算耗尽会产生 `RunNeedsAttention`。运行域把该事件视为终态，禁止 resume；队列域却把 `needs_attention` 永远视为 active，并保留队列项为 running。结果是原会话中后续已接受的消息永远无法执行。修复应保留旧 run、冻结配置、审批、工具证据和 NeedsAttention 提示，通过现有 FIFO 启动新的 run，不重放旧 run。

## 无未决副作用时的安全结算规则

在 store 内集中定义 NeedsAttention 是否安全结算的判断：事件链必须能通过现有原始 JSON/hash/sequence 校验，事件所属 project/conversation/run 必须全部匹配，必须存在唯一 `RunNeedsAttention` 终态，且 `has_unresolved_side_effect_dispatch` 为 false。不能通过错误文案匹配来判安全。缺失事件、损坏链、跨作用域事件、仅有 status、未完成的非 ReadOnly 派发以及无分类的孤立 Uncertain 标记均不能判为安全。已有 NeedsAttention 行没有事件的测试应继续通过。安全结算不是普通新请求推进的唯一条件：已确认停止的运行适用下文的独立调度规则，其未知副作用仍保留。

安全结算只把对应 queue item 改成 `failed` / `RunFailed`（表示此次队列执行未完成），run 仍然是 `needs_attention`，不能改成 completed/cancelled，不写新的终态，不变更 RuntimeJob 状态。已确认提交的后台作业可能仍在远端运行；后续 run 必须使用保留的 job 身份查询，队列结算不表示远端取消。

## 实施边界

1. `crates/omicsops-store/src/composer_queue_dispatch.rs`：事件观察与 reconcile 使用完整持久化事件链判定安全 NeedsAttention，并结算旧 running/uncertain queue 行。claim 与 commit 的 active 判断须复用同一安全检查；不能简单从 SQL status 列表删除 needs_attention。扫描会话所有 NeedsAttention run，包含没有 queue 行的旧直接启动 run。普通 planning/running/input/approval 仍阻塞。
2. 终态事件先落库、run status 后保存的窗口中，queue 可以先结算，但 `running` 状态继续阻塞下一次 claim。driver 只有在状态与终态一致且事件安全时推进。重启若留下 running 状态+终态证据，复用宿主在 run ownership lease 下的 `repair_agent_run_terminal_status_v4`；store 的普通 reconcile 不越过 ownership 擅自改运行状态。
3. `src-tauri/src/composer_queue_driver.rs`：NeedsAttention 的 settlement 取完整事件链判断；安全返回推进，未知或不安全停止。下一次派发前等待前驱 run 的 OS ownership lease 释放，避免终态保存和浏览器清理尾部与下一任务重叠；replacement 已有同类保护可复用。队列 scope lease 继续串行化 FIFO。
4. `src-tauri/src/agent_v4.rs`：正常结束的安全 NeedsAttention 应能触发已有 queue kick。已有 `composer_queue_reconcile` 每三秒读取并 kick，足够恢复旧数据库已存在的四条 pending，无需新命令、迁移或重建消息。claim/commit 必须再次检查状态与证据，不能只依赖先前 UI/driver 快照。
5. `crates/omicsops-store/src/composer_replacement.rs` 的 other-active 检查也不能被历史安全 NeedsAttention 永久阻塞；复用同一检查并排除精确 target。replacement 仍只允许替换当前 active run，不允许把已终态 NeedsAttention 伪装为 active。已提交 replacement 的旧 target 若安全 NeedsAttention 终止，可以通过同一安全结算判断；未确认副作用仍阻塞。
6. `src/features/workspace/WorkspaceShell.tsx`：header 不再无条件显示 `t.status` 的“远端分析运行中”，按当前 run/terminal/waiting/idle 的真实状态显示。正常 pending 时显示“加入队列”，无 backlog 且无 active run 时恢复普通发送。历史未决结果使用下述阻塞投影；最终是否可派发仍由宿主检查。

## 终态后的核实记录

当前 protocol 的 `validate_terminal_position_v4` 与 store append 都只允许 terminal 后追加一次 BrowserTabCleanupRequired；现有 `agent_v4_resolve_uncertain` 因此不能给 terminal run 追加 ToolDispatchResolved。本修复不改变该限制。安全上下文超限恢复和已确认停止后的新请求均不依赖放宽该协议；没有确认停止的不确定副作用继续阻塞。若后续支持终态后的人工核实，必须同步修改 protocol/store：只允许引用本 run 已存在未解决 Uncertain call、非空证据、一次性 resolution 的审计事件；仍禁止新 ToolRequested/DispatchStarted/ModelRequested/第二终态。核实不能 resume 旧 run、声称远端取消或自动重试旧调用。

## 确定性验证

- 新 terminal NeedsAttention + 完整正确链 + 无副作用：queue 失败结算，run 与证据不变，下一 pending 可 claim/commit。
- 从磁盘重开旧数据库：running queue + needs_attention run + 四条 pending，reconcile 幂等且 FIFO 顺序/消息与run身份不变。
- 无 queue 的旧安全 NeedsAttention 不阻塞；仅 status+空事件仍阻塞；坏 hash、跨作用域、未完成 Runtime/Mutating/Network/Delegation、孤立 Uncertain 均阻塞。
- 已 Finished/OutcomeReused/Resolved 的派发允许安全结算，ReadOnly 未完成不制造外部副作用；保持远端 job 内容与状态不变。
- terminal 先于 run status 时不推进；状态一致后仅一个 scope owner 派发；前驱 run lease 未释放时不启动下一 run。
- input/approval/active Plan/未观察 Stop/uncertain replacement 的既有栅栏保持有效。
- UI：terminal 真实状态、完成队列行不再制造 backlog、四条真实 pending 仍按队列提交；避免只断言文案而没有 store/driver 派发测试。

按 AGENTS.md 先近邻测试，再 workspace Rust、npm test、Web build、desktop build；真实模型/SSH 验收按 acceptance 文档单独记录，不能用替身通过冒充。

## 实施边界记录

store 对 NeedsAttention 的可推进性只读取持久事件 JSON 与其索引列，并校验事件链、作用域、唯一终态和未解决副作用。安全结算将关联队列行标成 failed/RunFailed，run 的 needs_attention 状态和事件保留。claim 与 commit 每次重新检查会话中所有历史 NeedsAttention run，旧直接运行无队列行也适用。运行状态仍是 running 时继续阻塞，即使终态事件已经落库。

桌面 driver 在提交下一队列运行之前获取此前队列 run 与历史 NeedsAttention run 的 OS 所有权锁；replacement target 只获取一次。锁仍被旧 driver 持有时释放当前队列准备 lease 并重试。真实远端作业状态不参与队列结算，也不会被自动取消或重新派发。terminal 后追加人工核实事件的协议限制保持不变。

## 2026-10-21：停止后仍显示“加入队列”

以下为第一次修复记录；它只纠正了状态显示，仍要求分叉才能继续，未满足已停止后在原会话发送新请求的需求。最终行为以下一节为准。

实际故障是运行已保存 `needs_attention`，但对应队列项仍为 `running`。Stop 已观察到，并不表示工具副作用或远端计算已经结束。队列终态观察和 reconcile 将不安全的 NeedsAttention 投影为 `uncertain`，保留原 run、事件、消息、冻结配置和 FIFO 屏障；旧数据库中的 running 行在常规 reconcile 时修复，无需迁移。证据不足时不能退回 pending 或自动重试。

界面把 `uncertain` 与普通忙碌/排队分开，显示“待核实”和阻塞说明，不再接受必然无法派发的普通发送（按钮和 Enter 均拦截）。文本框保持可编辑，草稿和附件保留。检查范围是当前项目/会话的全部历史事件，因为收到终态后 activeRunId 已清空；发现 NeedsAttention 未决副作用可在旧队列快照更新前先行显示阻塞。只读派发不制造副作用，已有 Finished/OutcomeReused/Resolved 的合法记录不视为未决；宿主 uncertain 结果始终优先，前端不据此放宽宿主派发条件。

阻塞提示提供显式“分叉并发送”，复用现有分支事务和材料校验；只有用户点击后才创建新会话并发送草稿，未确认成功前保留草稿。分支只延续文本，不复制旧运行执行权，不自动重试旧工具，也不声称远端计算已取消。缺少分支能力时可使用已有新建会话入口。终态后人工追加核实事件仍未支持，不新增无效的核实表单。

新增回归覆盖真实 Stop 的不安全终态、旧 running 行修复、持续阻断下一项、合法安全证据结算、历史事件触发阻塞、未知队列结果、Enter 禁发与草稿保留、显式分叉、正常取消、只读例外和跨会话隔离。手工 smoke：停止 SSH 工具后检查“待核实”及草稿保留；点击“分叉并发送”后确认新会话身份、旧事件不变；正常无副作用停止应恢复“发送”。确定性测试不要求真实 SSH/API；未执行真实 SSH smoke 不等于已通过。

本次验证：新增前端场景先复现 4 项失败，修复后通过；Store 新场景先复现 2 项 Running/Uncertain 状态断言失败，修复后相关 4 组共 66 项通过。最终 `npm run build`、`npm test`（1,504 项前端、22 项浏览器扩展）、`cargo test --workspace`（1,994 通过、20 ignored）、`cargo fmt --all -- --check`、`git diff --check`、`npm run build:desktop` 均通过。Windows release 与 NSIS 本地构建成功，未安装或发布。真实模型/SSH 验收及原任务重跑未执行。

## 2026-10-21：已确认停止后在原会话发送新请求

运行结果是否已核实，与本地 Agent 是否已停止并释放会话调度占用，是两个不同事实。用户明确停止的运行不可恢复或自动重放；但其遗留的未知结果不应永久禁止用户在原会话发起新请求。

宿主新增独立的队列推进判断，不改变 `safe_needs_attention_run` 的“副作用已安全结算”语义。允许推进的 attention 运行必须具有可信完整的原始 JSON 哈希链、匹配的索引与项目/会话/run 作用域、唯一 NeedsAttention 终态，并满足以下之一：

- 没有未决副作用，沿用已有安全结算规则。
- run 状态与 NeedsAttention 终态一致，且存在精确同作用域、有效 request ID 的持久化 `observed` Stop 回执。

只有 Stop 意图、未观察到的 requested 回执、缺失或损坏事件链、跨作用域回执、矛盾终态、仍在运行的状态不能解除占用。claim 与 commit 在各自事务内复查；派发前仍必须拿到前驱 run 的 OS 所有权锁。中断并替换仍保留原有目标与安全结算限制，不借普通新请求的规则恢复旧执行。

可推进的旧 queue 项记为 failed，原 run 仍是 needs_attention，旧事件、Stop 回执、RuntimeJob、冻结配置和审批保持不变。Stop 的 observed 保存晚于终态事件，因此 driver 每轮 claim 前重新 reconcile；重启或新提交也能修复旧 uncertain/running 行。新请求使用独立 run/message ID、新的冻结配置和审批，以及按 run 隔离的 RuntimeManager。任何路径均不得重放旧 call，也不能声称远端计算已取消。

直接执行新请求的冻结计划附带有界、宿主生成的停止边界上下文；Plan、直接执行和批准后执行的共享模型环境上下文也保留同一边界。内容仅包含必要的旧 run 身份与状态事实，不复制旧工具代码、参数或输出，不修改已接受的 reference_context，不改写旧计划或审批。模型必须将当前消息视作新请求，不把历史目标当作待继续的旧任务；如新请求涉及未知结果，优先只读核对，不能自动重试旧操作。

会话快照新增向后兼容的 `composer_blocking_run_ids`，由宿主在同一读取事务中检查所有历史 attention 运行，与正式队列使用相同规则。新版前端以此字段为权威，缺少字段的旧宿主保守回退到原事件判断。仍为 uncertain 的队列项须等宿主 reconcile，不能仅靠前端清空标志放行。显式刷新同时刷新队列、会话事件和宿主快照，覆盖没有 queue 行的旧直接运行。失败或过期响应不能清空现有阻塞、覆盖新会话或丢失草稿。

用户可在停止确认后使用原会话的“发送”或 Enter；不要求分叉、不增加人工核实表单。旧运行的“需要处理”保留在历史中，不能覆盖新运行的实时状态。未确认停止或证据损坏时继续明确提示真正的阻塞原因。

验收必须覆盖实际 claim、commit 和新 run 执行，不能只断言按钮可点击或 mock enqueue 被调用。确定性场景包含：旧未决运行 observed Stop 后同会话新 Agent 请求完成，旧事件/状态/回执不变且旧工具不被重放；旧数据重开后的恢复；未观察 Stop、错误作用域、损坏链继续拒绝；无 queue 的旧直接运行；多个历史运行中仍有真实阻塞；Stop 清 activeRunId 后的普通发送、Enter、刷新失败与迟到响应；新请求的停止边界上下文和冻结材料校验。真实模型/SSH 验收另外记录，不以替身测试替代。

队列刷新回调仅在宿主快照实际应用后记住当前队列状态。若刷新期间的新事件令快照过期，保留原状态与 generation 检查，并在下一轮轮询重读；不能把被丢弃的快照视为刷新成功，否则自动启动的下一运行可能仍显示旧阻塞。

原生手工 smoke：打开带有 observed Stop 和旧未决工具记录的会话，等待一次常规刷新，确认按钮恢复“发送”；在原会话提交新请求并观察新 run ID，旧记录仍在历史中。重复验证 Enter 发送、重启后恢复，以及已有下一条排队消息时界面随新运行更新。仅 requested Stop 或损坏证据的会话应继续阻塞。真实 SSH 操作必须按 acceptance/README.md 使用一次性环境，不重试用户原远端命令。

最终确定性验证：`cargo test -p omicsops-store --test composer_queue_dispatch -- --nocapture` 为 26 passed；`cargo test -p omicsops-desktop --lib agent_v4::stopped_turn_tests -- --nocapture` 为 2 passed。整链回归使用脚本模型、真实宿主工具和临时项目，从领取/提交新请求到完成核验均通过；脚本使用冻结计划的 criterion，并仅对新成功工具事件引用证据。`npm run build`、`npm test`（1,511 前端、22 浏览器扩展）和 `cargo test --workspace`（2,002 passed、0 failed、20 ignored）均通过。原生 GUI smoke 和真实模型/SSH 验收未执行，ignored 不等于验收通过。

`cargo fmt --all -- --check`、`git diff --check` 和 `npm run build:desktop` 均通过；Windows release 程序与 NSIS 本地安装包已生成，未安装或发布。构建成功不等于原生界面或真实服务验收。
