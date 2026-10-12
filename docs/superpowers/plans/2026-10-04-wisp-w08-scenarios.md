# W08 Offline V4 Scenarios Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用一个版本化 manifest 运行五个离线 V4 控制行为场景，生成稳定、脱敏且失败退出非零的 JSON 报告。

**Architecture:** 在现有 agent-core 私有 tests 模块下增加 scenario_evaluation 子模块，复用 ScriptedModel/MemoryStore 和真实 core 入口。manifest/fixtures 编译期本地加载；测试 seam 记录模型尝试、工具 proposal/actual dispatch/recovery，报告只投影计数和白名单原因。

**Tech Stack:** Rust cargo unit tests、serde/serde_json、Tokio test-util、dev SHA-256/hex、Windows CI。Python Skill helper 有独立计划，此 runner 不调用 Python。

**Spec:** [W08 Runtime 设计](../specs/2026-10-04-wisp-runtime-design.md#w08manifest-驱动的离线-v4-场景与稳定报告)，已审阅设计提交 `161199f`。三个任务源码均已实现并独立审阅；CI已配置，最终clean-HEAD及真实验收另记。

## Global Constraints

- “开发者统一执行 5 个有明确输入、错误注入和断言的场景”；只验证控制行为，不以机器耗时/真实模型答案质量作阈值。
- “不新增 CLI crate、production feature、Tauri 菜单或模型连接。”私有 `mod tests { mod scenario_evaluation; }`，无测试 fixture 产品 API。
- manifest “类型 deny_unknown_fields，版本 1”；scenario_kind 为 ReadEvidence、DenyApproval、RecoverUncertain、CorrectMcpArguments、CompactPreserveSources。
- “runner 顺序执行场景并汇总全部结果，不因第一项失败丢失报告。”脚本耗尽和剩余步骤均失败，无 network/model/SSH/GPU/R/科学库依赖。
- suite_hash 覆盖规范 manifest 和全部 fixture 路径/内容，LF 归一化；随机时间/attempt UUID 不进稳定比对报告。
- “不附 raw prompt、model output、reasoning、arguments/outcome、绝对路径、host 信息、原始异常、凭据或整条 transcript。”失败原因白名单。
- “显式设置 OMICSOPS_EVAL_REPORT_PATH 时”保存报告，缺省不写文件；默认 workspace tests 包含 suite，失败/写入失败退出非零。只写被忽略 target 产物，不提交报告。
- core 替身不证明 SQLite/SSH/UI，保留对应独立验收。共享 CI 中本计划只拥有 V4 scenario step/upload，Python discovery/setup-python 由 helper/W04 计划维护。

## Review Focus

1. 乱序键/CRLF checkout、fixture 改变：同输入同 suite_hash，内容改变必须换 hash（Task 1）。
2. manifest 自称批准/任意 action 或重复 seed：拒绝，不把脚本当宿主授权（Task 1、2）。
3. Denied/uncertain/参数 preflight：proposal 不等于 dispatch，恢复不重派，preflight 不耗尽服务器（Task 2）。
4. 首场景失败/脚本耗尽/错误含秘密：后续仍执行并写完整安全报告（Task 2、3）。
5. 已有报告、只读目录、默认运行：Windows 原子替换或受控失败；默认零写文件（Task 3）。

---

### Task 1: manifest/parser/hash 和测试模块接缝

**Files:** Modify `crates/omicsops-agent-core/src/lib.rs:tests`、`crates/omicsops-agent-core/Cargo.toml`（仅 dev）；Create `crates/omicsops-agent-core/src/tests/scenario_evaluation.rs`、`crates/omicsops-agent-core/src/tests/data/v4-scenarios.json`。首次 manifest fixtures 全部内联；将来外部 fixture 必须计 hash。

**Interfaces:**
- Consumes：现有 `ScriptedModel/MemoryStore/ordinary_execution_spec/seed_execution` 私有 fixtures；public `ModelTurnV4/ToolCallV4/ToolOutcomeV4/AgentEventV4`。
- Produces：test-only `SuiteManifestV4 { schema_version: u8, suite_id: String, runner_contract_version: u32, scenarios: Vec<ScenarioManifestV4> }`；版本固定 1。
- Produces：`ScenarioManifestV4 { id: String, label: String, scenario_kind: ScenarioKindV4, objective: String, seed: ScenarioSeedV4, model_script: Vec<ModelTurnV4>, tool_script: Vec<ScriptedToolStepV4>, seed_events: Vec<ScenarioSeedEventV4>, limits: ScenarioLimitsV4, assertions: Vec<AssertionSpecV4> }`。`ScenarioSeedV4 { project_id: Uuid, conversation_id: Uuid, run_id: Uuid }`；`ScenarioLimitsV4 { context_max_bytes: Option<usize>, max_turns: Option<u32>, max_tool_calls: Option<u32>, checkpoint_recent_events: Option<usize> }`。
- Produces：`ScriptedToolStepV4 { stage: ToolScriptStageV4, call_id: String, tool_id: String, outcome: Option<ToolOutcomeV4>, failure: Option<ScriptFailureV4> }`；stage 为 Prepare/Execute/Recover，failure 为 ArgumentPreflight/BusinessFailure/UncertainDispatch，outcome/failure 恰一项。`ScenarioSeedEventV4` 为 SuccessfulTool/McpDirectory（typed outcome）、UncertainDispatch（typed call）、UserGuidance（message_id/markdown）封闭 variants，不接受原始任意 event/shell/文件 action。`AssertionSpecV4 { name: ScenarioAssertionV4, expected: Value }` 的白名单为 ValidEvidenceRef/RejectedForeignRef/DeniedNoDispatch/UncertainNoReplay/RecoveredOnce/PreflightPreservesServer/CorrectedMcpSuccess/DiscoveredOnce/FullBudgetAdmitted/SourceRetrievable/FrozenAuthorityRetained/ModelAttempts/ToolProposals/ToolDispatches/RecoveredOutcomes；parser 按 name 检查 expected 为 bool 或 u64，Task 1 即定义该 enum。
- Produces：`parse_suite(source: &str) -> Result<SuiteManifestV4, ManifestErrorCodeV4>`；`suite_hash(manifest: &SuiteManifestV4, fixtures: &BTreeMap<String, String>) -> Result<String, ManifestErrorCodeV4>`；错误仅固定 InvalidVersion/UnknownField/DuplicateId/InvalidSeed/InvalidScript/InvalidAssertion。

- [x] **RED 写测试**：`manifest_rejects_invalid_version_fields_ids_seeds_and_actions`、`manifest_hash_is_canonical_across_crlf_and_order`、`manifest_hash_changes_with_fixture_content`。定义标准五场景 manifest，未搭 runner 之前仅测解析/身份/哈希。

```rust
assert_eq!(suite.scenarios.len(), 5);
assert_eq!(hash_lf, hash_crlf);
assert_ne!(hash_before_fixture_change, hash_after_fixture_change);
assert_eq!(parse_suite(&duplicate_id).unwrap_err(), ManifestErrorCodeV4::DuplicateId);
```

- [x] **RED 运行**：`cargo test -p omicsops-agent-core scenario_evaluation::manifest`。预期 parser/hash/模块缺失或断言失败。
- [x] **实现接口**：嵌套模块 `super::*`，不移动整个既有测试模块；serde unknown fields 拒绝，逐项核验唯一 id/UUID/script/assertion 类型，canonical BTree key 排序与 LF 字符内容。只增加 workspace `sha2/hex` dev-dependencies。manifest 不自带信任或能力授权。
- [x] **GREEN**：重复 RED；现有 `cargo test -p omicsops-agent-core`。预期 hash 固定、旧 fixtures 不回归。
- [x] **Commit**：仅暂存本任务文件，`git commit -m "test(agent): define offline V4 scenario manifest"`。

### Task 2: 五场景 runner 和故障注入断言

**Files:** Modify `crates/omicsops-agent-core/src/tests/scenario_evaluation.rs`、`src/tests/data/v4-scenarios.json`；现有 lib.rs 私有 fixtures 仅在需要时增加可复用的最小访问方法。

**Interfaces:**
- Consumes：Task 1 manifest/hash；原 `AgentCoreV4::execute_with_limits(&self, spec, limits, cancelled)`、public ModelPort/ToolPort/EventStore/ScientificStateStore 及既有 evidence/model_replay/context views 行为。
- Produces：`async fn run_scenario(scenario: &ScenarioManifestV4) -> ScenarioOutcomeV4`、`async fn run_suite(manifest: &SuiteManifestV4, hash: &str) -> SuiteReportV4`。每场景独立 fixtures；typed scenario actions 控制答案/Denied/resume，不越过宿主 effect/capability 契约。
- Produces：`ScenarioOutcomeV4 { id: String, status: ScenarioStatusV4, model_attempts: u64, tool_proposals: u64, tool_dispatches: u64, recovered_outcomes: u64, assertions_passed: u64, assertions_failed: u64, failure_reason_code: Option<ScenarioFailureCodeV4> }`。SuiteReport 为 schema_version/suite_id/suite_hash/runner_contract_version/passed/failed/scenarios。
- Consumes：Task 1 `ScenarioAssertionV4` 全部白名单；Produces assertion evaluator 和失败码 AssertionFailed/ScriptExhausted/UnusedScript/InvalidFixture/CoreRejected。报告不接收 raw error 字符串。

- [x] **RED 写测试**：`scenarios_run_all_five_and_measure_actual_dispatch`、`scenarios_continue_after_failure_and_require_exact_script_consumption`、`scenario_fault_injections_break_expected_assertions`、`scenario_report_omits_sensitive_payloads`。

```rust
assert_eq!(report.scenarios.len(), 5);
assert_eq!(denied.tool_dispatches, 0);
assert_eq!(uncertain.tool_dispatches, 0);
assert_eq!(recovered.recovered_outcomes, 1);
assert_eq!(mcp_directory_discoveries, 1);
assert!(source_read_back_verified);
assert!(!serialized_report.contains(secret_sentinel));
```

- [x] **RED 运行**：`cargo test -p omicsops-agent-core scenario_evaluation::scenarios`；`cargo test -p omicsops-agent-core scenario_evaluation::scenario_fault`。预期 runner/计数/行为失败。
- [x] **实现接口**：
  - ReadEvidence：seed 真正已签名证据，从 sequence/hash 取回并验证，损坏/跨 scope 拒绝；无数据的计划不能作为完成证据。
  - DenyApproval：先 core 等待审批，再以正确 scope 写 Denied 并 resume；实际 execute 为 0，capability 不扩大。
  - RecoverUncertain：载入无确认的副作用 dispatch，零重派；独立已完成 receipt fixture recover/reuse 恰一次，未知仍 Attention。
  - CorrectMcpArguments：typed fake preflight 首次返回参数错误且零 dispatch；更正调用成功，core 对 preflight/business 分类与发现目录去重按现有行为断言。此项证明 core 对宿主结果的响应，真实 MCP 参数校验仍由 host tests 负责。
  - CompactPreserveSources：schema 大/context 小触发完整预算，checkpoint/source引用和 guidance/frozen boundary 可取回；不依赖 W05 未实现接口，也不宣称 MemoryStore 证明事务原子性。
  使用 Tokio 虚拟时间，无耗时阈值；场景返回 typed failure 而非提前 panic。错误注入仅在 test seam 修改观测/fixture，不改 production 代码。
- [x] **GREEN**：重复 RED；`cargo test -p omicsops-agent-core scenario_evaluation -- --nocapture`。预期五场景通过，故障注入测试确认对应断言失败后自身通过，剩余脚本必失败。
- [x] **Commit**：仅暂存本任务文件，`git commit -m "test(agent): run deterministic V4 behavior scenarios"`。

### Task 3: 安全报告、非零失败与 Windows CI

**Files:** Modify `crates/omicsops-agent-core/src/tests/scenario_evaluation.rs`、`crates/omicsops-agent-core/Cargo.toml`（atomic report helper 仅 dev）、`.github/workflows/ci.yml`（仅 V4 step/upload）；Create `docs/testing/v4-scenarios.md`。

**Interfaces:**
- Consumes：Task 2 SuiteReportV4。只读取 `OMICSOPS_EVAL_REPORT_PATH`，不得枚举环境/账户/秘密。
- Produces：`emit_report(report: &SuiteReportV4, path: Option<&Path>) -> Result<(), ReportWriteErrorCodeV4>`；test-only `scenario_evaluation_suite`，先 emit 完整报告，再断言 failed==0。stdout 固定前缀 `OMICSOPS_V4_SCENARIOS=`，JSON stable id 排序、无时间/耗时/随机 UUID。
- Produces：ReportWriteErrorCodeV4 为 InvalidPath/WriteFailed；使用 `tempfile` dev dependency 的同目录 NamedTempFile/persist 进行 Windows 原子替换，拒绝未授权默认写入和失败前删除原报告。只使用 Rust 文件 API，不拼 shell，不删除目录。

- [x] **RED 写测试**：`report_default_writes_no_file`、`report_atomic_replace_retains_previous_on_failure`、`report_failure_still_emits_all_scenarios`、`report_stability_and_redaction`。给显式临时路径，测试重复写、现有目录/无效父路径等确定性故障，不依赖 Windows ACL 或机器权限。

```rust
assert_eq!(report_bytes_run1, report_bytes_run2);
assert_eq!(files_without_explicit_path, before_files);
assert_eq!(old_bytes_after_failed_replace, old_bytes);
assert_eq!(failed_report.scenarios.len(), 5);
assert!(!failed_report_json.contains(raw_provider_error));
```

- [x] **RED 运行**：`cargo test -p omicsops-agent-core scenario_evaluation::report`；预期报告接口/默认零写入/安全投影失败。
- [x] **实现接口**：写报告失败返回固定错误，外层测试 fail 而不打印 raw异常；suite 失败先保存全部安全结果再非零。CI 增定向 cargo command，step env 仅 `OMICSOPS_EVAL_REPORT_PATH: target/evaluation/v4-scenarios.json`；随后 `actions/upload-artifact@v4`，`if: always()`，固定 name `v4-scenarios` 与上述 path。不改 Python setup/discovery，不替换 npm ci/workspace/npm/build。
- [x] **GREEN**：重复 RED；正常定向 suite 通过。使用已有 test seam 的失败 SuiteReport 和 emission 测试断言 fail condition；额外一次性复制测试 manifest 的明确故障验证定向 cargo 非零且完整报告存在，恢复夹具后确认通过，禁止提交故障输入。默认 `cargo test --workspace`、`npm test`、`npm run build`、fmt/diff check；记录真实验收未执行。报告路径仅 target，不进 Git。
- [x] **Commit**：仅暂存本任务源码/CI/文档，`git commit -m "test(ci): publish safe V4 scenario reports"`。Cargo.lock 若发生 Rust dev 依赖变化说明原因；npm lock 若变化必须 npm ci，正常本任务不需改变 npm 依赖。

## Plan Self-Review

五场景、manifest/hash、脚本严格消费、故障注入、计数和安全报告/CI 分别绑定 Task 1–3；Review Focus 全有具名测试。无新增 CLI/product API/DTO/DB，report/upload 与 helper plan 的文件责任已协调。没有完整框架/实现 body，所有片段是最小断言；三个任务RED/GREEN/commit与独立审阅均已执行；逐项证据见下方记录。

2026-10-11：Task1 `c1f348e..a500df7` 独立完整审阅 Spec compliant / Approved；manifest 3、core 226 通过，fmt/diff通过。canonical LF/key/ID、完整payload和fixture hash、固定错误码均已覆盖；既有sha2复用，仅增加现有hex dev锁边。shared working source测试不替代最终clean-HEAD；Windows路径/cache警告留最终验证。累计33/50 SOURCE任务审阅完成，runner/report与真实模型/SSH/GUI仍未由此证明。

2026-10-12：Task2 `6f05b71..9711ef6` 独立完整审阅 Spec compliant / Approved；五场景真实 core 控制行为、直接调用计数、严格脚本消费、安全稳定报告类型和故障断言已实现。scenario 9/core 232通过，0 ignored，fmt/diff通过；初始 fmt 权限失败及通过的审批重试原始记录保留。固定 fixture 身份耦合和 Cargo/cache warnings 作为 Minor 留最终审阅；7个外部文件原始字节不变。累计34/50 SOURCE，报告写入/非零/CI仍待Task3，clean-HEAD与真实模型/SSH/GUI不由此证明。

2026-10-12：Task3 `97265dc..1081b28` 独立完整审阅 Spec compliant / Approved。评审最终消息发生capacity错误，但完整双verdict及检查证据已先保存，读取后恢复交付，未重复派发审阅。默认零写入、target限定原子报告、完整结果先emit后失败与Windows CI完成；真实注入故障cargo101/五项JSON(4通过1失败)、夹具RAW恢复后五项通过，写入失败cargo101保留完整安全stdout。缺target根修订前workspace1791通过/18ignored；修订后core238/scenario15/Python48/frontend1316+bridge22/Web/fmt/diff通过，最后仅测试断言追加单独1/1。首次受限权限失败与通过重跑、Cargo/cache/dead-code/Vite warnings保留。W08两部分共6任务源码完成，累计35/50；最终fresh workspace/clean-HEAD/native、CI与真实模型/SSH/R验收未由这些结果替代。
