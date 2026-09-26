# NeedsAttention 后恢复原会话队列

## 问题与范围

上下文预算耗尽会产生 `RunNeedsAttention`。运行域把该事件视为终态，禁止 resume；队列域却把 `needs_attention` 永远视为 active，并保留队列项为 running。结果是原会话中后续已接受的消息永远无法执行。修复应保留旧 run、冻结配置、审批、工具证据和 NeedsAttention 提示，通过现有 FIFO 启动新的 run，不重放旧 run。

## 安全结算规则

在 store 内集中定义 NeedsAttention 是否允许后续队列推进的判断：事件链必须能通过现有原始 JSON/hash/sequence 校验，事件所属 project/conversation/run 必须全部匹配，必须存在唯一 `RunNeedsAttention` 终态，且 `has_unresolved_side_effect_dispatch` 为 false。不能通过错误文案匹配来判安全。缺失事件、损坏链、跨作用域事件、仅有 status、未完成的非 ReadOnly 派发以及无分类的孤立 Uncertain 标记全部保持阻塞。已有 NeedsAttention 行没有事件的测试应继续通过。

安全结算只把对应 queue item 改成 `failed` / `RunFailed`（表示此次队列执行未完成），run 仍然是 `needs_attention`，不能改成 completed/cancelled，不写新的终态，不变更 RuntimeJob 状态。已确认提交的后台作业可能仍在远端运行；后续 run 必须使用保留的 job 身份查询，队列结算不表示远端取消。

## 实施边界

1. `crates/omicsops-store/src/composer_queue_dispatch.rs`：事件观察与 reconcile 使用完整持久化事件链判定安全 NeedsAttention，并结算旧 running/uncertain queue 行。claim 与 commit 的 active 判断须复用同一安全检查；不能简单从 SQL status 列表删除 needs_attention。扫描会话所有 NeedsAttention run，包含没有 queue 行的旧直接启动 run。普通 planning/running/input/approval 仍阻塞。
2. 终态事件先落库、run status 后保存的窗口中，queue 可以先结算，但 `running` 状态继续阻塞下一次 claim。driver 只有在状态与终态一致且事件安全时推进。重启若留下 running 状态+终态证据，复用宿主在 run ownership lease 下的 `repair_agent_run_terminal_status_v4`；store 的普通 reconcile 不越过 ownership 擅自改运行状态。
3. `src-tauri/src/composer_queue_driver.rs`：NeedsAttention 的 settlement 取完整事件链判断；安全返回推进，未知或不安全停止。下一次派发前等待前驱 run 的 OS ownership lease 释放，避免终态保存和浏览器清理尾部与下一任务重叠；replacement 已有同类保护可复用。队列 scope lease 继续串行化 FIFO。
4. `src-tauri/src/agent_v4.rs`：正常结束的安全 NeedsAttention 应能触发已有 queue kick。已有 `composer_queue_reconcile` 每三秒读取并 kick，足够恢复旧数据库已存在的四条 pending，无需新命令、迁移或重建消息。claim/commit 必须再次检查状态与证据，不能只依赖先前 UI/driver 快照。
5. `crates/omicsops-store/src/composer_replacement.rs` 的 other-active 检查也不能被历史安全 NeedsAttention 永久阻塞；复用同一检查并排除精确 target。replacement 仍只允许替换当前 active run，不允许把已终态 NeedsAttention 伪装为 active。已提交 replacement 的旧 target 若安全 NeedsAttention 终止，可以通过同一安全结算判断；未确认副作用仍阻塞。
6. `src/features/workspace/WorkspaceShell.tsx`：header 不再无条件显示 `t.status` 的“远端分析运行中”，按当前 run/terminal/waiting/idle 的真实状态显示。queueBacklog 不在前端猜测安全性，使用宿主结算后的 queue 状态；存在真实 pending 时继续显示“加入队列”，无 backlog 且无 active run 时恢复普通发送。

## 终态后的核实记录

当前 protocol 的 `validate_terminal_position_v4` 与 store append 都只允许 terminal 后追加一次 BrowserTabCleanupRequired；现有 `agent_v4_resolve_uncertain` 因此不能给 terminal run 追加 ToolDispatchResolved。本修复不能假装它已支持。安全上下文超限恢复不依赖放宽该协议；不确定副作用继续阻塞，并明确保留这一限制。若同时修复人工核实恢复，必须同步修改 protocol/store：只允许引用本 run 已存在未解决 Uncertain call、非空证据、一次性 resolution 的审计事件；仍禁止新 ToolRequested/DispatchStarted/ModelRequested/第二终态。核实只解除队列栅栏，不能 resume 旧 run、声称远端取消或自动重试旧调用。完成核实后需重新 reconcile/kick。

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
