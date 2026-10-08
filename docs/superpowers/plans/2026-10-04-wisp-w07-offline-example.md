# W07 Offline Example Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 显式创建并离线浏览一个双语合成示例，示例身份在重启和收据丢失后仍不进入生产 Agent 初始化。

**Architecture:** 固定编译 fixture 生成唯一的新本地 Project、文件收据和与 Project 同事务登记的 app_objects marker。专用 DTO/view 展示来源、结果与报告；所有 DesktopApp 项目选择入口先识别 marker，文件预览只复用 W02 本地读取叶子。

**Tech Stack:** Rust/core Project/sqlx/serde/Tauri 2、include_str!、SHA-256、React 19/TypeScript/Vitest；无模型、SSH、网络或科学运行时。

**Spec:** [W07 设计](../specs/2026-10-04-wisp-diagnostics-example-design.md#w07一个双语可离线打开的合成示例工作区)、[总设计](../specs/2026-10-04-wisp-improvements-design.md)、W02 [共享文件接缝](../specs/2026-10-04-wisp-composer-files-jobs-design.md#本地共享文件检查接缝)。设计基线 `161199f`；实施前读设计与 AGENTS.md。

## Global Constraints

- 唯一 example_id 为 `synthetic-evidence-report-v1`；Project 为 Blank/Ready，connection_id/remote_root=None，不新增 ProjectTemplate。
- `source_kind=synthetic`，独立 entry 为 question/narration/result；不写生产 conversation、Run、approval、Evidence 或 Agent event。
- fixture manifest 最多 256 KiB、单文件 64 KiB、全部 512 KiB/12 文件；LICENSE.txt 计入。原创许可 AGPL-3.0-only，复制现有 LICENSE，不导入上游 transcript/thinking/样本。
- `app_objects` kind=`offline_example_workspace_v1`、id=project UUID；Project/marker 同一 BEGIN IMMEDIATE。无新表/迁移，不以磁盘收据作为唯一身份。
- parent 拒绝 UNC/设备路径/symlink/junction/reparse；最终路径 containment/no-overwrite；普通文件预览只消费 W02 `ValidatedProjectFile`，不另建文件安全框架。
- 自动测试只用临时目录/Store、模拟 Tauri 与失败注入；默认完整检查与 `npm run build:desktop` 必须运行；真实工作流单独记录未执行/实际结果。

## Review Focus

1. DB marker 写失败、重复 request 或普通 Project ID/root 冲突：零半登记、不覆盖，原记录保留（Tasks 1–2）。
2. 收据被删/破坏或 root 改变：已有 marker 仍为 Some(unavailable)，普通无标记项目不被误判（Task 3）。
3. Windows junction、路径越界、额外文件、登记失败后的目录：受限创建/读取，用户文件不删除或重写（Tasks 2–3）。
4. 启动首项目、精确来源/usage/恢复导航直接选择：统一先识别，零 createConversation/Agent/SSH（Task 4）。
5. 晚到启动请求、changed/missing 文件、双语/外链和立即 Escape：不切错项目、来源准确、离线预览且只关顶层（Tasks 3–4）。

---

### Task 1: 独立 DTO 与 Project/marker 原子登记

**Files:** Create `crates/omicsops-dto/src/offline_example_workspace.rs`; Modify `crates/omicsops-dto/src/lib.rs`、`crates/omicsops-store/src/lib.rs`（save_project/delete_project/app_objects 与 tests）、`src-tauri/src/dto_contract_tests.rs`。

**Interfaces:**
- Consumes: core `Project`、Store `validate_project_input`/`insert_project`/现有 app_objects；全部跨 UI DTO 字段采用 Spec 表。
- Produces: Store 内部 public `OfflineExampleProjectMarkerV1 {schema_version: u8, project_id: Uuid, request_id: Uuid, example_id: String, fixture_sha256: String, root_binding_sha256: String}`；`offline_example_root_binding(project: &Project) -> String`（hash project UUID 与创建时规范 final local_root 的确定序列化）；`Store::save_offline_example_project(&self, project: &Project, marker: &OfflineExampleProjectMarkerV1) -> Result<(), StoreError>`；`get_offline_example_project_marker(&self, project_id: Uuid) -> Result<Option<OfflineExampleProjectMarkerV1>, StoreError>`。delete_project 同事务清理固定 kind/id。跨 UI DTO 使用 `OfflineExampleAvailability {Available, Unavailable}` 与 `OfflineExampleFileAvailability {Available, Changed, Missing, Unavailable}` 两个 snake_case 枚举。

- [x] **RED — 写 `offline_example_marker_*` 和 `offline_example_dto_contract`。** 用临时 DB 的失败 trigger 验证 app_objects 写失败导致 Project 也 rollback；再测反向失败、相同身份 no-op、普通 ID/root 冲突、marker 字段归属/版本/hash 校验、删除后 marker 清理。

  ```rust
  assert!(store.save_offline_example_project(&project, &marker).await.is_err());
  assert!(store.get_project(project.id).await.unwrap().is_none());
  assert!(store.get_offline_example_project_marker(project.id).await.unwrap().is_none());
  assert!(serde_json::from_value::<CreateOfflineExampleWorkspaceRequest>(extra_field).is_err());
  ```

- [x] **RED — 运行 FAIL。** `cargo test -p omicsops-store offline_example_marker -- --nocapture`、`cargo test -p omicsops-desktop --lib offline_example_dto_contract`；预期必要类型/方法不存在。
- [x] **实现上述签名/DTO。** BEGIN IMMEDIATE 先检查 ID/root/marker 冲突，再 insert_project 与固定 marker 插入，同成同败；相同记录保留用户 metadata。marker 不含文件正文、凭据、执行/科学状态。请求 deny_unknown_fields，availability/source/kind 枚举 snake_case。普通 save_project 不删除 marker。
- [x] **GREEN — 同命令 PASS。** 新开 Store 后 marker 稳定可读；已有普通项目不会因缺文件被写 marker，删除行为仍不删用户目录。
- [x] **Commit —** `git add -- crates/omicsops-dto/src/offline_example_workspace.rs crates/omicsops-dto/src/lib.rs crates/omicsops-store/src/lib.rs src-tauri/src/dto_contract_tests.rs`；`git commit -m "feat: persist isolated offline example identity"`。

**实施证据（2026-10-08）**：Task1 功能 `7511538` 与独立格式 `26629be` 经独立完整审阅通过，无阻塞发现。纯 DTO 严格请求与 snake_case 合同、Project/marker 同 BEGIN IMMEDIATE 登记、失败回滚、相同身份保留用户元数据、普通 save 保留身份与同事务删除清理均有行为测试。Store regression316、最终 marker7/DTO1/fmt/diff通过；已知 Windows cache/CRLF 提示保留。根 binding 对宿主提供的规范最终根做确定 SHA256，Store不访问文件系统；路径验证/fixture/receipt/view/UI由后续任务负责。外部修改原始字节与任务独立stage证明保留。

### Task 2: 固定双语 fixture 与受限定创建/恢复

**Files:** Create `examples/offline/synthetic-evidence-report-v1.json`、`examples/offline/README.md`、`src-tauri/src/offline_example_workspace.rs`; Modify `src-tauri/src/lib.rs`（module/invoke）；Test 新宿主模块。

**Interfaces:**
- Consumes: Task 1 DTO/marker/atomic Store，core Project，现有 project.json 格式及 LICENSE。
- Produces: 模块内部 `OfflineExampleFixtureV1`（version、example ID、双语 entries、固定文件内容）、`OfflineExampleReceiptV1`（Spec 收据字段）、`PreparedOfflineExampleProject {project: Project, marker: OfflineExampleProjectMarkerV1}`；`load_bundled_example() -> Result<OfflineExampleFixtureV1, String>`；`materialize_offline_example(request: &CreateOfflineExampleWorkspaceRequest, fixture: &OfflineExampleFixtureV1, write_file: fn(&Path, &[u8]) -> Result<(), String>) -> Result<PreparedOfflineExampleProject, String>`；`write_new_example_file(path: &Path, bytes: &[u8]) -> Result<(), String>` 为 create_new 生产 writer，fn 参数仅窄失败注入，不提供通用 FS 框架。Task 3 接上 view 后产生完整 create 命令。

- [ ] **RED — 写 `offline_example_create_*`。** fixture 数字为 A=2/4、B=5/7，means=3/6、difference=3；固定七文件及双语“合成数据，预置结果，未在本机执行”。测规范路径/UTF-8/预算、UNC/设备/遍历/reparse、已存目标/额外文件冲突、重复 request/new request、writer 失败与 marker 登记失败恢复。

  ```rust
  assert_eq!(child.file_name().unwrap(), format!("omicsops-example-{}", request.request_id).as_str());
  assert_eq!(std::fs::read(&user_file).unwrap(), original_user_bytes);
  assert_eq!(first.project.id, repeated.project.id);
  assert_ne!(first.project.id, other_request.project.id);
  ```

- [ ] **RED — 运行 FAIL。** `cargo test -p omicsops-desktop --lib offline_example_create -- --nocapture`；预期 fixture loader/materialize 不存在。
- [ ] **实现上述签名与 fixture。** include_str! 编译单例 fixture/现有 LICENSE。SOURCE hashes 不含 SOURCE 自身，receipt hashes 含全部七文件。只在验证 parent 下新 staging create_new，发布唯一 child 不覆盖，再调用原子 Store；相同 request 必须 receipt/manifest/全部 hashes/Store identity 相同才恢复。失败仅清理本请求拥有且路径已核验的 staging；final 已发布/登记失败保留目录并返回 `example_registration_failed`。
- [ ] **GREEN — 同命令 PASS。** 失败 trigger 证明 final 留存且 DB 零半登记，相同 request 可恢复；用户改动后重试冲突，绝不删除/覆盖。Windows 路径检查包含 reparse/junction，不将 Unix symlink 测试当完整证明。
- [ ] **Commit —** `git add -- examples/offline/synthetic-evidence-report-v1.json examples/offline/README.md src-tauri/src/offline_example_workspace.rs src-tauri/src/lib.rs`；`git commit -m "feat: materialize a bounded bilingual synthetic example"`。

### Task 3: marker 优先的只读 view 与安全文件预览

**Files:** Modify `src-tauri/src/offline_example_workspace.rs`、`src-tauri/src/lib.rs`; Test 新模块；W02 `src-tauri/src/project_file_read.rs` 只消费，不在此 task 重写其规则。

**Interfaces:**
- Consumes: Tasks 1–2，W02 `ValidatedProjectFile::open(&Project, &str)` / `read_complete(64 * 1024)`，`composer_references::public_text`。
- Produces: `load_offline_example_workspace(repository: &Store, project_id: Uuid) -> Result<Option<OfflineExampleWorkspaceView>, String>`；固定路径专用 `read_example_receipt(project: &Project) -> Result<OfflineExampleReceiptV1, String>`（64 KiB、同等 handle final-path/identity 检查，不放宽 W02 的内部目录拒绝）；Tauri `get_offline_example_workspace(state: State<'_, AppState>, project_id: Uuid) -> Result<Option<OfflineExampleWorkspaceView>, String>`、`create_offline_example_workspace(state: State<'_, AppState>, request: CreateOfflineExampleWorkspaceRequest) -> Result<CreateOfflineExampleWorkspaceResult, String>`。

- [ ] **RED — 写 `offline_example_view_*`。** 无 marker 返回 None；有 marker 丢/坏 receipt、root 丢失/错绑返回 Some(unavailable)，未知/坏 marker 为安全错误，不 fallback。changed/missing/over-limit/reparse 文件标状态、无写回；受控私钥/token/HTML 哨兵不能进入 preview 原文。

  ```rust
  assert!(without_marker.is_none());
  assert_eq!(after_receipt_removed.unwrap().availability, OfflineExampleAvailability::Unavailable);
  assert_eq!(edited_file.availability, OfflineExampleFileAvailability::Changed);
  assert_eq!(std::fs::read(&edited_path).unwrap(), edited_bytes);
  ```

- [ ] **RED — 运行 FAIL。** `cargo test -p omicsops-desktop --lib offline_example_view -- --nocapture`；预期 view/命令不存在。
- [ ] **实现签名并注册两个命令。** marker 永远优先，unavailable view 保留固定合成来源/banner、清空 entries/files；普通文件清单固定，完整有界 UTF-8读取，先脱敏再返回 preview。任一文件内容 changed 不改变来源身份，不把多文件读取宣称原子快照；请求不允许任意客户端路径。create 只在 materialize 与原子登记后加载 view，返回 ID 不复制 Project DTO。
- [ ] **GREEN — 同命令 PASS。** 重开 DB 与反复打开均零文件/生产表写入，changed/missing 不自动修复，只有完全可读未修改文件为 available。W02叶子未就绪时等待 master 接入，不另造安全框架。
- [ ] **Commit —** `git add -- src-tauri/src/offline_example_workspace.rs src-tauri/src/lib.rs`；`git commit -m "feat: read offline examples without production run state"`。

### Task 4: 全项目选择入口与双语独立 UI

**Files:** Create `src/offline-example-api.ts`、`src/features/projects/OfflineExampleWorkspace.tsx`、`src/features/projects/OfflineExampleWorkspace.test.tsx`; Modify `src/types.ts`、`src/DesktopApp.tsx`、`src/DesktopApp.test.tsx`、`src/features/projects/ProjectLibrary.tsx`、`ProjectLibrary.test.tsx`、`src/styles.css`、`docs/bundled-science.md`、`acceptance/README.md`。

**Interfaces:**
- Consumes: Task 3 host commands、listProjects、existing chooseProjectDirectory、Locale（`src/features/workspace/copy.ts`）、Escape stack；共享 DesktopApp 改动由 master 串行整合。
- Produces: `createOfflineExampleWorkspace(request: CreateOfflineExampleWorkspaceRequest): Promise<CreateOfflineExampleWorkspaceResult>`、`getOfflineExampleWorkspace(projectId: string): Promise<OfflineExampleWorkspaceView | null>`；DesktopApp 内 `openProjectWorkspace(project: WorkspaceProject, navigation?: {conversationId?: string; runId?: string; sourceId?: string}): Promise<boolean>`；ProjectLibrary callback `onCreateOfflineExample(parentDirectory: string): Promise<void>`；`OfflineExampleWorkspace({workspace, locale, onBack}: {workspace: OfflineExampleWorkspaceView; locale: Locale; onBack: () => void})`。

- [ ] **RED — 写 UI mocks。** 启动 items[0] 为示例、创建后、search 精确来源、usage conversation、workspace source、恢复导航都不得先 setSelected；删除 receipt 后重启仍 unavailable。晚默认启动响应不能覆盖显式导航，身份查询拒绝不能 fallback；原普通项目路线不回归。视图双语、四段/七文件、changed/missing/unavailable、外链/图片无请求，示例创建关闭立即 Escape 只关顶层。

  ```tsx
  expect(api.createConversation).not.toHaveBeenCalled();
  expect(api.agentV4StartDirect).not.toHaveBeenCalled();
  expect(api.agentV4StartPlanning).not.toHaveBeenCalled();
  expect(api.agentV4ComputeBackends).not.toHaveBeenCalled();
  expect(screen.getByText(/synthetic|合成/)).toBeInTheDocument();
  expect(screen.getByText(/unavailable|不可用/)).toBeInTheDocument();
  expect(parentClose).not.toHaveBeenCalled();
  ```

- [ ] **RED — 运行 FAIL。** `npx vitest run src/DesktopApp.test.tsx src/features/projects/ProjectLibrary.test.tsx src/features/projects/OfflineExampleWorkspace.test.tsx`；预期新组件/统一识别路径缺失，首项目示例当前仍会创建 conversation。
- [ ] **实现签名、入口与视图。** 全部新项目 selection 通过 openProjectWorkspace，只有 marker query None 才设普通 selected/application requestedConversation；Some 转专用 view，等待/错误不临时进入普通项目。project generation 丢晚响应、识别完成才结束 loading。独立 local-only action 不调用默认远端 beginCreate，request ID 在同次提交/重试稳定；取消 parent picker 不创建。HTML/Markdown 只显示安全文本，不载外部资源。
- [ ] **GREEN — 定向命令 PASS，默认/桌面检查 PASS。** `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`。Windows smoke：无 credentials 创建、重启、预览、改/删文件、删 receipt，确认合成标记和零派发；记录真实科学工作流未执行。docs 分开 bundle/helper/example 与实际工作流证据。
- [ ] **Commit —** `git add -- src/offline-example-api.ts src/features/projects/OfflineExampleWorkspace.tsx src/features/projects/OfflineExampleWorkspace.test.tsx src/types.ts src/DesktopApp.tsx src/DesktopApp.test.tsx src/features/projects/ProjectLibrary.tsx src/features/projects/ProjectLibrary.test.tsx src/styles.css docs/bundled-science.md acceptance/README.md`；`git commit -m "feat: route bilingual offline examples outside agent initialization"`。

本轮仅保存计划，尚未执行 checkbox。W02 读取叶子和 DesktopApp 共享文件整合顺序服从 master；计划审阅与执行方式留待下一阶段，未声称 W07 功能已实现。
