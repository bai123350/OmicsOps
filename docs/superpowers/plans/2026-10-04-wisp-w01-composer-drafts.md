# W01 Composer Draft Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 切项目/会话恢复各自文字，接受回执只清理原作用域已提交的版本。

**Architecture:** DesktopApp 持有单窗口内存存储，WorkspaceShell 用按作用域订阅的 Hook 读写。发送凭据绑定 scope/revision，附件与引用保留原授权及 generation 规则。

**Tech Stack:** React 19、TypeScript、useSyncExternalStore、Vitest/RTL；无新依赖。

**Spec:** [W01 设计](../specs/2026-10-04-wisp-composer-files-jobs-design.md#w01会话文字草稿)、[共同约束](../specs/2026-10-04-wisp-improvements-design.md)。设计已确认；用户于2026-10-06确认计划并选择子Agent逐项实施与审阅。Task1已通过独立审阅，Task2–3待实施。

## Global Constraints

- 存储生命期为当前应用窗口；不能使用模块全局单例、SQLite、localStorage 或日志保存草稿。
- 键为 `(project_id, conversation_id | null)`；revision 每次修改增加，即使文字再次相同。
- 选中引用与附件不加入文字缓存；恢复文字不恢复跨作用域材料或授权。
- 拒绝、失败或未知回执保留文字；收起会话不清理草稿。
- 使用当前 `codex/wisp-science-improvements` checkout；不另建工作树或聊天。

## Review Focus

- 修改、清空后重输入同文：旧回执不能触发 ABA 清空（Task 1）。
- React 相同 snapshot 的稳定引用：订阅不产生无限重渲染（Task 1）。
- WorkspaceShell 真正卸载后回来：缓存应仍属于同一应用窗口（Task 2）。
- 分支打开期间目标被编辑：源消息不能覆盖目标新草稿（Task 2）。
- A 发送后切 B 再返回 A：文字凭据与附件清理的 generation 不能混用（Task 3）。

---

### Task 1: 可订阅的纯内存版本存储

**Files:** Create `src/features/workspace/composerDrafts.ts`、`useScopedComposerDraft.ts`；Test 同目录 `composerDrafts.test.ts`、`useScopedComposerDraft.test.tsx`。

**Interfaces:** Produces `DraftScope={projectId:string;conversationId:string|null}`、`DraftSnapshot={text:string;revision:number}`、`DraftReceipt=DraftSnapshot & {scope:DraftScope}`；`ComposerDraftStore.read(scope):DraftSnapshot`、`subscribe(scope,listener:()=>void):()=>void`、`update(scope,updater:string|((text:string)=>string)):DraftSnapshot`、`capture(scope):DraftReceipt`、`clearAccepted(receipt):boolean`、`adoptUnassigned(projectId:string,conversationId:string):boolean`、`removeConversation(scope):void`、`removeProject(projectId:string):void`。Hook 为 `useScopedComposerDraft(store:ComposerDraftStore,scope:DraftScope):{draft:string;setDraft:Dispatch<SetStateAction<string>>;capture:()=>DraftReceipt}`。

- [x] **RED: 新增版本与订阅行为断言**；同一空 snapshot 引用稳定、仅本作用域通知、unsubscribe、不同 store 隔离、adoption 不覆盖目标、真实删除清空正文并推进版本均覆盖。

```ts
it("keeps edited same-text drafts when the old receipt arrives", () => {
  const store = new ComposerDraftStore(); const a = { projectId: "p", conversationId: "a" };
  store.update(a, "same"); const receipt = store.capture(a);
  store.update(a, ""); store.update(a, "same");
  expect(store.clearAccepted(receipt)).toBe(false);
  expect(store.read(a).text).toBe("same");
  expect(store.read(a)).toBe(store.read(a));
});
```

- [x] **Run RED:** `npx vitest run src/features/workspace/composerDrafts.test.ts src/features/workspace/useScopedComposerDraft.test.tsx`；实际missing-module失败；初次sandbox EPERM另记录，不作为RED。
- [x] **Implement:** 在上述文件实现 Interfaces。键用 JSON tuple；snapshot 不可变且 read 不新建相同版本对象；Hook 用 useSyncExternalStore，setter 绑定 render 的 scope。clear/delete 推进 revision，保留无正文版本标记，避免旧回执复用。无会话 adoption 仅移交已有文字给无文字目标。
- [x] **Run GREEN:** 同命令19/19通过，tsc通过。独立审阅发现adoption重入回调丢失新源文字；先增加失败回归，再以捕获revision条件clear修复；复审通过。
- [x] **Commit:** `afb1af3 feat: isolate composer drafts by conversation`；`ff8595b fix: preserve reentrant edits during draft adoption`。仅任务文件，无未解决审阅发现。

### Task 2: 应用生命期、导航与分支起始草稿

**Files:** Modify `src/DesktopApp.tsx`、`src/features/workspace/WorkspaceShell.tsx`；Test `src/DesktopApp.test.tsx`、`src/features/workspace/WorkspaceShell.test.tsx`、`src/features/workspace/ComposerIntegration.test.tsx`。下方 renderShell 片段使用 ComposerIntegration 中的已有 fixture。

**Interfaces:** Consumes Task 1；WorkspaceShell Props 新增 `composerDraftStore?:ComposerDraftStore`（独立示例默认实例仍只在该组件生命期）；DesktopApp 用稳定 ref 创建并注入。branch 回执使用已有 `ConversationBranchV4.project_id/branch_conversation_id`，不能使用 source ID 作为目标。

- [ ] **RED: 导航、卸载与新建断言**；跨项目相同 conversation 字符串不混用；无会话明确新建 adoption，普通导航不 adoption；活跃 A 新建 B 空白；删除失败保留、成功清理；收起保留。

```tsx
it("restores a project draft after the workspace unmounts", () => {
  const store = new ComposerDraftStore(); const scope = { projectId: "project-composer", conversationId: "a" };
  store.update(scope, "A notes");
  const view = renderShell({ activeConversationId: "a", composerDraftStore: store }); view.unmount();
  renderShell({ activeConversationId: "a", composerDraftStore: store });
  expect(screen.getByRole("textbox", { name: /Describe a research goal/ })).toHaveValue("A notes");
});
```

- [ ] **Run RED:** `npx vitest run src/DesktopApp.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/ComposerIntegration.test.tsx`；新 Props/恢复测试应失败，已有测试失败单独判断。
- [ ] **Implement:** 将所有文字 setter 接入 Hook；DesktopApp 仅在确认新建来源后 adoption，真实删除成功后 purge。branch cache seed 写明确目标，只在目标仍为空且 revision 与打开前一致时写入；其他命令/引用插入保留原行为。不改变附件或引用持久化。
- [ ] **Run GREEN:** 上述两文件及 `npx vitest run src/features/workspace/ComposerIntegration.test.tsx` 通过；分支 pending 时编辑目标后完成旧 callback，目标保留新文字。
- [ ] **Commit:** `feat: restore workspace drafts across navigation`；暂存本任务四文件与必要合同改动。

### Task 3: 全部发送入口的 scope/revision 回执

**Files:** Modify `src/features/workspace/WorkspaceShell.tsx`；Test 同目录 `ComposerIntegration.test.tsx`、`WorkspaceShell.test.tsx`、`useComposerAttachments.test.ts`（现有文件）。

**Interfaces:** Consumes `capture/clearAccepted`。普通 send/queue、replacement/retry、side-chat/retry、branch-send 每次 attempt 记录不可变 `DraftReceipt`；重试保留原凭据，不捕获新文字。

- [ ] **RED: 异步接受矩阵**；用现有 RTL fixtures 和 deferred Promise，覆盖上述入口 accepted/rejected/thrown/pending、切作用域与同文字新 revision。附件/引用只在原 generation/已接受 ID 下清理。

```tsx
it("clears only the accepted offscreen version", async () => {
  const store = new ComposerDraftStore();
  const a = { projectId: project.id, conversationId: "a" }; const b = { ...a, conversationId: "b" };
  let accept!: (value: boolean) => void;
  const onQueue = vi.fn(() => new Promise<boolean>((resolve) => { accept = resolve; }));
  store.update(a, "A notes");
  const view = renderShell({ activeConversationId: "a", composerDraftStore: store, onQueue });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  view.rerender(<WorkspaceShell project={project} locale="en-US" activeConversationId="b"
    composerDraftStore={store} onQueue={onQueue} computeBackends={[localBackend]}
    computeBackendId="local" runtimeDraftSelection={runtimeSelection} />);
  const input = screen.getByRole("textbox", { name: /Describe a research goal/ });
  fireEvent.change(input, { target: { value: "B notes" } });
  await act(async () => { accept(true); });
  expect(store.read(a).text).toBe("");
  expect(store.read(b).text).toBe("B notes");
  expect(input).toHaveValue("B notes");
});
```

- [ ] **Run RED:** `npx vitest run src/features/workspace/ComposerIntegration.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/useComposerAttachments.test.ts`；旧仅比较 draft 文本/generation 的清理不能通过新矩阵。
- [ ] **Implement:** 接受后始终按捕获凭据尝试清理原文字版本；reference/attachment UI 清理继续受 generation 与原 selection/ID 限制。异步回执不修改另一 attempt 的重试状态；纯浏览器无宿主 demo 发送也用同一凭据，保留既有 accepted=false 语义。
- [ ] **Run GREEN:** 同一矩阵及 `npm test`、`npm run build` 通过。纯前端改动无需单独 desktop build；全任务最终 Rust/desktop 检查见总计划。
- [ ] **Commit:** `fix: bind composer receipt cleanup to draft revisions`；记录验证结果，标记 W01 实现完成后才更新总清单。
