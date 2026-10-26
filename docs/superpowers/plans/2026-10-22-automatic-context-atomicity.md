# Automatic Context Atomicity Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task; independent Store implementation and read-only reviews may be delegated.

**Goal:** 自动上下文压缩失败时保留原事件链，成功时原子保存已经准入的精确归档和检查点。

**Architecture:** Core 纯构造并准入精确事件链，通过必需的 EventStore 原子接口提交。Store 用同一 SQLite 事务校验来源、冻结身份和科学状态后写入；桌面仅在事务成功后广播。

**Tech Stack:** Rust、async_trait、sqlx SQLite、Tauri；不新增依赖。

**Spec:** `docs/superpowers/specs/2026-10-22-automatic-context-atomicity.md`

## Global Constraints

- Windows-first；测试不依赖真实模型、SSH、网络或凭据。
- 保留原始工具证据、冻结 spec 和批准计划；不新增自动重派或取消语义。
- 不新增数据库模式、前端 DTO 或手动压缩回执。

## Review Focus

- 检查点拟合改变哈希后，最终准入和持久化必须仍使用同一精确事件。
- 科学状态、事件来源或冻结身份变化时提交零写入。
- 归档或任一事件插入失败必须整体回滚。
- OrdinaryAgent 和 ApprovedPlan 都能在运行中压缩，终态或暂停不得提交。
- 可选代码视图存储失败只在原请求已准入时回退；预算拒绝无持久化。

## Task 1: Core 准入和原子接口

**Files:** `crates/omicsops-agent-core/src/lib.rs`

**Interface:** `EventStoreV4::commit_context_compaction(&self, spec: &RunSpecV4, transcript: &str, events: &[AgentEventV4; 2]) -> Result<(), ContextCompactionCommitErrorV4>` replaces `archive_context`. Errors distinguish `Rejected(String)` and `Unavailable(String)`; only `Unavailable` permits the optional code-view fallback.

- [x] 更新自动压缩引用回归，证明旧实现正式哈希与预检不一致。
- [x] 拟合后固定完整事件身份，再验证所有完整/投影视图分支，原子提交。
- [x] 更新内存替身，验证拒绝零写入和可选回退，运行 Core 测试。

## Task 2: SQLite 与宿主

**Files:** `crates/omicsops-store/src/context_compaction.rs`；`src-tauri/src/agent_v4.rs` 及其测试替身。

**Interface:** `Store::commit_automatic_context_compaction_v4` accepts the same arguments and returns `Result<(), StoreError>`.

Resume interface: `Store::start_agent_execution_v4(&self, spec: &RunSpecV4, expected_status: &str, expected_head: &str) -> Result<(), StoreError>` checks source and stop/terminal state atomically, then updates only the durable running status under caller driver ownership.

- [x] 添加运行中两种执行模式、来源失效、逐点 SQL 失败及重开测试。
- [x] 实现校验与同一事务的归档/事件插入；成功后宿主发布精确事件。
- [x] 更新所有 EventStore 实现；只读准备端口必须显式拒绝提交。
- [x] 修复独立审查发现的恢复状态缺口：两种模式暂停→运行→压缩；来源变化、停止和终态不得被运行状态覆盖。
- [x] 运行 Store 和相关桌面测试。

## Task 3: 集成验证与提交

- [x] 独立审查完整差异，修复具体问题。
- [x] 运行 `npm run build`、`cargo test --workspace`、`npm test`、`npm run build:desktop`、格式和差异检查。
- [x] 记录实际结果及未执行的真实验收，更新上一轮待办状态，按功能提交本地代码。

## Execution Notes

- 复用现有 `codex/wisp-science-improvements` 工作分支；开始时工作区干净。
- 本轮只推进首项 P1；其他已记录问题保留独立范围。
- Ruling: 原子提交端口使用结构化错误分类 — 复用旧的全部错误回退会掩盖来源冲突，因此只有数据库/I/O 不可用允许可选回退；不改变 Store 的公共错误类型。
- Core 身份回归先在旧实现失败（预检与正式 event_hash 不同），固定身份后通过；首轮 Core 全量 260 项通过，随后补充来源拒绝不回退的回归。
- Ruling: 补充恢复启动事务 — 独立审查证明旧恢复入口始终保留暂停状态，与新增自动压缩运行中校验冲突；使用小型状态转换 API，避免扩大通用 run 保存接口的语义。
- 2026-10-26 最终验证：workspace 2053 通过、21 ignored，前端 1518 项及扩展 22 项通过，Web 和 Windows Release/NSIS 构建通过；真实模型与 SSH 验收未执行。
- 功能提交：先以 `9606646` 提交恢复执行状态事务，再提交自动压缩原子化及其设计、测试和验证记录。
