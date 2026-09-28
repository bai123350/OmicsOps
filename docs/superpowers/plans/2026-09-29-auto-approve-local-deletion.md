# Auto Approve Except Local Deletion Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans. User-selected roles: Astra high architecture/review, Sol high implementation, Luna max lookup. User has explicitly authorized this policy change; do not request duplicate approval.

**Goal:** “帮我批准”自动批准未知或危险操作，仅检测到本地删除时询问。

**Architecture:** 新枚举保留旧冻结策略语义，宿主正向检测本地删除，Core 和 Desktop 执行授权一致；新 composer 显式选择新策略。

**Tech Stack:** Rust、既有 RustPython AST、React/TypeScript；不新增依赖。

**Spec:** `docs/superpowers/specs/2026-09-29-auto-approve-local-deletion.md`

## Global Constraints

- 保留 RiskBased serde 默认、旧冻结配置/哈希、历史精确调用审批；新值必须显式持久化。
- 不执行代码进行分类；未知或解析失败按新策略自动批准，有限识别不能宣传为沙箱。
- 本地删除批准只用于本次调用；启用、冻结能力、参数、MCP 启动批准/目录/schema 与浏览器目标检查仍有效。
- 不启用浏览器功能、不写永久授权、不改真实数据库、不执行真实删除或研究任务；保留 `website/`。
- 新策略名称为 `AutoApproveExceptLocalDeletion`，wire value 为 `auto_approve_except_local_deletion`。

## Review Focus

- 旧配置缺省值与已冻结队列/恢复路径不能被新默认升级。
- SSH runtime 的远端删除与本机 MCP、容器映射删除必须区分。
- 只读标签及会话/全局授权不能跳过检测到的本地删除。
- Core 自动批准和实际 MCP/browser executor 授权必须一致，非法能力仍拒绝。
- 删除词出现在日志、注释、列表修改或 HTTP DELETE 时不能误报。

### Task 1: 宿主策略、删除检测与执行接线

**Files:** `crates/omicsops-protocol/src/lib.rs`、`crates/omicsops-agent-core/src/lib.rs`、`crates/omicsops-tools/src/lib.rs`、`src-tauri/src/agent_v4.rs`、`src-tauri/src/lib.rs`；新增 `src-tauri/src/local_deletion_approval.rs`，DTO 合同测试按实际需要更新。

**Interfaces:** 新 policy 枚举；`ToolPortV4`、`ToolExecutorV4` 默认方法 `fn local_deletion_approval_reason(&self, call: &ToolCallV4) -> Option<String>`，Registry 验证后转发；Desktop 结合 frozen backend 和工具参数给出固定原因。

- [ ] 添加 RED：新策略未知 runtime、危险非删除及写入 MCP 自动批准，本地删除产生请求，RequestApproval/RiskBased 保持原样；保留缺省 RiskBased 的序列化与 hash，显式新策略 roundtrip。
- [ ] 实现新策略及宿主 hook、Registry 转发和 Desktop 正向检测；按 spec 列出的 Python/R、字面命令、MCP 形态和 backend 分支建立正反例矩阵，原因不含源码或路径。
- [ ] 运行真实审批链但模拟执行的集成测试：合法非删除派发一次；本地删除零派发；批准后一次派发；拒绝不派发；旧会话授权不能豁免；SSH runtime 删除自动派发，MCP 删除仍询问；非法工具/能力/参数仍拒绝。
- [ ] 接通 MCP/browser 执行层本次授权，验证普通写入 MCP 可执行而未启动批准、目录/schema 失配仍拒绝；浏览器开关及既有合法性检查不变。
- [ ] 运行相关 protocol/core/tools/desktop focused tests；Astra 独立审查后提交 `feat: auto-approve calls except local file deletion`。

### Task 2: Composer 使用新规则并说明边界

**Files:** `src/types.ts`、`src/DesktopApp.tsx`、`src/features/workspace/WorkspaceShell.tsx` 及对应前端测试；必要时只读历史展示 helper。

**Interfaces:** 新联合类型值 `auto_approve_except_local_deletion`。`currentComputeSelection()` 仅为新 composer 配置升级遗留 `risk_based`，不改通用 normalize、RunSpec 读取/恢复、已排队冻结配置和已批准计划。

- [ ] 添加 RED：初始“帮我批准”、菜单回调、退出 FullAccess fallback 和新 run 参数均为新值；RequestApproval 不变；历史策略不冒充新策略。
- [ ] 实现新默认与新调用边界；说明为“自动批准操作，仅检测到本地删除时询问”，附“动态代码或第三方工具中的间接删除可能无法识别”。
- [ ] 运行对应前端测试，独立复核新调用与历史路径；提交 `feat: use local deletion approval in composer`。

### Task 3: 全量验证和交付

- [ ] `cargo fmt --all -- --check`；仅格式失败则 `cargo fmt --all` 并单独提交。
- [ ] `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`。
- [ ] 确定性检查后按 acceptance/README.md 显式执行临时 nonce 真实模型读文件验收；不代替真实删除/SSH/科研端到端验收。
- [ ] 记录原生 UI smoke 实际是否执行、新 exe 元数据、最终提交及工作区状态；提交验证记录。
