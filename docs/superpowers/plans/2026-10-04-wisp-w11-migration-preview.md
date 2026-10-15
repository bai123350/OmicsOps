# W11 Project Migration Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从项目卡片进入显式可选择的只读迁移预览，显示安全metadata、文件状态/hash、遗漏和能力重绑定需求。

**Architecture:** Store单个只读事务返回project、COUNT、有界摘要、science header和安全能力计数；宿主通过W02唯一文件读取接缝核验明确选择的小文本。预览仅存在内存，UI不提供导出/导入；digest是公开观察内容的指纹，不能作为授权或未来迁移凭据。

**Tech Stack:** Rust/sqlx/SQLite、Tauri 2、现有sha2、React 19/TypeScript/Vitest；不增加依赖。

**Spec:** [证据与工作区设计 W11](../specs/2026-10-04-wisp-evidence-workspace-design.md#w11可选择迁移的清单预览)，并读取共同DTO/W02契约。

## Global Constraints

- 当前只写计划；执行方式仍待下一阶段。首范围没有实际export/import、全SQLite导出、复制、重绑定、远端连接、运行恢复或目的地参数。
- W09 Task 1唯一创建共享四基础DTO；本计划只消费，不重复定义。W02 `ValidatedProjectFile::open(&Project,&str) -> Result<Self,FileReadError>`、`metadata() -> PublicFileMetadata`、`read_complete(&mut self,usize) -> Result<BoundedFileRead,FileReadError>`是唯一项目文件读取接缝，不另建目录验证/Win32框架。
- W10 Task 2提供共享纯 `inspect_small_workspace_text<'a>(&str,&'a [u8]) -> Result<&'a str,SmallWorkspaceTextIssue>`；只依赖该函数及内部三值enum，不依赖baseline表/blob/创建命令，可先交付该纯契约与测试。
- 类别数组去重，空数组合法；默认类别均不选、文件为空，project基本metadata始终包含。description最多4 KiB、name最多800字节、每类别examples最多20条。
- 文件最多32项，backend ID最多128字节、相对路径最多1,024字节；小文本单项512 KiB、总读取4 MiB，不递归、不输出正文、不hash超预算大文件。
- local完整读取hash标FullFile；可信已有hash标RecordedFile；未知size/hash为None，unknown不算零。SSH只存储引用，不stat/download；expected_full_sha256必须实际完整核验，否则整次拒绝。
- MCP/Skills rebind count仅为应用全局已登记配置/包的COUNT，固定文案明确不是project grant或启用/授权数；不读取profile JSON、SKILL.md或memory文件。其余绑定计数按project-owned关系读取。`mcp_server`固定kind与`skill_packages`表COUNT不暗示迁移权限。
- preview_sha256不包括observed_at或hash自身；不成为批准token。DB事务与随后逐文件读取不称为原子项目快照。
- 公开project/摘要和固定rebind/exclusions不含roots、服务器身份、profile密文/keyring定位、批准计划、权限或job恢复句柄；无Store/file写入、无模型/kernel/SSH/MCP/keyring调用。
- DTO纯数据/deny_unknown_fields/snake_case/RFC3339 UTC，Windows-first；合成记录、temp目录、mock Tauri与外部接口零调用，立即Escape关顶层、选择/generation旧响应丢弃。
- RED/GREEN记录actual命令、失败原因和非零测试数，不能用ignored/0 tests算通过；最终AGENTS.md默认与desktop检查，锁变化npm ci，纯fmt独立提交。

## Review Focus

1. 空选择与未选类别：project metadata可预览，未选类别不读取正文，没有静默默认复制（Task 1 `migration_metadata_empty_selection`；Task 3 `migration_preview_defaults_empty`）。
2. 历史SSH位置/legacy unverified metadata：reference-only、unknown size/hash为null，不能绑定成新当前位置或触发远端刷新（Task 2 `migration_preview_remote_unknown`；Task 3 `migration_preview_no_ssh_refresh`）。
3. 大文件/聚合读取预算/expected hash：不偷做全文件hash，无法完整验证expected时整次失败（Task 2 `migration_preview_hash_budget`）。
4. Credentials/活跃run与隐藏会话：安全计数包括shelved，凭据/授权/恢复句柄只列rebind，原run无任何操作（Task 1 `migration_metadata_all_sessions_and_safe_bindings`；Task 2 `migration_preview_no_effects`）。
5. 点击卡片、改选择、切项目与立即Escape：旧结果过期，不打开/恢复会话，迟到回包不能覆盖当前项目（Task 3 `migration_preview_scope_and_escape`）。

---

## File Map and Dependencies

共享DTO来自W09 Task 1，共享handle读取来自W02，共享纯文本判断来自W10 Task 2的独立纯模块。feature本身不依赖W09报告/UI或W10基线存储。新增Store/DTO/host preview模块不复用任何写出、sync或整库导出路径；ProjectLibrary callback与DesktopApp dialog是确定UI接缝。主实施者串行合并lib注册/types/tauri-api。

### Task 1: Read-only metadata categories and safety contract

**Files:**

- Create: `crates/omicsops-dto/src/project_migration.rs`, `crates/omicsops-store/src/project_migration.rs`, `crates/omicsops-store/tests/project_migration_preview.rs`.
- Modify: `crates/omicsops-dto/src/lib.rs`, `crates/omicsops-store/src/lib.rs:53`, `src-tauri/src/dto_contract_tests.rs`.

**Interfaces:**

- Consumes: `Store::get_project(&self, id: Uuid) -> Result<Option<Project>, StoreError>`、project-owned conversation/publication/science分表和安全绑定列；W09 Task 1 `ProjectFileSelectionV1`/hash类型。
- Produces: spec全部 `ProjectMigrationPreviewRequestV1`/report/error DTO；`Store::workspace_project_migration_metadata(&self, project_id: Uuid, categories: &[MigrationMetadataCategoryV1]) -> Result<MigrationMetadataReadContext, StoreError>`。内部context含project、类别COUNT/选中类别examples、science header与能力计数，不成为前端请求；不用Sqlite schema枚举或整state JSON。

- [x] **Step 1: Write failing tests.** `migration_metadata_empty_selection`、`migration_metadata_selected_categories_bounded`、`migration_metadata_all_sessions_and_safe_bindings`、`migration_metadata_owner_and_readonly`。构造>20条/类别、legacy与shelved会话、active mock run、绑定但无秘密的配置；读取前后DB状态一致。DTO测试unknown字段/nullable hash与unknown size。

```rust
assert!(empty.categories.iter().all(|c| !c.selected && c.examples.is_empty()));
assert!(selected.categories.iter().all(|c| c.examples.len() <= 20));
assert_eq!(conversation_count, visible_count + shelved_count);
assert_eq!(db_state_after, db_state_before);
assert!(!public_json.contains("credential_reference"));
```

- [x] **Step 2: Run RED.** `cargo test -p omicsops-store --test project_migration_preview`；`cargo test -p omicsops-desktop --lib dto_contract_tests::migration_preview`。Expected: FAIL，缺DTO/method或COUNT/selection/safety断言失败；新测试target先创建。
- [x] **Step 3: Implement the DTO and Store signature.** 单只读事务、明确列与表白名单、COUNT独立于examples限制；只读选中类别安全摘要及science header，不查transcript/事件正文/credentials，不调用会写旧title的list_conversations。Conversations计数包括shelved；name/description公开截断说明进入exclusions。
- [x] **Step 4: Run GREEN.** 重跑RED命令；Expected: PASS，DB/active run状态不变，unknown/hash/null契约与spec一致。
- [x] **Step 5: Commit.**

```powershell
git add crates/omicsops-dto/src/project_migration.rs crates/omicsops-dto/src/lib.rs crates/omicsops-store/src/project_migration.rs crates/omicsops-store/src/lib.rs crates/omicsops-store/tests/project_migration_preview.rs src-tauri/src/dto_contract_tests.rs
git commit -m "feat: project safe migration metadata preview"
```

### Task 2: Bounded selected files, digest and native preview

**Files:**

- Create/Test: `src-tauri/src/project_migration_preview.rs`（unit tests）。
- Modify: `src-tauri/src/lib.rs:62`, `:198`, `src-tauri/src/dto_contract_tests.rs`.
- Consume only: `src-tauri/src/project_file_read.rs`, `src-tauri/src/workspace_file_content.rs`.

**Interfaces:**

- Consumes: Task 1metadata context/DTO、W02唯一handle接缝和W10共享纯文本判定；现有saved science artifact/source投影只取可信记录，不推断legacy backend。
- Produces: `workspace_preview_project_migration(state: State<'_, AppState>, request: ProjectMigrationPreviewRequestV1) -> Result<ProjectMigrationPreviewV1, MigrationPreviewErrorV1>`；测试用 `preview_project_migration(repository: &Store, request: &ProjectMigrationPreviewRequestV1, observed_at: &str) -> Result<ProjectMigrationPreviewV1, MigrationPreviewErrorV1>`，只依赖本地/保存metadata，不接受destination。

- [x] **Step 1: Write failing tests.** 前缀 `migration_preview_`：remote_unknown、hash_budget、source_changed、unsafe_windows_file、sensitive_content、stable_digest、no_effects。构造>512 KiB文件和合计>4 MiB，assert没有全读/hash；expected必须完整核验；metadata/文件读取各自范围清楚，preview前后源字节/Store不变。mock运行/SSH/keyring/MCP计数全0。

```rust
assert_eq!(remote.hash, None);
assert_eq!(remote.size_bytes, None);
assert_eq!(preview.unknown_size_file_count, 1);
assert_eq!(same_content_later.preview_sha256, preview.preview_sha256);
assert_eq!(external_call_count, 0);
assert_eq!(store_write_count, 0);
```

- [x] **Step 2: Run RED.** `cargo test -p omicsops-desktop --lib project_migration_preview::tests::migration_preview_`。Expected: FAIL，缺接缝/command或budget/digest/zero-effect断言失败。
- [x] **Step 3: Implement both signatures and register.** 校验project/backend/selection长度、排序去重；W02metadata先判预算，只安全小文本完整读后立即丢弃正文。recorded/current/unknown hash区分；invalid identity/无法验证expected整次拒绝，单项missing/unsafe保留状态。稳定公开内容digest排除观察时间/自身；固定rebind/exclusions、不输出任意metadata，绝不使用sync/export/import或runtime恢复API。
- [x] **Step 4: Run GREEN.** 重跑RED与Task 1Store测试；Expected: PASS；验证未读取oversized正文、无temp/blob/DB新增文件，能力身份不出preview/errors。
- [x] **Step 5: Commit.**

```powershell
git add src-tauri/src/project_migration_preview.rs src-tauri/src/lib.rs src-tauri/src/dto_contract_tests.rs
git commit -m "feat: preview selected project migration references"
```

### Task 3: Project-card dialog, explicit selectors and delivery checks

**Files:**

- Create: `src/features/projects/ProjectMigrationPreviewDialog.tsx`, `src/features/projects/project-migration-preview.css`, `src/features/projects/ProjectMigrationPreviewDialog.test.tsx`.
- Modify: `src/features/projects/ProjectLibrary.tsx:18`, `:114`, `src/features/projects/ProjectLibrary.test.tsx`, `src/DesktopApp.tsx:1250`, `src/types.ts`, `src/tauri-api.ts`.
- Create/Test: `src/tauri-api.migration-preview.test.ts`, `src/DesktopApp.migration-preview.test.tsx`.

**Interfaces:**

- Consumes: native preview；`listLocalComposerFiles(projectId: string): Promise<RemoteFileEntry[]>`；已有 `getWorkspaceJourney(request: WorkspaceJourneyRequest): Promise<JourneyPage>` / `getWorkspaceSourceDetail(source: WorkspaceSourceRef): Promise<WorkspaceSourceSnapshot>`用于选择已有artifact历史引用，不调用list_remote_files。
- Produces: `previewProjectMigration(request: ProjectMigrationPreviewRequestV1): Promise<ProjectMigrationPreviewV1>`；`ProjectLibrary`新增 `onPreviewMigration?: (project: WorkspaceProject) => void`；`ProjectMigrationPreviewDialog(props: { project: WorkspaceProject; locale: Locale; onClose: () => void }): ReactNode`，由DesktopApp持dialog所选project，与active会话分离。

- [ ] **Step 1: Write failing tests.** `migration_preview_defaults_empty`、`migration_preview_no_ssh_refresh`、`migration_preview_scope_and_escape`；卡片callback不调用onOpen/latest/model；local metadata明确多选，saved artifact只在host给安全backend/path且归属兼容时可选择，legacy未记录backend显示不可选原因。选择变更标旧preview过期、再次点击才生成；旧项目回包不覆盖，立即Escape只关dialog。

```ts
expect(request.categories).toEqual([]);
expect(request.files).toEqual([]);
expect(listRemoteFiles).not.toHaveBeenCalled();
expect(openConversation).not.toHaveBeenCalled();
expect(screen.queryByRole("button", { name: /Export|Import/ })).not.toBeInTheDocument();
expect(invoke).toHaveBeenCalledWith("workspace_preview_project_migration", { request });
```

- [ ] **Step 2: Run RED.** `npx vitest run src/tauri-api.migration-preview.test.ts src/features/projects/ProjectMigrationPreviewDialog.test.tsx src/features/projects/ProjectLibrary.test.tsx src/DesktopApp.migration-preview.test.tsx`。Expected: FAIL，缺callback/dialog/wrapper或empty/no-effect/generation/Escape断言失败。
- [ ] **Step 3: Implement the TS interfaces and actual project-card entry.** 默认空选择、固定project metadata、有界类别examples、file状态/unknown count、固定rebind/exclusions。显式“生成预览”，无Export/Import、无自动project打开/SSH刷新。request选择digest与generation绑定、readonly报告、window Escape层；不把preview digest当授权。
- [ ] **Step 4: Run GREEN and delivery checks.** 重跑RED；执行 `cargo fmt --all -- --check`、`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`；锁变化先 `npm ci`。Expected: PASS并记录实际数/ignored。Windows合成project smoke：卡片直接预览、空/有选择、known/unknown hash、分类切换、过期/重生成、关dialog，无真实样本/SSH或整库导出。
- [ ] **Step 5: Commit.**

```powershell
git add src/features/projects/ProjectMigrationPreviewDialog.tsx src/features/projects/project-migration-preview.css src/features/projects/ProjectMigrationPreviewDialog.test.tsx src/features/projects/ProjectLibrary.tsx src/features/projects/ProjectLibrary.test.tsx src/DesktopApp.tsx src/types.ts src/tauri-api.ts src/tauri-api.migration-preview.test.ts src/DesktopApp.migration-preview.test.tsx
git commit -m "feat: add explicit project migration preview dialog"
```

## Plan Self-Review

spec覆盖：Task 1metadata COUNT/白名单/只读与DTO；Task 2有界文件/hash/digest/重绑定/无副作用；Task 3项目卡片、空默认选择、scope/Escape与完整验证。五项Review Focus各有测试。shared四DTO与handle契约不重复，pure text issue不依赖baseline存储；没有destination/export/import接口。只列最小断言和实现签名，比例小于spec的数倍；计划阶段未执行任何列出的命令。

2026-10-15：W11 Task1 source72a3c1c及独立纯格式f55860a完成独立FULL审阅，Spec compliant/Approved，无Critical/Important，累计48/50 SOURCE。只读单事务白名单metadata、独立COUNT/最多20摘要、所有会话含Shelved与legacy Archived、science header/分kind计数、当前publication revision/hash摘要、固定安全project/rebind/exclusions及纯DTO完成。新增正式裁决：原精确DTO只有aggregate/examples不能表达设计要求的五种科学计数，故增加kind_counts与MigrationKindCountV1，固定dataset/analysis/artifact/evidence/provenance含零、其他类别为空；成本是纯DTO字段/类型与serde契约，以及后续digest/TS/UI同步，不新增持久化或重载label/重复category。最终semantic Store5/nativeDTO1（724 filtered）通过，纯fmt前RAW与实际rustfmt输出证明连接最终字节，fmt/diff显式exit0；初始及kind_counts编译RED为0 executed，publication hash行为RED1失败、两次fixture失败均保留。一次DTO/Store启动重叠仅观察到build-directory lock等待，两个终态保留，不冒充严格串行；后续检查串行。六路径候选/逆向/重放/RAW/canonicalHEAD/protected7/外部差异/两个工作区兼容残留/空index证明通过。Minor为hardlink cache警告，留待最终整分支审阅。Task2仍需文件identity/预算/expected hash/稳定digest（含kind_counts）/固定安全错误与仅public_project序列化；Task3仍需TS/UI、显式生成/范围失效/Escape及完整交付检查。全量工作树与干净归档、CI、Windows手工/真实SSH/模型/R/Micromamba/PBMC均非本Task1通过项；物理Escape STOP保持。

2026-10-15：W11 Task2 semanticcbc0dd6及独立纯格式e56fc12完成独立FULL，Spec compliant/Approved，无Critical/Important，累计49/50 SOURCE。原生只读preview命令/注册、公开metadata与kind_counts/Shelved注记、稳定digest、显式文件身份/预算/完整hash与保存SSH引用已实现。actual W02保留handle、每项512KiB/请求4MiB在尝试前预留并传同一allowance、失败不退款、EOF探针单列；expected_full必须实际完整核验，否则整次拒绝。shared纯文本策略在敏感文件名前置，unsupported可报告metadata但不读payload；未知size/hash保持None，Unavailable记Unreadable不猜Missing。避免会读取profile_json的既有backend helper，直接校验内部project的精确canonical绑定，再使用已审阅Store typed V4 recorded-reference；仅public_project进入响应，底层SQL/路径异常固定安全映射。此为既定白名单约束内实现选择，无新增正式Ruling。最终semantic native15（725 filtered）、Store5、DTO1（739 filtered）通过，fmt/diff0；初始编译RED0 executed、FK fixture在preview前12pass/1fail及临时legacy构造/恢复、unsupported binary sizeNone对Some7行为RED1均保留。前后15项属不同断言阶段不累加；后续断言并非各有独立RED。三路径候选/逆向/重放、preformat等于semanticHEAD、纯rustfmt精确输出、RAW/canonical/protected7/外部差异/两个工作区兼容残留/空index证明通过。Minor环境hardlink警告留最终审阅。Task1 native消费CV已关闭；Task3 TS/UI、空默认显式生成、catalog无SSH刷新、scope/selection/generation失效和立即Escape及完整交付检查仍待实施。DB与逐文件观察非原子快照，digest不是批准token；GUI STOP下手工及真实SSH/模型/R/Micromamba/PBMC未执行，全量工作树/干净归档/CI/最终整分支不属于本任务通过项。
