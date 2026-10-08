# W05 Manual Context Compaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让符合暂停条件的 ordinary Run 真正完成可恢复的手动压缩，完整 provider 请求准入后原子保存 archive/checkpoint。

**Architecture:** core 从固定 snapshot 纯准备 candidate/final replay 请求，不写事件或发模型。Store 在一次写事务中重新核对冻结身份、事件头及科学/指导状态并保存拟定事件与 archive；宿主使用原有租约及普通 resume composition，UI 沿用既有请求/receipt。

**Tech Stack:** Rust/serde/Tokio，现有 AgentCore V4、ModelPort 和 SQLite/sqlx；Tauri 2、React 19/Vitest。

**Spec:** [W05 Runtime 设计](../specs/2026-10-04-wisp-runtime-design.md#w05为暂停普通-run-手动压缩工作上下文)，已审阅设计提交 `161199f`；用户已确认 `8515da8` 全部实施计划。Task1契约已独立审阅及必要复审通过，Task2–5继续实施。

## Global Constraints

- “只支持 RunExecutionKindV4::OrdinaryAgent 且 waiting_for_input，恰好一个未回答的 InputRequested，无实际活动 driver、无 pending guidance、无待审批工具、无停止请求和未决副作用 dispatch。”completed 延续 NotNeeded，ApprovedPlan/planning/运行中拒绝。
- “第一版使用确定性 checkpoint，不调用付费摘要模型，不改变 main/delegated/reviewer 配置，不自动恢复 Run 或提交问题答案。”
- “源工作集超出预算正是压缩要解决的问题，不要求原始大请求先通过；只要求拟合候选和提交后所表示的最终请求通过。”计入 system/schema/宿主后缀/native replay/images/output reserve。
- pending replay closures 需要落库时 manual 拒绝；不调用会写 closures 的 model_turn_with_policy。
- “不为 admission 新增数据库列”；optional admission 存 Completed event 的既有 value_json，None 不序列化，旧事件 hash 必须保持。
- “成功一次写入 receipt、archive、checkpoint 和关联事件；任一步失败全部 rollback。”不修改冻结 spec/审批/Run 状态、不消费 guidance、不解决 unknown dispatch。
- no stream/no tool execute/no compute initialize/no SSH connect；prospective 引用不在 commit 前交模型/UI。预算未知不填零、不推断模型家族能力。
- prepare不持久化/派发；既有sync `usage_request_metadata`可能核验并读取本地图片bytes，这是完整请求测量，不可用占位图片字节冒充。新暂停路径要求validated anchored `spec_hash:Some`并取其String；unanchored legacy仍可读历史/旧completed路径，但不授予新压缩提交。NotNeeded首次/回读optional admission字段均None，仅Completed从事件hydrate。
- 新 UI 请求 capture scope/generation；所有关闭层遵守 Escape 栈。自动化使用临时 DB/fake，不要求网络或凭据。

## Review Focus

1. context 小但 schema/replay/images 大：候选与最终 provider 请求都准入，未知成本不为零（Task 2、4）。
2. prepare 后另一窗口答题/停止/指导/科学或 profile 改变：事务拒绝且零半成品（Task 3、4）。
3. 大 guidance、旧失败证据或 task/cycle 信息：保留完整权威内容/取回引用，不用 Store Null 投影（Task 2、3）。
4. 原子保存失败、重启或重复 request ID：保留旧上下文、同 receipt/同 archive，跨 scope ID 拒绝（Task 1、3、4）。
5. busy/离屏重复点击/错误包含秘密：无隐式 resume、零资源初始化、旧响应不写新会话、受控错误（Task 4、5）。

---

### Task 1: 纯 prepare 数据契约和旧事件兼容

**Files:** Modify `crates/omicsops-protocol/src/lib.rs`、`crates/omicsops-protocol/src/context_usage.rs`、`crates/omicsops-dto/src/lib.rs`、`src/types.ts`、`src-tauri/src/dto_contract_tests.rs`；Create `crates/omicsops-protocol/src/context_compaction.rs`。另在 `crates/omicsops-store/src/context_compaction.rs`、`crates/omicsops-store/tests/context_compaction.rs` 的现有 Completed variant 构造处仅补 `admission: None`。

**Interfaces:**
- Consumes：原 `CompactContextRequestV4`、`ContextCheckpointV4`、`ContextArchiveV4`、`AgentEventV4`、`RunSpecV4`。
- Produces：`ContextCompactionAdmissionV4 { configuration_hash: String, prompt_sha256: String, provider_request_bytes: Option<u64> }`；Completed event 增 `admission: Option<ContextCompactionAdmissionV4>`，serde default/skip None；receipt 增可缺省 `provider_request_bytes/admission_configuration_hash`，保持原 bytes 含义。
- Produces：`ContextCompactionSnapshotV4 { request: CompactContextRequestV4, spec: RunSpecV4, events: Vec<AgentEventV4>, scientific_state: Value, scientific_revision: u64, scientific_sha256: String, source_through_sequence: u64, source_head_hash: String }`。
- Produces：`PreparedContextCompactionV4` 字段为 `request: CompactContextRequestV4`、`source_through_sequence: u64`、`source_head_hash: String`、`frozen_spec_hash: String`、`scientific_revision: u64`、`scientific_sha256: String`、`transcript: String`、`candidate_context: String`、`checkpoint: Option<ContextCheckpointV4>`、`archive: Option<ContextArchiveV4>`、`planned_events: Vec<AgentEventV4>`、`status: ContextCompactionStatusV4`（仅 Completed/NotNeeded）、`before_bytes: u64`、`after_bytes: u64`、`admission: ContextCompactionAdmissionV4`。Completed 必须有 archive/checkpoint；NotNeeded 不创建 archive。
- Produces：`PreparedContextCompactionV4::validate_structure(&self) -> Result<(), ContextCompactionValidationErrorV4>`；错误固定 InvalidScope/InvalidStatus/InvalidArchive/InvalidEventChain，验证纯数据一致性，Store 仍另核验当前 DB 状态。
- pure snapshot/prepared 不重导出为 UI DTO；protocol 不依赖 core/store/science，科学状态传 JSON Value。

- [x] **RED 写契约测试**：`legacy_compaction_event_hash_is_unchanged`、`legacy_receipt_admission_is_unknown`、`prepared_compaction_scope_and_status_are_consistent`。使用保存的旧 JSON fixture，而非用新 struct 反向生成“旧”输入。

```rust
assert!(legacy_event.verify().is_ok());
assert_eq!(legacy_receipt.provider_request_bytes, None);
assert!(!serde_json::to_string(&legacy_event).unwrap().contains("admission"));
```

- [x] **RED 运行**：`cargo test -p omicsops-protocol compaction`；`cargo test -p omicsops-desktop --lib dto_contract_tests`。预期缺失字段/契约接口失败。
- [x] **实现接口**：新增 bounded optional metadata，旧字段和请求不变；修正现有 exhaustive 构造/匹配，不改旧事件 payload。记录准备数据的 Completed/NotNeeded 一致性规则，拒绝 Attention prepared。
- [x] **GREEN**：重复 RED，`cargo test -p omicsops-dto`；预期旧 event hash 和 DTO 序列化均通过。
- [x] **Commit**：只暂存本任务文件，`git commit -m "feat(protocol): define manual compaction preparation and admission"`。

### Task 2: core 纯候选与 final native replay 准入

**Files:** Modify `crates/omicsops-agent-core/src/lib.rs:execution_request/context_for_internal/model_turn_with_policy`、`context_views.rs`、`model_replay.rs`；Test 原 core 模块和 context_views/model_replay tests。为计算 transcript/checkpoint/prompt 的精确 SHA-256，仅在 `crates/omicsops-agent-core/Cargo.toml` 加现有 workspace `sha2` 依赖，并最小更新 `Cargo.lock` 中该 crate 的依赖列表；不新增 hash 实现或其他依赖。

**Interfaces:**
- Consumes：Task 1 snapshot/prepared；原 ModelPortV4::validate_request、usage_metadata、usage_request_metadata、supports_native_replay 与 AgentLimitsV4。
- Produces：`AgentCoreV4::prepare_context_compaction(&self, snapshot: &ContextCompactionSnapshotV4, limits: AgentLimitsV4) -> Result<PreparedContextCompactionV4, AgentCoreErrorV4>`（sync，所有输入已捕获）。
- Produces：private `shape_final_execution_request(&self, spec: &RunSpecV4, context: String, events: &[AgentEventV4]) -> Result<ModelRequestV4, AgentCoreErrorV4>`；使用同 binding 的 project_model_replay/remove_replayed_context_events；pending closures 非空则拒绝 manual，不写其结果。
- Produces：private `prospective_scientific_state_view(spec: &RunSpecV4, state: &Value, source: &AgentEventV4) -> Option<Value>`，仅 prepare 可用；现有 durable view 入口保持。render/checkpoint fitting 与自动路径共享纯逻辑，compaction 管理事件不进入模型 recent_events。

- [x] **RED 写测试**：`manual_prepare_full_request_admits_candidate_and_final`、`manual_prepare_native_replay_is_budgeted_without_closures`、`manual_prepare_preserves_guidance_science_task_and_sources`、`manual_prepare_unshrinkable_is_read_only`。录制 validator 收到的完整 requests，source 超限但候选可准入。

```rust
assert!(original_request_over_budget);
assert!(validated_candidate.replay.len() > 0);
assert!(validated_final_request_fits);
assert_eq!(store.events_after(), store.events_before());
assert_eq!((streams, archives_written, guidance_consumed), (0, 0, 0));
assert_eq!(checkpoint_guidance, original_guidance); // 不截断
```

- [x] **RED 运行**：`cargo test -p omicsops-agent-core manual_prepare`。预期 prepare/final shaping 缺失或预算/来源断言失败。
- [x] **实现接口**：捕获完整 transcript/science，构造固定预分配 archive 与 Started/Archived/Checkpointed/Completed 事件（NotNeeded 只有对应管理事件），固定 sequence/timestamp/hash；按 actual final replay/provider 预算拟合。拟定事件追加后，正常上下文渲染与最终 native replay 请求须复用同一纯拟合/整形接缝，确保真正持久投影对应的完整请求仍可准入；大 replay 不得仅在临时 clone 中丢 recent steps。显式手动压缩在 `auto_compact=false` 时仍可使用本次明确手动操作产生、经哈希和事件链验证的 checkpoint 受控投影，不全局开启自动压缩。只有已有经核验的手动 Completed checkpoint 的模型投影与新投影相同且完整请求可准入才 NotNeeded；等价的旧版/自动 checkpoint 首次显式手动压缩仍作一次 Completed 升级，之后重复请求可 NotNeeded；不能用 after_bytes 与归档 transcript bytes 大小比较代替判断。无法缩小且不能准入返回错误。final metadata 导致的管理事件不回填到模型工作集。旧 snapshot/state/source references 保留且 commit 前禁止使用。不调用自动 context_for_internal 的写入路径。
- [x] **GREEN**：重复 RED，再运行 `cargo test -p omicsops-agent-core full_request_budget`、`cargo test -p omicsops-agent-core same_run_checkpoint`、`cargo test -p omicsops-agent-core replay`。预期新纯路径和旧自动/恢复均通过。
- [x] **Commit**：仅暂存本任务文件，`git commit -m "feat(agent): prepare manual compaction with final request admission"`。

**实施证据（2026-10-08）**：Task 2 的 feature 5662c21、独立格式提交 621ca99 及四轮修复 8ffd455、76b3996、277f7f8、5559298 经完整任务审阅和各轮 scoped 复审清零。最终改动源上 core 222、protocol 43、manual 20、replay 21、full-request-budget 3、same-run-checkpoint 1 与 cargo fmt --all -- --check 通过；更早的 workspace/npm/Web 结果属于修复前源版本，交付前仍需完整 clean-HEAD 验证。手动 prepare 保持零写；自动归档采用完整回放请求的归档前准入与归档后真实事件复验。原自动归档在意外写后 Store/语义失败时仍非原子，不能借此降低 Task 3 手动提交的原子要求。证据和限制见 .superpowers/sdd/2026-10-04-wisp-w05-context-compaction/task-2-report.md 与各复审记录。

### Task 3: Store snapshot 和无 migration 的原子 commit

**Files:** Modify `crates/omicsops-store/src/context_compaction.rs`、`crates/omicsops-store/src/lib.rs`（export）；Test `crates/omicsops-store/tests/context_compaction.rs` 与同模块临时 SQLite tests。不改 init.sql/schema。

**批准的窄接口补充**：为在同一事务中重验快照后的项目计算绑定和宿主 run 输入，在 `crates/omicsops-protocol/src/context_compaction.rs` 的内部 Snapshot/Prepared 加入固定 64 位十六进制 `source_bindings_sha256`、`source_run_sha256`；`crates/omicsops-protocol/src/lib.rs` 只更新已有三个 Prepared 测试构造，`crates/omicsops-agent-core/src/lib.rs` 只复制并提前校验这两个字段。Store 在 snapshot 只读事务与 commit 的 `BEGIN IMMEDIATE` 中，用同一白名单算法比较有效本地根（含 workspace_dir fallback）、远端根、连接 ID 与非秘密连接身份/信任元数据，并分别比较原始 run value_json 的 UTF-8 SHA-256。它们不替代 spec、模型配置、science、event head 和暂停输入的独立重验；不读凭据、不输出原始值、不改 schema/UI/旧 event/receipt。绑定元数据变化或等价 JSON 字节重排会保守要求重新 snapshot/prepare。

**Interfaces:**
- Consumes：Task 1 pure types；Task 2 prepared。原 conversation/run ownership、paused validation、event chain、guidance/stop/unknown checks。
- Produces：Store `CompactionSnapshotV4 { context: ContextCompactionSnapshotV4, run_value: Value }`；`context_compaction_snapshot_v4(&self, request: &CompactContextRequestV4) -> Result<CompactionSnapshotV4, StoreError>` async。
- Produces：`commit_context_compaction_v4(&self, request: &CompactContextRequestV4, prepared: &PreparedContextCompactionV4) -> Result<ContextCompactionReceiptV4, StoreError>` async。
- Produces：`context_compaction_receipt_v4(&self, request: &CompactContextRequestV4) -> Result<Option<ContextCompactionReceiptV4>, StoreError>` async，核验 request scope、hydrate 已核验 Completed event admission；旧 STARTED 仍走 reconciliation。

- [x] **RED 写测试**：`manual_compaction_commit_is_atomic_and_idempotent`、`manual_compaction_commit_rechecks_all_pause_inputs`、`manual_compaction_restart_preserves_refs_and_admission`、`manual_compaction_has_no_schema_change`。prepare 后逐项改变 head/science/profile/指导/答案/停止/审批/unknown；使用临时触发器在 archive/任一事件/receipt 写入时注入失败。

```rust
assert_eq!(failed_commit_counts, before_counts); // receipt/archive/events 全部
assert_eq!(first_receipt, duplicate_receipt);
assert!(cross_scope_same_request_id.is_err());
assert_eq!(reopened_archive_hash, first_archive_hash);
assert_eq!(schema_after, schema_before);
```

- [x] **RED 运行**：`cargo test -p omicsops-store manual_compaction`。预期 snapshot/commit/hydration 缺失或竞态/原子断言失败。
- [x] **实现接口**：snapshot 一致读取，不创建 STARTED；commit `BEGIN IMMEDIATE` 重验普通 pause、source/spec/science、绑定 profile hash/project compute、guidance/stop/审批/dispatch；验证 prepared transcript/checkpoint/事件序列及 scope。原样一次写 receipt/archive/planned events，失败全 rollback。Completed optional admission 已在 event JSON，hydrate 不新增列；新请求无对外 STARTED，保留旧 recovery。
- [x] **GREEN**：重复 RED，再 `cargo test -p omicsops-store context_compaction`；预期旧 completed no-op/STARTED reconciliation 不回归。证明 checkpoint 非 Null 科学状态、引用重开可解析。
- [x] **Commit**：仅暂存本任务文件，`git commit -m "feat(store): atomically commit admitted context checkpoints"`。

**实施证据（2026-10-08）**：Task 3 提交 `786e2d9` 经独立完整任务审阅通过。最终源上 Store 306、core 223、protocol 44 与格式检查通过；临时 SQLite 的六个写入中断点、源状态和绑定 CAS、回执重开、旧 STARTED 恢复、无 schema 变化均有真实断言。宿主租约、活动 driver 排除和桌面组合仍属于 Task 4；完整 clean-HEAD 检查与真实模型/SSH 验收仍待交付前记录。Windows 路径规范化与增量缓存提示是通过日志中的环境警告，最终交付记录须保留。

### Task 4: 宿主租约、resume composition 和 manual 命令

**Files:** Modify `src-tauri/src/agent_v4.rs:spawn_execution/compose`、`src-tauri/src/context_compaction.rs`、`src-tauri/src/dto_contract_tests.rs`；Test 既有 mock resources/model ports 与 context_compaction 模块。

**Interfaces:**
- Consumes：Task 3 getter/snapshot/commit；Task 2 prepare；原 `try_run_lease`、active_runs、frozen profile/selection、input_images/preferences/service_tier/reviewer/delegated bindings。
- Produces：private `compose_frozen_ordinary_run_v4(state: &AppState, record: &RunRecordV4, spec: &RunSpecV4) -> Result<OrdinaryRunCompositionV4, String>` async，`OrdinaryRunCompositionV4 { model: Arc<DesktopModelPortV4>, tools: ComposedToolsV4, limits: AgentLimitsV4 }` 仅 agent_v4 内部；普通 spawn 与 manual 共用，lazy compute 不变。
- Produces：`pub(crate) async fn prepare_manual_compaction_v4(state: &AppState, snapshot: &CompactionSnapshotV4) -> Result<PreparedContextCompactionV4, String>`；Tauri `agent_v4_compact_context(app: AppHandle, state: State<'_, AppState>, request: CompactContextRequestV4) -> Result<ContextCompactionReceiptV4, String>` 沿用原签名。

- [ ] **RED 写测试**：`manual_compaction_host_commits_paused_ordinary_run`、`manual_compaction_host_rejects_lease_and_frozen_profile_change`、`manual_compaction_host_never_initializes_or_streams`、`manual_compaction_error_is_safe_and_receipt_retry_is_idempotent`。模拟大 schema/图片预算与 native replay，不调用网络。

```rust
assert_eq!(receipt.status, ContextCompactionStatusV4::Completed);
assert_eq!((streams, executes, factory_initializations, ssh_connects), (0, 0, 0, 0));
assert_eq!(run_status_after, "waiting_for_input");
assert!(!public_error.contains(secret_sentinel));
```

- [ ] **RED 运行**：`cargo test -p omicsops-desktop --lib manual_compaction_host`。预期当前仅 completed gate 失败。
- [ ] **实现接口**：先读取已成功同请求 receipt 并核验 scope；新操作持 run lease 至 commit、确认无 active driver。completed 用原 no-op；普通 pause 获取 snapshot、同 frozen resume composition prepare、commit。重新核验配置，白名单错误不泄露原异常；成功不注册/启动 driver。外部规则变化由正常 resume 再准入，不声称文件系统/SQLite 跨域事务。
- [ ] **GREEN**：重复 RED；`cargo test -p omicsops-desktop --lib dto_contract_tests`；既有 lazy resources、model budget/replay/profile tests。预期 Completed 实际有 archive/checkpoint，不以 NotNeeded 冒充实现。
- [ ] **Commit**：仅暂存本任务文件，`git commit -m "feat(desktop): enable guarded manual context compaction"`。

**实施证据（2026-10-08）**：Task 4 提交 `f6d9f13` 经独立完整任务审阅通过，无待修复发现。宿主持有真实 run lease 至原子提交并复查活动 driver；普通恢复与手动准备共用冻结配置组合，手动路径不派发模型、工具、计算或 SSH。实际图片字节和完整请求参与预算验证，未知图片成本保守拒绝；同请求 receipt 幂等且公开错误固定脱敏。最终源上宿主定向 8、desktop 581（11 ignored）、workspace 1714（18 ignored）、前端 1251 加扩展 22 通过，Web 与 Windows NSIS 构建、格式及 diff 检查通过。NSIS 仅构建，未安装或分发；测试包含工作区原有未提交改动，最终 clean committed-HEAD 与真实模型/SSH/GUI 验收仍待交付检查。证据见 `.superpowers/sdd/2026-10-04-wisp-w05-context-compaction/task-4-report.md` 与 `task-4-review.md`。

### Task 5: 接通 UI、真实恢复回归与交付记录

**Files:** Modify `src/context-usage-api.ts`、`src/features/workspace/ContextUsagePanel.tsx`、`src/features/workspace/ContextUsagePanel.test.tsx`、`src/features/workspace/WorkspaceShell.tsx`、`src/features/workspace/WorkspaceShell.test.tsx`、`src/features/workspace/useContextUsage.ts`、`README.md`、`acceptance/README.md`；Create `src/features/workspace/useContextCompaction.ts`、`src/features/workspace/useContextCompaction.test.tsx`。

**Interfaces:**
- Consumes：原 CompactContextRequest/receipt 及 Task 4 命令；现有 Run summary/events、问题答案与 resume 入口。
- Produces：`compactContextV4(request: CompactContextRequestV4): Promise<ContextCompactionReceiptV4>`；`useContextCompaction(scope: CompactContextRequestV4 | null, enabled: boolean)` 返回 `{ busy: boolean, receipt: ContextCompactionReceiptV4 | null, error: boolean, compact: () => Promise<void> }`。scope 中 request_id 为一次操作固定 ID，成功后的新明确尝试才换 ID。
- Produces：`useContextUsage` 可调用 `refresh: () => Promise<void>`；WorkspaceShell 根据普通 pause/无 pending 条件接 onCompact，宿主仍为权威 gate。

- [ ] **RED 写测试**：`manual_compact_button_requires_ordinary_pause`、`manual_compact_duplicate_click_reuses_request_id`、`manual_compact_discards_late_scope_and_retains_question`、`manual_compact_error_does_not_resume`、`context_panel_immediate_escape_keeps_parent`。

```ts
expect(compactMock).toHaveBeenCalledTimes(1);
expect(resumeMock).not.toHaveBeenCalled();
expect(screen.getByText(pendingQuestion)).toBeVisible();
expect(newConversationReceipt).toBeNull();
```

- [ ] **RED 运行**：`npx vitest run src/features/workspace/useContextCompaction.test.tsx src/features/workspace/ContextUsagePanel.test.tsx src/features/workspace/WorkspaceShell.test.tsx`；预期禁用按钮/无 receipt 行为失败。
- [ ] **实现接口**：启用符合条件按钮、busy/receipt/固定错误状态、刷新用量；不自动答案/resume，不重置会话历史；generation/Scope 丢弃旧响应，Escape 原栈不变。文档说明准入只覆盖本次快照，新答案/规则/科学变化仍需 resume 终端检查。
- [ ] **GREEN**：重复 RTL；`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、fmt check、diff check。用临时 DB 的 desktop/core 联合回归证明压缩后 answer/resume 读取同 checkpoint 且旧证据可分页；真实模型/SSH仅按 acceptance 显式执行并记录未执行。
- [ ] **Commit**：仅暂存本任务文件，`git commit -m "feat(ui): compact paused ordinary run context"`。

## Plan Self-Review

契约/纯 full-request 准备/Store 原子性/宿主 gate/UI 分别绑定 Task 1–5；Review Focus 每项已有测试。无 migration、摘要模型、审批修改或 unknown 收口；签名不漂移，private composition 不暴露到 DTO。代码片段仅断言，步骤为 RED/实现/GREEN/commit；Task1已执行并审阅，Task2–5未执行。

## 当前实施证据

2026-10-08：Task1提交c125a71及修复2a191c8，独立full/scoped审阅通过。旧Completed事件的固定JSON/hash保持可读，公开receipt新增可缺省admission字段，snapshot/prepared留在纯protocol内部；无migration或Store原子提交。实际初RED缺契约字段，desktop同次含OUT_DIR沙箱失败并分开记录；修复前补充bounded-admission RED。最终protocol33单元+10集成、DTO crate、Store压缩6、desktop DTO52、Web构建及fmt/diff通过，沙箱失败按原命令授权重试。审阅发现hash一致的非V4新计划事件/checkpoint可通过结构校验，修复用重算哈希的0/2 RED→2/2 GREEN证明V4拒绝，未改变旧事件verify或可选字段hash。全workspace、npm测试、native打包及真实模型/SSH尚未为此叶子重跑；Task2–5和最终分支检查仍待执行。
