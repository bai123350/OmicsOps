# Reviewer 工具参数 JSON 失败后的完整重生成

2026-10-21 补充：审核报告字段缺失和校验失败也纳入同一次格式再生额度，
详见 `2026-10-21-reviewer-schema-recovery.md`，不自动补字段或跳过审核。

2026-10-20 补充：明确的输出截断现与畸形 JSON 共享一次完整重生成额度，
详见 `2026-10-20-reviewer-truncated-output-recovery.md`。以下保留当时的故障与验证记录。

## 故障证据

用户的 OpenCode Go `deepseek-v4.1-flash` 截图报告工具参数 JSON 在第 1 行、
第 3134 列缺少分隔符。对本机数据库的只读、白名单元数据检查确认：运行已
提交 `agent.complete`，随后记录 `CompletionProposalSubmitted` 和
`DeterministicVerificationFinished`，最后直接 `RunFailed`，未记录纠错重试。
未读取 keyring 凭据，也未复制原研究内容或畸形参数到日志或仓库。

`DesktopModelPortV4::review` 直接通过 `stream` 解析 `agent.submit_review`，
绕过普通模型轮次的 JSON 纠错分支。严格参数解析将畸形 JSON 归为永久
`InvalidResponse`，而 `AgentCoreV4::review_with_retry` 原先只处理 transient
错误。因此审核报告的格式错误会使已经执行并通过确定性验证的任务失败。

## 行为和边界

- 每次独立审核对明确的 `returned malformed JSON arguments:` 错误有一次
  完整重新生成机会；额度与原有 transport 重试分别计数。第二次畸形输出停止。
- 原样保留冻结的审核请求，包括目标、完成提案、确定性报告、科学状态和证据。
  只重新请求只读审核报告，不重新执行研究工具或接受失败响应中的部分内容。
- 用现有 `ModelRetrying` 事件记录恢复，不存储畸形参数。严格 JSON、报告验证、
  科学错误 finding 的修正门槛以及取消检查保持生效。
- 认证失败、其他永久响应错误和有效审核的科学错误不属于 JSON 格式恢复。
  无数据库迁移、凭据变更或新增授权；审核提示和证据仍发送给已选模型服务。

## 验证

确定性回归覆盖：首次畸形后恢复、冻结请求相等、重复畸形仅请求两次、保留
有效报告中的科学错误、认证/其他永久错误不重试、取消时不重新派发，以及
持久化完成提案恢复时不重派发执行工具。

增加独立 ignored OpenCode nonce 验收，开启 `auto_review` 并要求
`ReviewerFinished` 与 `RunCompleted`。既有关闭自动审核的 nonce 用例保持
原义。真实验收只使用指定已保存 profile、只读源数据库、keyring 和一次性
项目；无法保证真实模型恰好产生畸形 JSON，该分支由确定性测试验证。

手工 Windows smoke：重启包含修复的应用，在一次性项目使用 OpenCode 模型
开启自动审核，完成一次读文件任务；格式错误时应出现一次审核重生成事件，
随后产生有效审核或明确失败。原生 UI、SSH、macOS 与完整文献科研验收的
执行状态应单独记录，构建或 nonce 验收不替代这些流程。

## 实际验证（2026-10-06，Windows）

- `cargo test -p omicsops-agent-core reviewer_malformed_arguments -- --nocapture`：
  先观察到 5 个测试因缺失重生成失败，再实现修复；同命令 5/5 通过。
- `cargo test -p omicsops-agent-core`：196 通过、0 失败。
- `cargo test --workspace`：1519 通过、0 失败、17 ignored，退出 0。
- `npm test`：992 个前端测试和 22 个扩展测试通过，退出 0。
- `npm run build`：通过，退出 0；保留已有大 chunk 提示。
- `cargo fmt --all -- --check`：发现新增测试格式偏差后执行
  `cargo fmt --all`，复查通过。无其他文件的纯格式变更。
- 独立代码审查和相关文件 `git diff --check`：通过。
- `npm run build:desktop`：通过，退出 0；release 编译耗时 4 分 38 秒，生成
  `target/release/omicsops-desktop.exe`。仅生成本机验证构建，未发布或安装。

沙箱内首次前端测试、Web 构建和格式检查被 Windows 路径访问错误
（EPERM / canonicalize 拒绝访问）阻止；在获准的正常用户权限下上述检查通过。

真实 Reviewer 验收命令为：

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reviews_file_and_returns_nonce -- --ignored --exact --nocapture
```

**未执行**：自动审批拒绝此命令，理由是使用已有凭据向外部 OpenCode 服务
发送一次性项目的测试文件内容，需要用户明确授权这项数据传输。拒绝发生在
进程启动前，没有读取 keyring 或发出模型请求，不算模型验收失败。
原生 GUI、SSH、macOS 与原文献科研工作流验收也未执行。
