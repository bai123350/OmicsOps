# W04、W05、W08 Runtime 与离线场景设计

日期：2026-10-04。状态：用户已确认设计，实施计划待审阅，尚未实现。

本文细化[候选清单](2026-10-04-wisp-science-improvement-backlog.md)中的 W04、W05 和 W08 manifest/V4 runner。W08 的 Python helper 回归及共享 CI 配置由配套设计文档负责。三项可以独立交付；本文件没有授权新增远端执行、扩大能力或修改已冻结的计划。

上游参考固定为 `xuzhougeng/wisp-science@2c1ad2e1ee2e600be22c08f826490b1c627c71b4`。本文以当前 OmicsOps 接口为实施基础，不移植另一套 runtime manager，也不把已审计的上游旧机制当作新变化。

## W04：观察当前内核中的对象

### 用户目标与有限范围

用户在 Runtime 对话框中查看当前 Run 已经启动的 Python/R 内核中有哪些对象，以及这些对象的安全元数据。查看不启动解释器、不连接新 SSH 会话、不安装库、不执行用户提供的代码。

第一版支持当前应用进程中仍存活的 V4 内核，Local 和 SSH 使用同一个 UI DTO。本地容器复用同一 process worker，随该协议支持元数据观察，但不扩大已冻结的容器能力。历史 Run、应用重启后的内核、未初始化的计算资源均显示“未启动/当前不可观察”。交互内核跨重启恢复、持久保留暂停内核、Agent 工具调用 inspect、对象依赖执行约束、数据内容查看不在此项范围内。

现有资源随执行组合对象存活，暂停后未必仍存在。此项不会承诺暂停后仍能浏览内存，也不为了观察引入新的内核保活行为。

### 已有接口与缺少的接缝

- `crates/omicsops-runtime/src/lib.rs:18` 的 `KernelProcessV4` 有 `session_id/process_identity/is_healthy/execute/interrupt`，没有 inspect；`RuntimeManagerV4:105` 的 `sessions` 私有。`acquire:120` 会启动或替换进程，不能用于观察。
- `ExecutionContextKeyV4` 位于 `crates/omicsops-protocol/src/lib.rs:819`，键为 project/run/backend/language/environment。宿主必须从 Run 冻结配置解析这些字段；前端不能提交替代 backend 或 environment。
- `src-tauri/src/agent_v4.rs:4701` 的 `ExecutionResourcesV4.runtime` 在私有、按 Run 组合的资源中；`AppState` 没有 V4 runtime 注册表。`commands.rs:38` 的 `active_kernels` 属于 `kernel_commands.rs` 的旧内核，不能拿来冒充 V4 会话。
- `process_backend.rs:293` 的 Local/容器进程用简单 JSONL；`agent_v4.rs:7740` 的 SSH 进程用 `omicsops-agent::KernelRequest/KernelEvent`。两个 worker 分别在 `process_backend.rs:624/661` 与 `omicsops-adapters/src/kernel.rs:75/120`，必须都扩展。
- 两种 `execute` 都在整个请求期间持有进程 Mutex。观察必须复用这个锁和解码边界，不能另开 reader 抢走 stdout。

### 选定方案与接口

在 protocol 中增加以下共享数据契约，由 DTO crate 重新导出；字段和状态均为建议签名。

```rust
pub struct InspectKernelRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub language: KernelLanguageV4,
    pub expected_session_id: Option<Uuid>,
}
pub enum KernelInspectionStatusV4 {
    Ready, NotStarted, Busy, StaleSession, Unavailable,
}
pub struct KernelObjectMetadataV4 {
    pub name: String,
    pub type_label: String,
    pub dimensions: Option<Vec<u64>>,
    pub element_count: Option<u64>,
    pub summary: String,
}
pub struct KernelInspectionV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub language: KernelLanguageV4,
    pub session_id: Option<Uuid>,
    pub status: KernelInspectionStatusV4,
    pub objects: Vec<KernelObjectMetadataV4>,
    pub truncated: bool,
}
```

Runtime 层不持有 conversation 权限，新增内部 `KernelInspectionPayloadV4`，只含 objects/truncated；宿主负责补齐已核验的作用域。纯 metadata/request/response 数据放 protocol，DTO 只重新导出这些 protocol 数据；Runtime 的错误、manager 结果、进程句柄、锁和注册表均留在 Runtime/宿主，不让 DTO 依赖 runtime 或 async runtime。`KernelProcessV4` 增加 `async fn inspect(&self) -> Result<KernelInspectionPayloadV4, KernelInspectionErrorV4>`，默认实现返回 Unsupported，避免测试替身或其他 backend 被迫伪造实现。manager 增加 `async fn inspect_existing(&self, key: &ExecutionContextKeyV4, expected_session_id: Option<Uuid>) -> KernelInspectionResultV4`，返回当前 session 与结构化状态，绝不调用 `acquire/launch/rebuild`。

宿主新增 `agent_v4_inspect_kernel(request)`。`AppState` 增加以 `(project_id, conversation_id, run_id)` 为键的弱引用注册表，指向当前 `ExecutionResourcesSlotV4`。实际 execution driver 在所有权注册及取消重验后登记已组合的 slot；干式目录/预算组合不登记，避免覆盖真实运行。命令只升级 weak 引用并使用 `ready.get()`，不能调用 `initialize()`。失效条目按读取清理；重复 driver 登记用代次区分，旧 guard 不删除新条目。guard显式移入并保留在实际异步driver内，不能只新增struct字段后被Rust按字段捕获提前释放。此注册表不持久化，不获取 SSH 凭据，不增加内核保活。

命令先核验 project/conversation/run 所有权、冻结 spec 完整性、语言及冻结 compute selection，再查注册表。未初始化返回 NotStarted；进程忙返回 Busy，UI 提供手动刷新。前端按作用域和 session 校验异步结果，切换 Run、关闭窗口、重建后丢弃旧响应。

进程 inspect 使用与 execute 相同的 Mutex，但先 `try_lock`，不排队等待长作业。取得锁后再次验证健康状态，发送固定的 inspect action；读取有界、严格匹配 request/session 的独立终端响应，固定 3 秒截止。manager 返回前再次确认键下仍是同一 session；已替换则返回 StaleSession，丢弃元数据。此检查不改变 rebuild/interrupt 的授权和执行语义。

Local/容器简单协议增加 `action: inspect` 与 `kind: inspection` 终端帧；SSH typed 协议增加 `KernelRequest::Inspect { session_id, request_id }` 和 `KernelEventKind::Inspection { objects, truncated }`。旧 execute/shutdown 的序列化形式不变。解码器拒绝重复终端帧、错 ID、超长帧及非法 metadata；共享 DTO 不等于把两个传输协议强行合并。

### 安全元数据与失败语义

宿主固定预算：最多返回 64 个对象，最多检查 4,096 个绑定，总响应不超过 16 KiB；名称不超过 256 UTF-8 字节、摘要与受控type_label各128字节、维度最多8个。16 KiB检查内部payload及包含scope/session/status的完整公开response；接近payload上限时须留出envelope空间。第一版仅exact builtin/opaque短标签，保守拒绝长标签，不把未知对象自定义类名当必要输出。预算不由前端或模型增大。固定摘要只说明类型/尺寸，例如 `list, 42 elements`，不包含 scalar/string 内容、文件路径或对象 repr。

Python 只对 exact built-in 类型读取受控元数据，不调用任意 `repr/str/len` 或对象属性方法；未知、子类和第三方科学对象显示 opaque 类型与未知尺寸。第一版不主动 import NumPy/Pandas，不通过它们的 property 获取 shape。R 跳过 active binding，使用不强制求值的 `substitute` 取得绑定描述；未求值表达式和无法安全判定的对象保持 opaque。只对无自定义 class 的基础值读取 primitive 元数据及原始 dim 属性，不调用 `print/summary/str`、S3/S4 泛型或用户函数。不得用 `get` 强制 promise。R 的 delayedAssign/active binding 必须先由测试证明无副作用；无法证明的类型保留名称并省略其余元数据。

worker 过滤内部/明显凭据绑定名，宿主再次应用既有脱敏规则。结果仅送本次 UI，不写事件、日志、SQLite、模型上下文、科研 Artifact 或导出；用户提供的对象名称和 worker 原始错误不能作为日志字符串。Ready 是内存元数据快照，不能当科研结论证据。

Busy、StaleSession、NotStarted 不改变会话健康或计算状态。inspect 请求已经发出后若超时或协议失步，沿用现有不确定响应的保守处理：该会话标记不可继续复用，后续观察返回 Unavailable；本次查看不杀进程、不重建、不重试执行、不把未知作业记为 cancelled。Manager有覆盖lookup/await/recheck的外层3秒deadline，worker写入可能已发送后的健康guard须在future被取消时仍标不可复用，不能依赖内层timeout分支一定执行。正常pending时health保持true，既有execute拿进程锁后、写请求前须重验health，防止先取得Arc的排队执行复用后来失步的会话；只拒绝此不确定派发，健康协议/结果生命周期保持。宿主补齐scope并脱敏后再次完整response验证，避免payload预算漏计envelope。传输上限和超时由固定宿主策略控制，错误UI只显示白名单原因。

Local/容器的终端验证后及下次写入前，检查buffer与当前descriptor已经排队的数据；buffer恰好耗尽不代表pipe clean。Windows使用read-only PeekNamedPipe，Linux/macOS使用poll(timeout0)，无wait/delay或第二读者。此实现依赖没有同句柄未决读：[Microsoft PeekNamedPipe主文档](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-peeknamedpipe)说明synchronous handle在多线程下可能阻塞。execute可能写入至已验证terminal的drop-health guard关闭被取消Blocking读仍healthy的入口；正常pending不改变health，terminal验证后/archival前解除，未知计算仍不能称为取消。Native错误固定映射且会话不可复用。Windows须实际编译并测试empty/EOF/queued；Linux/macOS本轮未编译/执行，不作支持保证。Readiness只是当下queued/pre-send可检测数据，不能保证阻止clean检查后任意新异步帧；后续失步仍拒绝复用、不重试未知派发。

### 自动验收与文件地图

确定性测试证明零 launch/零 SSH connect/零 execute；mock Local/SSH 请求错序或错 ID 均拒绝；执行占锁时立即 Busy；检查期间 rebuild/interrupt 导致旧 session 结果拒绝；弱注册表不会延长内核寿命或跨 scope 泄漏。超量对象、超长名称、敏感哨兵及非法 dimensions 有界处理。Python 元数据 helper 可用系统测试 Python 验证恶意 repr/len/property 不触发，无科研库依赖；未知类型不伪造 shape。R 协议用 mock 验证，不把 mock 当作真实 R 无副作用证明；R active binding/promise 行为作为显式 ignored worker 验收，在一次性、有 R 的环境验证，默认 CI 不要求 R。未完成真实 R 验收时记录未执行，不声称完整 worker 端到端通过。DTO 旧 execute/shutdown 契约不变。UI 空态、Busy 手动刷新、异步切换和打开后立即 Escape 均测试。

文件地图：protocol 类型；`omicsops-dto/src/lib.rs` 导出；`omicsops-runtime/src/lib.rs` trait/非启动 lookup；`process_backend.rs` 两种嵌入 worker 与解码；`omicsops-agent/src/lib.rs` SSH 请求/事件；`omicsops-adapters/src/kernel.rs` SSH Python/R worker；`src-tauri/src/commands.rs` 注册表；`agent_v4.rs` slot 注册与 SSH inspect；新增桌面 `kernel_inspection_v4.rs` 命令；`src-tauri/src/lib.rs` 命令注册；DTO 契约测试；前端 types/API/RuntimeDialog/WorkspaceShell 与近邻测试。不新增数据库表。

依赖：不依赖 W03 后台作业取消或 W05。W06 如展示会话状态，只消费元数据状态，不能把观察结果收编为审计证据。若用户以后要求 NumPy/Pandas 内容或暂停后保留内核，分别需要新的受控类型适配和内核生命周期设计。

## W05：为暂停普通 Run 手动压缩工作上下文

### 用户目标、选定范围与取舍

用户在同一个会话中为暂停的普通 Run 生成更小的模型工作集，然后继续回答待处理问题。可视历史、原始证据和冻结计划保留。第一版使用确定性 checkpoint，不调用付费摘要模型，不改变 main/delegated/reviewer 配置，不自动恢复 Run 或提交问题答案。

只支持 `RunExecutionKindV4::OrdinaryAgent` 且 `waiting_for_input`，恰好一个未回答的 InputRequested，无实际活动 driver、无 pending guidance、无待审批工具、无停止请求和未决副作用 dispatch。planning、waiting_for_approval、needs_attention、运行中及 ApprovedPlan 均拒绝。completed 延续现有 NotNeeded 行为。unknown 不等于 cancelled，也不能借压缩消除恢复阻塞。

### 现有基础与必要修正

`src-tauri/src/context_compaction.rs:18` 已有 `agent_v4_compact_context` 和跨进程 run lease，但只开放 completed no-op。`protocol/src/context_usage.rs:138/150` 的请求/receipt 有请求 ID、作用域、源 head、大小、archive/checkpoint/spec hash。Store 的 `compact_context_v4:41` 已有事务、幂等和恢复。

当前 Store 私有 `build_checkpoint:948` 将 scientific_state 置 Null，并截断 recent/guidance；`build_candidate:1017` 不是 core 的完整请求。不能直接给这个候选补一个模型检查后启用按钮。core 的 `execution_request:4553`、`validate_execution_context:4626`、`context_for_internal:4653` 才负责现有 prompt/schema/image/reference/science/guidance 工作集；它们目前私有且自动压缩会写 archive/events，不是安全的 manual prepare 接缝。

`agent_v4.rs:5012` 的 `compose` 载入冻结 main/reviewer/delegated 绑定并构建工具目录；普通 Run 的 resume 使用 lazy compute。`DesktopModelPortV4::prepare_request:5292/validate_request:5391` 加宿主 system 后缀、工具 schema、provider 形状和图片预算，是完整准入边界。不能只比较 context 字符串长度，也不能为了压缩调用 `resources.initialize()` 打开 SSH。

另一个必要接缝是 `model_turn_with_policy:2910`：native replay 在 context 检查以后才加入请求，此方法还可能写 pending replay closures。manual 不能调用此方法，也不能把 replay 为空的早期检查当作最终请求准入。需提取无写入的 final request/replay shaping，用相同 model binding 投影 replay 并移除重复 context；有需要写入的 pending closures 时本次 manual 拒绝，交由原有恢复/答案路径处理。

### prepare 与原子 commit 接口

UI 请求沿用 `CompactContextRequestV4`，不接受前端摘要、checkpoint、模型 ID 或预算覆盖。宿主保留 lease 直至提交结束，同时检查 `active_runs`。从普通 resume 组合中提取仅用于准备请求的共享方法，复用相同 profile hash 验证、工具能力过滤、附件、route、prompt 与 lazy resource 策略；不得 stream、执行工具或初始化计算资源。

Store 建议新增两个只供宿主调用的 Rust 接口：

```rust
async fn context_compaction_snapshot_v4(
    &self, request: &CompactContextRequestV4,
) -> Result<CompactionSnapshotV4, StoreError>;
async fn commit_context_compaction_v4(
    &self, request: &CompactContextRequestV4,
    prepared: &PreparedContextCompactionV4,
) -> Result<ContextCompactionReceiptV4, StoreError>;
```

Snapshot 含 frozen spec/Run record、完整经核验事件、source sequence/hash、scientific snapshot revision/hash，以及 pause/guidance 状态。prepare 不持有 SQLite 写事务。core 新增 `prepare_context_compaction(&self, snapshot: &ContextCompactionSnapshotV4, limits: AgentLimitsV4) -> Result<PreparedContextCompactionV4, AgentCoreErrorV4>`；方法只构建 checkpoint 和候选请求，不调用 EventStore 的 append/archive/consume_guidance，不调用模型 stream。

跨 crate 的纯数据 snapshot/prepared 放 protocol，Store 的 Run record 留宿主侧映射，不新增 protocol 对 Store 的依赖；scientific snapshot 用既有 JSON Value/版本字段传递，不让 protocol 反向依赖 science。prepared 包含 source head/spec hash/science revision/hash、完整 transcript 与 checkpoint、候选 context、预分配的 archive 身份、拟追加的事件、before/after context bytes、provider admission 元数据。敏感 transcript/checkpoint 不是 UI DTO，不写日志。receipt 的原 before/after_bytes 保持历史含义（归档 transcript 字节/候选 context 字节），另以可缺省字段 `provider_request_bytes`、`admission_configuration_hash` 表示本次准入；只有能够测量完整 provider payload 才填 bytes，图片等未知成本不能填零，不能将旧 receipt 的数字改称 token。

core 提取纯 rendering/checkpoint fitting 接缝与 automatic 路径共用，保留任务/phase/cycle、completion criteria、已消费 guidance 全部内容、科学状态或可分页证据引用。不再使用 Store 的独立 Null/scalar 截断投影。待回答问题仍在原事件中，并有可取回引用；未回答/审批状态仍以原链为权威。

完整工作集需要 scientific/reference projection 时，prepare 预分配 archive ID 并依据冻结 source head 构建待追加事件（Started、Archived、Checkpointed、Completed）的固定序列与 hash。由此产生的分页引用可在写入前形成最终请求；commit 必须原样核验并写入这些事件，不得重新生成时间或 hash。现有 context_views 只对 durable source 发引用的入口保持不变，增加内部 prospective helper 只用于纯 prepare；未 commit 的引用和请求不能交模型、UI 或执行工具。渲染和准入需同时检查压缩候选及包含这些实际新事件后的工作集，避免将管理事件留在 next recent_events 导致预算膨胀。自动 rendering 统一排除无模型意义的 compaction 管理事件，仍保留原始审计链。

准入使用同一 `execution_request`、无写入的 final replay shaping 和 DesktopModelPort 的 provider-shaped validation；计入 system、全部实际工具 schema、宿主后缀、replay/图片成本及 output reserve。未知模型目录能力不能被假定为足够；缺少可用预算、profile 改变、无法安全重构 prompt 或候选无法缩小且无法准入，均不写 archive/checkpoint。已拟合候选的“不扩大证据权限”必须通过依赖现有 read-result 的引用验证。源工作集超出预算正是压缩要解决的问题，不要求原始大请求先通过；只要求拟合候选和提交后所表示的最终请求通过。

Store commit 在 `BEGIN IMMEDIATE` 内重新核验请求 ID 作用域、普通 Run 状态、spec hash、source head、science revision/hash、未回答问题、guidance/approval/stop/未决 dispatch，以及数据库中冻结绑定 main/delegated/reviewer profile 的配置 hash 和 project compute binding。宿主在 prepare/commit 边界记录 prompt 快照 hash；外部规则文件后续改变仍由 resume 重新准入，不作跨文件系统事务保证。核验 transcript 哈希、checkpoint source、拟追加事件链和 prepared 数据的内部一致性，不能信任前端或未经验证的序列。成功一次写入 receipt、archive、checkpoint 和关联事件；任一步失败全部 rollback。已有成功 request ID 直接回同 receipt；相同 ID 跨 scope 拒绝。新的成功原子路径不会对外留下 STARTED；保留旧 STARTED 的 reconciliation 支持。

不为 admission 新增数据库列：`agent_context_compactions_v4` 目前没有 receipt_json/value_json，但已存在 `agent_events_v4.value_json`。为 ContextCompactionCompleted 增加可缺省、None 时不序列化的 bounded admission 数据（配置/prompt hash、可用的 provider_request_bytes）；新增事件原子保存这些数据。读取 receipt 时按 request_id 从已核验 Completed 事件恢复可缺省字段；旧事件/receipt 缺省为 None，旧 hash 序列化保持一致并加回归。无需 schema migration，也不在现有 message 列塞结构化 JSON。

归档持有完整 transcript 与科学 checkpoint，摘要不替代原证据。commit 不更新 Run spec/审批/状态、不消费 guidance、不解决 uncertain dispatch、不写终端事件。若准备期间其他窗口答题、停止、修改科学状态或提交 guidance，commit 拒绝并保持旧记录；重新尝试需要显式新 request ID，不自行重跑。

### 恢复、UI 与失败

lease 忙返回可理解的“Run 正由其他操作持有”；预算/配置/原始数据校验失败返回固定原因，不保存原始 provider/SSH 错误。操作开始前已经落库的同请求结果仍可读取；新失败不会仅为记录失败而改变原始上下文。

按钮只对符合条件的普通暂停 Run 启用。它调用现有请求 ID API，显示进行中、Completed/NotNeeded/受控错误，刷新 receipt 与上下文用量；重复点击复用本次 request ID。切换会话后的响应不写当前界面。成功不自动 resume，问题回答行为仍走既有宿主入口。

应用重启后使用同一持久 checkpoint/archive 引用。准入证明只针对本次冻结配置和工作集；后续新答案、变化的项目规则或科学状态仍由正常 resume 的最终请求检查保护。SSH 后续加载远端 project rules 会触发正常的新 model turn/准入，不能把 manual 成功解释为未来所有 SSH 请求都必然可发送。

### 自动验收、文件地图与依赖

Store 临时数据库测试：重复 ID、跨 scope、完成 no-op、普通 pause gate、批准计划拒绝、guidance/stop/审批/uncertain 拒绝、source/science/profile 变化竞态、任一事务写入失败零半成品、旧 STARTED reconciliation、重开 DB 后引用与 hash 一致。core 测试：system/schema 大而 context 小仍触发准入；图像未知成本不为零；native replay binding、pending closures 拒绝和最终 replay 预算；原请求超限但候选/最终工作集均可准入；大 guidance 不被截断；task/science/证据引用保留；不可缩小失败不改变原链。

桌面 mock 验证配置变化拒绝，零 stream/零工具 execute/零 SSH connect/零资源初始化；跨窗口租约竞态；原方法不会走有副作用的 auto compact。UI 测试覆盖 gate、幂等点击、receipt 状态、切换丢弃旧响应及真实问题恢复；所有关闭层仍遵守 Escape 堆栈。

文件地图：`omicsops-agent-core/src/lib.rs` 的纯候选/render/final replay 接缝与近邻测试、`context_views.rs` prospective helper、`model_replay.rs` 纯投影复用；`omicsops-protocol/src/context_usage.rs`/context pure types 与 lib.rs Completed event optional admission；`omicsops-store/src/context_compaction.rs` snapshot/原子 commit/事件 admission hydration 与旧 receipt 兼容；`omicsops-dto` 导出；`src-tauri/src/context_compaction.rs`；`agent_v4.rs` 普通 request composition 共享方法；DTO 契约；前端 types/API/WorkspaceShell/context usage 控件及测试。没有数据库 migration、新摘要模型、持久凭据或 runtime backend。

W05 独立于 W04。W08 的 compaction 场景能复用纯 prepare，但 Store 原子性仍必须由 Store 测试验证，不能由 MemoryStore 替身冒充。第一版确定性摘要与普通 Run 范围是明确建议；若要求语义模型摘要、ApprovedPlan 或运行中压缩，需要另行设计事务外模型调用、并发边界和审批保护。

## W08：manifest 驱动的离线 V4 场景与稳定报告

### 用户目标与选定方案

开发者统一执行 5 个有明确输入、错误注入和断言的场景，验证 Agent V4 的证据/审批/恢复/预算行为，得到可以对比和保留的脱敏 JSON 报告。它衡量确定性控制行为，不衡量真实模型答案质量、科研有效性或机器耗时。

使用现有 `omicsops-agent-core` crate 的 unit test 接缝，不新增 CLI crate、production feature、Tauri 菜单或模型连接。`lib.rs:6385` 的 ScriptedModel、`MemoryStore:7624` 等位于私有 tests 模块；嵌套 `mod scenario_evaluation;`，文件放 `src/tests/scenario_evaluation.rs`，通过 `super::*` 复用必要 fixtures。无需将所有测试改造成框架，或把测试 fixture 暴露为产品 API。

AgentCore 使用现有 public `ModelPortV4/ToolPortV4/EventStoreV4/ScientificStateStoreV4`，入口为 `execute_with_limits`。辅助 fake 可以记录 model requests、tool proposals、审批前 prepare、actual execute/recover 等不同计数；不能用 ToolRequested 数量冒充真实 dispatch。

### Manifest 与 runner 契约

manifest 放 `crates/omicsops-agent-core/src/tests/data/v4-scenarios.json`，由 `include_str!` 读编译期本地文件。类型 `deny_unknown_fields`，版本 1；schema 包含 `suite_id`、`runner_contract_version`、按唯一 id 声明的 scenarios。每项包含 label、scenario_kind、objective、固定 UUID seed、ModelTurnV4 脚本、工具返回/错误脚本、预置事件描述、AgentLimitsV4 的允许测试字段和具名 assertions。

scenario_kind 为 runner 已知的封闭集合：ReadEvidence、DenyApproval、RecoverUncertain、CorrectMcpArguments、CompactPreserveSources。seed/action 是类型化测试步骤，不是 shell、Python、网络或任意文件执行指令。工具与事件脚本都要在对应 scenario 的 effect/capability 契约内；manifest 不能宣告自己已获得审批或将尚未执行的计划设为成功证据。

runner 顺序执行场景并汇总全部结果，不因第一项失败丢失报告。无 real network/model/SSH/GPU/R；Tokio 虚拟时间处理重试和等待。每场景独立 model/tool/store/science fixture，不共享 mutable 全局状态，固定脚本耗尽或未消费均为失败。事件链用真实 AgentEventV4 构造并校验，不把随机时间/attempt UUID 当作报告比对字段。

首组断言：

| 场景 | 必须观测的行为 | 既有测试基础 |
| --- | --- | --- |
| 读取证据 | 从已核验 sequence/hash 取回正文；不存在或跨 scope 引用拒绝；结论需要真实引用 | materialize/verify evidence helpers，`reviewer_materialization_does_not_turn_missing_refs_into_evidence` |
| 拒绝审批 | 先等待确切审批，Denied 后 actual execute 为 0；恢复不扩大 capability | `approval_policy_matrix_requires_the_expected_tool_decisions`、ordinary approval tests |
| 恢复不重派 | 未决副作用零 execute；已完成 durable outcome 可 reuse，未知仍需 Attention | `crash_recovery_never_replays_an_uncertain_side_effect`、durable receipt recovery tests |
| MCP 参数更正 | 预检参数失败未 dispatch、不耗尽健康服务器重试；下一次更正能成功且不重复目录发现 | bundled MCP argument/business failure tests、directory binding tests |
| 压缩保留来源 | 小 context 但大 schema 仍按完整请求处理；checkpoint 后能取回原证据，冻结边界和 guidance 保留 | `full_request_budget_compacts_even_when_context_bytes_fit`、same-run checkpoint/reference tests |

在 test seam 中注入少量错误，例如绕过 Denied 的假 execute、恢复假重派、损坏引用 hash、将预检失败记业务失败、丢弃 compaction 源引用，证明相应 assertion 会失败。对注入后断言失败的测试自身应通过；不修改 production 代码制造缺陷。

### 报告、退出语义和 CI

报告 JSON schema_version 为 1，包含 suite_id、suite_hash、runner_contract_version、passed/failed 总数，以及按 id 稳定排序的 `status`、`model_attempts`、`tool_proposals`、`tool_dispatches`、`recovered_outcomes`、`assertions_passed/failed` 与白名单 failure_reason_code。可附失败事件 sequence/tool ID；不附 raw prompt、model output、reasoning、arguments/outcome、绝对路径、host 信息、原始异常、凭据或整条 transcript。

suite_hash 为 SHA-256，覆盖版本化、规范排序 JSON manifest 及所有被引用 fixture 的相对路径/规范内容；换行归一化为 LF，不能依赖 Windows checkout 的 CRLF。runner_contract_version 单独表明断言解释版本；suite_hash 不是待测产品代码版本或科研结果哈希。最小新增 `sha2/hex` 仅 dev-dependencies，复用 workspace 版本。

默认 `cargo test --workspace` 执行 suite 并断言 failed 为 0。定向命令为 `cargo test -p omicsops-agent-core scenario_evaluation -- --nocapture`，打印一条带固定前缀的报告 JSON。显式设置 `OMICSOPS_EVAL_REPORT_PATH` 时，将同一报告保存到该路径，例如根目录已忽略的 `target/evaluation/v4-scenarios.json`；缺省不写工作区文件。测试代码用 Rust 文件 API 创建父目录并写临时文件后原子替换，不拼接 shell 命令、不删除已有目录、不将报告提交 Git。报告写入失败或任何场景失败，cargo test 退出非零。

Windows CI 可在独立定向步骤设置报告路径，然后 `if: always()` 上传该固定测试产物；现有 workspace/npm/build 检查继续保留。Python helper 的 setup-python/受控 unittest 独立配置；此 runner 不调用 helpers、不安装科学库、不把 helper 通过计入 V4 场景通过。

### 自动验收、文件地图与依赖

manifest 解析覆盖重复 ID、未知版本/字段、无效 seed、非法 assertions、缺失脚本。重复运行报告稳定，随机审计事件不使 suite hash 或计数抖动；fixture 内容变动必使 suite hash 变化。敏感哨兵、私有推理和原始 tool/provider 错误不进入报告；五个场景分别验证关键计数和状态；故障注入能触发正确的 failure code。默认全测试运行无网络；定向失败仍先写完整失败报告再失败退出。

文件地图：现有 core tests 中仅新增模块声明和必要 fixture 可见性；新增 `src/tests/scenario_evaluation.rs` 与 data manifest/少量 fixture；`omicsops-agent-core/Cargo.toml` dev hashing dependencies；Windows CI 定向报告/上传；开发文档记录命令、报告 schema 和 suite 范围。无需生产数据库/DTO/迁移，不复制上游 debug 配置、凭据或 debug runner crate。

W08 可先于 W04/W05 交付。后续给已实现 W04/W05 加场景时升 manifest/suite hash，按真正存在的 host seam 测试；core runner 无法证明 SSH worker、宿主租约或 SQLite 事务，必须保留对应 crate/desktop tests。真实模型、SSH、R、Micromamba/PBMC 验收仍按 `acceptance/README.md` 独立显式执行。

## 审阅与后续约束

本次只写设计文档，没有实现三项功能，没有运行产品测试。上述有限范围和接口已获用户确认；对应逐项计划见[实施总计划](../plans/2026-10-04-wisp-improvements.md)，待计划审阅及选择执行方式。已选默认值为：W04 只观察存活内核的无内容基础元数据；W05 只做普通暂停 Run 的确定性压缩；W08 使用 core unit-test runner 和显式可选报告路径。这些实现选择没有缺失的凭据或环境前提。

实施交付先跑相近确定性检查，再按仓库规则执行 `cargo test --workspace`、`npm test`、`npm run build`；DTO/桌面组合变化另跑 `npm run build:desktop`。真实 Python/R worker smoke、Windows GUI/SSH 及 ignored 验收必须记录实际执行与未执行结果，不把模拟协议或构建成功当生产端到端验收。
