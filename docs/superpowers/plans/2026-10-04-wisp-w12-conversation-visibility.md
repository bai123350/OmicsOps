# W12 Conversation Visibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 持久化可逆会话收起/恢复，让默认导航、搜索与 `#` 不再发现已收起Session，同时保留全部精确引用和历史运行身份。

**Architecture:** 新visibility表与ConversationStatus/branch state分离，缺行即Visible/revision0；Store负责project ownership、CAS和request ledger。展示查询先过滤再分页，内部ALL和精确getter保留；DesktopApp持mutation/requery状态，UI独立收起集合和active banner不取消运行或自动恢复。

**Tech Stack:** Rust/sqlx/SQLite、Tauri 2、React 19/TypeScript/Vitest；不增加依赖。

**Spec:** [证据与工作区设计 W12](../specs/2026-10-04-wisp-evidence-workspace-design.md#w12独立可见性会话收起与恢复)，并读取共同约束。精确visibility getter由主设计评审补齐，用于既有source直接打开后的banner/expected revision；它只读，不改变core对象或授权。

## Global Constraints

- 本计划当前供审阅，执行方式仍待下一阶段；不能开始实现或以文档声称完成。
- 可见性独立于 `ConversationStatus::Archived`、分支active/merged/archived、run状态；缺行Visible/revision0/changed_at=None，第一次实际变化revision1，恢复后保留row与递增revision。
- Store `conversations_for_project`保持ALL；仅展示/default latest/catalog Session发现改Visible。保存来源、Session chip、稿件、branch/run精确打开仍按原project解析。
- page query最多200字符/1KiB、limit=1..100、offset<=10,000，UI默认50；project/visibility/query SQL过滤先于分页，updated_at DESC/id稳定排序。并发变更不承诺offset快照，提交后回第一页。
- `BEGIN IMMEDIATE`同事务CAS+ledger；同request相同digest返回原receipt、异正文RequestConflict；stale revision冲突；同目标no-op不改visibility版本/时间或Conversation updated_at，但记录receipt。
- exact getter只按project/conversation读取，不通过Visible列表判断存在；replay receipt是历史结果，前端必须再读当前状态，不能用旧回包覆盖后来恢复。
- Running也可收起，active transcript继续并显示banner；新消息不自动unshelf；项目默认latest只选Visible，无候选显示空态/收起入口。关闭集合、收起、停止等待都不取消远端计算。
- DTO纯数据/deny_unknown_fields/snake_case/Uuid/u64/RFC3339 UTC；新表/父index幂等、旧会话无回填、不改title/status/updated_at；复合FK拒绝跨project行，legacy Archived不迁为Shelved。
- Windows-first，自动化只用temp Store/mock Tauri/fake run；无keyring/SSH/kernel/模型调用；所有新overlay进入window Escape栈，立即按键一次关顶层，旧scope回包丢弃。
- RED/GREEN记录真实FAIL原因/PASS与非零test数；最终AGENTS.md默认+desktop检查，锁变化npm ci、纯fmt独立提交；无真实SSH/runtime验收须写明。

## Review Focus

1. 原request回执丢失且之后已经恢复：replay不再应用旧动作，UI重读当前visibility，不被旧receipt覆盖（Task 1 `visibility_replay_after_restore`；Task 4 `visibility_lost_reply_requery`）。
2. legacy Archived/branch merged/Running：迁移默认Visible，收起只改偏好，不改时间、消息、分组或run/branch状态（Task 1 `visibility_migration_preserves_legacy`；Task 4 `visibility_running_stays_open`）。
3. 首页隐藏大量Session或同名标题：SQL先过滤page，精确visibility getter按ID取revision，不靠title分页（Task 1 `visibility_filter_before_pagination`、`visibility_getter_by_owner_and_id`）。
4. 旧Session chip/稿件/run/branch来源：发现被过滤但精确解析仍成功，DesktopApp不能用Visible list.find判不存在（Task 2 `visibility_catalog_vs_resolve`；Task 4 `visibility_direct_source_open`）。
5. shelf/restore后搜索/#缓存与旧scope回包、立即Escape：提交后cache失效，旧generation不会再出现候选或替换项目状态（Task 3 `visibility_dialog_scope_and_escape`；Task 4 `visibility_catalog_refresh`）。

---

## File Map and Dependencies

W12独立于W09/W10/W11/W02；W09历史source与W11ALL计数必须继续可用。新增DTO `conversation_visibility.rs`、Store `workspace_conversation_visibility.rs`、UI `ShelvedConversationsDialog.tsx`；宿主扩展现有workspace commands和catalog，不重建对话/搜索索引。Root串行整合lib/API/WorkspaceShell/DesktopApp；W01草稿继续以同project/conversation身份保持。

### Task 1: Visibility DTO, idempotent schema and project-owned CAS

**Files:**

- Create: `crates/omicsops-dto/src/conversation_visibility.rs`, `crates/omicsops-store/src/workspace_conversation_visibility.rs`, `crates/omicsops-store/tests/conversation_visibility.rs`.
- Modify: `crates/omicsops-dto/src/lib.rs`, `crates/omicsops-store/migrations/init.sql:71`, `crates/omicsops-store/src/lib.rs:53`, `:4721`, `:5247`, `src-tauri/src/dto_contract_tests.rs`.

**Interfaces:**

- Consumes: current `Conversation`、`conversation_records`/groups/branch relations、workspace request ledger；现有Store `conversations_for_project(&self, project_id: Uuid) -> Result<Vec<Conversation>, StoreError>`保持ALL。
- Produces spec DTO与Store methods：`workspace_conversations_for_presentation(&self, project_id: Uuid) -> Result<Vec<Conversation>, StoreError>`；`workspace_latest_visible_conversation(&self, project_id: Uuid) -> Result<Option<Conversation>, StoreError>`；`workspace_list_conversation_visibility(&self, request: &ConversationVisibilityListRequestV1) -> Result<ConversationVisibilityPageV1, StoreError>`；`workspace_set_conversation_visibility(&self, request: &SetConversationVisibilityRequestV1) -> Result<ConversationVisibilityReceiptV1, StoreError>`。
- Produces direct state getter：`workspace_get_conversation_visibility(&self, project_id: Uuid, conversation_id: Uuid) -> Result<ConversationVisibilityRecordV1, StoreError>`；先验证同项目conversation存在，缺visibility row返回Visible/rev0；不存在/异project拒绝，不通过title或分页查找。

- [x] **Step 1: Write failing tests.** `visibility_migration_preserves_legacy`（fresh/legacy/reopen/twice、Archived/merged保留）、`visibility_filter_before_pagination`（>100隐藏项与50可见项）、`visibility_getter_by_owner_and_id`（同名/超page/缺row）、`visibility_replay_after_restore`、`visibility_noop_and_revision_conflict`、`visibility_composite_owner_fk`、`visibility_latest_and_delete_cascade`；检查message/group/run与Conversation时间不变。DTO合同unknown/nullable/enum/默认revision。

```rust
assert_eq!(default_state.visibility, ConversationVisibilityV1::Visible);
assert_eq!(default_state.revision, 0);
assert_eq!(page.items.len(), 50);
assert_eq!(replay.state.revision, first_receipt.state.revision);
assert_eq!(current.visibility, ConversationVisibilityV1::Visible);
assert_eq!(conversation_after.updated_at, conversation_before.updated_at);
```

- [x] **Step 2: Run RED.** `cargo test -p omicsops-store --test conversation_visibility`；`cargo test -p omicsops-desktop --lib dto_contract_tests::conversation_visibility`。Expected: FAIL，缺DTO/method或FK/CAS/filter断言失败；测试target必须先创建。
- [x] **Step 3: Implement the methods and exact spec schema.** 父unique `(frame_id,project_id)` index先于新表；CREATE IF NOT EXISTS、compositeFK/检查、缺row默认、不回填旧表；CAS+ledger同BEGIN IMMEDIATE，no-op只记receipt。Visible latest按真实user message时间。direct state getter验证owner后left join，不误把异project缺row当默认Visible。
- [x] **Step 4: Run GREEN.** 重跑RED与 `cargo test -p omicsops-store --test latest_used_conversation`、`cargo test -p omicsops-store --test conversation_branches`。Expected: PASS；原内部ALL/latest方法语义不变，新的presentation方法按visibility过滤。
- [x] **Step 5: Commit.**

```powershell
git add crates/omicsops-dto/src/conversation_visibility.rs crates/omicsops-dto/src/lib.rs crates/omicsops-store/migrations/init.sql crates/omicsops-store/src/lib.rs crates/omicsops-store/src/workspace_conversation_visibility.rs crates/omicsops-store/tests/conversation_visibility.rs src-tauri/src/dto_contract_tests.rs
git commit -m "feat: persist reversible conversation visibility"
```

### Task 2: Presentation commands, direct getters and discovery filtering

**Files:**

- Modify/Test: `src-tauri/src/workspace_commands.rs:266`, `:305`, `src-tauri/src/composer_references.rs:85`, `:155`, `:325`.
- Create/Test: `src-tauri/src/conversation_visibility_tests.rs`.
- Modify: `src-tauri/src/lib.rs:62`, `:198`（cfg test注册和commands）。

**Interfaces:**

- Consumes: Task 1DTO/Store；`workspace_source_conversation(&self, project: Uuid, id: Uuid) -> Result<Option<Conversation>, StoreError>`；existing exactsource resolver。
- Produces: `workspace_list_conversation_visibility(state: State<'_, AppState>, request: ConversationVisibilityListRequestV1) -> Result<ConversationVisibilityPageV1, ConversationVisibilityErrorV1>`；`workspace_set_conversation_visibility(state: State<'_, AppState>, request: SetConversationVisibilityRequestV1) -> Result<ConversationVisibilityReceiptV1, ConversationVisibilityErrorV1>`；`workspace_get_conversation(state: State<'_, AppState>, project_id: Uuid, conversation_id: Uuid) -> Result<Conversation, String>`；`workspace_get_conversation_visibility(state: State<'_, AppState>, project_id: Uuid, conversation_id: Uuid) -> Result<ConversationVisibilityRecordV1, ConversationVisibilityErrorV1>`。
- Produces compatibility behavior：既有list_conversations/latest_used命令保留响应形状，改用Visible presentation；catalog只过滤Session discovery，exact ensure/resolve仍ALL/direct owner。

- [x] **Step 1: Write failing tests.** `visibility_catalog_vs_resolve`（含消息的Shelved缺Session候选，但直接Session与保存稿件来源仍解析）、`visibility_direct_getter_default_and_foreign_owner`、`visibility_command_shape_and_errors`、`visibility_no_runtime_effect`、`visibility_latest_default`。Artifact/Skill/compute候选不因Session收起消失。命令replay不返回raw SQL/秘密。

```rust
assert!(!catalog.iter().any(|item| item.reference == shelved_session));
assert!(resolved_session.is_ok());
assert!(resolved_publication_source.is_ok());
assert_eq!(direct_visibility.revision, 0);
assert_eq!(runtime_call_count, 0);
```

- [x] **Step 2: Run RED.** `cargo test -p omicsops-desktop --lib conversation_visibility_tests`。Expected: FAIL，缺commands或discovery/direct/compatibility断言失败。
- [x] **Step 3: Implement the four signatures and register.** 仅展示commands改Visible；catalog在加载message/限制候选前用Visible查询，exact resolve不使用它；typed安全errors及direct owner验证。保留旧title修复行为的展示命令，但readonly visibility/direct/source路径不调用title修复或runtime恢复。
- [x] **Step 4: Run GREEN.** 重跑RED；运行 `cargo test -p omicsops-desktop --lib composer_references::tests`、`cargo test -p omicsops-desktop --lib workspace_navigation::tests`。Expected: PASS，跨项目来源拒绝与missing source历史行为仍成立。
- [x] **Step 5: Commit.**

```powershell
git add src-tauri/src/workspace_commands.rs src-tauri/src/composer_references.rs src-tauri/src/conversation_visibility_tests.rs src-tauri/src/lib.rs
git commit -m "feat: filter conversation discovery while preserving exact references"
```

### Task 3: Typed API and independently testable shelved collection

**Files:**

- Modify: `src/workspace-navigation-types.ts`, `src/workspace-navigation-api.ts`, `src/workspace-navigation-api.test.ts`, `src/tauri-api.ts`.
- Create: `src/features/workspace/ShelvedConversationsDialog.tsx`, `src/features/workspace/shelved-conversations.css`, `src/features/workspace/ShelvedConversationsDialog.test.tsx`.

**Interfaces:**

- Consumes: four commands/DTOs；existingResearchDialog/windowEscape；existing `WorkspaceConversation["status"]`而非另复制enum。
- Produces: `listConversationVisibility(request: ConversationVisibilityListRequestV1): Promise<ConversationVisibilityPageV1>`；`setConversationVisibility(request: SetConversationVisibilityRequestV1): Promise<ConversationVisibilityReceiptV1>`；`getConversationVisibility(projectId: string, conversationId: string): Promise<ConversationVisibilityRecordV1>`；`getWorkspaceConversation(projectId: string, conversationId: string): Promise<WorkspaceConversation>`。
- Produces: `ShelvedConversationsDialog(props: { projectId: string; locale: Locale; onClose: () => void; onOpenConversation: (id: string) => Promise<void> | void; onSetVisibility: (id: string, visibility: ConversationVisibilityV1, expectedRevision: number) => Promise<void> }): ReactNode`，集合查询Shelved/default50、打开不恢复。

- [ ] **Step 1: Write failing tests.** `visibility_dialog_query_and_page`、`visibility_dialog_open_does_not_restore`、`visibility_dialog_scope_and_escape`、`visibility_api_exact_payloads`、`visibility_safe_error`。同名行以conversation ID/revision操作；刷新/恢复重置page，query长度错误可见，not-found安全呈现。

```ts
expect(listRequest.visibility).toBe("shelved");
expect(listRequest.limit).toBe(50);
expect(onOpenConversation).toHaveBeenCalledWith(hiddenId);
expect(onSetVisibility).not.toHaveBeenCalled(); // open is separate from restore
expect(invoke).toHaveBeenCalledWith("workspace_get_conversation_visibility", { projectId, conversationId: hiddenId });
```

- [ ] **Step 2: Run RED.** `npx vitest run src/workspace-navigation-api.test.ts src/features/workspace/ShelvedConversationsDialog.test.tsx`。Expected: FAIL，缺API/dialog或query/open/scope/Escape断言失败。
- [ ] **Step 3: Implement the TS interfaces and dialog.** exact serde payload；paged readonly列表/搜索/empty/error/打开/恢复按钮；onRestore传该row的当前revision；关闭不触mutation；立即Escape遵循顶层window栈、generation绑定project/query/page。
- [ ] **Step 4: Run GREEN.** 重跑RED与 `npx vitest run src/features/workspace/ResearchOverlays.test.tsx`。Expected: PASS，输入焦点外立即Escape一次只关dialog，不触父层。
- [ ] **Step 5: Commit.**

```powershell
git add src/workspace-navigation-types.ts src/workspace-navigation-api.ts src/workspace-navigation-api.test.ts src/tauri-api.ts src/features/workspace/ShelvedConversationsDialog.tsx src/features/workspace/shelved-conversations.css src/features/workspace/ShelvedConversationsDialog.test.tsx
git commit -m "feat: add shelved conversation collection and typed visibility API"
```

### Task 4: Active banner, stable requests and navigation/search integration

**Files:**

- Modify: `src/DesktopApp.tsx:375`, `:943`, `:985`, `:1030`, `src/DesktopApp.test.tsx`, `src/features/workspace/WorkspaceNavigation.tsx:104`, `:109`, `src/features/workspace/WorkspaceNavigation.test.tsx`, `src/features/workspace/WorkspaceShell.tsx:82`, `:363`, `src/features/workspace/WorkspaceShell.test.tsx`, `src/features/workspace/workspace-navigation.css`.
- Modify/Test: `src/features/workspace/WorkspaceNavigationIntegration.test.tsx`, `src/features/workspace/ConversationBranchIntegration.test.tsx`, `src/features/workspace/ComposerReferences.test.tsx`, `src/use-workspace-search.test.tsx`.

**Interfaces:**

- Consumes: Task 3wrappers/dialog；existing `useWorkspaceSearch(open: boolean, projects: WorkspaceSearchProject[]): WorkspaceSearchState` 返回的 `retry(): void`刷新globalSession catalog；direct conversation和visibility getter。
- Produces shared UI callbacks: `onSetVisibility(id: string, visibility: ConversationVisibilityV1, expectedRevision: number): Promise<void>`、`onOpenShelvedConversations(): void`；WorkspaceShell新增 `activeConversationVisibility?: ConversationVisibilityRecordV1 | null` 和 `referenceCatalogVersion?: number`，后者只使catalog请求generation失效，不复用workflow版本。
- Produces behavior: DesktopApp按project/conversation保存pending原request；receipt之后精确重读state+Visible list并调用search.retry/增加referenceCatalogVersion；active getter与source direct打开各自按tuple/generation，banner restore用exact revision。

- [ ] **Step 1: Write failing integration tests.** `visibility_lost_reply_requery`（lost reply后same request ID，old receipt不覆盖later restore）、`visibility_running_stays_open`（message/group/run/status/draft不变、新消息不unshelf）、`visibility_direct_source_open`（source/search collection/usage/branch精确打开不依赖Visible list.find）、`visibility_catalog_refresh`（global/#除去/恢复Session且已选chip可用）、`visibility_no_visible_latest`、`visibility_scope_and_escape`。

```ts
expect(retryRequest.request_id).toBe(firstRequest.request_id);
expect(getConversationVisibility).toHaveBeenCalledWith(projectId, hiddenId);
expect(screen.getByText("running transcript")).toBeVisible();
expect(cancelRun).not.toHaveBeenCalled();
expect(reconnectKernel).not.toHaveBeenCalled();
expect(searchRetry).toHaveBeenCalled();
expect(defaultNavigation.queryByText(hiddenTitle)).not.toBeInTheDocument();
```

- [ ] **Step 2: Run RED.** `npx vitest run src/DesktopApp.test.tsx src/features/workspace/WorkspaceNavigationIntegration.test.tsx src/features/workspace/ConversationBranchIntegration.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/ComposerReferences.test.tsx src/use-workspace-search.test.tsx`。Expected: FAIL，缺callbacks/banner/direct opening/cache generation或stable-request断言失败。
- [ ] **Step 3: Implement the integration interfaces.** 默认nav只Visible、独立集合入口、activeShelved保持当前transcript和restore banner。审计所有listConversations.find精确ID入口并改direct getter，不自动unshelf或恢复runtime。pending同ID同body重试，unknown未解决不发相反操作；replay后requery current，不用receipt投影覆盖新状态。搜索retry/#独立version刷新，旧scope回包丢弃；分组只是visible成员投影。
- [ ] **Step 4: Run GREEN and delivery checks.** 重跑RED并运行Task 3dialog与navigation单元测试；执行 `cargo fmt --all -- --check`、`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`；锁变化先 `npm ci`。Expected: PASS并记录actual/ignored。Windows合成Idle/Running会话smoke：收起/重开应用、default search/#、旧来源精确打开、restore、分组/消息/status不变；真实runtime/SSH未执行明确记录。
- [ ] **Step 5: Commit.**

```powershell
git add src/DesktopApp.tsx src/DesktopApp.test.tsx src/features/workspace/WorkspaceNavigation.tsx src/features/workspace/WorkspaceNavigation.test.tsx src/features/workspace/WorkspaceShell.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/workspace-navigation.css src/features/workspace/WorkspaceNavigationIntegration.test.tsx src/features/workspace/ConversationBranchIntegration.test.tsx src/features/workspace/ComposerReferences.test.tsx src/use-workspace-search.test.tsx
git commit -m "feat: integrate reversible session shelving with search and navigation"
```

## Plan Self-Review

spec覆盖：Task 1migration/default/ownership/CAS/replay/pagination；Task 2presentation/direct/candidate-vs-reference；Task 3paged集合/typed边界/Escape；Task 4active banner/stable请求/精确跳转/cache与最终验证。Review Focus五项都有测试，精确state getter保证任意同名/旧ID可取revision而不靠分页。方法/DTO签名一致、不改core或run语义；没有产品实现body/真实外部前提，当前计划命令均未执行。

2026-10-15：W12 Task1 feature3809c25、独立格式提交8a47454及I1修复e4faae6完成完整与定向独立审阅，累计44/50 SOURCE。独立visibility表/DTO、exact owner getter、CAS和历史回执重放、分页前public-title查询及Visible latest已实现；旧ALL/内部latest不变，新默认latest按user时间的差异与O(全部合格标题字节)每页投影成本明确记录。I1发现core窄脱敏不等于native完整public_text，修复将原赋值/PEM纯算法共享到core并保留native原路径；实际修复RED2，最终Store14/core4/native19共37通过，fmt/diff通过。最初Store12/DTO2/latest6/branches9为此前阶段；compile RED67/8错误零执行、后置mutation1失败与真实修复RED分别保留，不混称。预格式候选仅按留存diff重建、不冒充曾捕获RAW；source7/fmt1/fix4及protected7/两个兼容残留审计通过。next_offset越界与缓存警告Minor交最终审阅；宿主/UI、全量/clean-HEAD/native手工及真实验收仍待后续。

2026-10-15：W12 Task2 展示过滤/精确来源命令源码已独立审阅通过（source 8f20cc5 + separate fmt bd6a4ec；FULL spec compliant / quality Approved），累计45/50。四个宿主命令保持 Store-only，Visible 展示/用户消息时间排序与 ALL 精确引用分离，直接读取只投影安全标题而不写回；历史 receipt 与当前 getter 分离。最终新测试5、composer19、navigation11（35项独立覆盖），fmt/diff 通过。导航临时文件 persist 在 sandbox 内 OS5 失败，经同操作诊断与相同测试正常提权1/1、完整最近导航11/11通过；原失败保留，无导航代码修改。立即格式化前 RAW4 已捕获，feature4/format3 分别提交，protected7/外部差异/空 index 与 candidate/inverse/replay/postproof 通过。继承 SQL/DTO/CAS/脱敏约束由 Task1 FULL+scoped 闭合；M1缓存 hard-link 警告保留最终审阅，UI Tasks3–4、最终全量/clean HEAD及人工/真实验收尚未执行。
