# W06 Run Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在当前对话提供 run/tool/失败筛选、精确 sequence/hash 定位和宿主校验的脱敏 HTML/JSON 导出。

**Architecture:** Store 有界只读读取完整事件链，宿主生成字段白名单投影，UI 与导出共用同一查询规则。导出重新读取并比对预览 heads，只扩充已有文件保存叶子，不创建通用导出框架或第二套事件存储。

**Tech Stack:** Rust/sqlx/SQLite、serde DTO、Tauri 2 native dialog、React 19/TypeScript/Vitest；沿用 tempfile、SHA-256、现有 public_text 与 V4 链校验。

**Spec:** [W06 设计](../specs/2026-10-04-wisp-diagnostics-example-design.md#w06只读轨迹诊断与安全-htmljson-导出)、[总设计](../specs/2026-10-04-wisp-improvements-design.md)。设计基线 `161199f`；实施前读两份设计与 AGENTS.md。

## Global Constraints

- Windows-first；自动测试只用临时 Store/目录、有效事件 fixtures、fake chooser 和 mock Tauri，无真实模型/SSH/网络。
- 整 conversation 最多 10,000 事件、16 MiB 原始序列化事件、8 MiB 公开投影；完整校验后过滤，不截断链。
- tool_id 最多 256 UTF-8 字节；脱敏词最多 32 个、每个 1 KiB、共 16 KiB，空词拒绝；词列表不持久化或导出。
- 自定义脱敏词只在 `public_text` 后的原文本上做非递归 literal 匹配；相交匹配合并遮盖，生成的 `[REDACTED]` 不再参与匹配；匹配先于 UTF-8 安全截断，显示标签最多 256 字节。
- DTO 进入 omicsops-dto，snake_case/严格请求；无 SQLite 迁移、执行/审批状态变化、新计时/用量采集。
- 不调用会 terminal reconciliation 的旧 agent_v4_events 命令；共享入口文件与其他功能按 master 顺序整合。
- 默认完整检查为 `cargo test --workspace`、`npm test`、`npm run build`；宿主组合另跑 `npm run build:desktop`。实际 live acceptance 结果单独记录。

## Review Focus

1. 过滤隐藏的 run 已损坏或 envelope/索引错绑：整个读取拒绝，不能输出“已验证”部分链（Task 1）。
2. 两个 run 同 call_id、重用 outcome 或反向时间：分组不混用，耗时 unknown，用量 incomplete 不补零（Task 2）。
3. 自由文本/工具 ID 含 secret、私有推理或 HTML：白名单投影与两种导出均无原文（Tasks 2–3）。
4. 活跃 run 追加事件、保存取消或写失败：head guard 拒绝变化，None/失败均无成功状态（Tasks 3–4）。
5. 过滤变更、项目晚响应、ModelText 定位与立即 Escape：旧预览失效，精确三元组定位，一次只关顶层（Task 4）。

---

### Task 1: DTO 合同与有界只读 Store

**Files:** Create `crates/omicsops-dto/src/run_diagnostics.rs`; Modify `crates/omicsops-dto/src/lib.rs`、`crates/omicsops-store/src/lib.rs`（agent_events_for_context_v4 邻近与 tests）、`src-tauri/src/dto_contract_tests.rs`; Test 后两文件现有测试模块。

**Interfaces:**
- Consumes: `deserialize_event_chain_v4(&[String])`、protocol `AgentEventV4`/`UsageTotalsV4`、既有 project/conversation/run 索引；DTO 字段逐项采用 Spec 表。
- Produces: `Store::agent_events_for_context_v4_bounded(&self, project_id: Uuid, conversation_id: Uuid, max_events: usize, max_serialized_bytes: usize) -> Result<Vec<AgentEventV4>, StoreError>`；Spec 中全部 `RunDiagnostics*V4`/`RunDiagnostic*V4` 和 `SaveRunDiagnosticsRequestV4` 的 serde 类型。

- [ ] **RED — 写合同与 Store 失败测试。** 命名 `run_diagnostics_dto_contract`、`diagnostics_bounded_read_rejects_hidden_invalid_chain`、`diagnostics_bounded_read_checks_scope_and_limits`、`diagnostics_bounded_read_is_read_only`。fixture 用真实 AgentEventV4 构造有效 hash，再向数据库注入错绑 envelope/序列/终态；测试比较调用前后事件数和 run JSON，不经 reconcile。

  最小断言片段（fixture 使用本文件已有临时 Store 帮助函数）：

  ```rust
  assert!(store.agent_events_for_context_v4_bounded(project, conversation, 1, 16 * 1024 * 1024).await.is_err());
  assert_eq!(before_event_count, after_event_count);
  assert!(serde_json::from_value::<RunDiagnosticsRequestV4>(unknown_field_request).is_err());
  ```

- [ ] **RED — 运行并记录真实 FAIL。** `cargo test -p omicsops-store diagnostics_bounded_read -- --nocapture`、`cargo test -p omicsops-desktop --lib run_diagnostics_dto_contract`；预期缺少方法/类型而编译 FAIL，不以 ignored 计数代替失败。
- [ ] **实现上述 Store 签名与 DTO。** 一次读事务先 COUNT/SUM(value_json UTF-8 bytes)，再读所有 run 链并核验索引/envelope/run 所有权及完整链；确认 conversation 属于 project，错误不返回其他 scope 内容。source heads 覆盖全部有事件 run，DTO 请求拒绝未知字段；保留原无限定 API。
- [ ] **GREEN — 同两命令 PASS。** 补 0 事件、精确上限、超过字节预算、未选中 run 损坏与窗口 scope 归属测试；无新表/写事件。`cargo fmt --all -- --check` 格式偏差按 AGENTS.md 独立处理。
- [ ] **Commit —** 仅 stage 本 task 四个文件：`git add -- crates/omicsops-dto/src/run_diagnostics.rs crates/omicsops-dto/src/lib.rs crates/omicsops-store/src/lib.rs src-tauri/src/dto_contract_tests.rs`；`git commit -m "feat: add validated bounded run diagnostic reads"`。

**实施证据（2026-10-08）**：Task 1 功能提交 `8296078` 与依 AGENTS.md 分离的纯格式提交 `bb47a89` 经独立完整审阅通过，无待修复问题。Store 在同一只读事务中验证对话归属、整个作用域的事件数与 UTF-8 原始 JSON 字节预算，读取含错绑索引的 run 并核对原 JSON hash 链、SQL 索引、事件 envelope 与持久化 run 归属；旧无限定 API 不变，无迁移或 reconciliation。最终定向 Store 3、desktop DTO 1、格式和 diff 检查通过；初次 desktop 沙箱 OUT_DIR 失败另行记录并原命令重试。固定 10,000/16 MiB 宿主调用、公开投影、导出、UI 和完整 W06 检查仍属于后续任务；真实模型/SSH 未验收。证据见 `.superpowers/sdd/2026-10-04-wisp-w06-run-diagnostics/task-1-report.md` 与 `task-1-review.md`。

### Task 2: 宿主公开投影与查询命令

**Files:** Create `src-tauri/src/run_diagnostics.rs`; Modify `src-tauri/src/lib.rs`（module/invoke 注册）; Test 新模块 `#[cfg(test)]`。

**Interfaces:**
- Consumes: Task 1 Store/DTO，`composer_references::public_text(&str) -> String`、宿主有界非递归 literal 词遮盖、`UsageTotalsV4::from_observations`。
- Produces: `project_run_diagnostics(events: &[AgentEventV4], request: &RunDiagnosticsRequestV4) -> Result<RunDiagnosticsSnapshotV4, String>`；`load_run_diagnostics(repository: &Store, request: &RunDiagnosticsRequestV4) -> Result<RunDiagnosticsSnapshotV4, String>`；Tauri `get_run_diagnostics_v4(state: State<'_, AppState>, request: RunDiagnosticsRequestV4) -> Result<RunDiagnosticsSnapshotV4, String>`。错误为固定安全码。

- [ ] **RED — 写 `run_diagnostics_projection_*` 测试。** 两 run 同 call_id，正常/failed/uncertain/cancelled/reused outcome，缺 start/歧义 dispatch/反向时间，缺 observation 与 incomplete subtotal；固定 UTC 首事件/run UUID/sequence 排序。tool filter 精确匹配，不将无法归属的 RunFailed 混入某工具。

  ```rust
  assert_ne!(rows_for_two_runs[0].call_ref, rows_for_two_runs[1].call_ref);
  assert_eq!(reused_outcome_row.duration_ms, None);
  assert!(!serde_json::to_string(&snapshot).unwrap().contains("PRIVATE_SENTINEL"));
  assert_eq!(snapshot.usage[0].missing_observation_attempts, 1);
  ```

  敏感 fixture 覆盖每个被排除 payload、tool label、自定义词、token/password/API/Bearer/私钥、原始 provider/SSH 错误；无任意 raw event/payload/model_content 字段。工具 label 先脱敏后限长；call_ref 用 `(run UUID, call_id)` SHA-256。
- [ ] **RED — 运行 FAIL。** `cargo test -p omicsops-desktop --lib run_diagnostics_projection -- --nocapture`；预期新函数不存在。
- [ ] **实现三签名并注册 get 命令。** 完整链读取后才过滤；usage 是选中整 run 的 qualified 小计，不随工具/失败筛选重算。filtered 取是否启用任何筛选，不以数量恰好相同冒称完整。输入限额和投影 8 MiB 统一检查；keyword/filter 仅内存。每个 kind 可有固定元数据行，私有 payload 不进入输出；uncertain 不标失败/取消。
- [ ] **GREEN — 同命令 PASS。** 加空结果、tool_id UTF-8 边界、关键词空/超量/超字节与投影超限测试，确认错误无原始内容，读取仍零模型/工具/事件写入。
- [ ] **Commit —** `git add -- src-tauri/src/run_diagnostics.rs src-tauri/src/lib.rs`；`git commit -m "feat: project safe run diagnostic metadata"`。

**实施证据（2026-10-08）**：Task 2 功能提交 `f51d321`、独立格式提交 `399129a` 的完整独立审阅发现一个 P1：合法的重复自定义词会在旧共享替换函数中反复匹配新增的 `[REDACTED]`，在限长前指数膨胀。仅在本 Task 的诊断模块修复：`566c1b7`、独立格式提交 `240abd4`；保持 `public_text` 既有凭据规则，后续词匹配只读原文本、合并相交范围、输出最多 256 UTF-8 字节。实际有界 RED→GREEN、最终与复审的诊断 17、Store 3、格式和 diff 检查通过；独立局部复审关闭唯一 P1，未发现新问题。完整链/原始 JSON 哈希兼容、公开字段白名单、用量与筛选合同依完整审阅继续成立。词列表仅请求内存，不写入事件/导出。证据见 `.superpowers/sdd/2026-10-04-wisp-w06-run-diagnostics/task-2-review.md`、`task-2-fix1-report.md` 与 `task-2-fix1-rereview.md`。安全导出、UI、完整 W06 与真实环境验收仍属后续任务。

### Task 3: 宿主重新校验的 HTML/JSON 保存

**Files:** Modify `src-tauri/src/run_diagnostics.rs`、`src-tauri/src/conversation_export.rs`、`src-tauri/src/lib.rs`（save invoke 注册）; Test 两模块现有/新测试。

**Interfaces:**
- Consumes: Task 2 `load_run_diagnostics`、Task 1 `SaveRunDiagnosticsRequestV4`。
- Produces: `render_run_diagnostics(snapshot: &RunDiagnosticsSnapshotV4, format: RunDiagnosticsExportFormatV4) -> Result<Vec<u8>, String>`；`prepare_run_diagnostics_export(repository: &Store, request: &SaveRunDiagnosticsRequestV4) -> Result<Vec<u8>, String>`（async，reread/head guard/renderer，供 command 与测试共用）；`conversation_export::write_export_bytes(path: &Path, expected_extension: &str, bytes: &[u8]) -> Result<(), String>`（pub(crate)，既有 write_export 委托，原解码契约不变）；Tauri `save_run_diagnostics_v4(app: AppHandle, state: State<'_, AppState>, request: SaveRunDiagnosticsRequestV4) -> Result<Option<String>, String>`。本模块窄保存 seam `save_diagnostic_bytes(bytes: &[u8], format: RunDiagnosticsExportFormatV4, choose_path: impl FnOnce(&str) -> Result<Option<PathBuf>, String>) -> Result<Option<String>, String>` 供 fake chooser。

- [ ] **RED — 写 `run_diagnostics_export_*`。** 全 heads 与预览相等才能保存，追加事件返回 `diagnostics_snapshot_changed`；fake chooser None、不匹配扩展名、临时写失败均不成功。HTML 特殊字符/脚本只能是转义文本，严格 CSP、无脚本/远端资源；JSON/HTML 无敏感哨兵/关键词列表。

  ```rust
  assert_eq!(save_diagnostic_bytes(&bytes, Json, |_| Ok(None)).unwrap(), None);
  assert!(rendered_html.contains("default-src 'none'"));
  assert!(!rendered_html.contains("<script>"));
  assert_eq!(stale_save_error, "diagnostics_snapshot_changed");
  ```

- [ ] **RED — 运行 FAIL。** `cargo test -p omicsops-desktop --lib run_diagnostics_export -- --nocapture`；预期新 renderer/save seam 不存在。
- [ ] **实现上述签名并注册 save 命令。** 客户端不传 bytes；host reread/验证/完整 head 比对后生成固定快照，8 MiB 内才打开 dialog。文件明确原/选中数、过滤与 head 截点、usage unknown/incomplete、“公开诊断投影，非完整科研证据”；原 event_hash 只作引用，不能声称投影可重算原 hash。临时文件 sync 后原子替换，固定安全错误码，不 raw OS to_string。
- [ ] **GREEN — 同命令与 `cargo test -p omicsops-desktop --lib conversation_export` PASS。** 原 HTML/PNG 合同和 exact bytes 不回归；静态 snapshot 不因 native dialog 等待期间追加事件宣称“最新”。
- [ ] **Commit —** `git add -- src-tauri/src/run_diagnostics.rs src-tauri/src/conversation_export.rs src-tauri/src/lib.rs`；`git commit -m "feat: export validated redacted run diagnostics"`。

### Task 4: 诊断 UI、精确定位与交付验证

**Files:** Create `src/run-diagnostics-api.ts`、`src/features/workspace/RunDiagnosticsDialog.tsx`、`RunDiagnosticsDialog.test.tsx`; Modify `src/types.ts`、`WorkspaceShell.tsx`、`WorkspaceShell.test.tsx`、`src/styles.css`；完整路径的 WorkspaceShell/新增对话框均在 `src/features/workspace/`。

**Interfaces:**
- Consumes: host get/save commands，window `useWindowEscapeLayer`，现有 V4RunTrace 与作用域。
- Produces: `getRunDiagnostics(request: RunDiagnosticsRequestV4): Promise<RunDiagnosticsSnapshotV4>`、`saveRunDiagnostics(request: SaveRunDiagnosticsRequestV4): Promise<string | null>`；`RunDiagnosticLocator = {run_id: string; sequence: number; event_hash: string}`；`RunDiagnosticsDialog({projectId, conversationId, locale, onLocate, onClose}: {projectId: string; conversationId: string; locale: Locale; onLocate: (target: RunDiagnosticLocator) => void; onClose: () => void})`。Locale 沿用 `src/features/workspace/copy.ts` 的定义。

- [ ] **RED — 写 UI mock 行为测试。** 筛选变化清空旧预览；跨作用域晚响应被丢弃；save null/错误无成功 toast，pending 禁重复；点击 ModelText 元数据行按三元组展开/focus、hash mismatch 不定位相邻行。立即 Escape 仅关闭顶层。

  ```tsx
  expect(onLocate).toHaveBeenCalledWith({run_id: "run-a", sequence: 3, event_hash: hashA});
  fireEvent.keyDown(window, {key: "Escape"});
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(parentClose).not.toHaveBeenCalled();
  expect(screen.queryByText(/saved successfully/i)).not.toBeInTheDocument();
  ```

- [ ] **RED — 运行 FAIL。** `npx vitest run src/features/workspace/RunDiagnosticsDialog.test.tsx src/features/workspace/WorkspaceShell.test.tsx`；预期新入口/组件/定位行为缺失。
- [ ] **实现 API/组件及 WorkspaceShell 接入。** 以 project/conversation generation 丢晚响应；三字段 DOM 标记覆盖所有 kind，包括 ModelText，避免 ActivityWindow 裁剪；若源行未在缓存，通过新只读命令刷新再定位。不能用旧 reconcile 命令。browser 无宿主时给明确受限状态，不能伪称 host 校验下载。
- [ ] **GREEN — 定向命令 PASS，执行默认完整与桌面检查。** `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`；手工 Windows smoke 筛选/定位/取消保存/打开静态 HTML，无外部请求；记录 live acceptance 未执行。
- [ ] **Commit —** `git add -- src/run-diagnostics-api.ts src/features/workspace/RunDiagnosticsDialog.tsx src/features/workspace/RunDiagnosticsDialog.test.tsx src/types.ts src/features/workspace/WorkspaceShell.tsx src/features/workspace/WorkspaceShell.test.tsx src/styles.css`（共享入口按 master 整合）；`git commit -m "feat: add scoped run diagnostics controls"`。

本轮仅保存计划，尚未执行任何 checkbox。计划审阅与执行方式由下一阶段统一确认；不以文档或旧测试基线声称 W06 已交付。
