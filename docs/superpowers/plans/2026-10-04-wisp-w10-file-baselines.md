# W10 File Baselines Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为显式选择的小型安全本地文本保存不可变基线，提供保存正文查看与有界diff，远端/大文件只保留引用。

**Architecture:** Store保存typed immutable manifest、ownership、quota和request ledger，宿主app-data保存内容寻址blob。W02 `ValidatedProjectFile`是唯一项目文件读取入口；捕获、blob生命周期与diff都在宿主完成，UI只选择明确文件和展示校验后的结果。

**Tech Stack:** Rust/sqlx/SQLite、Tauri 2、现有sha2/tempfile、React 19/TypeScript/Vitest；不增加依赖。

**Spec:** [证据与工作区设计 W10](../specs/2026-10-04-wisp-evidence-workspace-design.md#w10显式小文件基线与有界差异)，并读取共同约束/W02文件契约。

## Global Constraints

- 当前只写计划；后续评审/执行方式确认前不实现。没有执行、自动同步、Git切换、源文件恢复/删除命令。
- 消费W09 Task 1唯一创建的 `workspace_files.rs` 四基础DTO；W10只扩展基线/diff类型，不重建该契约。不要求W09报告/UI完成。
- 项目文件读取仅复用W02 `src-tauri/src/project_file_read.rs` 的 `ValidatedProjectFile::open(&Project,&str) -> Result<Self,FileReadError>`、`metadata() -> PublicFileMetadata`、`read_complete(&mut self,usize) -> Result<BoundedFileRead,FileReadError>`；不另建目录验证或Win32框架。
- 每次1–32项、backend ID最多128字节、相对路径最多1,024字节；规范化排序去重、不递归。标题1–80字符/320字节。
- UTF-8正文仅 `.py`、`.r`、`.rmd`、`.md`、`.txt`、`.json`、`.csv`、`.tsv`；单项512 KiB、合计4 MiB；每项目64基线、逻辑正文256 MiB、持久blob实际256 MiB，额外staging最多4 MiB。
- public_text/敏感检查改变正文即reference-only；敏感路径公开和内部manifest均为None。TextSnapshot必须安全Some路径、FullFile hash与blob。密码、私钥/API token不进入SQLite/blob/日志/错误；不读keyring做扫描。
- 大文件只metadata，SSH只项目绑定的已有size/hash引用；未知为None，recorded不冒充current full。expected_full_sha256无法完整核验时整次拒绝，不能用recorded hash匹配。
- diff输出最多256 KiB/2,000行；每侧最多10,000行、最多200,000比较单元。多文件顺序读取不声称原子快照；根变更为BindingChanged。
- legacy `workspace_snapshots`原样保留；新表迁移幂等，branch删除只unlink，源项目文件从不删除；blob清理仅固定app-data目录，拒绝reparse、无shell拼接删除。
- Windows-first；使用合成记录/临时目录、mock Tauri/SSH/dialog；立即Escape关顶层，旧scope回包丢弃。RED/GREEN记录非零测试数；最终AGENTS.md完整检查，锁变化npm ci，纯fmt独立提交。

## Review Focus

1. 失去创建回执后源文件已变化：原request重试返回已提交基线，不重新捕获；同ID异正文冲突（Task 1 `baseline_request_replay`；Task 2 `capture_retry_does_not_reread`）。
2. 私钥/凭据正文或敏感相对路径：reference-only且原文不进任何持久文件、SQLite或错误（Task 2 `capture_sensitive_is_reference_only`）。
3. blob持久化成功但DB失败、坏blob或磁盘上限：不提交缺正文的基线，不让失败重试积累越过实际预算（Task 2 `capture_blob_commit_failure`、`capture_orphan_budget`）。
4. 项目根变化/Windows junction/并发file identity变化：不读取替代对象，不把prefix/recorded当full（Task 2 `capture_uses_validated_handle`；Task 3 `diff_binding_changed`）。
5. 分支删除、CRLF/末尾换行和巨大diff：只unlink association、保留manifest/blob/source；输出明确换行字段且有界（Task 1 `baseline_branch_unlink`；Task 3 `diff_newlines_and_budget`）。

---

## File Map and Dependencies

先完成W09 Task 1共享基础DTO和W02宿主读取接缝；不用等待W09 UI。新增Store `workspace_file_baselines.rs`管理metadata；新增宿主同名模块管理捕获/blob/diff。聚焦纯文本安全判定在 `src-tauri/src/workspace_file_content.rs`，供W11消费，不包含路径遍历/Win32或存储操作；W11不必启用基线命令或表。Root串行整合DTO reexport、command registration、AppState与WorkspaceShell。

### Task 1: Typed baseline persistence, migration and replay

**Files:**

- Modify: `crates/omicsops-dto/src/workspace_files.rs`, `crates/omicsops-dto/src/lib.rs`, `crates/omicsops-store/migrations/init.sql:679`, `crates/omicsops-store/src/lib.rs:53`, `:4721`, `:5247`, `src-tauri/src/dto_contract_tests.rs`.
- Create: `crates/omicsops-store/src/workspace_file_baselines.rs`, `crates/omicsops-store/tests/workspace_file_baselines.rs`.

**Interfaces:**

- Consumes: W09 Task 1四基础DTO；现有workspace request ledger；同项目branch/conversation查询。
- Produces: spec全部baseline/diff DTO与Store内部 `SaveFileBaselineRecord`/`StoredFileBaseline`；`workspace_find_file_baseline_request(&self, request_id: Uuid, request_sha256: &str) -> Result<Option<FileBaselineV1>, StoreError>`；`workspace_save_file_baseline(&self, record: SaveFileBaselineRecord) -> Result<FileBaselineV1, StoreError>`；`workspace_list_file_baselines(&self, request: &ListFileBaselinesRequestV1) -> Result<FileBaselinePageV1, StoreError>`；`workspace_get_file_baseline(&self, project_id: Uuid, baseline_id: Uuid) -> Result<Option<StoredFileBaseline>, StoreError>`。

- [ ] **Step 1: Write failing tests.** `baseline_request_replay`、`baseline_request_conflict`、`baseline_foreign_branch_rejected`、`baseline_branch_unlink`、`baseline_quotas`、`baseline_migration_twice_keeps_legacy`；用synthetic host-validated metadata，不保存真实样本。合同测试覆盖relative_path null、nullable hashes和line-ending字段。删除branch后当前association为None但原manifest/hash不变。

```rust
assert_eq!(replay.id, original.id);
assert_eq!(after_unlink.baseline.branch_conversation_id, None);
assert_eq!(after_unlink.baseline.manifest_sha256, original.manifest_sha256);
assert_eq!(legacy_row_count_after, legacy_row_count_before);
assert!(save_sixty_fifth.is_err());
```

- [ ] **Step 2: Run RED.** `cargo test -p omicsops-store --test workspace_file_baselines`；`cargo test -p omicsops-desktop --lib dto_contract_tests::file_baseline`。Expected: FAIL，缺DTO/Store方法或replay/迁移/ownership断言失败；新测试target先创建，不能只因target不存在失败。
- [ ] **Step 3: Implement the Store signatures and spec schema.** 新表CREATE/INDEX IF NOT EXISTS、同项目branch/manifest invariants、64/256 MiB逻辑quota、immutable metadata及同事务ledger；现有branch relation单独移除路径同事务unlink，conversation FK SET NULL，archive/merge不清理；不要重解释legacy rows，不保存文件正文/请求原文。
- [ ] **Step 4: Run GREEN.** 重跑RED命令及 `cargo test -p omicsops-store --test conversation_branches`。Expected: PASS；打开旧库两次、重开Store后读取基线一致。
- [ ] **Step 5: Commit.**

```powershell
git add crates/omicsops-dto/src/workspace_files.rs crates/omicsops-dto/src/lib.rs crates/omicsops-store/migrations/init.sql crates/omicsops-store/src/lib.rs crates/omicsops-store/src/workspace_file_baselines.rs crates/omicsops-store/tests/workspace_file_baselines.rs src-tauri/src/dto_contract_tests.rs
git commit -m "feat: persist typed exploration file baselines"
```

### Task 2: Safe capture, app-data blobs and saved text read

**Files:**

- Create/Test: `src-tauri/src/workspace_file_baselines.rs`, `src-tauri/src/workspace_file_content.rs`（各自unit tests）。
- Modify: `src-tauri/src/commands.rs`（app-scoped baseline storage/locks）、`src-tauri/src/lib.rs:62`, `:198`（storage初始化与注册）。
- Consume only: `src-tauri/src/project_file_read.rs`（W02）。

**Interfaces:**

- Consumes: Task 1 Store/DTO；W02唯一ValidatedProjectFile接缝。
- Produces: `workspace_create_file_baseline(app: AppHandle, state: State<'_, AppState>, request: CreateFileBaselineRequestV1) -> Result<FileBaselineV1, FileBaselineErrorV1>`；`workspace_read_baseline_text(app: AppHandle, state: State<'_, AppState>, request: ReadBaselineTextRequestV1) -> Result<BaselineTextSnapshotV1, FileBaselineErrorV1>`。
- Produces shared pure function: `inspect_small_workspace_text<'a>(relative_path: &str, bytes: &'a [u8]) -> Result<&'a str, SmallWorkspaceTextIssue>` in `workspace_file_content.rs`；内部enum只有 `TooLarge`、`UnsupportedContent`、`SensitiveContent`，W10映射到BaselineOmissionCodeV1、W11映射到MigrationFileStateV1，不依赖基线DTO/Store。只做UTF-8/extension/公开一致/敏感判断，不验证目录、开文件或读keyring。宿主 `FileBaselineStorage` app-scoped持有固定app-data根与project mutex，`FileBaselineStorage::new(app_data_root: PathBuf) -> Self`，初始化一次。

- [ ] **Step 1: Write failing tests.** 前缀 `capture_`：安全小文本、32/512 KiB/4 MiB预算、unknown/recorded SSH引用、expected hash无法核验/冲突、invalid selection、sensitive/path null、retry不重读、file identity变化、坏/缺blob、temp/persist/DB故障、orphan物理预算、reparse app-data cleanup不触项目根。`saved_text_`：entry越界/reference-only拒绝、坏blob/UTF-8/再检查拒绝及root改变仍能查看历史正文。纯函数测试前缀 `small_text_`，使用假凭据sentinel。

```rust
assert_eq!(sensitive.entries[0].content_mode, WorkspaceContentModeV1::ReferenceOnly);
assert_eq!(sensitive.entries[0].relative_path, None);
assert!(!persisted_bytes.windows(secret.len()).any(|w| w == secret));
assert_eq!(reread_count_after_retry, reread_count_before_retry);
assert_eq!(saved_text.full_sha256, full_hash);
assert_eq!(ssh_call_count, 0);
```

- [ ] **Step 2: Run RED.** `cargo test -p omicsops-desktop --lib workspace_file_content::tests`；`cargo test -p omicsops-desktop --lib workspace_file_baselines::tests::capture_`；`cargo test -p omicsops-desktop --lib workspace_file_baselines::tests::saved_text_`。Expected: FAIL，缺capture/read接缝或安全/atomic断言失败。
- [ ] **Step 3: Implement the listed signatures.** 固定app-data content-addressed路径；project mutex覆盖创建/清理；查replay在读取源文件之前，事务再判replay。所有source读取经W02同一handle完整读；大/SSH仅metadata/reference，正文降引用不持久化。temp/sync/persist blob后才DB提交，计逻辑与物理预算，清理仅确认未引用blob；saved read验证length/hash/UTF-8与公开一致。不调用shell/runtime/SSH，也不删除源文件。
- [ ] **Step 4: Run GREEN.** 重跑RED三命令；检查临时app-data与SQLite无sentinel、fault后无已提交坏基线、persisted blob<=256 MiB/staging<=4 MiB。Expected: PASS；W02 handle/reparse测试继续通过，不复制其框架。
- [ ] **Step 5: Commit.**

```powershell
git add src-tauri/src/workspace_file_baselines.rs src-tauri/src/workspace_file_content.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: capture safe bounded file baseline blobs"
```

### Task 3: Metadata commands and bounded current-file diff

**Files:**

- Modify/Test: `src-tauri/src/workspace_file_baselines.rs`.
- Modify: `src-tauri/src/lib.rs:198`, `src-tauri/src/dto_contract_tests.rs`.

**Interfaces:**

- Consumes: Task 1 StoredFileBaseline/get/list、Task 2saved blob reader/纯判定与storage、W02read_complete。
- Produces: `workspace_list_file_baselines(state: State<'_, AppState>, request: ListFileBaselinesRequestV1) -> Result<FileBaselinePageV1, FileBaselineErrorV1>`；`workspace_get_file_baseline(state: State<'_, AppState>, project_id: Uuid, baseline_id: Uuid) -> Result<FileBaselineV1, FileBaselineErrorV1>`；`workspace_diff_file_baseline(app: AppHandle, state: State<'_, AppState>, request: DiffFileBaselineRequestV1) -> Result<FileBaselineDiffV1, FileBaselineErrorV1>`。

- [ ] **Step 1: Write failing tests.** `diff_binding_changed`、`diff_reference_only_never_reads`、`diff_newlines_and_budget`、`diff_current_sensitive_or_too_large`、`diff_missing_and_bad_manifest`、`diff_foreign_owner`；列表pagination/get与每项状态匹配spec。当前变更敏感时无hunks；未比较项换行字段均None。

```rust
assert_eq!(entry.state, BaselineDiffStateV1::BindingChanged);
assert!(entry.hunks.is_empty());
assert_eq!(newline_entry.line_endings_changed, Some(true));
assert_eq!(newline_entry.final_newline_changed, Some(true));
assert!(large_diff.hunks_truncated);
assert!(serialized_hunks.len() <= 256 * 1024);
```

- [ ] **Step 2: Run RED.** `cargo test -p omicsops-desktop --lib workspace_file_baselines::tests::diff_`。Expected: FAIL，缺diff或hash/binding/output上限断言失败。
- [ ] **Step 3: Implement the three signatures.** 先验证owner/manifest/root binding，只重读TextSnapshot安全Some路径；reference-only不升级读取。完整hash相等Unchanged，差异Changed，按spec限算法输入/工作量/输出；换行差异由DTO明确表示；已捕获blob验证失败给StorageFailed，不当成源Missing。此任务不写Store/源文件。
- [ ] **Step 4: Run GREEN.** 重跑RED与 `cargo test -p omicsops-desktop --lib workspace_file_baselines::tests`。Expected: PASS；验证Store/源字节前后相同、runtime/SSH调用数0。
- [ ] **Step 5: Commit.**

```powershell
git add src-tauri/src/workspace_file_baselines.rs src-tauri/src/lib.rs src-tauri/src/dto_contract_tests.rs
git commit -m "feat: compare file baselines with bounded local diffs"
```

### Task 4: Explicit file selection, baseline panel and verification

**Files:**

- Modify: `src/workspace-navigation-types.ts`, `src/workspace-navigation-api.ts`, `src/workspace-navigation-api.test.ts`, `src/features/workspace/WorkspaceShell.tsx:537`, `:569`, `:1205`, `src/features/workspace/RemoteFileTree.tsx:34`, `src/features/workspace/ConversationBranchBanner.tsx:5`.
- Create: `src/features/workspace/WorkspaceFileBaselinesPanel.tsx`, `src/features/workspace/workspace-file-baselines.css`, `src/features/workspace/WorkspaceFileBaselinesPanel.test.tsx`.
- Modify/Test: `src/features/workspace/RemoteFileTree.test.tsx`, `src/features/workspace/WorkspaceShell.test.tsx`.

**Interfaces:**

- Consumes: five native commands/DTO；现有backend/project身份、RemoteFileTree、branch banner、window Escape层。
- Produces TS wrappers: `createWorkspaceFileBaseline(request: CreateFileBaselineRequestV1): Promise<FileBaselineV1>`；`listWorkspaceFileBaselines(request: ListFileBaselinesRequestV1): Promise<FileBaselinePageV1>`；`getWorkspaceFileBaseline(projectId: string, baselineId: string): Promise<FileBaselineV1>`；`readWorkspaceBaselineText(request: ReadBaselineTextRequestV1): Promise<BaselineTextSnapshotV1>`；`diffWorkspaceFileBaseline(request: DiffFileBaselineRequestV1): Promise<FileBaselineDiffV1>`。
- Produces component: `WorkspaceFileBaselinesPanel(props: { projectId: string; branchConversationId: string | null; locale: Locale; selectedFiles: ProjectFileSelectionV1[]; onClose: () => void }): ReactNode`；显式选择、列表、保存正文与diff视图，不加恢复/执行按钮。

- [ ] **Step 1: Write failing tests.** API exactinvoke；明确选file而不展开目录、TextSnapshot/reference-only/unknown显示、retry原request、branch可选association、源丢失仍看snapshot、根变更说明、budget truncation、project/generation旧回包、立即Escape只关正文/diff顶层。panel保存回执不清空另一项目选择。

```ts
expect(invoke).toHaveBeenCalledWith("workspace_create_file_baseline", { request: selectedRequest });
expect(selectedRequest.files).toHaveLength(1);
expect(screen.getByText(/Reference only/)).toBeVisible();
expect(screen.queryByRole("button", { name: /Restore files/ })).not.toBeInTheDocument();
expect(parentPanel).toBeVisible();
```

- [ ] **Step 2: Run RED.** `npx vitest run src/workspace-navigation-api.test.ts src/features/workspace/WorkspaceFileBaselinesPanel.test.tsx src/features/workspace/RemoteFileTree.test.tsx src/features/workspace/WorkspaceShell.test.tsx`。Expected: FAIL，缺panel/wrapper/选择动作或scope/Escape断言失败。
- [ ] **Step 3: Implement the TS interfaces and wire actual file sidebar.** RemoteFileTree只提供显式file选择，WorkspaceShell绑定project/backend；branch banner只能传已验证分支ID；保留原sync/attachment能力而不调用它们捕获。pending请求稳定ID、typed错误、metadata/reference标签、逐项顺序捕获提示和generation/Escape；不渲染HTML或执行脚本。
- [ ] **Step 4: Run GREEN and delivery checks.** 重跑RED；运行 `cargo fmt --all -- --check`、`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`；锁变化 `npm ci`。Expected: PASS并记录非零测试数/ignored。Windows合成小脚本smoke覆盖空格路径、保存后改动、缺源/根变更、branch删除只unlink、重启后查看与diff；真实SSH/样本未执行须写明。
- [ ] **Step 5: Commit.**

```powershell
git add src/workspace-navigation-types.ts src/workspace-navigation-api.ts src/workspace-navigation-api.test.ts src/features/workspace/WorkspaceShell.tsx src/features/workspace/RemoteFileTree.tsx src/features/workspace/ConversationBranchBanner.tsx src/features/workspace/WorkspaceFileBaselinesPanel.tsx src/features/workspace/workspace-file-baselines.css src/features/workspace/WorkspaceFileBaselinesPanel.test.tsx src/features/workspace/RemoteFileTree.test.tsx src/features/workspace/WorkspaceShell.test.tsx
git commit -m "feat: expose explicit exploration baseline and diff UI"
```

## Plan Self-Review

spec覆盖：Task 1typed migration/ownership/replay；Task 2安全捕获、物理预算与正文查看；Task 3根身份/完整hash/有界diff；Task 4显式file/branch UI与最终验证。Review Focus五项均映射具体测试。命令/DTO与spec一致，nullable路径和未比较换行字段未被TS改成必填；共享读取接缝不复制。计划只列签名和断言，不含实现body/真实样本；未运行列出的检查，没有首范围外的恢复/执行工作。
