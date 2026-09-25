# Agent Runtime Boundaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让用户和科研 Agent 从同一宿主事实了解执行位置、隔离、审批和停止/恢复边界。

**Architecture:** 纯数据 DTO 位于 `omicsops-dto`；唯一边界投影和文本格式化位于桌面宿主。只读查询在现有 `agent_v4.rs` 复用私有运行记录读取，Agent 通过已有 `ModelPortV4::prompt_layers()` 获得同一摘要，前端只翻译宿主返回的枚举/限制代码。`agent-core` 不依赖 DTO 或桌面实现，不增加执行协议或数据库模式。

**Tech Stack:** Rust workspace、serde、Tauri 2、React 19、TypeScript、Vitest、现有 SQLite Store。

**Spec:** [已确认设计](../specs/2026-09-26-wisp-agent-runtime-sandbox-design.md)。先读该文档和根目录 `AGENTS.md`。

**Source audit:** [固定提交源码对照](../specs/2026-09-26-wisp-agent-runtime-source-audit.md)。只读观察、宿主授权与远端防重派已核对到实现；上游二次取消强制收口不照搬。本计划仍待审，三个任务和接口保持不变。

## Global Constraints

- “不新增服务器 daemon、云平台、作业表、对象检查器或新的模型执行工具；不迁移 Agent 编排；不改变本机 system Python/R 与 SSH system/Micromamba 支持范围。”
- “不新增表或写回历史 run，不修改旧 RunSpec 的序列化/hash，也不回填审批记录。”
- “不触碰 `website/`。不为此变更更新版本、创建 tag/Release 或分发安装包。”
- “此命令只做本地校验、必要的 Store 读取和纯投影：不调用资源 `initialize`、不连接 SSH、不启动解释器/容器、不拉镜像、不创建环境。”
- “当前容器代码只设置 PID 上限，不笼统写‘资源配额完整’。”计算容器 network none 不代表模型/MCP/浏览器离线；整个项目目录仍为可写挂载。
- 生命周期描述不是探测结果；`ssh_linux_only` 不确认当前远端平台、Python3 或连接可用。无自动轮询、后台取消、跨重启交互内核接回。
- 使用 Windows 可运行的临时目录、假执行器和模拟 Tauri 命令；不要求真实 SSH、R、GPU、网络或密钥。保留 keyring、审批和科研证据边界。
- 用户已指定 Astra high 做架构，Sol high 实施代码，Luna max 做简单检索。此计划需用户审查后才实施；无需重问模型安排。三个任务顺序实施，每项通过验证后单独提交。

## Review Focus

1. 查看旧 SSH run 时项目已经改绑或移除连接：仍显示原运行配置，不能用当前绑定拒绝历史读取（任务 1）。
2. 容器草稿只有 tag、尚无已解析 image ID：显示配置未完成，不伪造 ID，也不因查看边界执行 inspect/pull（任务 1、3）。
3. 请求包含 `frozen_run` 和额外 `compute_selection`，或 run 内两份选择不一致：拒绝混合来源，不静默采用可变输入（任务 1）。
4. 压缩后的完整模型请求遗漏或不计算边界提示成本：candidate 和 compacted 请求的 system 均保留摘要，完整预算仍可拒绝（任务 2）。
5. 对话框开启后快速切换会话/运行、或从运行轨迹打开上层对话框：旧异步响应不得覆盖新身份，第一次 Escape 仅关闭最上层且不停止计算（任务 3）。

## 文件职责与依赖

新增 `crates/omicsops-dto/src/runtime_boundary.rs` 只定义传输类型；`src-tauri/src/runtime_boundary_v4.rs` 只承担选择到边界的纯映射和固定上限英文模型摘要。后者不能导入 Store、CredentialVault、SSH、RuntimeManager 或进程启动器。

查询 resolver 和 Tauri wrapper 放在 `src-tauri/src/agent_v4.rs`，因此无需公开私有 `RunRecordV4` 或复制其 serde 结构。该文件已有 `load_record`、`workspace_project`、`validate_compute_binding` 和 `validate_frozen_spec`；最后两者分别用于草稿绑定检查和已有冻结规格检查，**不得**在只读查询调用会探测解释器/镜像的 `validate_compute_selection` 或 `validate_container_selection`。

前端新增 `useRuntimeBoundary.ts` 管理一次性只读请求、身份过滤和重试；现有 RuntimeDialog 展示，WorkspaceShell 管理窗口状态。`V4RunTrace` 实际是 `WorkspaceShell.tsx` 内部函数，不创建同名新模块。DesktopApp 继续拥有草稿计算配置。

## Task 1: 宿主边界契约与无副作用查询

**Files:**
- Create: `crates/omicsops-dto/src/runtime_boundary.rs`、`src-tauri/src/runtime_boundary_v4.rs`。
- Modify: `crates/omicsops-dto/src/lib.rs`（导出）、`src-tauri/src/agent_v4.rs`（只读 resolver/wrapper）、`src-tauri/src/lib.rs`（模块及 invoke handler）。
- Test: 上述两个新模块内测试、`src-tauri/src/agent_v4.rs` 内现有测试模块、`src-tauri/src/dto_contract_tests.rs`。

**Interfaces:** 以下为新接口；所有 enum/struct 派生 Debug/Clone/PartialEq/Eq/Serialize/Deserialize，线格式统一 snake_case。

```rust
// omicsops-dto: deny_unknown_fields prevents overriding a frozen run.
#[serde(tag = "source", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeBoundaryRequestV4 {
    DraftSelection { project_id: Uuid, compute_selection: ComputeSelectionV4 },
    FrozenRun { project_id: Uuid, run_id: Uuid },
}
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeBoundarySourceV4 { DraftSelection, FrozenRun { run_id: Uuid } }
pub enum RuntimeExecutionLocationV4 { LocalHost, SshHost, LocalContainer }
pub enum InteractiveLifecycleV4 { RunScopedNoRestartReconnect }
pub enum DetachedJobLifecycleV4 { Unsupported, SshLinuxOnly }
pub enum RuntimeBoundaryVerificationV4 { NotCheckedByThisView }
pub enum RuntimeBoundaryLimitV4 {
    SameUserPermissions, ProjectCwdNotAccessControl, ProjectMountReadWrite,
    ReadOnlyRootfs, CapabilitiesDropped, NoNewPrivileges, PidsLimit256,
    SharedKernelNotVm,
}
pub struct RuntimeBoundaryViewV4 {
    pub project_id: Uuid,
    pub source: RuntimeBoundarySourceV4,
    pub compute_selection: ComputeSelectionV4,
    pub execution_location: RuntimeExecutionLocationV4,
    pub isolation: IsolationStrengthV4,
    pub limits: Vec<RuntimeBoundaryLimitV4>,
    pub interactive_lifecycle: InteractiveLifecycleV4,
    pub detached_job_lifecycle: DetachedJobLifecycleV4,
    pub verification: RuntimeBoundaryVerificationV4,
}
// src-tauri/src/runtime_boundary_v4.rs
pub(crate) fn describe_runtime_boundary(
    project_id: Uuid, source: RuntimeBoundarySourceV4, selection: &ComputeSelectionV4,
) -> Result<RuntimeBoundaryViewV4, String>;
// src-tauri/src/agent_v4.rs
pub(crate) async fn runtime_boundary_response(
    repository: &Store, request: RuntimeBoundaryRequestV4,
) -> Result<RuntimeBoundaryViewV4, String>;
#[tauri::command]
pub async fn agent_v4_runtime_boundary(
    state: State<'_, AppState>, request: RuntimeBoundaryRequestV4,
) -> Result<RuntimeBoundaryViewV4, String>;
```

- [ ] **1. 写失败测试。** DTO 契约测试拒绝混合来源，断言 view JSON 中 source 对象、限制代码及 `not_checked_by_this_view`。投影测试使用 serde 构造的现有 `ComputeSelectionV4`，覆盖 Local/SSH/Docker/Podman、SSH 非 system、非法 FullAccess+process、容器缺 image ID。代表性代码：

```rust
#[test]
fn runtime_boundary_request_cannot_override_frozen_run() {
    let value = json!({"source":"frozen_run", "project_id":Uuid::new_v4(),
        "run_id":Uuid::new_v4(), "compute_selection":null});
    assert!(serde_json::from_value::<RuntimeBoundaryRequestV4>(value).is_err());
}
```

Resolver 测试复用 `prepared_direct_run_retains_reserved_identity_and_frozen_material` 中 `StartDirectV4Request`、`prepare_direct_run_v4`、`DirectRunSnapshotV4` 的构造，不手拼一个不完整 spec。Store 使用 `Store::open_in_memory()` 和现有 `Project::new`/`Conversation::new` 模式；保存 record、必要的 `RunSpecFrozen` 事件后检查跨项目拒绝、缺失选择、两份选择冲突、当前连接改绑后历史仍可读、查询前后 record JSON 与事件数量相同。没有 spec 的历史/计划记录用现有 `missing_terminal_events_are_repaired_without_interrupting_live_runs` 的完整 `RunRecordV4` fixture 改造，避免省略新增字段。

- [ ] **2. 运行 RED。** `cargo test -p omicsops-desktop runtime_boundary -- --nocapture`；新增 API 尚不存在时应编译失败或行为断言失败，确认不是网络/真实运行时失败。
- [ ] **3. 实现 DTO 与纯映射。** 先 `selection.validate()`；Local/SSH 生成 Process 和前两个限制代码，Docker/Podman 生成 Container 及后六个容器限制代码。所有后端为 `RunScopedNoRestartReconnect`，只有 SSH 为 `SshLinuxOnly`。project/source 只来自宿主参数；不复制网络/审批字段，不返回 profile、路径或认证内容。摘要格式化留任务 2。
- [ ] **4. 实现 resolver 并注册命令。** wrapper 仅委托 `runtime_boundary_response(&state.repository, request)`。草稿调用 `workspace_project` 和不启动进程的 `validate_compute_binding`；完整镜像选择只做结构验证，不能重新 inspect。冻结分支先核对 `record.run_id/project_id`；`record.spec` 存在时验证其身份、`validate_frozen_spec` 完整性/事件锚点，采用 `spec.compute_selection`，若 record 外层也存在且不同则拒绝；无 spec 时可使用该 run 已保存的 `record.compute_selection`。两者均无选择返回 `runtime_boundary_missing_selection`，不得调用 `legacy_ssh_selection`。冻结查询不检查当前 SSH/环境/镜像是否仍存在，不调用当前绑定验证。其余可行动错误使用稳定前缀 `runtime_boundary_not_found`、`runtime_boundary_scope_mismatch`、`runtime_boundary_selection_mismatch` 或 `runtime_boundary_invalid_selection`，不要将凭据/底层服务器错误插入消息。
- [ ] **5. 验证 GREEN 与只读结构。** `cargo test -p omicsops-dto`；`cargo test -p omicsops-desktop runtime_boundary`；`cargo test -p omicsops-desktop dto_contract_tests`。检查新查询的调用树只有 Store/纯验证；用不安装解释器、不配置镜像引擎的合成配置仍能返回描述，测试既不调用进程 runner，也不取 keyring。缺 image ID 的草稿必须失败，不能“补全”。
- [ ] **6. 单独提交。** `git diff --check` 后只暂存本任务文件，提交 `feat(runtime): expose read-only execution boundary views`。未通过测试不提交成功声明。

## Task 2: Agent 上下文复用同一边界事实

**Files:** Modify `src-tauri/src/runtime_boundary_v4.rs`、`src-tauri/src/agent_v4.rs`；Test `crates/omicsops-agent-core/src/lib.rs` 现有测试模块。不修改 core 的生产协议或 Cargo 依赖。

**Interfaces:** 消费任务 1 的 `describe_runtime_boundary`；新增 `pub(crate) fn format_runtime_boundary(view: &RuntimeBoundaryViewV4) -> String`。输出以 `HOST RUNTIME BOUNDARY` 标记，最多 4,096 UTF-8 字节；固定模板只列枚举值和已验证且最多 128 字符的 environment，完整 backend/image 身份仍保留在已有 context 的 `compute_selection`，不截断后冒称完整。

- [ ] **1. 写失败测试。** 在桌面 `budget_test_model` 上放入实际投影生成的摘要；验证 `prepare_request(request, false)` system 保留它，`validate_request` 的完整预算计入它。复用 `mock_resource_slot`/`MockResources`，只读取 `prompt_layers` 时 `attempts == 0`；资源初始化后依旧包含完全相同摘要及原项目规则。覆盖四后端、不宣称依赖就绪、SSH stop 不确认远端停止、容器 network none 不涵盖模型/MCP、固定上限。

在 core 的 `full_request_budget_compacts_even_when_context_bytes_fit` 旁增加只用于测试的 `BoundaryBudgetModel { inner: BudgetOnlyModel }`：`prompt_layers()` 返回 environment 中含固定边界标记的默认 layers，`validate_request()` 委托 inner，stream 保持不应被调用。复用带冻结选择的 `ordinary_execution_spec`、`MemoryStore`、`seed_execution`、`FakeTools` 和现有 20 条长 ModelText 的构造。取得 requests 后断言：

```rust
assert!(requests.len() >= 2); // original candidate, then compacted request
for request in requests.iter() {
    assert!(request.system.contains("HOST RUNTIME BOUNDARY"));
    let context: serde_json::Value = serde_json::from_str(&request.context).unwrap();
    assert_eq!(context["compute_selection"], serde_json::to_value(&spec.compute_selection).unwrap());
}
assert!(requests.last().unwrap().context.len() < requests[0].context.len());
```

再用同样 fixture 的低完整预算证明连固定摘要也容不下时停止且不派发，不从 system 删除摘要以凑预算；原事件链保留。

- [ ] **2. 运行 RED。** `cargo test -p omicsops-desktop runtime_boundary`；`cargo test -p omicsops-agent-core runtime_boundary`（新增 core 测试名称包含该前缀）。确认失败来自未接入摘要。
- [ ] **3. 只在 desktop 组合一次摘要。** 在 `compose` 用 project_id/run_id/冻结 selection 构造 view，格式化为 `boundary_summary`。给 `DesktopResourceFactoryV4` 增加该 String 字段。用它替换约 5043 行 lazy 初始 environment 中重复的选择说明；保留资源未检查、没有 local fallback、SSH 规则需要下一模型轮次的说明。约 4820 行初始化后的 filesystem prompt 附加同一摘要，保留环境/规则事实与“filesystem 不验证依赖”说明。不要改变 `request_contains_context` 观察门禁或 `before_call` 初始化规则。

```rust
let boundary = describe_runtime_boundary(
    project.id, RuntimeBoundarySourceV4::FrozenRun { run_id }, selection,
)?;
let boundary_summary = format_runtime_boundary(&boundary);
// Factory stores boundary_summary.clone(); lazy prompt uses the same string.
```

`DesktopModelPortV4::prompt_layers` 已优先读取 ready slot，否则用初始 prompt；`AgentCoreV4::execution_request` 每次从它组装 system，`validate_execution_context` 对 candidate/compacted 都调用此方法，随后 `DesktopModelPortV4::validate_request → prepare_request(false) → client.validate_request` 检查完整 provider 请求。沿用该路径即可，不向两份 context JSON 添加 UI DTO，也不新增 core 到 desktop/dto 依赖。
- [ ] **4. 验证 GREEN 和回归。** `cargo test -p omicsops-desktop runtime_boundary`；`cargo test -p omicsops-desktop lazy_resources`；`cargo test -p omicsops-agent-core runtime_boundary`；`cargo test -p omicsops-agent-core full_request_budget`；`cargo test -p omicsops-desktop remote_jobs_v4`。新的摘要必须在原上下文预算内；不可扩大预算掩盖问题。
- [ ] **5. 单独提交。** `git diff --check` 后提交本任务三个文件：`feat(agent): share host runtime boundaries in model context`。

## Task 3: 草稿与当前/历史运行的真实边界展示

**Files:**
- Create: `src/features/workspace/useRuntimeBoundary.ts`、`src/features/workspace/useRuntimeBoundary.test.tsx`、`src/tauri-api.runtime-boundary.test.ts`。
- Modify: `src/types.ts`、`src/tauri-api.ts`、`src/DesktopApp.tsx`、`src/features/workspace/WorkspaceShell.tsx`、`src/features/workspace/RuntimeDialog.tsx`、`src/features/workspace/runtime-dialog.css`。
- Test/Update: `src/DesktopApp.test.tsx`、`src/features/workspace/RuntimeDialog.test.tsx`、`src/features/workspace/ComposerIntegration.test.tsx`、`src/features/workspace/WorkspaceShell.test.tsx`；最终更新 spec 的实施/验证状态与本计划勾选项。

**Interfaces:** TypeScript 精确镜像任务 1 JSON；不在前端重新计算 isolation/limits/lifecycle。

```ts
export function agentV4RuntimeBoundary(request: RuntimeBoundaryRequestV4): Promise<RuntimeBoundaryViewV4>;
export function useRuntimeBoundary(request: RuntimeBoundaryRequestV4 | null): {
  view: RuntimeBoundaryViewV4 | null; loading: boolean; error: string; refresh: () => void;
};
// Additional WorkspaceShell prop; absent/null means incomplete draft.
runtimeDraftSelection?: ComputeSelectionV4 | null;
// Additional RuntimeDialog props; keep existing language/close props.
boundary: RuntimeBoundaryViewV4 | null;
boundaryLoading: boolean;
boundaryError: string;
onRetryBoundary: () => void;
// onPrepare becomes optional and is supplied only for draft mode.
```

- [ ] **1. 写失败 API/hook 测试。** 参照 `tauri-api.model.test.ts` mock `invoke` 与 `__TAURI_INTERNALS__`；请求必须是 `invoke("agent_v4_runtime_boundary", { request })`。非 Tauri 直接拒绝“Runtime boundaries require the desktop app”，不能复制一套假边界映射或返回成功。hook 复用 `useConversationCapabilities.test.tsx` 的 `renderHook`/延迟 Promise/身份变化模式，新增 null 请求零调用、重试、错误后 view 清空、project/source/run/草稿完整选择不匹配拒绝、旧成功及旧失败均不能覆盖新请求。

```ts
it("does not query an incomplete draft", () => {
  const query = vi.spyOn(api, "agentV4RuntimeBoundary");
  const { result } = renderHook(() => useRuntimeBoundary(null));
  expect(query).not.toHaveBeenCalled();
  expect(result.current.view).toBeNull();
});
```

- [ ] **2. 写失败 UI 测试。** 扩展 RuntimeDialog 的 `renderDialog`/localBackend/sshBackend fixture，加入四后端中英文宿主 view；测试 available 显示“已找到解释器”、system/SSH unverified 不再称镜像、依赖未验证、容器项目可写及网络作用域、SSH Linux 限定和 Stop 语义。冻结模式不显示当前探测状态、当前环境准备或修改入口。用 `DesktopApp.test.tsx` 的 `setupConversationStateHarness`/`stateSnapshot`/`agentEvent`/`deferred` 覆盖：草稿修改不改变所查看 run；容器未解析 ID/配置加载时显示未完成且不请求边界；切换会话立即清除旧视图。

在 WorkspaceShell 的现有轨迹 fixture 中打开“运行轨迹”，点击某次 run 的“运行环境”，立即执行 `fireEvent.keyDown(window, { key: "Escape" })`：RuntimeDialog 消失、轨迹仍打开，第二次 Escape 关闭轨迹；`onCancelRun`、interrupt 与 `onPrepare` 均零调用。这是实际父子覆盖层测试，不只测试聚焦在对话框内的 Escape。
- [ ] **3. 运行 RED。** `npx vitest run src/tauri-api.runtime-boundary.test.ts src/features/workspace/useRuntimeBoundary.test.tsx src/features/workspace/RuntimeDialog.test.tsx src/features/workspace/ComposerIntegration.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/DesktopApp.test.tsx`。
- [ ] **4. 实现 API 与防过期 hook。** 用包含完整 request 的稳定 JSON key 标识请求；state 保存该 key。返回值只有 state key 与当前 key 相同时才可见，effect cleanup/disposed 防止旧 Promise 提交；null 输入同步屏蔽 view/error。加载失败不恢复上一条成功数据，refresh 只重试当前请求。核对响应 project/source/run，草稿还需逐字段比较 compute selection（包含旧 approval_policy 默认值的现有 TS 规范化），不能仅比较 backend_id。
- [ ] **5. 接线三种入口。** DesktopApp 用现有纯 `currentComputeSelection()` 构造完整草稿，捕获配置未完成并传 null，不增加后端探测。容器需当前 project/image 对应的既有 `agentV4ComputeBackends` 响应：在该 effect 保存成功响应所属 `{projectId, containerImage}`，不匹配当前输入或 computeBusy 时草稿为 null，防止新 tag 临时搭配旧 image ID。该键只防止过期观察，不改变派发验证或偷偷发新请求。

Composer Python/R 入口明确展示“下一次运行的配置”，用 `draft_selection`。给内部 `V4RunTrace` 增加 `onOpenRuntime?: (runId: string) => void` 并在当前、历史、轨迹各调用点传入；其按钮用自身 events 的 run_id 打开 `frozen_run`。WorkspaceShell 保存显式来源和语言，创建 request 传 hook；关闭或切换 project/conversation 后 request 为 null。已有 run 查询绝不使用 composer selection。
- [ ] **6. 展示和 Escape。** RuntimeDialog 从 host view 渲染边界、来源、环境/策略；完整镜像身份可以放次级详情，不将配置当运行成功。只有 draft 且请求身份匹配的当前探测数据可单独标为“最近探测”；历史 view 全部显示该视图未执行验证，不携带当前 backend 状态。无 view/不完整/错误分别显示配置未完成、加载或可重试错误，不显示安全/可用承诺；frozen 模式禁用准备环境操作。沿用 `useWindowEscapeLayer`，从轨迹打开时 runtime 层须位于 trajectory 层之上，不能依赖 DOM focus。关闭只清空展示状态。
- [ ] **7. GREEN 与完整验证。** 先重跑步骤 3 的定向命令；然后顺序执行 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`cargo fmt --all -- --check`、`git diff --check`。如仅格式失败，按 AGENTS.md 运行 fmt 并将纯格式变化单独提交；锁文件若变化必须 `npm ci`。不提交构建产物。
- [ ] **8. 记录与提交。** 更新 spec/计划中实际命令结果；清楚记录真实模型、SSH、R/Micromamba、PBMC 验收是通过、失败或未执行，不能把 ignored 写成通过。完成 Windows 对话框/冻结来源/Escape smoke；若本机无法执行记录原因。Linux SSH 真环境按 `acceptance/README.md` 的一次性环境独立执行，缺条件不扩大权限或使用真实科研项目；macOS 未验证不承诺。提交 `feat(ui): show draft and frozen runtime boundaries`。

## 自审与交接

三个任务覆盖 spec 第 4 节全部首期契约；第 7 节服务器沙盒保持后续方向。核心不新增 DTO 依赖、模型工具、运行状态机或持久表。五项 Review Focus 均已归入对应失败测试，前端不会把当前探测当历史事实，查询不会因历史 SSH 改绑而拒绝说明。

本计划尚未实施；编写计划不构成测试已通过。请用户审查计划是否准确体现已确认设计；获得审查确认后按已约定的模型分工和逐项验证/提交方式执行。
