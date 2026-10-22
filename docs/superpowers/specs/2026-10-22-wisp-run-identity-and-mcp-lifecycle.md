# Run 科学身份隔离与 MCP 会话生命周期

## 范围与参照

本轮继续对照 Wisp Science，限定为两项可独立验证的修复：科学操作的跨 Run 身份隔离，以及 MCP 配置变化时的会话锁生命周期。不重做 W01–W12、近期搜索或有界 `project.read`，也不将下述后续项计为已完成。

上游参照固定为 `4f2957a309377bd605f9509b87361b6e10617575`：

- [wisp-core/provenance.rs:100–305](https://github.com/xuzhougeng/wisp-science/blob/4f2957a309377bd605f9509b87361b6e10617575/crates/wisp-core/src/provenance.rs#L100-L305) 按会话作用域区分并行来源。本轮借鉴来源身份应包含运行作用域的原则，不照搬基于文件修改时间的方案。
- [wisp-mcp/connection.rs:92–110](https://github.com/xuzhougeng/wisp-science/blob/4f2957a309377bd605f9509b87361b6e10617575/crates/wisp-mcp/src/connection.rs#L92-L110) 在短锁内取得会话快照，释放锁后执行 shutdown。本轮借鉴这一异步生命周期边界。

## 科学操作身份

问题：项目科学状态跨 Run 持续存在，仅以 `call_id` 查找操作或证据，会把不同 Run 中复用的工具调用标识误认为同一次操作。

目标行为：

- 科学操作身份使用 `(run_id, call_id)`；分析的 before、finish、fail 和 recovery 查询均限定来源 Run。
- 同 Run 的重复处理维持原有幂等行为。不同 Run 的同名调用建立各自的分析和证据，不修改另一 Run 的状态。
- 宿主找不到当前 Run 的分析时返回无更新；同名的其他 Run 分析不会被选中，也不进入其初始化回执检查。领域完成/失败接口仍对未知作用域返回错误，且不修改状态。
- `EvidenceRecordV4` 增加 `run_id: Option<Uuid>`，使用 serde default 与 skip-none；新证据写入 `Some(run_id)`，旧证据保持 `None`。
- 旧无 Run 身份的证据继续可读、可核验，但不能抑制新 Run 的证据记录。不得根据同名调用猜测并回填旧来源。

这是科学状态内部身份与兼容序列化的调整，不新增 SQLite 表。缺省字段不序列化，使旧 JSON 和既有 digest 保持原样；须以固定旧格式 fixture 验证，而非仅凭 serde 属性推断兼容。
不回写历史来源，也不保证旧版本重写带新字段的记录后仍保有相同哈希。
来源导航界面暂不映射这一可选字段；本轮保障科学状态归属，不新增证据到 Run 的 UI 跳转。

## MCP 会话锁

问题：`session_for` 从 `sessions` 取得既有会话时，锁守卫可能跨越后续异步检查；配置或凭据 fingerprint 变化进入失效路径后，再次获取同一锁会自锁。

目标行为：在明确的短作用域内克隆会话 `Arc` 并释放 `sessions` 锁，再 await 检查、失效或重连。配置或凭据变化能完成失效与重新连接，未变化的会话仍复用。

本轮只缩短持锁范围，不改变同时连接的竞态策略、fingerprint 定义、MCP 授权、发现策略或工具执行语义。

## 安全与兼容边界

- API key、SSH 密码和私钥继续只经过现有 keyring/Windows Credential Manager 路径；新增 Run 身份和会话处理不将凭据写入 SQLite、事件、日志或模型上下文。
- 不扩大工具权限，不绕过宿主审批，不改写冻结计划或批准计划；科学操作隔离不等于授予新执行能力。
- 不改变本地/SSH 计算、远端作业重连或取消语义，也不新增自动重派。科学摘要仍不能替代原始工具证据。
- 本轮不声称解决 MCP stderr 的既有脱敏边界；该问题单独列于后续项。

## 验证计划与实际记录

自动化使用纯状态 fixture、临时 SQLite 和 fake MCP；不以网络、真实 MCP 服务、模型凭据或 SSH 主机为前提。

| 检查 | 应验证的行为 | 实际记录 |
| --- | --- | --- |
| 科学身份回归 | 两个 Run 复用 call ID，before/finish/fail/recovery 仅更新各自记录 | `cargo test -p omicsops-desktop --lib scientific_failure_contract_tests`：15 通过，0 ignored |
| 科学幂等与旧数据 | 同 Run 重复不重复落证据；旧 None 证据不抑制新 Run；旧 JSON/digest 不变 | `cargo test -p omicsops-science`：11 通过，0 ignored |
| SQLite 恢复 | 保存并重开后 Run 身份和证据关联仍保持隔离 | 上述宿主测试的 `scientific_snapshot_recovery_preserves_other_runs_with_the_same_call_id` 通过 |
| MCP 生命周期 | fingerprint 未变复用；配置或凭据变化在有界等待内完成失效/重连，无自锁 | `cargo test -p omicsops-mcp --features test-fixtures`：4 单元测试、7 集成测试通过 |
| Web 与前端 | `npm run build`、`npm test` | 构建通过；129 个测试文件、1518 项前端测试及 22 项扩展测试通过 |
| Rust workspace | `cargo test --workspace` | 2041 通过，0 失败，21 ignored；ignored 不计作真实验收 |
| 桌面组合检查 | `npm run build:desktop` | Windows Release 编译与 x64 NSIS 打包通过；未安装或分发 |
| 格式与差异检查 | `cargo fmt --all -- --check`、`git diff --check` | 通过 |

所有通过项目退出码为 0。Rust workspace 测试沿用已确认需要的临时目录和本机回环权限，仅为该测试进程将 `127.0.0.1,localhost,::1` 加入 `NO_PROXY`；不改变应用或持久化代理设置。桌面构建沿用已审查的 NSIS 构建权限。构建仍有现有 Web chunk 体积、Windows 链接器输出与增量缓存硬链接回退提示。

新增测试先在旧实现上复现：science 的两项跨 Run 完成/失败测试错误选中另一分析；宿主的三项 `scientific_failure_contract_tests::scientific_` 测试分别复现新 Run 被抑制、未知 Run 完成他人分析、证据跨 Run 去重。MCP 的 `environment_rotation_reconnects_only_the_matching_project_session` 在 5 秒预算后超时。修复后再验证同一回归，不使用真实凭据或服务器。

独立只读代码审查未发现阻塞问题；按建议更新了证据来源导航的过时注释，并明确其 UI 跳转未纳入本轮。MCP 并发配置替换的既有竞态没有在本轮增加保证；新增测试证明顺序配置轮换与项目隔离。

手工 smoke 尚未执行：同项目开启两个 Run 并检查来源关联；桌面连接受控 MCP 后修改配置或凭据，再次发现/调用并确认可完成重连。真实模型、SSH、R、Micromamba、PBMC 验收均未执行，仍须按 `acceptance/README.md` 独立开展。

## 已发现、未纳入本轮的独立后续

以下优先级用于后续排序，不代表这些问题已有修复或当前生产端到端复现。

| 优先级 | 已核实边界 | 后续最小范围 |
| --- | --- | --- |
| P1 | 自动压缩先独立写 archive，再追加 Archived/Checkpointed，最终准入失败仍改变持久 checkpoint；现有注入测试明确断言失败后增加两条事件和一份 archive | 固定最终事件身份后纯准入，再原子提交；独立设计运行中/批准计划边界和 SQLite 逐点回滚测试 |
| P1 | 同 Run 重复 call ID 会命中既有结果缓存；本轮仅处理跨 Run 科学身份 | 单独明确调用身份冲突与幂等复用条件，并覆盖参数、工具或调用来源不一致的回归 |
| P1 | MCP stderr 的持久化脱敏仍有缺口 | 独立梳理 stderr 到持久化、日志和公开错误的路径，以合成敏感标记验证脱敏 |
| P2 | 原生 replay 从完整事件链累积，不随 checkpoint 缩小；只缩小 JSON context 无法消除 replay 自身的预算占用 | 设计保留完整 call/result 配对与 provider continuation 的 replay 压缩边界，不直接截断历史 |
| P2 | MCP 实际目录搜索的分词仍有缺口 | 用真实目录搜索入口的确定性 fixture 明确分词行为，再做局部修复 |

自动压缩的具体证据在 `omicsops-agent-core/src/lib.rs` 的 `context_for_internal`、`automatic_full_context_postarchive_refusal_returns_error`，以及 Store 的 `archive_agent_context_v4`。原始事件和工具证据没有被删除；缺口是失败仍改变恢复投影及可能留下部分写入，不能表述为原始科研证据已丢失。W05 计划已明确保留这一非原子边界。

原生 replay 结论来自 `model_replay.rs` 的 `project_replay` 与 `shape_request` 源码；当前未进行真实 provider 长对话复现，也未测量生产性能。两项上下文后续需要独立设计与测试，不混入本轮身份和锁生命周期修复。
