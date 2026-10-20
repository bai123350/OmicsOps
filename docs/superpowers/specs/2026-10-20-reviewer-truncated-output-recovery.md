# 独立审核输出截断后的完整重生成

## 故障证据

用户截图显示 `glm-5.3-flash` 在完成多个工具步骤后报
`truncated_output: provider exhausted its output allowance; partial tool calls cannot execute`。
只读检查本机运行记录的事件种类、顺序、用量和大小元数据确认：

- 主 Agent 已成功执行 7 个工具步骤并提交完成提案。
- `CompletionProposalSubmitted` 后的 `DeterministicVerificationFinished` 为通过；
  随后直接出现 `RunFailed`，没有 `ReviewerFinished` 或 `ModelRetrying`。
- 主 Agent 最后一次成功响应的输出为 3,659 tokens。失败的 Reviewer 响应
  未记录用量，不能据此声称测得了它实际消耗的 token 数。
- 当前配置计算出的每次输出预留为 16,384 tokens。错误来自输出截断，
  没有证据表明它是认证失败或输入上下文溢出。

诊断没有读取或输出 API key，没有将用户研究正文、工具参数或证据复制到仓库。

`DesktopModelPortV4::review` 通过严格的 provider 解析器生成
`agent.submit_review`。输出上限终止仍必须拒绝整份响应，即使部分工具参数
看起来完整。该错误被归为永久 `InvalidResponse`。
`AgentCoreV4::review_with_retry` 已为畸形 JSON 提供一次重生成，但漏掉了
明确的 `truncated_output:`，因此审核失败直接终止整个运行。
普通 Agent 的截断恢复不经过这条审核路径。

## 修复范围

- 将既有的审核输出重生成额度同时用于畸形 JSON 和明确的输出截断；
  两种错误共享一次额度，不能交替出现来无限重试。
- 重新请求完整的只读审核报告，原样复用冻结目标、完成提案、确定性验证、
  科学状态与原始证据；不拼接部分 JSON，也不重新派发研究工具。
- 保持 transport 重试额度独立。认证、其他永久响应错误、有效审核中的
  科学错误及取消检查仍按原有语义处理。
- 通过既有 `ModelRetrying` 事件记录不含失败参数的恢复原因。
- 保持模型配置、输出预算和冻结哈希不变，不推断模型家族能力，不降级或绕过
  审核。连续失效仍明确失败，不能把未完成审核的研究任务标为成功。
- 既有完成提案和确定性验证检查点保留。core 的
  `resume_pending_review` 路径保持不变，科学状态摘要检查继续生效。
  桌面恢复入口仍拒绝 `failed` 终态；本次修复不会重新打开用户已失败的运行。
  它在后续活跃运行的审核阶段自动恢复，旧失败记录及其产物保留。

无数据库迁移、凭据更改或新增工具授权。重新审核仍会将同一冻结审核输入发送给
已选择的模型服务。此次变化补充了
`2026-10-06-reviewer-json-regeneration.md` 的错误分类覆盖。

## 验证要求

确定性回归覆盖首次截断后完整恢复、冻结请求相等、连续截断以及截断/畸形混合
共享一次额度、transport 额度不被占用、取消优先，以及在内存事件存储中注入
完成检查点与 `RunFailed` 后，core 审核恢复路径零执行工具派发。最后一项是
core 隔离测试，不代表桌面入口允许恢复 `failed` 运行，也不是数据库重启验收。
严格 provider 终止检查继续拒绝部分工具调用。Chat、Messages 和 Ollama 的
解析 fixture 同时覆盖带完整合法工具参数的截断响应，以及分块接收到完整或
不完整参数后再收到截断终止帧；终止帧和后续 `finish` 均必须失败。

交付前执行 `npm run build`、`cargo test --workspace`、`npm test`、
`cargo fmt --all -- --check`、`git diff --check` 和本地桌面构建。
真实模型验收遵循 `acceptance/README.md`，使用现有 ignored Reviewer nonce
fixture、只读 profile 选择、keyring 和一次性项目；它不接触原研究项目，
也不能保证真实服务恰好产生截断，截断分支由确定性测试验证。

Windows 手工 smoke：运行包含修复的桌面程序，开启自动审核；若审核输出截断，
执行记录应显示一次只读报告重生成。成功后出现 `ReviewerFinished` 与
`RunCompleted`；再次截断仍失败，已有产物和提案保留。已经失败的终态记录保持
失败，须在新版程序启动后续运行验证。原文献流程、原生 UI、
SSH、macOS 和完整科研验收的执行状态须单独记录。

## 实际验证

2026-10-20，Windows：

- RED：`cargo test -p omicsops-agent-core reviewer_truncated_output -- --nocapture`
  在生产修改前为 0 通过、5 失败，均复现截断直接失败而未重生成。
- GREEN：`cargo test -p omicsops-agent-core reviewer_ -- --nocapture`
  为 17/17 通过；`cargo test -p omicsops-agent-core` 为 243/243 通过。
- `npm run build` 通过，保留已有大 chunk 提示。
- `cargo test --workspace` 通过：1,947 passed、0 failed、19 ignored。
- `npm test` 通过：1,441 个前端测试和 22 个扩展测试。
- 独立审查后补强的 provider fixture 单独运行
  `cargo test -p omicsops-adapters --test model_provider_contracts truncated_ -- --nocapture`，
  2/2 通过，其中新增流式测试含六组协议/参数组合；它不计入上面的全量测试数量。
- `cargo fmt --all -- --check` 与 `git diff --check` 通过。

真实 Reviewer nonce 验收已执行并通过：

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reviews_file_and_returns_nonce -- --ignored --exact --nocapture
```

显式选择当前保存的 `glm-5.3-flash` profile，保留其模型与推理设置，使用现有
Windows keyring 凭据；源数据库只读，一次性项目和内存 Store 中仅包含随机
nonce。测试目标编译耗时 2 分 28 秒，实际测试 51.10 秒，1 passed、0 failed、
0 ignored，进程退出 0。生产路径读取文件恰好一次，完成提案返回正确 nonce，
并在 `RunCompleted` 前记录 `ReviewerFinished`。该测试不保证真实服务产生
截断；截断恢复由上述 RED/GREEN 和解析 fixture 验证。

`npm run build:desktop` 通过，退出 0；release 编译耗时 5 分 05 秒，生成
`target/release/omicsops-desktop.exe` 并完成本地 NSIS bundle。保留已有 Vite
大 chunk 提示及 Windows linker 输出提示，均未导致构建失败。未安装、发布或
分发安装包，运行中的旧程序不会自动切换到新代码。

完整文献工作流、原生 UI、SSH 与 macOS 验收未执行；未修改、重开或重跑原研究运行。

2026-10-21 按功能提交前，在包含新增 provider fixture 的最终源码上重新检查：
`cargo test --workspace` 为 1,948 passed、0 failed、19 ignored；
`npm test` 为 1,441 个前端测试和 22 个扩展测试通过；
`cargo fmt --all -- --check` 与 `git diff --check` 均通过。
