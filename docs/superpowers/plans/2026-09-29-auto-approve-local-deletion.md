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

**Files:** `crates/omicsops-protocol/src/lib.rs`、`crates/omicsops-agent-core/src/lib.rs`、`crates/omicsops-tools/src/lib.rs`、`src-tauri/src/agent_v4.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/runtime_boundary_v4.rs`；新增 `src-tauri/src/local_deletion_approval.rs`，DTO 合同测试按实际需要更新。

**Interfaces:** 新 policy 枚举；`ToolPortV4`、`ToolExecutorV4` 默认方法 `fn local_deletion_approval_reason(&self, call: &ToolCallV4) -> Option<String>`，Registry 验证后转发；Desktop 结合 frozen backend 和工具参数给出固定原因。

- [x] 添加 RED：新策略未知 runtime、危险非删除及写入 MCP 自动批准，本地删除产生请求，RequestApproval/RiskBased 保持原样；保留缺省 RiskBased 的序列化与 hash，显式新策略 roundtrip。
- [x] 实现新策略及宿主 hook、Registry 转发和 Desktop 正向检测；按 spec 列出的 Python/R、字面命令、MCP 形态和 backend 分支建立正反例矩阵，原因不含源码或路径。
- [x] 运行真实审批链但模拟执行的集成测试：合法非删除派发一次；本地删除零派发；批准后一次派发；拒绝不派发；旧会话授权不能豁免；SSH runtime 删除自动派发，MCP 删除仍询问；非法工具/能力/参数仍拒绝。
- [x] 独立审查回归：语言大小写与 `py` 和执行器一致；新策略的一次 MCP 删除批准不能被后来 RequestApproval/RiskBased 运行复用；字面命令分隔符、常见 PowerShell 参数、Python 命名 args、R 命名参数与字面向量正确识别，日志负例仍不误报。
- [x] 接通 MCP/browser 执行层本次授权，验证普通写入 MCP 可执行而未启动批准、目录/schema 失配仍拒绝；浏览器开关及既有合法性检查不变。
- [x] 运行相关 protocol/core/tools/desktop focused tests；Astra 独立审查后提交 `feat: auto-approve calls except local file deletion`（`4376e4c`，纯格式提交 `4d8869f`）。

### Task 2: Composer 使用新规则并说明边界

**Files:** `src/types.ts`、`src/DesktopApp.tsx`、`src/features/workspace/WorkspaceShell.tsx`、`src/features/workspace/RuntimeDialog.tsx` 及对应前端测试；同步 `docs/agent-modes.md` 和 `docs/browser-security.md` 当前使用说明（文档由 root 维护）。

**Interfaces:** 新联合类型值 `auto_approve_except_local_deletion`。`currentComputeSelection()` 仅为新 composer 配置升级遗留 `risk_based`，不改通用 normalize、RunSpec 读取/恢复、已排队冻结配置和已批准计划。

- [x] 添加 RED：初始“帮我批准”、菜单回调、退出 FullAccess fallback 和新 run 参数均为新值；RequestApproval 不变；历史策略不冒充新策略。
- [x] 实现新默认与新调用边界；说明为“自动批准操作，仅检测到本地删除时询问”，附“动态代码或第三方工具中的间接删除可能无法识别”。
- [x] RuntimeDialog 为新策略单独显示本地删除审批；`useRuntimeBoundary` 与 `V4PlanPanel` 的旧配置缺省保持 `risk_based`。保留旧 `ComposerQueueIntegration` 冻结选择，恢复及批准已冻结计划只使用原 run/hash/revision；直接运行测试断言新 composer 发出的新值。
- [x] 运行对应前端测试，独立复核新调用与历史路径；提交 `feat: use local deletion approval in composer`。

### Task 3: 全量验证和交付

- [x] `cargo fmt --all -- --check`；仅格式失败则 `cargo fmt --all` 并单独提交（`4d8869f`，复查通过）。
- [x] `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`。
- [x] 确定性检查后按 acceptance/README.md 显式执行临时 nonce 真实模型读文件验收；不代替真实删除/SSH/科研端到端验收。
- [x] 记录原生 UI smoke 实际是否执行、新 exe 元数据、最终提交及工作区状态；提交验证记录。

## 已完成的确定性验证

- 后端针对新策略的序列化、冻结哈希、Core/Registry/宿主派发和 MCP 授权进行 RED/GREEN；独立审查发现的语言别名、跨运行授权复用和字面命令边界回归已修正并复核。
- 前端初始 6 项回归在旧实现下失败，改动后通过；FullAccess fallback 用回退旧值的临时变更验证失败后还原。相关 5 个测试文件 239/239 通过，历史队列及计划绑定保持。
- `cargo test --workspace`：1,406 通过，0 失败，12 忽略；忽略项不算真实验收。
- `npm test`：108 个测试文件、957 项前端测试以及 22 项浏览器桥接测试通过。
- `npm run build`：通过；保留现有大 chunk 提示，无新增依赖。
- 后端与前端任务分别由 Astra high 独立复核通过；Sol high 编写实现，Luna max 负责读取和路径查找。

## 整体验证与使用边界

前端提交 `a5b0413` 后，由新的 Astra high 审查实例检查整个策略变更，未发现新的可操作回归，之前的审查问题全部关闭。

确定性检查通过后，按 `acceptance/README.md` 显式执行：

```powershell
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

使用现有 `deepseek-v4.1-flash` 配置和系统凭据引用，临时 Store/项目，真实模型读取随机 nonce：1/1 通过，测试耗时 8.95 秒（不含 2 分 35 秒编译），退出码 0。没有修改原项目或用户数据库，没有输出凭据或推理正文。这仅验证模型 → 文件读取 → 模型结果，不证明本地删除拦截、MCP、SSH 或科研流程的真实端到端行为，也不是延迟保证。

新策略需要运行本次构建，并在“帮我批准”下新发送任务。历史待审批运行、已冻结队列和已批准计划保持原策略；若旧运行仍等待审批，应结束该运行后重新发送，而不是恢复原审批绑定。动态或第三方工具内部的间接删除可能无法识别。

原生 Windows UI smoke、真实 MCP 删除、SSH、R/Micromamba 和科研流程验收未执行。待执行的原生 smoke：在一次性项目打开权限菜单核对说明，发送无删除的 runtime 请求确认不出现审批卡；对一次性路径的直接删除请求核对审批卡并拒绝；重新打开历史运行核对仍显示原风险审批；打开菜单/运行环境面板后不移动焦点直接按 Escape，确认仅关闭顶层。不得对真实样本执行删除验收。

`npm run build:desktop` 退出码 0，release 编译耗时 4 分 18 秒，默认 NSIS 构建结束；仅完成本地构建，没有安装、分发、创建 Release 或 tag。保留现有大 chunk 提示和 Windows linker 创建库提示。

本次程序：`E:\Project\OmicsOps\target\release\omicsops-desktop.exe`，79,660,544 字节，修改时间 `2026-09-29 03:24:33 +08:00`，SHA-256 `ba32c7366554023dbdb803e30db904c79c0a2ab33e340a4b47b1785f2dc6571b`。构建产物未纳入 Git。

提交序列：设计 `c257c79`、后端策略 `4376e4c`、纯格式 `4d8869f`、前端与使用说明 `a5b0413`，随后提交本验证记录。最终仅保留用户原有未跟踪 `website/`，未修改其内容。默认完整检查和真实 nonce 验收日志位于被忽略的 `target/auto-approve-local-deletion-*.log`。
