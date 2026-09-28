# Runtime Approval Paths Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. User-selected roles remain Astra high architecture, Sol high implementation, Luna max reading.

**Goal:** 常规项目建目录与日期命名结果不再反复审批，仍需审批的调用显示准确且不泄密的固定原因。

**Architecture:** 在现有 runtime classifier 内增加小型有类型值证明与有限原因。Desktop 通过默认宿主 hook 将原因填入既有请求字段，不修改持久化协议或授权优先级。

**Tech Stack:** Rust、现有 serde_json、现有 token lexer、现有 Rust/TypeScript 测试；不新增 Python 解析/运行依赖。

**Spec:** `docs/superpowers/specs/2026-09-28-runtime-approval-paths.md`

## Global Constraints

- 判定是 best-effort 审批路由，不是沙箱；未知能力保持审批。
- 不执行研究代码进行分类，不将真实原文或数据库写进仓库。
- 不自动批准历史 pending，不改精确调用 hash、冻结计划、Plan/MCP/浏览器门禁。
- 不新增数据库或 DTO 字段；默认 trait hook 保持现有实现兼容。
- 每项测试完成后 commit；纯格式变化独立 commit；不触碰未跟踪 website。

## Review Focus

- 当前 lexer 会扁平化 f-string，不能由表面 String token 建立动态路径证明。
- 日期方法必须有来源证明，重新绑定模块、对象或方法后原证明不能继续使用。
- `%s` 多参数与模板边界必须精确消费，额外表达式不能隐形通过。
- `exists`/`getsize` 返回值不能进入路径类型，安全片段不能由未知值 replace 得到。
- 风险理由属于宿主诊断，不能覆盖其他审批策略或泄露路径、URL 与源码。

---

### Task 1: 有类型路径证明覆盖完整研究代码形态

**Files:** Modify/test `src-tauri/src/runtime_approval.rs`。不另拆模块。

**Interfaces:** 保留 `ordinary_runtime_call_is_low_risk(&Value) -> bool`；内部值状态至少区分已知相对路径、日期对象、安全文件名片段。生产入口继续不执行代码。

- [ ] 添加 `project_date_named_output_is_ordinary`，包含 import os/datetime、UTC now、strftime `%Y-%m-%d`、replace `-` 到空、项目 `%s` 模板、makedirs/open/getsize；断言完整输入为 true。
- [ ] 执行 `cargo test -p omicsops-desktop --lib runtime_approval`，记录该测试在旧生产实现下失败。
- [ ] 按 spec 实现共享的日期/片段/相对路径证明、精确参数消费与 makedirs 路由，保留旧危险检查和输入上限。
- [ ] 增加正例矩阵：变量名/日期格式/目录变化、join、多个安全片段、exist_ok 默认/True/False。
- [ ] 增加反例矩阵：未知插值、日期格式含路径字符、模板/参数不匹配、日期/模块/属性重绑、条件/def/循环污染、OS危险调用、外部路径、布尔/大小结果、f-string 动态表达式，全部断言 false。旧绑定测试必须仍通过。
- [ ] 再运行 focused 测试，确认全部通过；root 用 TEMP 完整真实参数与危险变体仅做生产分类，记录结果。
- [ ] Commit: `fix: recognize proven date-named project outputs`。

### Task 2: 固定分类原因进入已有审批请求

**Files:** Modify/test `src-tauri/src/runtime_approval.rs`、`src-tauri/src/agent_v4.rs`、`crates/omicsops-agent-core/src/lib.rs`。

**Interfaces:** classifier 增加 crate 内分析结果与有限原因枚举，bool 由该结果派生；`ToolPortV4::risk_based_approval_reason(&self, call: &ToolCallV4) -> Option<String>` 默认 None。具体 enum/type 名由实现者统一命名，无跨 crate DTO。

- [ ] 添加 classifier 固定原因断言：unsupported OS operation 与 unproven project path 可同时出现，去重且最多四个；明确危险/解析失败保持拒绝，消息不包含输入原文。
- [ ] 添加 core mock host hook 测试：RiskBased runtime 审批保存 host 原因；None 回退；Request 和 Plan 不被 runtime 原因覆盖。调用参数放入可识别的敏感测试标记，断言原因不包含标记。
- [ ] 运行对应 focused 测试确认新增行为尚未实现。
- [ ] 实现统一分析结果、默认 hook、desktop runtime hook 与 core reason 接线，保留 MCP/浏览器文案优先级；原因不参与授权判断。
- [ ] 运行 `cargo test -p omicsops-desktop --lib runtime_approval`、`cargo test -p omicsops-agent-core risk_based` 和新增 reason 测试；Local/SSH 判定同样输入结果一致。
- [ ] Commit: `fix: explain runtime approval classification`。

### Task 3: 全量验证与交付记录

**Files:** 本计划追加实际验证记录；需要时更新本 spec 的实际已交付边界。

- [ ] 执行 `cargo fmt --all -- --check`；若只有格式偏差，运行 `cargo fmt --all` 并把纯格式变化单独提交。
- [ ] 执行 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`，记录实际结果。锁文件未变不强制重装。
- [ ] root 对完整真实脚本进行最终生产分类，并确认同形危险改写仍被拒绝；随后清理 TEMP 诊断文件。不得执行仍可能远端运行的研究调用，不把停止等待视为取消计算。
- [ ] 记录未执行真实模型/SSH 端到端验收；手工 smoke 为重启新构建后新发起安全项目输出调用、确认无新审批，再检查危险调用显示具体原因，不能编辑旧审批事件模拟通过。
- [ ] Commit: `docs: record runtime path approval validation`。
