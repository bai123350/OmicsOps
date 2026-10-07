# W04 Kernel Inspection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 Runtime 对话框观察仍存活的 V4 Python/R 内核元数据，缺失内核时不启动任何执行资源。

**Architecture:** protocol 保存纯数据，Runtime 提供非启动 lookup 和进程 inspect。宿主从冻结 Run 解析环境，经 weak live registry 查询当前资源；Local/容器简单 JSONL 与 SSH typed JSONL 分别扩展，共用元数据契约。

**Tech Stack:** Rust/async-trait/Tokio/serde，Tauri 2，React 19/TypeScript/Vitest，Python stdlib unittest；真实 R 仅显式 ignored 验收。

**Spec:** [W04 Runtime 设计](../specs/2026-10-04-wisp-runtime-design.md#w04观察当前内核中的对象)，设计及8515da8全部实施计划已确认。Task1–3协议/manager及Local/SSH worker已独立审阅/必要复审通过；Task4–5宿主/UI与完整交付仍待实施，真实worker/live验收未执行。

## Global Constraints

- “最多返回 64 个对象，最多检查 4,096 个绑定，总响应不超过 16 KiB；名称不超过 256 UTF-8 字节、摘要 128 字节、维度最多 8 个。”inspect 固定 3 秒截止。type_label同摘要限制为128 UTF-8字节；16 KiB同时检查内部payload及包含scope/session/status的完整公开response，不能漏计envelope。长标签或接近payload上限的数据可能保守拒绝，后续adapter须提供受控短标签/留出envelope空间。
- “查看不启动解释器、不连接新 SSH 会话、不安装库、不执行用户提供的代码。”不调用 acquire/initialize/launch/rebuild。
- “结果仅送本次 UI，不写事件、日志、SQLite、模型上下文、科研 Artifact 或导出。”不得展示值、repr、原始错误或敏感绑定名。
- “此项不会承诺暂停后仍能浏览内存，也不为了观察引入新的内核保活行为。”不复用 legacy active_kernels。
- “Python 只对 exact built-in 类型读取受控元数据”；R 跳过 active binding、不强制 promise、不调用 S3/S4/用户方法。未知类型 opaque/尺寸未知。
- DTO 仅重新导出 protocol；所有权、弱注册表、错误、进程锁留宿主/Runtime。所有弹层遵守窗口 Escape 栈。
- 默认 CI 不要求真实 R、SSH、模型或科学库。无未知派发重跑、无停止等待冒充远端取消。

## Review Focus

1. 未初始化资源或 historical Run：返回 NotStarted 且 launch/connect/initialize 均为零（Task 1、4）。
2. 请求刚完成时 session 已被替换：返回 StaleSession，不投递旧对象（Task 1、4、5）。
3. 超时/错序帧后残余响应：标不可复用，不杀/重建/重试执行；Busy 不改变健康（Task 2、3）。
4. 恶意 Python methods、R binding/promise：不执行用户代码，安全未知值退为 opaque（Task 2、3）。
5. 超量对象/敏感哨兵/离屏响应：预算严格、UI 脱敏、无持久化、Escape 只关顶层（Task 1、4、5）。

---

### Task 1: 纯协议和非启动 manager lookup

**Files:** Modify `crates/omicsops-protocol/src/lib.rs`；Create `crates/omicsops-protocol/src/kernel_inspection.rs`；Modify `crates/omicsops-dto/src/lib.rs`、`crates/omicsops-runtime/src/lib.rs`；Test 同文件模块及 `src-tauri/src/dto_contract_tests.rs`。

**Interfaces:**
- Consumes：现有 `ExecutionContextKeyV4`、`KernelLanguageV4`、`KernelProcessV4::session_id/is_healthy`。
- Produces：设计原名 `InspectKernelRequestV4/KernelObjectMetadataV4/KernelInspectionV4/KernelInspectionStatusV4`；`KernelInspectionPayloadV4 { objects: Vec<KernelObjectMetadataV4>, truncated: bool }` 放 protocol。`KernelInspectionErrorV4 { Unsupported, Busy, Unavailable, Protocol }`、`KernelInspectionResultV4 { status: KernelInspectionStatusV4, session_id: Option<Uuid>, payload: Option<KernelInspectionPayloadV4> }` 留 Runtime。
- Produces：`KernelProcessV4::inspect(&self) -> Result<KernelInspectionPayloadV4, KernelInspectionErrorV4>` 为 async、默认 Unsupported；`RuntimeManagerV4::inspect_existing(&self, key: &ExecutionContextKeyV4, expected_session_id: Option<Uuid>) -> KernelInspectionResultV4` 为 async。
- Produces：`validate_inspection_payload(payload: &KernelInspectionPayloadV4) -> Result<(), KernelInspectionValidationErrorV4>` 为 protocol 纯预算验证；错误为 TooManyObjects/TooLarge/InvalidDimension/InvalidText 固定枚举，常量来自 Global Constraints。

- [x] **RED 写测试**：`inspect_existing_never_acquires`、`inspect_rejects_replaced_session`、`inspection_contract_is_bounded`；fixture 使用现有 fake backend/process，观察前后核对计数，不先 acquire。

```rust
assert_eq!(result.status, KernelInspectionStatusV4::NotStarted);
assert_eq!(launches.load(Ordering::SeqCst), 0);
assert_eq!(old_session_result.status, KernelInspectionStatusV4::StaleSession);
assert!(validate_inspection_payload(&over_64_objects).is_err());
```

- [x] **RED 运行**：`cargo test -p omicsops-runtime inspect_`；`cargo test -p omicsops-protocol inspection_`。预期缺失接口/断言失败，记录实际原因。
- [x] **实现接口**：仅查 sessions，不创建或替换；调用进程后再次查同一 session。Ready 才带 payload，其余状态 payload=None；纯类型 serde unknown fields 拒绝，旧 DTO 可读。
- [x] **GREEN**：重复上述命令，再 `cargo test -p omicsops-desktop --lib dto_contract_tests`；预期全部通过，DTO 无 runtime 依赖。
- [x] **Commit**：仅暂存本任务文件，`git commit -m "feat(runtime): add non-starting kernel inspection contract"`。

### Task 2: Local/容器 worker 与安全 Python metadata

**Files:** Modify `crates/omicsops-runtime/src/process_backend.rs`；Create `scripts/test_kernel_inspection.py`（只测试 helper）；Test process_backend 内模块。

**Interfaces:**
- Consumes：Task 1 trait/payload/validator/error 与现有 ProcessKernelV4 Mutex/健康标记。
- Produces：`ProcessKernelV4::inspect`；简单请求 `{ action: "inspect", request_id }`、单个终端 `{ kind: "inspection", request_id, objects, truncated }`，execute 原格式不变。
- Produces：嵌入 Python 的 `inspect_objects(scope: dict) -> dict`，返回 objects/truncated；嵌入 R 的 `inspect_bindings(scope) -> list` 返回相同字段。worker 内 helper 不成为 Agent tool。

- [x] **RED 写测试**：`inspect_busy_preserves_health`、`inspect_protocol_timeout_marks_unhealthy_without_interrupt`、`inspect_frame_identity_and_budget`。Python unittest 从 Rust 嵌入源码 AST 白名单加载 metadata helper/常量，不运行主循环，测试 `test_exact_builtins_and_opaque_methods`、`test_object_and_utf8_budgets`、`test_secret_names_have_no_values`。

```rust
assert_eq!(busy, Err(KernelInspectionErrorV4::Busy));
assert!(healthy_after_busy);
assert!(!healthy_after_sent_request_timeout);
assert_eq!((executes, launches, kills), (0, 0, 0));
```

```python
self.assertEqual(method_calls, [])
self.assertIsNone(opaque_object["element_count"])
self.assertLessEqual(len(payload["objects"]), 64)
self.assertNotIn(secret_sentinel, serialized_payload)
```

- [x] **RED 运行**：`cargo test -p omicsops-runtime inspect_`；`python -B -m unittest discover -s scripts -p "test_kernel_inspection.py"`。预期 inspect action/helper 缺失或断言失败。
- [x] **实现接口**：try_lock 同一 process handle；固定 3 秒/16 KiB 截止和 request ID 核验。sent 请求失步/超时才置 unhealthy，不中断/rebuild；Busy 不发帧。Task1 manager有外层3秒deadline，worker须在写入可能已发送后持drop-safe健康guard：外层先取消future也置不可复用，不能只依赖内层timeout分支；新增取消后health回归，正常验证完成才解除guard。正常pending期间health保持true；已取得同一Arc的execute可能排队在进程锁后，故既有execute只增加拿锁后/写请求前health重验，拒绝已失步会话并覆盖真实queued regression，健康时wire/success/error生命周期保持。Python exact 内建类型使用受控 primitive 元数据；R 仅不求值描述/无 class 基础属性，无法安全确认则 opaque；无 `get` 强制求值。容器自然复用协议。
- 审阅buffer边界finding：终端验证后及inspect/execute写请求前，还需检查当前buffer外已经排队的额外数据，不能把一次async poll的Pending当clean。采用private descriptor readiness：Windows PeekNamedPipe/Linux及macOS poll(timeout0)，无新增依赖、读者、协议或delay；Windows须验证live empty与EOF/error，Linux/macOS代码条件ABI只静态核对、未执行。Microsoft文档指出synchronous handle在多线程下可能阻塞，因此execute也须在可能写入至已验证terminal之间持drop-health guard，取消后先置不可复用，阻止旧Blocking读进入native probe；terminal后/archival前解除，正常pending保持healthy。公开拒绝不是计算取消。检查只是当下可检测的queued数据，不保证拦截clean检查后任意异步新帧。
- [x] **GREEN**：重复 RED 命令；增加保留 execute/capture 回归，预期旧行为通过；无需安装科学库或 R。
- [x] **Commit**：仅暂存本任务文件，`git commit -m "feat(runtime): inspect local kernel metadata without execution"`。

### Task 3: SSH typed worker 和隔离 R 验收入口

**Files:** Modify `crates/omicsops-agent/src/lib.rs`、`crates/omicsops-adapters/src/kernel.rs`、`src-tauri/src/agent_v4.rs:SshKernelProcessV4`、`scripts/test_kernel_inspection.py`、`acceptance/README.md`；窄幅增加 `crates/omicsops-adapters/src/ssh.rs` producer（inspection bounded receive/current queue readiness，既有receive/send/shutdown保留）。审阅fix增加canonical `scripts/kernel_inspection_helpers.py` / `scripts/kernel_inspection_helpers.R`，以及 `crates/omicsops-runtime/src/process_backend.rs` Local consumer；仅提取重复安全helper/固定预算，不改两个transport envelope/loop。

**Interfaces:**
- Consumes：Task 1 payload/validator；Task 2 helper 行为和测试加载接缝；现有 KernelEventDecoder identity/sequence 与 SSH process Mutex。
- Produces：`KernelRequest::Inspect { session_id: Uuid, request_id: Uuid }`；`KernelEventKind::Inspection { objects: Vec<KernelObjectMetadataV4>, truncated: bool }`；`SshKernelProcessV4::inspect` 同 Task 1 trait。
- Produces：SshJsonlProcess的inspection-only bounded receive与ensure_clean，由KernelConnection委派；fill_buf累积前限16 KiB，不用原read_line分配完整内容后才检查。Russh0.62.4控制消息会wake再Pending，故readiness仅在观察到wake时有限重试，最多4096次，未wake的Pending仅表示当前可观察队列为空，queued/EOF/error/超限保守拒绝，不保证network-in-flight或后到帧。无第二读者/新连接/依赖或delay。
- Produces：每语言唯一canonical metadata定义/固定policy，经compile-time concat/include_str嵌入Local与SSH；不增加runtime文件加载、CLI/依赖，header/import/captured-builtin/helper/loop顺序保持。两种实际assembled worker的adversarial与provenance测试核验唯一policy inclusion/definition；原四worker字符串byte equality作为重构证据。补direct paused-time3秒 expiry与Unavailable/health false/无后续send，原deadline正确baseline PASS不虚称修复。
- Produces：ignored `live_r_inspection_bindings_have_no_side_effects`，只执行一次性本地 R worker；显式 `OMICSOPS_LIVE_R_INSPECTION_DISPOSABLE=1`，不连接 SSH/真实项目。

- [x] **RED 写测试**：`inspect_typed_events_reject_identity_sequence_and_duplicate_terminal`、`ssh_inspect_does_not_execute_or_reconnect`、旧 execute/shutdown JSON snapshot；Python helper unittest 同时覆盖 SSH worker。ignored R fixture 放 active binding/delayedAssign 哨兵，核对 metadata 不触发。

```rust
assert!(decoder.accept(wrong_session_frame).is_err());
assert_eq!((fake_transport.connects(), fake_transport.executes()), (0, 0));
assert_eq!(r_binding_side_effect_count, 0); // 仅 opt-in R 测试
```

- [x] **RED 运行**：`cargo test -p omicsops-agent inspection`；`cargo test -p omicsops-adapters kernel`；`cargo test -p omicsops-desktop --lib ssh_inspect`；Python discovery 同 Task 2。预期 typed inspect 接口缺失/行为失败。
- [x] **实现接口**：增 typed variant、双 worker inspect 分派，旧 execute/shutdown 原序列化不变；不以任意代码模拟 inspect。实现 SSH try_lock、3 秒截止、严格 ID/sequence、预算与健康语义；同Task2覆盖manager外层deadline取消sent future的drop-safe不可复用guard，不连接新SSH、不kill/rebuild。检查SSH既有execute拿锁后健康重验、可能写入到verified terminal的取消guard与当前transport可检测queued/pre-send污染；不要假定一次Pending就是clean，不机械移植ChildStdout的native API。正常pending healthy、terminal后/ordinary结果处理前解除，未知派发不重试/不称计算取消；仍不保证clean检查之后任意异步帧。为所有现有 exhaustive match 增必要分支。ignored R 测试文档列命令及未执行语义。
- [x] **GREEN**：重复 RED 命令；真实 R 在完成默认确定性检查后按 acceptance 文档显式执行或记录未执行，不能把 ignored 当通过。
- [x] **Commit**：仅暂存本任务文件，`git commit -m "feat(runtime): add scoped SSH kernel inspection protocol"`。

### Task 4: 宿主 weak live registry 和只读命令

**Files:** Modify `src-tauri/src/agent_v4.rs:compose/ExecutionResourcesSlotV4`、`src-tauri/src/commands.rs:AppState`、`src-tauri/src/lib.rs`；Create `src-tauri/src/kernel_inspection_v4.rs`；Test 新模块及 agent_v4 existing mock resources。

**Interfaces:**
- Consumes：Task 1 manager；Task 3 SSH trait；现有 frozen Run/compute scope 验证、`ExecutionResourcesSlotV4.ready.get()`。
- Produces：`pub(crate) struct LiveRuntimeRegistryV4`，键 `LiveRuntimeScopeV4 = (Uuid, Uuid, Uuid)`，值弱 slot+registration generation；`LiveRuntimeRegistryV4::register(&self, scope: LiveRuntimeScopeV4, slot: &Arc<ExecutionResourcesSlotV4>) -> LiveRuntimeRegistrationV4`，Drop 仅清自己代次。guard 字段为 `ComposedToolsV4._runtime_registration: Option<LiveRuntimeRegistrationV4>`，compose默认None；实际execution driver所有权注册/取消重验后赋Some。guard或整个tools显式移入async driver并持有至结束，避免Rust仅捕获tools.registry导致其他字段提前drop；guard不持slot强引用。planning不执行持久内核，无须为其伪造可观察session。
- Produces：`pub(crate) async fn inspect_live_kernel_v4(state: &AppState, request: &InspectKernelRequestV4) -> Result<KernelInspectionV4, String>`（agent_v4 内部）；`pub async fn agent_v4_inspect_kernel(state: State<'_, AppState>, request: InspectKernelRequestV4) -> Result<KernelInspectionV4, String>`（新命令）。

- [ ] **RED 写测试**：`inspection_host_rejects_foreign_scope_and_override`、`inspection_host_never_initializes_resources`、`inspection_registry_old_guard_cannot_remove_new_slot`、`inspection_host_redacts_ephemeral_metadata`，以及 `inspection_driver_guard_survives_spawn`（真实异步driver期间条目存在、结束后移除）、`inspection_dry_compose_is_unregistered`（干式组合不会覆盖live slot）。

```rust
assert_eq!((factory.initializations(), backend.launches()), (0, 0));
assert!(foreign_scope_result.is_err());
assert!(registry.contains_current_generation(scope));
assert_eq!(persisted_event_count_after, persisted_event_count_before);
assert!(!serde_json::to_string(&response).unwrap().contains(secret_sentinel));
```

- [ ] **RED 运行**：`cargo test -p omicsops-desktop --lib inspection_host`；`cargo test -p omicsops-desktop --lib inspection_registry`。预期注册表/命令缺失或断言失败。
- [ ] **实现接口**：compose不登记；实际execution driver通过ownership/cancel gate后注册已创建slot（从model.resources取得），显式将guard或整个tools移入driver；W05 manual/目录等干式组合保持None，不促发initialize。scope从Store frozen run验证，前端不提供backend/environment。升级weak后只ready.get；结果再核验session/generation，固定原因映射，用户元数据不记录日志/事件。补齐scope并脱敏后调用 `KernelInspectionV4::validate()` 再返回，不能仅核验worker payload而漏计完整response字节；增加near-limit envelope保守拒绝测试。AppState初始化和现有测试fixtures同步补默认registry。
- [ ] **GREEN**：重复 RED；`cargo test -p omicsops-desktop --lib dto_contract_tests`；预期越权拒绝、默认 lazy compute 回归通过。
- [ ] **Commit**：仅暂存本任务文件，`git commit -m "feat(desktop): expose live kernel metadata inspection"`。

### Task 5: Runtime UI、完整验证与 smoke 记录

**Files:** Modify `src/types.ts`、`src/tauri-api.ts`、`src/features/workspace/RuntimeDialog.tsx`、`src/features/workspace/RuntimeDialog.test.tsx`、`src/features/workspace/WorkspaceShell.tsx`、`src/features/workspace/WorkspaceShell.test.tsx`；Create `src/features/workspace/useKernelInspection.ts`、`src/features/workspace/useKernelInspection.test.tsx`；Modify `README.md`、`acceptance/README.md`。

**Interfaces:**
- Consumes：`agent_v4_inspect_kernel` 与 Task 1 DTO。
- Produces：`inspectKernelV4(request: InspectKernelRequestV4): Promise<KernelInspectionV4>`；`useKernelInspection(scope: { projectId: string; conversationId: string; runId: string; language: KernelLanguage } | null, enabled: boolean)` 返回 `{ value: KernelInspectionV4 | null, loading: boolean, error: boolean, refresh: () => Promise<void> }`，前端语言复用现有 KernelLanguage，Rust DTO 仍是协议 enum。
- Produces：RuntimeDialog Props 增 `kernelInspection?: KernelInspectionV4 | null`、`kernelInspectionLoading?: boolean`、`kernelInspectionError?: boolean`、`onRefreshKernelInspection?: () => void`；已冻结 Run 接入，不复用 legacy kernel sessions。

- [ ] **RED 写 RTL 测试**：`inspection_refresh_is_manual_and_scope_bound`、`inspection_busy_is_not_ready`、`inspection_discards_rebuilt_session_and_closed_dialog_response`、`runtime_escape_closes_only_top_overlay`。

```ts
expect(inspectMock).toHaveBeenCalledTimes(1);
expect(screen.queryByText("old_session_object")).not.toBeInTheDocument();
expect(screen.getByText(/未启动|Not started/)).toBeVisible();
expect(parentOverlay).toBeVisible(); // 打开立即 Escape 后
```

- [ ] **RED 运行**：`npx vitest run src/features/workspace/RuntimeDialog.test.tsx src/features/workspace/useKernelInspection.test.tsx src/features/workspace/WorkspaceShell.test.tsx`；预期入口/状态/作用域测试失败。
- [ ] **实现接口**：固定来源、无内容元数据、NotStarted/Busy/Stale/Unavailable 与手动刷新；请求 capture scope/session/generation，切换/关闭丢弃旧响应，无自动轮询；按既有 Escape 栈接入。
- [ ] **GREEN**：重复 RTL；Python discovery；`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`cargo fmt --all -- --check`、`git diff --check`。记录真实 R/SSH/Windows GUI smoke 的执行或未执行；只允许一次性环境，mock/build 不代替端到端。
- [ ] **Commit**：仅暂存 W04 UI/文档，`git commit -m "feat(ui): show bounded live kernel metadata"`；不得提交 target/dist/node_modules 或报告产物。

## Plan Self-Review

已将设计的非启动、双协议、安全元数据、失败健康语义、ephemeral宿主和UI分别绑定Task1–5；每项有具名测试，未新增CLI/DB/权限路径。Task1–3已实现并审阅，Task4–5未完成，不把协议/helper/mock通过记为真实worker或完整功能交付。

## 当前实施证据

2026-10-08：Task1提交a1a7467独立spec/quality审阅通过，无Critical/Important。实际RED runtime31/protocol19缺接口编译错误，中间方法误落测试模块的编译失败保留且已修正。最终runtime inspect5/protocol inspection3/desktop DTO51通过，fmt/diff通过；原OUT_DIR/fmtOS5沙箱日志与授权原命令重跑保留。五路径只含protocol/DTO/runtime契约及测试，external文件完整保留未暂存，既存canonicalization/cache warning非blocking Minor。检查证明零launch、旧session及相同UUID不同Arc拒绝、固定3秒截止、Ready才有payload、完整公开JSON16384/16385字节边界；type_label受控128字节。Manager外层deadline使Task2/3sent取消drop-safe健康guard为必需，Task4scope投影后必须完整validate；均未提前实现或宣称已通过。完整W04/default/native、真实Python/R/SSH/模型/GUI验收仍未执行，Task5及最终分支gate另行负责。

Task2提交45dd6db/91964e3经full/scoped独立审阅通过，无open blocking finding。Python exact builtin/opaque/no-method helper4通过；原Rust14及preserved13是fix前证据。队列执行race真实13通过/1失败→post-lock健康guard后14通过，仍无kill/rebuild/重试/计算取消承诺。审阅I1真实duplex边界16测试15通过/1失败证明8192/16384 dirty terminal接受/health true；next-write及Windows acknowledged queued pre-send各1真实失败。旧execute取消后healthy true的1失败也保留，补sent至verified terminal的drop guard后阻断native入口。最终Windows inspect20/preserved process_backend19通过/0 ignored，fmt/diff通过，unique8192/live empty/EOF/queued及healthy artifacts/capture/failed-code同session实际核验；未复跑未变Python，不把4通过称新fix结果。ABI/Tokio completed-buffer与取消析构顺序经独立复审支持quiescent-read invariant；native peek无无条件nonblocking保证，Linux/macOS未编译/执行，后到帧限制明确。Mock Python协议进程与helper单测不等于真实worker/science验收；实际R/容器/SSH/模型/GUI、CI版本与全产品交付仍未执行，Task5/最终gate负责。

Task3提交f644d0c/ef7da02经full/scoped独立审阅通过。Typed Inspector、bounded receive与wake-aware russh queue、sent/partial/execute-read取消guards和真实mutex queued拒绝有回归；旧execute/shutdown wire、正常stdout/stderr/artifact/failed-code保持。实际初RED agent2/adapter1/Python8中4SSH缺helper，取消4中2失败，parser2通过/1失败；临时移除新queued guard的1失败是controlled assertion-sensitivity，不冒称原BASE缺陷。审阅I1复制policy由canonical Python/R唯一源修复，10 Python tests的8 adversarial通过/2 missing-provenance失败后10通过；四assembled worker bytes与fix BASE完全一致，编译后inclusion/definition一次。MinorM2新增0/2999 Pending healthy、EXACT3000 Ready Unavailable/health false/无后续send，生产deadline未改且原正确baseline PASS。最终Local20/adapters5 unit+3 integration/1 R ignored、protocol3、desktop7、Python10、fmt/diff通过；已保存OUT_DIR/OS5重跑及一次model capacity同writer恢复记录。Shared agent/acceptance candidate-only staging、forward/reverse及raw outside proofs由独立审阅核验；原agent390行/acceptance18行仍未暂存，其他298/299 Rust raw文件未变。Real R fixture虽已写且编译，仍ignored/未执行；不能称R no-side-effect、真实SSH/科学/GUI/CI验收通过。Cache warnings保留非blocking，full W04/default/native/clean HEAD由Task5/最终gate执行；宿主weak registry、redaction及完整公开response validate尚待Task4。
