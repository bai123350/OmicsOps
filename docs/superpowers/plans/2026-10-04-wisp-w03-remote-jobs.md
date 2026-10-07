# W03 Remote Job Observation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 无需重新启动 Agent 即可查看已派发 SSH 后台作业，手动查询状态，并明确启用一次有限的退避查询窗口。

**Architecture:** Store读取project/conversation-owned原作业与冻结身份；宿主先验证当前绑定，再只调用已有status协议。DTO分别表示持久记录与观察，UI单项串行调度。

**Tech Stack:** Rust/sqlx/Tokio、现有 remote_jobs_v4 Transport与SshSession、Tauri 2、React 19/TypeScript/Vitest；无新表或依赖。

**Spec:** [W03 设计](../specs/2026-10-04-wisp-composer-files-jobs-design.md#w03ssh-后台作业列表与查询)、[共同约束](../specs/2026-10-04-wisp-improvements-design.md)。设计及8515da8实施计划已确认；Task1 Store/DTO、Task2 host status已独立审阅/必要fix复审通过，Task3–4待实施。

## Global Constraints

- metadata list只SQLite、最多100条，project/可选conversation先过滤再limit；无SSH、凭据、运行恢复或status写入。旧绑定仍列出安全记录，无时间戳旧行不推断时间。
- refresh只接受 project_id/job_id；原 frozen spec/context/root/host key/request hash 从Store读取，UI不能传binding/root/命令/code。
- whole20秒包含connection/trust/current canonical root/status，不仅invoke；当前profile信任/实际fingerprint/root必须精确匹配原记录。root别名不能仅凭未canonicalize字符串断言一致。
- 只status、原 identity、resubmitted=false；不submit/cancel/release reservation/重开Agent/生成evidence。未知/断连/超时不写failed/cancelled、不安全重跑。
- terminal ledger只在原 result/request/session/hash身份及bounded receipt校验成功时本地呈现，不需要SSH；corrupt receipt拒绝。
- public stdout/stderr各4KiB、artifact/capture最多128项且只安全相对引用/摘要；project/backend/call/environment文本用public_text限界。不返回完整RuntimeResult/根/指纹/密钥引用。
- 自动查询默认关闭，选一job串行2/4/8/16/30秒，窗口最多10次；手动刷新不叠加请求。关闭/切scope/绑定变化/终态/次数用尽停止，重启不恢复。
- Tests用Store fixture、FakeTransport、fake connector和fake timers；无真实SSH/API前提。所有覆盖层window Escape一次只关闭视觉顶层。

## Review Focus

1. 旧root/profile作业仍可列出，但别名先标需核对；刷新才canonicalize且绝不换身份（Tasks 1–2 `remote_jobs_binding_eligibility`）。
2. slow connect/root必须计入总20秒；失败不泄漏sentinel、不更改持久状态（Task 2 `remote_jobs_total_timeout`）。
3. submit ACK丢失后、重开Store后仍只status；unknown保持unknown观察并无第二次提交（Task 2 `remote_jobs_query_without_resubmission`）。
4. terminal本地receipt原始字节hash/请求身份损坏时拒绝；并发status只有一致终态可CAS成功（Task 2 `remote_jobs_terminal_receipt_integrity`）。
5. 双击/手动+timer/切project晚响应不能并发或污染新scope；停止调度不取消远端计算（Tasks 3–4 `remote_jobs_serial_and_scope`）。

---

### Task 1: Project-owned metadata/record 与纯DTO

**Files:** Modify `crates/omicsops-store/src/runtime_jobs.rs`、`crates/omicsops-store/src/lib.rs`（仅重导出两个内部读取结果类型）；Create `crates/omicsops-store/tests/remote_job_observation.rs`、`crates/omicsops-dto/src/remote_jobs.rs`；Modify DTO `src/lib.rs`、`src-tauri/src/dto_contract_tests.rs`。

**Interfaces:** Store内部 `RemoteJobMetadataV4 {job:RuntimeJobV4,conversation_id:Uuid}`、`StoredRemoteJobV4 {job:RuntimeJobV4,conversation_id:Uuid,frozen_spec:RunSpecV4,result:Option<RuntimeResultV4>}`；`remote_job_metadata_v4(&self,project_id:Uuid,conversation_id:Option<Uuid>)->Result<Vec<RemoteJobMetadataV4>,StoreError>`；`remote_job_record_v4(&self,project_id:Uuid,job_id:Uuid)->Result<Option<StoredRemoteJobV4>,StoreError>`。既有 remote_runtime_jobs_v4 scope/query保持兼容。

DTO request `ListRemoteJobsV4Request {project_id:Uuid,conversation_id:Option<Uuid>}`、`RefreshRemoteJobV4Request {project_id:Uuid,job_id:Uuid}`；enum `RemoteJobRecordedStateV4::{Reserved,Running,Unknown,Succeeded,Failed}`、`RemoteJobQueryEligibilityV4::{NeedsRootCheck,BindingChanged,Unsupported,IntegrityMismatch,RecordedReceipt}`、`RemoteJobObservationStatusV4::{Running,Unknown,Completed,Timeout,Unavailable,Rejected}`；`RemoteJobSummaryV4 {project_id:Uuid,conversation_id:Uuid,job_id:Uuid,run_id:Uuid,call_id:String,backend_id:String,language:String,environment:String,request_sha256:String,recorded_state:RemoteJobRecordedStateV4,query_eligibility:RemoteJobQueryEligibilityV4,recorded_at:Option<String>}`；`RemoteJobsPageV4 {jobs:Vec<RemoteJobSummaryV4>,limit:u32,remote_liveness_checked:bool}`；`RemoteJobCaptureReferenceV4 {relative_path:String,sha256:String,size_bytes:Option<u64>}`；`RemoteJobObservationV4 {job:RemoteJobSummaryV4,status:RemoteJobObservationStatusV4,observed_at:String,stdout_excerpt:String,stderr_excerpt:String,captures:Vec<RemoteJobCaptureReferenceV4>,resubmitted:bool}`。serde snake_case/deny_unknown_fields，旧job recorded_at=None。

- [x] **RED:** tempStore真实frozen run/job构造，project/会话ownership与filter-before-limit；其它project ID拒绝、非detached排除、旧绑定保留；record加载验证spec integrity/context/run/request/canonical original ToolRequested hash和已存result原serialized字节hash。损坏返回安全错误，不默默供重新提交。

```rust
assert_eq!(metadata.iter().filter(|item| item.conversation_id == conversation_a).count(), metadata.len());
assert!(store.remote_job_record_v4(other_project, job_id).await.unwrap().is_none());
assert_eq!(loaded.job.request_sha256, original_call.canonical_hash().unwrap());
assert_eq!(summary.recorded_at, None);
assert!(!serialized_summary.contains("remote_host_key"));
assert!(!serialized_summary.contains("remote_root"));
```

- [x] **Run RED:** `cargo test -p omicsops-store --test remote_job_observation`、`cargo test -p omicsops-desktop --lib dto_contract_tests::remote_jobs`；缺接缝/DTO应FAIL。
- [x] **Implement:** join agent_runs ownership，校验project/context/run相符；list不加载输出/code，只有限metadata，固定safe eligibility。Selected record在只读事务内加载冻结spec、validated event chain原call和result，验证原identity/hash；boundedJSON不暴露。DTO中语言映射已有协议值，绝不复制protocol执行结构进入公共DTO；list真实limit=100、remote_liveness_checked=false。
- [x] **Run GREEN:** 同命令PASS；`cargo test -p omicsops-store --test runtime_jobs` 保持reserve/CAS原语义。
- [x] **Commit:** `feat: list project-owned detached job metadata`。

### Task 2: 状态命令与全路径 timeout/身份核对

**Files:** Create `src-tauri/src/remote_job_commands.rs`（service/tests）；Modify `src-tauri/src/remote_jobs_v4.rs`、`src-tauri/src/lib.rs`；窄幅 Modify `crates/omicsops-store/src/runtime_jobs.rs` / `tests/remote_job_observation.rs`（internal metadata has_receipt bool/owned-job EXISTS与presence/retirement测试，不读取body/code）。复用existing sync_commands canonical root、connection profile信任与SshSession；不改原Agent调用授权流程。

**Interfaces:** `list_remote_jobs_v4(state:State<'_,AppState>,request:ListRemoteJobsV4Request)->Result<RemoteJobsPageV4,String>`；`refresh_remote_job_v4(state:State<'_,AppState>,request:RefreshRemoteJobV4Request)->Result<RemoteJobObservationV4,String>`。Host injectable async `RemoteJobConnector::connect(&self,project:&Project,job:&RuntimeJobV4)->Result<BoundRemoteJobTransport,RemoteJobObservationStatusV4>`；`BoundRemoteJobTransport {transport:Box<dyn remote_jobs_v4::Transport>,canonical_root:String}`；`refresh_remote_job_with_connector(store:&Store,connector:&dyn RemoteJobConnector,request:RefreshRemoteJobV4Request)->Result<RemoteJobObservationV4,String>`。Connector owns cleanup; transport.host_key来自实际连接，service再次精确核对原root/key。metadata.has_receipt仅表示行存在，终态回执在绑定改变时仍可尝试离线校验；不是已验证正文。全20秒约束所有remote connector/trust/root/guard/status，成功有界status后的SQLite CAS原子提交不被timeout中途取消，可能另有DB等待；不称为包含DBcommit的公开总时延。local terminal validation抽取 `remote_jobs_v4::validate_recorded_result(job:&RuntimeJobV4,result:&RuntimeResultV4)->Result<(),String>` 并在现有validate_observation复用。

- [x] **RED:** fakeConnector明确记录connect/root/status且生产路径零submit；当前profile/backend/key/root不符拒绝、metadata零connection、terminal零connection；ACKlost后的status、Store reopen、unknown、并发一致/不一致terminal、原receipt byte hash错/伪session/request/artifact拒绝；safeprojection secret+64KiB logs；敏感样式出现在4KiB之后或software map敏感键/值时，拒绝发生在CAS/receipt INSERT之前，旧job不变；safe结果字节/hash保持原值。fake time connect/root耗尽20秒仍不会invoke。

```rust
assert_eq!(fake_transport.actions(), vec!["status"]);
assert_eq!(fake_transport.submission_count(), 0);
assert!(!observation.resubmitted);
assert_eq!(terminal_connector.connection_count(), 0);
assert!(observation.stdout_excerpt.len() <= 4096);
assert!(!serde_json::to_string(&observation).unwrap().contains("secret-sentinel"));
assert_eq!(stored_after_timeout.state, stored_before_timeout.state);
```

- [x] **Run RED:** `cargo test -p omicsops-desktop --lib remote_job_commands`；Expected FAIL，缺commands/service/helper或identity/no-connect/whole-timeout断言失败。
- [x] **Implement:** 首先Store精确owner record；有validated terminal receipt直接public投影；其它路径读取当前project/profile并核对binding和trust，用20秒包connector+canonicalroot+status全链，status前重验当前绑定；已发出的只读请求不声称可原子撤回。共享validate_recorded_result在validate_observation返回typed结果前实施身份/限额/安全引用及敏感样式拒绝，覆盖所有持久化文本及结构敏感标签，固定error；不改原可接受结果字节/hash。复用remote_jobs_v4::query固定Some(job_id)、原backend/root及实际key；验证返回原job和结果，CAS沿原逻辑。timeout/unavailable安全观察不修改job；binding/identity失效固定拒绝。safe errors无底层字符串；projection将远端capture绝对archive路径只投影已验证job目录内相对引用。
- [x] **Run GREEN:** 上述及 `cargo test -p omicsops-desktop --lib remote_jobs_v4::tests` PASS；ignored live test数量明确记录。未运行真实SSH。
- [x] **Commit:** `feat: observe detached jobs with frozen identity checks`。

### Task 3: 单项串行退避与独立列表面板

**Files:** Modify `src/types.ts`、`src/tauri-api.ts`；Create `src/remote-job-api.ts`、`src/remote-job-api.test.ts`、workspace目录 `useRemoteJobs.ts`、`useRemoteJobs.test.tsx`、`RemoteJobsPanel.tsx`、`remote-jobs.css`、`RemoteJobsPanel.test.tsx`。

**Interfaces:** wrappers `listRemoteJobsV4(request:ListRemoteJobsV4Request):Promise<RemoteJobsPageV4>`、`refreshRemoteJobV4(request:RefreshRemoteJobV4Request):Promise<RemoteJobObservationV4>`，invoke `{request}`。Hook `useRemoteJobs(scope:{projectId:string;conversationId:string|null;bindingVersion:string},open:boolean):{jobs:RemoteJobSummaryV4[];selectedJobId:string|null;observation:RemoteJobObservationV4|null;loading:boolean;error:string;polling:boolean;attempts:number;selectJob:(id:string|null)=>void;reload:()=>Promise<void>;refresh:()=>Promise<void>;startPolling:()=>void;stopPolling:()=>void}`。Component `RemoteJobsPanel(props:{projectId:string;conversationId:string|null;bindingVersion:string;locale:Locale;onClose:()=>void}):ReactNode`；bindingVersion只是UI invalidation token，不能传宿主当授权。

- [ ] **RED:** fakeTimers/deferredPromise证明默认零refresh；opt-in单项序列2/4/8/16/30（之后30）且10次封顶；计时从每次settled后开始，无重叠；错误继续有限backoff；manual同pending不叠加、manual启动新窗口前结束旧timer；terminal/close/scope/binding/endstop；latepromise/timer丢弃。Panel recorded_state vs observation区分、无cancel/resubmit/恢复Agent按钮、immediateEscape。

```ts
expect(refreshRemoteJobV4).not.toHaveBeenCalled();
act(() => result.current.startPolling());
await act(async () => { await vi.advanceTimersByTimeAsync(1999); });
expect(refreshRemoteJobV4).not.toHaveBeenCalled();
await act(async () => { await vi.advanceTimersByTimeAsync(1); });
expect(refreshRemoteJobV4).toHaveBeenCalledTimes(1);
expect(maxConcurrentRefreshes).toBe(1);
```

- [ ] **Run RED:** `npx vitest run src/remote-job-api.test.ts src/features/workspace/useRemoteJobs.test.tsx src/features/workspace/RemoteJobsPanel.test.tsx`；缺Hook/API/panel应FAIL。
- [ ] **Implement:** 明确start/stop与唯一inFlight gate，generation同时guardtimer/promise；selectedjob/scope变化清掉旧窗口；手动默认一次，自动窗口最多10次，避免infiniteeffect重建timer。自动查询已启用时手动刷新结束旧timer，以本次手动查询为新窗口第1次，settled后再按2秒起退避；同一inFlight期间忽略重复点击。panel可切current conversation/all project过滤、safe禁用原因、recorded timestamp unknown、terminallocalreceipt说明；window Escape/focus trap沿现有overlay机制。停止只影响本地调度。
- [ ] **Run GREEN:** 同命令全部PASS；fakeTimers统一cleanup，无真实等待/SSH。
- [ ] **Commit:** `feat: add bounded remote job polling panel`。

### Task 4: 执行环境入口与准确边界文档

**Files:** Modify `src/features/workspace/RuntimeDialog.tsx`、`RuntimeDialog.test.tsx`、`WorkspaceShell.tsx`、`WorkspaceShell.test.tsx`、`ResearchOverlays.test.tsx`、`README.md`、`AGENTS.md`、`acceptance/README.md`。

**Interfaces:** RuntimeDialog Props加 `onOpenRemoteJobs?:()=>void`；WorkspaceShell持RemoteJobsPanel状态，project/conversation/current binding变化影响UI generation；共享RuntimeDialog与W04内核观察入口按总计划串行集成。readonlyjob入口不因FrozenRun而改选执行环境，不启动Agent。

- [ ] **RED:** RuntimeDialog有效入口显示project/current conversation jobs、FrozenRun仍只读、列表不触环境probe/Agent；父RuntimeDialog打开子Panel立即Escape只关子，第二次才关父；close/scope stop schedule；无支持后端安全空态。W04入口和两者window堆栈同时回归。

```tsx
fireEvent.click(screen.getByRole("button", { name: "Remote jobs" }));
fireEvent.keyDown(window, { key: "Escape" });
expect(screen.queryByRole("dialog", { name: "Remote jobs" })).not.toBeInTheDocument();
expect(screen.getByRole("dialog", { name: "Runtimes" })).toBeVisible();
expect(startAgent).not.toHaveBeenCalled();
expect(screen.queryByRole("button", { name: /Cancel remote job/ })).not.toBeInTheDocument();
```

- [ ] **Run RED:** `npx vitest run src/features/workspace/RuntimeDialog.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/ResearchOverlays.test.tsx`；缺入口/stack/stop断言应FAIL。
- [ ] **Implement:** 加入口并接入应用级/局部清理的window Escape层。README/AGENTS当前“没有自动轮询/面板”替换为“手动默认、面板明确启用有限10次窗口”；仍无job cancel/跨重启自动轮询/交互kernel重连，停止Agent仍不终止独立后台作业。acceptance写Windows fake操作及可选一次性Linux SSH真实smoke，不能宣称已运行。
- [ ] **Run GREEN:** 同命令，继而 `cargo fmt --all -- --check`、`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`。记录非零/ignored/未执行，锁变化npm ci，pure fmt独立提交。
- [ ] **Commit:** `feat: expose remote job observation in runtime workspace`；仅验证实际通过后更新清单。

## 当前实施证据

2026-10-07：Task1提交9d415c6/bad6feb经独立full/scoped审阅通过，无open finding。最终Store observation6/runtime_jobs12、先前desktop DTO3/DTO --lib22通过，fmt/diff通过。DTO初RED27为26缺类型+OUT_DIR基础设施，总数未误称纯缺接口；新增terminal退休metadata RED实际0pass/1fail→6pass。正常nonterminal None与retired body None保留；旧writer历史兼容性从committed blobs核对。Task2提交6e7519d已由独立审阅通过：service/gate14、remote3+1 ignored live、Store7/host1/fmt通过；before-CAS敏感gate与首次identity race均有实际RED，clock挂起partial结果未计PASS。Metadata EXISTS仅presence；remote20秒不含之后SQLite atomic commit等待。Task3 panel和Task4实际入口未实施；不得把leaf检查当完整W03交付或live验收。

## Plan Self-Review

Store owner/frozen identity为Task1；Task2原status协议和终态receipt/timeout；Task3单项串行退避；Task4实际入口、stack和边界文档。五项Review Focus有具体测试，无schema/credential/approval/evidence新增行为。Task1/2共用RuntimeResult内部而仅DTO投影出UI；W04共用RuntimeDialog按顺序编辑。计划命令没有实际运行，不是功能完成证明。
