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
- [x] adapters 契约测试及 core/desktop 针对性测试通过；`cargo test --workspace` 覆盖完整 Rust 套件，实际命令与结果见末尾验证记录。
- [x] 检查 diff 并提交本项代码、测试和相关文档，建议 `fix: stabilize provider tool identities and bound invalid-call recovery`。

## Task 2：允许已证明项目相对路径的 exists 元数据检查

**Files:** `src-tauri/src/runtime_approval.rs`、`docs/superpowers/specs/2026-09-27-runtime-path-approval.md`；本次 spec/plan 的验证记录。无需改动 agent_v4 或 ToolPort 接口。

**Interfaces:** 保持 `ordinary_runtime_call_is_low_risk(&Value) -> bool`；`safe_os_path_call` 将 exists 作为 getsize 同级的一参数操作，`safe_relative_path_expr` 仍仅把 join 认作返回路径的操作。

- [x] 只读关联真实事件：2026-09-28 08:49:27 UTC；代码 hash 见 spec。生产分类 false，仅在临时诊断副本加入一参数 exists 后为 true；研究代码未执行。
- [x] 添加失败正例：`import os\np = 'results/review.csv'\nif os.path.exists(p):\n    print('present')`；以及直接字面量、join 参数。加入绝对/父级/控制目录、未知/重绑/条件赋值路径、别名/裸方法、零/多参数和 exists 布尔结果再作路径的危险对照。
- [x] 最小修改 `safe_os_path_call` 允许 exists，单参检查与 getsize 相同；不增加 isfile/isdir，不把 exists 结果认作路径证明。
- [x] 运行 `cargo test -p omicsops-desktop --lib runtime_approval`，并保持 Local/SSH、旧批准/拒绝、Plan 隔离相关 core 测试通过。再次用修复后的生产函数仅分类原始调用，期望 true。
- [x] 提交本项独立修改及实际证据说明，保留未执行真实验收的声明。

## Delivery

- [x] 执行 `cargo fmt --all -- --check`；必要格式变化依 AGENTS.md 单独提交。
- [x] 执行 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`。
- [x] 如有一次性真实环境，按 `acceptance/README.md` 显式运行对应 ignored 测试；否则记为未执行，不能称真实端到端通过。
- [x] 记录真实命令和结果、手工 smoke、审批/凭据/数据/恢复边界，以及未解决的真实运行来源归因。检查 Git 只含本次授权文件。


## 2026-09-28 实施与确定性验证

- `6ffd19c`：允许已证明项目相对路径的 `os.path.exists`，沿用 getsize 单参规则。focused 测试先红（7 通过、2 失败），修复后 `cargo test -p omicsops-desktop --lib runtime_approval` 9 通过。用最终生产函数仅分类真实调用得到 `low_risk=true`；没有执行原始研究代码，临时参数和诊断程序已清理。
- `4b09873`：稳定 provider 工具身份，拒绝未知或歧义工具名，复用一次输出修复额度。先红后绿的 adapters 契约测试 22 通过；core 单次修复及共享额度测试 1 通过；desktop 分类与混合响应测试各 1 通过。Astra high 独立审查未发现剩余实质问题。
- `f6cc35b`：`cargo fmt --all` 的纯格式修改单独提交；随后 `cargo fmt --all -- --check` 通过。
- `cargo test --workspace`：1,381 通过、12 ignored、0 失败（格式化前的相同语义代码）。
- `npm test`：945 前端测试与 22 浏览器桥接测试通过。
- `npm run build`、`npm run build:desktop`：退出码 0；新的本地 Windows 程序为 `target/release/omicsops-desktop.exe`。保留既有 Web 大 chunk 与 Windows linker informational warning，未发布构建产物。

本次没有新增依赖、数据库迁移、凭据存储或授权范围；没有改写原会话、原审批决定或恢复事件。项目外路径、动态路径和危险操作仍走审批，远端未知派发仍不自动重放。开发模型分工按用户要求采用 Astra high 分析/审查、Sol high 编码、Luna max 检索；没有改动产品中已保存的模型与推理设置。


## 真实验收和手工检查边界

先运行 workspace 过滤的 ignored 测试时，Windows linker 在 adapters 单元测试目标链接阶段退出 1；模型尚未被调用。随后严格选择桌面端目标：

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

在一次性项目中使用只读选择的现有 DeepSeek `deepseek-v4.1-flash` 配置和系统 keyring，1/1 通过，退出码 0，70.11 秒。生产 Agent 读取随机验证文件并正确返回其内容；2 次有内容的模型请求，1,104 次瞬态推理观察均发生在结果前，只记录统计，未输出/持久化推理正文或凭据。首次链接失败与最终验收通过分别记录在 `acceptance/README.md`。这不是原 miRNA 流程的速度对照，也不证明供应商内部等待已经优化。

未执行：原始 miRNA 长流程、真实 SSH、原生 UI 手工 smoke。手工复查应使用新构建和临时项目，选择“帮我批准”，先运行 `os.path.exists('results/check.csv')`，确认不出审批卡，再确认未知路径、绝对路径、删除或命令执行仍要求审批；切换“请求批准”后同一普通调用仍应按模式请求批准。已有历史审批决定不被自动改写。工具名错误仅在尚未派发时可自动修复一次，第二次停止；已派发且结果不明的操作不能自动重跑。

工作区仅保留原有未跟踪的 `website/`；没有创建发布、tag 或分发安装包。
