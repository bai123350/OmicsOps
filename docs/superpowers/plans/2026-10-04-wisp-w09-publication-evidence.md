# W09 Publication Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在稿件所选保存版本上提供只读准备度报告，并由宿主导出 reference-only 引用证据包。

**Architecture:** Store 在一个只读事务中返回所选 revision、当前来源及有界关联科学记录；宿主做公开投影、准备度分类与包生成。React 始终传保存版本身份，保存对话框之后重新读取 context，一次生成报告、四项包内容及全部 hash，最后 atomic persist。

**Tech Stack:** Rust/sqlx/SQLite、Tauri 2、现有 zip/sha2/tempfile、React 19/TypeScript/Vitest；不增加依赖。

**Spec:** [证据与工作区设计 W09](../specs/2026-10-04-wisp-evidence-workspace-design.md#w09保存版本的准备度报告与-reference-only-证据包)，并读取该文件共同约束。

## Global Constraints

- 用户已通过实施总计划确认全部首次范围及子Agent逐项实施/独立审阅；按既定任务边界继续，不重复询问授权。
- DTO 不依赖 Tauri、Store、异步运行时或 Win32；请求使用 `#[serde(deny_unknown_fields)]`，枚举使用 `snake_case`；UUID 使用 `Uuid`，字节数和 revision 使用 `u64`，时间使用 RFC 3339 UTC 字符串。
- 最多 32 个保存引用；关联闭包上限 64 个科学记录、深度 4；每个科学 JSON 最多 256 KiB、合计 4 MiB；报告最多 512 KiB、128 条 finding，每条消息最多 512 字节；包总未压缩字节最多 16 MiB。
- 不提供 `ready: bool`；`reproduction_status` 只有 `NotExecuted`；原始文件状态只有 `NotChecked`。不读取 live 科学文件、整个 scientific state JSON、凭据、SSH、kernel、MCP 或模型。
- structured revision hash 是 `StoredRevisionContent`，legacy 公开 markdown hash 是 `PublicLegacyMarkdown`；saved snapshot、exported text、recorded file、manifest 和 zip hash 各自按设计范围解释。
- 包只有 `publication.md`、`manifest.json`、`readiness.json`、`checksums.sha256`；不包含任意 metadata map、真实样本、运行恢复身份、权限或秘密；目的地只能来自宿主保存对话框。
- 无 DB migration、无隐式保存/新 revision、无投稿/发布动作。沿用公开脱敏并做白名单/敏感内容检查，错误不回显秘密。
- Windows-first；自动化用临时 Store/目录、合成记录、mock dialog/写入器/运行接口；新弹层进入窗口 Escape 栈，立即按键只关顶层；晚到响应按作用域/generation丢弃。
- RED/GREEN 必须记录实际命令、失败原因和非零测试数；0 tests 或 ignored 不算通过。最终运行 workspace、前端、Web/desktop 构建；锁文件变化执行 `npm ci`，纯 fmt 偏差按 AGENTS.md 独立提交。

## Review Focus

1. 对话框打开后 revision 删除/改变：导出必须重新读所选 revision/context并停止，不能换成head或混用旧包与新报告（Task 3 `package_reloads_after_dialog`）。
2. 来源被删除或SSH重新绑定：保存正文仍保留，历史位置和原文件仍未核验，不猜测新backend（Task 1 `context_keeps_missing_snapshot`；Task 2 `readiness_historical_location_unverified`）。
3. legacy与再次脱敏：分别计算公开legacy、保存摘要、导出text的hash，凭据样例不出包/错误（Task 2 `readiness_legacy_hash_scope`；Task 3 `package_public_projection_hashes`）。
4. 科学闭包/记录预算超限：明确InspectionLimit/truncated，不把未检查条目当正常，不加载整state（Task 1 `context_bounds_related_records`；Task 2 `readiness_limit_is_unverified`）。
5. 编辑/切版本时旧回包与立即Escape：保存版本身份不被未保存draft替代，旧报告不覆盖新选择，一次只关顶层（Task 4 `saved_revision_and_generation`、`escape_top_layer`）。

---

## File Map and Dependencies

Task 1 唯一创建 `crates/omicsops-dto/src/workspace_files.rs` 的四个基础类型：`ProjectFileSelectionV1`、`WorkspaceContentModeV1`、`FileReferenceHashScopeV1`、`FileReferenceHashV1`，字段直接采用spec共同契约。W10 Task 1只扩展该模块，W11只消费基础类型；它们只依赖本任务基础契约，不必等W09 UI。W09不依赖W02文件读取或W10基线存储。

新增DTO `publication_evidence.rs` 只放spec定义的数据。Store在已有 `workspace_navigation.rs` 增加聚焦只读context，不拆无关模块。新增宿主 `workspace_publication_evidence.rs` 负责报告/包；UI扩展现有 `WorkspacePublicationPage`；共享lib注册、API文件由主实施者串行整合。

### Task 1: Shared DTO and bounded immutable read context

**Files:**

- Create: `crates/omicsops-dto/src/workspace_files.rs`, `crates/omicsops-dto/src/publication_evidence.rs`, `crates/omicsops-store/tests/publication_readiness_context.rs`.
- Modify: `crates/omicsops-dto/src/lib.rs:29`, `crates/omicsops-store/src/workspace_navigation.rs:720`, `crates/omicsops-store/src/workspace_journey.rs:12`, `src-tauri/src/dto_contract_tests.rs`.

**Interfaces:**

- Consumes: `Store::workspace_get_publication(&self, project_id: Uuid, publication_id: Uuid) -> Result<PublicationDetail, StoreError>`；现有 `WorkspaceSourceRef`/`WorkspaceSourceSnapshot`、科学记录分表及 project/run/sequence 所有权查询。
- Produces: spec全部W09 DTO与共享四基础DTO；`Store::workspace_publication_read_context(&self, project_id: Uuid, publication_id: Uuid, revision: u64) -> Result<PublicationReadContext, StoreError>`。内部context字段使用 `revision`（所选PublicationRevision）、`current_sources`（当前来源投影）、`related_records`（关联科学记录）、`scientific_revision`/`scientific_state_sha256`（可空header）和 `truncated`；不能成为UI请求或读取任意表。

- [x] **Step 1: Write failing tests.** 在新Store测试构造两个保存revision、已删来源、跨项目重复ID、合成分表记录及较大的state JSON；新增 `context_keeps_missing_snapshot`、`context_selects_exact_revision`、`context_rejects_foreign_owner`、`context_bounds_related_records`、`context_does_not_read_whole_state`。DTO合同覆盖unknown字段拒绝、nullable字段、snake_case及u64。最小断言片段放入对应测试：

```rust
assert_eq!(context.revision.revision, 1);
assert_eq!(context.revision.references[0].text, "saved source");
assert!(context.truncated);
assert!(context.related_records.len() <= 64);
assert!(serde_json::from_value::<PublicationSelectionV1>(with_extra).is_err());
```

- [x] **Step 2: Run RED.** `cargo test -p omicsops-store --test publication_readiness_context`；`cargo test -p omicsops-desktop --lib dto_contract_tests::publication_evidence`。Expected: FAIL，缺新method/type或身份/预算断言失败；不能以测试target不存在作为预期失败。
- [x] **Step 3: Implement the signatures above.** 按spec加入DTO/reexport与只读context；单事务精确取一个revision，长度检查在JSON解析前完成；从科学分表沿明确lineage读取，stable key去重和预算截断，只读science header，不调用标题修复路径。保持已保存snapshot正文，当前source missing只装饰availability。
- [x] **Step 4: Run GREEN.** 重跑RED命令，再运行 `cargo test -p omicsops-store --test workspace_navigation`，Expected: PASS，既有append-only/legacy/ownership行为保持。
- [x] **Step 5: Commit.** 仅提交本任务文件：

```powershell
git add crates/omicsops-dto/src/workspace_files.rs crates/omicsops-dto/src/publication_evidence.rs crates/omicsops-dto/src/lib.rs crates/omicsops-store/src/workspace_navigation.rs crates/omicsops-store/src/workspace_journey.rs crates/omicsops-store/tests/publication_readiness_context.rs src-tauri/src/dto_contract_tests.rs
git commit -m "feat: add bounded publication evidence read contract"
```

### Task 2: Read-only readiness classification and host command

**Files:**

- Create: `src-tauri/src/workspace_publication_evidence.rs`（含 `#[cfg(test)] mod tests`）。
- Modify: `src-tauri/src/workspace_navigation.rs:225`, `:273`, `:506`, `src-tauri/src/lib.rs:62`, `:198`.
- Modify: `crates/omicsops-store/src/workspace_navigation.rs`（同事务有界 typed `run_conversations`，仅已到达的明示V4 run）。
- Test: `src-tauri/src/workspace_publication_evidence.rs`, `crates/omicsops-store/tests/publication_readiness_context.rs`.

**Interfaces:**

- Consumes: Task 1 `Store::workspace_publication_read_context(&self, project_id: Uuid, publication_id: Uuid, revision: u64) -> Result<PublicationReadContext, StoreError>`与 `PublicationSelectionV1`/报告DTO；既有host公开投影/来源availability。
- Produces: `workspace_check_publication_readiness(state: State<'_, AppState>, request: PublicationSelectionV1) -> Result<PublicationReadinessReportV1, PublicationEvidenceErrorV1>`；测试用聚焦repository接缝 `check_publication_readiness(repository: &Store, request: &PublicationSelectionV1, observed_at: &str) -> Result<PublicationReadinessReportV1, PublicationEvidenceErrorV1>`，不另建来源解析框架。

- [x] **Step 1: Write failing tests.** 测试名统一 `readiness_`：invalid evidence/artifact、stale analysis、缺seed/software/database/sample、missing lineage、record budget、历史backend、legacy hash及cross-project。用真实Store调用前后状态/SQLite写入计数不变与实际repository-only命令委派证明只读；核对源码无运行/SSH/keyring/model能力端口。现有AppState为具体实现，未提供可注入外部计数接口，故不声称instrumented numeric zero，不增加无连接mock计数器。

```rust
assert_eq!(report.reproduction_status, ReproductionStatusV1::NotExecuted);
assert!(report.findings.iter().any(|f| f.code == ReadinessFindingCodeV1::InvalidEvidence));
assert_eq!(legacy_report.revision_hash_scope, PublicationRevisionHashScopeV1::PublicLegacyMarkdown);
assert_eq!(store_changes_after, store_changes_before); // actual SQLite connection
```

- [x] **Step 2: Run RED.** `cargo test -p omicsops-desktop --lib workspace_publication_evidence::tests::readiness_`。Expected: FAIL，缺命令/repository接缝或finding/hash/scope断言失败。
- [x] **Step 3: Implement both signatures and register command.** 从Task 1 context分类，不改证据或revision；对齐现有公开legacy hash，source call ID不建立lineage；超限返回InspectionLimit，原文件始终NotChecked，unsafe公开内容给typed安全错误。公开投影只提取必要共用函数，不重写既有resolver。
- [x] **Step 4: Run GREEN.** 重跑RED命令，运行 `cargo test -p omicsops-desktop --lib workspace_navigation::tests`。Expected: PASS，已删会话引用和legacy restore回归仍通过；报告测试前后Store可观察状态相同。
- [x] **Step 5: Commit.**

```powershell
git add src-tauri/src/workspace_publication_evidence.rs src-tauri/src/workspace_navigation.rs src-tauri/src/lib.rs crates/omicsops-store/src/workspace_navigation.rs crates/omicsops-store/tests/publication_readiness_context.rs
git commit -m "feat: report saved publication evidence readiness"
```

### Task 3: Reference package with one post-dialog observation

**Files:**

- Modify/Test: `src-tauri/src/workspace_publication_evidence.rs`.
- Modify: `src-tauri/src/lib.rs:198`, `src-tauri/src/dto_contract_tests.rs`.

**Interfaces:**

- Consumes: Tasks 1/2的context、报告与公开投影；现有zip/sha2/tempfile和可模拟宿主save dialog/文件写入。
- Produces: `workspace_export_publication_reference_package(app: AppHandle, state: State<'_, AppState>, request: PublicationSelectionV1) -> Result<Option<PublicationPackageExportResultV1>, PublicationEvidenceErrorV1>`。固定四entry manifest字段直接采用spec，不从客户端接正文或目的地。

- [x] **Step 1: Write failing tests.** `package_reloads_after_dialog` 用mock chooser回调改变/delete所选revision；`package_keeps_saved_missing_snapshot`、`package_public_projection_hashes`、`package_fixed_entries_and_checksums`、`package_cancel_and_write_failure`、`package_bound_and_deterministic_bytes`。假私钥/password/API token/认证URI sentinel不出任何包entry或错误；同时覆盖结构化与legacy。

```rust
assert_eq!(entry_names, ["publication.md", "manifest.json", "readiness.json", "checksums.sha256"]);
assert_eq!(cancelled, None);
assert_eq!(error.code, PublicationEvidenceErrorCodeV1::RevisionChanged);
assert!(!String::from_utf8_lossy(&manifest_bytes).contains("PRIVATE KEY"));
assert_eq!(checksum_publication, sha256(&publication_bytes));
```

- [x] **Step 2: Run RED.** `cargo test -p omicsops-desktop --lib workspace_publication_evidence::tests::package_`。Expected: FAIL，命令缺失或重读/entry/hash/atomic失败语义不符合断言。
- [x] **Step 3: Implement the export signature.** 固定执行顺序：预检→chooser→重读revision/context→一次生成四entry与全部hash→同目录temp/sync/persist。稳定序列化、zip顺序/时间戳与压缩方式；超过16 MiB拒绝；cancel返回None、写失败无DB状态；省略原路径/恢复身份和raw metadata。不可保留旧包搭配新报告。
- [x] **Step 4: Run GREEN.** 重跑RED命令与 `cargo test -p omicsops-desktop --lib workspace_publication_evidence::tests`；Expected: PASS。解zip逐文件校验，并证明Store/外部接口无写入/调用。
- [x] **Step 5: Commit.**

```powershell
git add src-tauri/src/workspace_publication_evidence.rs src-tauri/src/lib.rs src-tauri/src/dto_contract_tests.rs
git commit -m "feat: export reference-only publication evidence package"
```


实现边界补充：科学/Run保存快照无法解析时使用固定安全省略正文及原因，保留原saved hash并计算实际exported hash；Message/Tool普通说明沿用安全检查。宿主返回目的地必须已有大小写不敏感的`.zip`扩展名，无扩展名或其他扩展名安全拒绝，不改写为未确认的同名路径。

### Task 4: Saved-revision UI, API and delivery verification

**Files:**

- Modify: `src/workspace-navigation-types.ts`, `src/workspace-navigation-api.ts`, `src/workspace-navigation-api.test.ts`, `src/features/workspace/WorkspacePublicationPage.tsx:61`, `:131`, `:156`, `src/features/workspace/workspace-research-pages.css`.
- Create/Test: `src/features/workspace/WorkspacePublicationEvidence.test.tsx`.
- Regression: `src/features/workspace/WorkspaceResearchPages.test.tsx`, `src/features/workspace/ResearchOverlays.test.tsx`.

**Interfaces:**

- Consumes: two native commands above；现有 `PublicationRevision`、ResearchDialog/window Escape层与dirty leave guard。
- Produces: `checkWorkspacePublicationReadiness(request: PublicationSelectionV1): Promise<PublicationReadinessReportV1>`、`exportWorkspacePublicationReferencePackage(request: PublicationSelectionV1): Promise<PublicationPackageExportResultV1 | null>`；保存revision报告/包按钮与只读结果，保留现有Markdown导出。

- [x] **Step 1: Write failing API/component tests.** `saved_revision_and_generation` 检查dirty正文不进invoke、revision变更后旧report丢弃；`cancel_and_safe_error`；`escape_top_layer`；展示attention/unverified/truncated/NotExecuted和recorded hash标签。API测试精确command/request，无frontend目的地字段。

```ts
expect(invoke).toHaveBeenCalledWith("workspace_check_publication_readiness", { request: savedSelection });
expect(JSON.stringify(savedSelection)).not.toContain("unsaved markdown");
expect(screen.queryByText("old report")).not.toBeInTheDocument();
expect(parentDialog).toBeVisible(); // one immediate Escape closed only the report
```

- [x] **Step 2: Run RED.** `npx vitest run src/workspace-navigation-api.test.ts src/features/workspace/WorkspacePublicationEvidence.test.tsx`。Expected: FAIL，缺wrapper/按钮或saved identity/generation/Escape断言失败。
- [x] **Step 3: Implement the TS signatures and UI.** exact DTO映射不手工复制native类型；按钮明确“保存版本N”，未保存编辑不隐式保存；选择tuple/generation绑定、typed错误、安全取消说明、原文件未检查/未执行复现始终可见；沿用window Escape和dirty guard。
- [ ] **Step 4: Run GREEN and delivery checks.** 重跑RED命令与两份Regression测试；运行 `cargo fmt --all -- --check`、`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`。Expected: 全部PASS；记录actual/ignored，依赖锁变化先 `npm ci`。用合成Windows项目手工检查保存取消、空格路径、写失败、旧revision来源变化和zip校验；没有真实复现/SSH验收则明确未执行。
- [x] **Step 5: Commit.**

```powershell
git add src/workspace-navigation-types.ts src/workspace-navigation-api.ts src/workspace-navigation-api.test.ts src/features/workspace/WorkspacePublicationPage.tsx src/features/workspace/workspace-research-pages.css src/features/workspace/WorkspacePublicationEvidence.test.tsx
git commit -m "feat: show publication readiness and reference package actions"
```


源码及自动化交付检查完成；Step4保留未勾选，因为其中Windows native手工smoke受用户物理STOP而未执行。构建成功不替代真实窗口/保存对话框验收。

## Plan Self-Review

spec覆盖：共同DTO/ownership由Task 1、有效性/预算由Task 2、hash/导出失败由Task 3、保存选择/Escape由Task 4承接；无首范围遗漏。五项Review Focus各有具体测试。接口名称与spec一致，内部接缝不新增UI授权。只有断言片段和签名，没有产品body；计划不重复全部DTO。上述为2026-10-04初始计划自审记录；后续实际实现、独立审阅和检查结果见下方按日期记录。

2026-10-12：Task1 `1b19093..15d727a` 经一次完整及三轮定向独立审阅完成，36/50 SOURCE。四共享DTO、W09合同、精确保存revision的单事务有界context已实现。修复Tool/Run关联分类、Run时间投影、重复producer预算及LegacyArtifact既有合法选择兼容；最终context13/navigation18/fmt/diff通过，未变DTO最近1/1通过。current_sources的None表示内部raw待Task2公开投影比较，Changed/Unavailable/Missing分别保留；不直出raw、不额外读库。7外部RAW未改，cache警告保留；Tasks2–4报告/包/UI、完整功能与clean-HEAD/native/live检查仍独立待执行。

2026-10-13：W09 Task2 `b00993f..24073b4` 完整及一轮定向独立审阅完成，累计37/50 SOURCE。只读准备度/同context公开投影与有界run-conversation内部映射已实现；修复JSON转义前原始source字符串安全扫描，实际RED3/GREEN3、最终readiness13/fmt/diff通过，未变host navigation11/Store context14/navigation18先前通过。外部能力缺席为结构证明而非插桩零调用计数，真实Store不变有测试；包/UI与完整及clean-HEAD/native/live检查仍待后续。7外部RAW保留，cache警告不隐去。

2026-10-13：W09 Task3 `e0c9223..dcdd8c6` 完整及一轮定向独立审阅完成，累计38/50 SOURCE。宿主保存对话框后的单次只读context生成固定四entry引用包、各范围hash和原子写入；修复畸形科学快照绕过白名单及无扩展名目的地被改写的问题。实际修复RED2、最终package10/evidence23/fmt/diff通过；DTO2为未变合同的先前通过。7外部RAW及candidate/inverse/replay/postcommit证明保留；UI、全量/clean-HEAD、native手工与真实复现仍待各自后续验证。

2026-10-13：W09 Task4 `38bfd11..2649097` 保存版本UI/API完成独立完整审阅，累计39/50 SOURCE。dirty正文不进入请求、不隐式保存；选择/generation隔离、类型化安全错误、中性取消及立即Escape保护已覆盖。最终focused32和既有regression32、全量Vitest1345+Node22、Web/native构建通过；Rust1831通过/18ignored及Python48为未变源码的实际通过。最初native106错误、限定DTO/Store release缓存恢复及最终UI字节4m34s构建分别记录，缓存陈旧根因未确定。源码与确定性检查完成，native手工在用户STOP下未执行，真实模型/SSH/复现与最终clean-HEAD/全分支交付检查未完成。
