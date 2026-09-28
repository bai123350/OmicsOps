# 审批诊断与稳定工具身份 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 阻止未知 provider 工具名破坏研究运行，并以真实调用证据处理审批问题。

**Architecture:** Provider adapter 负责稳定别名及当前请求身份校验；desktop 负责明确错误分类；core 复用一次输出修复额度，不执行被拒响应的任何工具。审批规则只根据实际生产分类结果作最小更改。

**Tech Stack:** Rust、serde_json、现有 sha2、mock provider/ToolPort、React 现有测试。

**Spec:** `docs/superpowers/specs/2026-09-28-approval-and-tool-identity.md`

## Global Constraints

- Windows-first；不引入真实 API、SSH、GPU 或网络作为自动化测试前提。
- 不改写历史审批、冻结能力、凭据或事件；不猜测旧数字别名；不重试已派发副作用。
- 每个独立修复包含测试和实际验证结果后提交；不动不相关工作区文件。

## Review Focus

- 合法 canonical ID 与生成前缀重叠：保持稳定且不可混淆。
- 过滤列表后的过期别名：拒绝而非错映射到邻近工具。
- 一个响应先有合法调用后有非法调用：全部零派发。
- 错误工具名含状态码或 timeout：不会被误分类为 transport 重试。
- 旧审批卡与当前规则不一致：原决定和 hash 继续有效。

## Task 1：稳定身份、拒绝未知工具与有界修复

**Files:** `crates/omicsops-adapters/src/llm.rs`、`crates/omicsops-adapters/tests/model_provider_contracts.rs`、`src-tauri/src/agent_v4.rs`、`crates/omicsops-agent-core/src/lib.rs`。

**Interfaces:** 保留现有 request-scoped builder/decoder 公开入口；内部 alias 生成只依赖 canonical ID，request-scoped canonical 解析返回 `AdapterResult<String>`。未知工具使用固定 `unknown_provider_tool:` 错误标识，不包含模型任意文本。无请求的兼容解析入口不冒充已校验目录；生产入口必须 request-scoped。

- [x] 写测试并确认失败：同工具在 reorder/insert/remove earlier tool 后 provider 名相同；前缀同名 canonical 无碰撞；名字合法且 <=64；三协议流/非流精确映射；未知、旧数字、移除工具拒绝。
- [x] 最小修改 alias 和当前请求成员校验；保持流式字节拆分与 arguments 增量。不要以仅覆盖正常单块流的测试替代分块验证。
- [x] 写 desktop/core mock 测试：明确 InvalidResponse；非法名称的状态码不能触发 transport；好坏混合响应不派发；修复下一响应成功；第二次未知名或 malformed JSON 超出共享修复额度；已有证据保持。
- [x] 在 `classify_model_failure` 优先识别固定错误码；在 `model_turn_with_policy` 现有 output repair 分支接入该错误，按错误类别生成准确指令与用户可见解释。
- [ ] 运行 `cargo test -p omicsops-adapters --test model_provider_contracts`、`cargo test -p omicsops-agent-core`、`cargo test -p omicsops-desktop --lib`，记录实际结果。
- [ ] 检查 diff 并提交本项代码、测试和相关文档，建议 `fix: stabilize provider tool identities and bound invalid-call recovery`。

## Task 2：允许已证明项目相对路径的 exists 元数据检查

**Files:** `src-tauri/src/runtime_approval.rs`、`docs/superpowers/specs/2026-09-27-runtime-path-approval.md`；本次 spec/plan 的验证记录。无需改动 agent_v4 或 ToolPort 接口。

**Interfaces:** 保持 `ordinary_runtime_call_is_low_risk(&Value) -> bool`；`safe_os_path_call` 将 exists 作为 getsize 同级的一参数操作，`safe_relative_path_expr` 仍仅把 join 认作返回路径的操作。

- [x] 只读关联真实事件：2026-09-28 08:49:27 UTC；代码 hash 见 spec。生产分类 false，仅在临时诊断副本加入一参数 exists 后为 true；研究代码未执行。
- [ ] 添加失败正例：`import os\np = 'results/review.csv'\nif os.path.exists(p):\n    print('present')`；以及直接字面量、join 参数。加入绝对/父级/控制目录、未知/重绑/条件赋值路径、别名/裸方法、零/多参数和 exists 布尔结果再作路径的危险对照。
- [ ] 最小修改 `safe_os_path_call` 允许 exists，单参检查与 getsize 相同；不增加 isfile/isdir，不把 exists 结果认作路径证明。
- [ ] 运行 `cargo test -p omicsops-desktop --lib runtime_approval`，并保持 Local/SSH、旧批准/拒绝、Plan 隔离相关 core 测试通过。再次用修复后的生产函数仅分类原始调用，期望 true。
- [ ] 提交本项独立修改及实际证据说明，保留未执行真实验收的声明。

## Delivery

- [ ] 执行 `cargo fmt --all -- --check`；必要格式变化依 AGENTS.md 单独提交。
- [ ] 执行 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`。
- [ ] 如有一次性真实环境，按 `acceptance/README.md` 显式运行对应 ignored 测试；否则记为未执行，不能称真实端到端通过。
- [ ] 记录真实命令和结果、手工 smoke、审批/凭据/数据/恢复边界，以及未解决的真实运行来源归因。检查 Git 只含本次授权文件。
