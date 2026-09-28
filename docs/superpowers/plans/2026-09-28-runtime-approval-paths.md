# Runtime Approval Paths Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. User-selected roles remain Astra high architecture, Sol high implementation, Luna max reading.

**Goal:** 常规项目建目录与日期命名结果不再反复审批，仍需审批的调用显示准确且不泄密的固定原因。

**Architecture:** 保持原 runtime classifier 的 lexer 与危险操作检查，将 OS/path 证明交给独立的完整 Python AST 与保守绑定审计。Desktop 通过默认宿主 hook 将有限原因填入既有请求字段，不修改持久化协议或授权优先级。

**Tech Stack:** Rust、现有 serde_json、原 token 危险检查、固定版本 rustpython-parser 0.4.0、现有 Rust/TypeScript 测试；不新增 Python 运行依赖。

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

**状态：2026-09-29 继续实施。** 用户在上一轮风险说明和受限方案确认后，再次明确要求修复自动批准。本轮 token 解析器修改再次被自动审查拒绝，没有落盘或通过其他工具重试。Astra 复核后采用实质不同的完整 AST 方案，消除旧 lexer 对数字和 f-string 的信息丢失，仍保持既定能力范围。上一轮保留的 RED/危险反例 patch 仅供核对；在当前已交付诊断的基础上重新运行测试、实施并独立复审。本节只有验证通过后才能标为完成；此前 Task2 结果不能代替本项。

**Files:** Modify/test `src-tauri/src/runtime_approval.rs`、`src-tauri/src/agent_v4.rs`；新增 `src-tauri/src/runtime_approval_ast.rs` 并在桌面组合中声明模块；更新根目录与桌面 `Cargo.toml`、`Cargo.lock`。独立语法解析与绑定审计构成明确职责边界，不按文件长度重构。

**Interfaces:** 保留 `ordinary_runtime_call_is_low_risk(&Value) -> bool`；内部值状态至少区分已知相对路径、日期对象、安全文件名片段。生产入口继续不执行代码。

- [x] 添加 `project_date_named_output_is_ordinary`，包含 import os/datetime、UTC now、strftime `%Y-%m-%d`、replace `-` 到空、项目 `%s` 模板、makedirs/open/getsize；断言完整输入为 true。
- [x] 定向执行该测试，记录旧生产实现下 1 failed 的 RED；修复后通过。
- [x] 按 spec 使用完整 AST 实现共享的日期/片段/相对路径证明、精确参数消费与 makedirs 路由；先全树绑定审计，保留旧 lexer、危险检查和输入上限，增加解析预算。删除被替代的旧 token OS/path 证明，未增加旧成功结果绕过 AST 的分支。
- [x] 增加正例矩阵：变量名/日期格式/目录变化、join、多个安全片段、exist_ok 默认/True/False、精确当前目录段。
- [x] 增加反例矩阵：未知插值、日期格式含路径字符、模板/参数不匹配、数字额外实参、日期/模块/属性重绑及修改后重新 import、条件/def/循环/except/match/walrus 污染、OS危险调用、外部路径、布尔/大小结果、f-string 动态表达式，全部断言 false。补充模块表、Win32 特殊路径、前向引用及预算边界，旧绑定测试仍通过。
- [x] 最终 `cargo test --offline -p omicsops-desktop runtime_approval::tests --lib`：20/20；完整原始参数和七个危险变体经生产 bool/reason 静态分类符合预期。原始参数哈希再次核对一致；临时环境测试已移除，原文未入库，未执行研究代码。
- [x] `cargo test --offline -p omicsops-desktop lazy_interpreter_probe_is_language_specific_and_never_reserves_a_job_on_failure --lib`：1/1，增加真实 Desktop 审批入口、ToolRegistry、AgentCore 的四场景单步回归；独立 Astra 实际代码审查无剩余阻塞问题。
- [ ] Commit: `fix: recognize proven date-named project outputs`。

### Task 2: 固定分类原因进入已有审批请求

**Files:** Modify/test `src-tauri/src/runtime_approval.rs`、`src-tauri/src/agent_v4.rs`、`crates/omicsops-agent-core/src/lib.rs`、`crates/omicsops-tools/src/lib.rs`。宿主 executor 的默认 hook 必须由授权过滤后的 registry 转发到 `ToolPortV4`。

**Interfaces:** classifier 保留现有 bool 判定，诊断入口先检查该 bool 再生成有限固定原因，不反向改变批准结果；`ToolPortV4::risk_based_approval_reason(&self, call: &ToolCallV4) -> Option<String>` 默认 None。具体 enum/type 名由实现者统一命名，无跨 crate DTO。

- [x] 添加 classifier 固定原因断言：只输出可确认的原因或诚实总括，去重且最多四个；不以删除调用后重新分类的方式推断原因；不承诺一次枚举所有根因；明确危险/解析失败保持拒绝，消息不包含输入原文。
- [x] 添加 core mock host hook 测试：RiskBased runtime 审批保存 host 原因；None 回退；Request 保持原文案，Plan 仍走独立请求分支。调用参数放入可识别的敏感测试标记，断言原因不包含标记。
- [x] 运行对应 focused 测试确认新增行为尚未实现。
- [x] 保留批准布尔判定，增加独立诊断入口、默认 hook、desktop runtime hook 与 core reason 接线，保留 MCP/浏览器文案优先级；原因不参与授权判断。
- [x] 运行 `cargo test -p omicsops-desktop --lib runtime_approval`、`cargo test -p omicsops-agent-core risk_based_runtime` 和新增 registry reason 测试。诊断只读取工具参数，无 Local/SSH 分支；未执行真实 SSH。
- [x] Commit: `a19a853 fix: explain runtime approval classification`。

### Task 3: 全量验证与交付记录

**Files:** 本计划追加实际验证记录；需要时更新本 spec 的实际已交付边界。

- [x] 执行 `cargo fmt --all -- --check`；首次仅格式失败，已运行 `cargo fmt --all`，复查通过；纯格式提交 `add131a`。
- [x] 最终 `cargo test --workspace`：1,389 通过、12 忽略；此前未包含最后一个回归的首轮为 1,388 通过、12 忽略。
- [x] `npm test`：953 前端、22 浏览器桥接测试通过；`npm run build` 通过。锁文件未变。
- [x] `npm run build:desktop` 通过（exit 0），原生 release 编译和 NSIS 本地打包完成；未发布或分发。可执行文件 `target/release/omicsops-desktop.exe` 为 75,761,152 字节，本地修改时间 2026-09-28 18:35:51。
- [x] 实现者使用完整真实参数仅调用生产 bool/reason，确认仍为 false 且诊断不泄露输入；临时 ignored 测试已移除。九个辅助 TEMP probe/变体按已核实的精确路径清理。原参数和忽略的 Task1 测试 patch 保留用于授权后继续，未执行研究调用。
- [x] 确定性检查全部结束后按 `acceptance/README.md` 显式执行一次临时项目的模型读文件 ignored 验收，1/1 通过（12.86 秒）；不代替审批规则、SSH 或 miRNA 流程验收。
- [x] 已记录当前手工 smoke 步骤，尚未执行原生 UI 手工 smoke：重启新构建后，在 RiskBased 中新发起仍超出现有规则的调用，检查固定原因且不能直接执行；Request 模式仍按原策略审批。工具派发后超过 90 秒无结果时应说明工具进度未知，委派节点进度与恢复事件应重置对应静默计时。历史审批不回填。Task1 的免审批 smoke 要待授权并实现后再做。
- [x] Commit: `docs: record runtime path approval validation`。

## 定向验证记录

`cargo test -p omicsops-desktop --lib runtime_approval` 最终 15/15；新增变量名回归先 RED 后 GREEN。`cargo test -p omicsops-desktop --lib lazy_interpreter_probe_is_language_specific_and_never_reserves_a_job_on_failure` 1/1；`cargo test -p omicsops-agent-core risk_based_runtime` 2/2；`cargo test -p omicsops-tools risk_based_reason_is_forwarded_only_for_valid_enabled_calls` 1/1。上述检查不需要真实 SSH、样本数据或模型凭据。

独立 Astra 审查覆盖 UI 与诊断四个 Rust 文件，具体发现的委派阶段、恢复计时、路径原因误报和变量名误报均已修复。Task1 的生产补丁未获准，不能将这些测试结果描述为自动审批误判已解决。

真实模型命令为 `cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture`，按 README 在子进程环境中选择既有 OpenCode Go DeepSeek profile，凭据仍从系统 keyring 读取。1/1 通过（exit 0，测试执行 12.86 秒，不含编译），临时 nonce 被读取并返回。仅打印元数据：121 次非空推理快照均先于对应结果，2 次有内容的模型请求。未修改用户数据库、模型配置或原研究目录；未执行原生 UI、真实 SSH 或 miRNA 端到端验收，未证明原工作流提速。

## 2026-09-29 AST 修复的本轮验证

以上 Task 2/3 和 12.86 秒模型记录为上一轮诊断改动的验证，不能代替本节。此次原始调用已通过新的生产分类函数；其余七个危险变体均保留审批。测试只读取 TEMP 中的原始参数，不执行脚本，原始参数不提交 Git。

运行层回归使用真实 Desktop 审批 hook、ToolRegistry 与 AgentCore，执行器仅计数：RiskBased 下日期命名项目结果产生一次派发与成功工具完成事件，没有审批事件；subprocess、未知路径与 RequestApproval 模式产生审批事件，没有派发或执行。每次仅运行一轮，安全场景随后因测试步数预算进入 NeedsAttention；其余为 WaitingForApproval。这验证审批链路，不代表完整科研运行成功或真实 Python/SSH 验收。

- [x] `npm ci`：干净安装成功，304 个包；Node 锁文件未改。
- [x] `npm test`：953 前端与 22 浏览器桥接测试通过。
- [x] `npm run build`：通过，保留既有 bundle 大小提示。
- [x] `cargo test --workspace`：1,394 通过，0 失败，12 忽略；exit 0。忽略项不计为真实环境验收通过。
- [ ] `cargo fmt --all -- --check`；如仅格式差异失败，格式修正单独提交。
- [ ] `npm run build:desktop`。
- [ ] 确定性检查后按 acceptance/README.md 显式执行临时 nonce 的真实模型读文件验收。
- [ ] 最终提交和工作区检查；保留用户的未跟踪 website。

原生 UI 手工 smoke 尚未执行：关闭旧程序并启动新构建后，用新的普通调用验证 RiskBased 不出现审批卡片，未知路径与 RequestApproval 仍出现卡片。历史 pending/denied 记录不追溯自动批准；不得据此自动重跑可能已派发的原研究作业。
