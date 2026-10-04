# Agent 后续输入精简与验证

## 提交与范围

上一轮工具往返优化先提交为 `5724987`，再按功能分别交付本轮三项改动：

- [审查证据正文去重](2026-10-04-reviewer-evidence-dedup.md)：重复引用按类型和
  精确标识展开一次，条件关系仍在 proposal 中；提交 `f630c49`。
- [MCP 结果正文去重](2026-10-04-mcp-result-dedup.md)：成功的逐字重复正文只在
  完整 data.result 中提供一次，Host 审计元数据保留；提交 `61e2f46`。
- [压缩 checkpoint 与原生 replay 去重](2026-10-04-checkpoint-replay-dedup.md)：
  只移除已在经过验证的原生 replay 中表达的精确签名投影；保守保留旧格式、
  不匹配内容和活动错误，持久化 checkpoint 不变。

这三项都不增加模型调用、不修改模型推理强度或自动审查设置、不新增通用缓存。
审批、科学状态、签名来源、计算绑定、远端运行及恢复边界保持。
原有 tools 的路径说明、agent_v4.rs 文件系统修复、adaptive spec 和 website
工作区修改未纳入本轮提交。

## Windows 实际验证（2026-10-04）

- `cargo test -p omicsops-agent-core reviewer_materialization -- --nocapture`：
  修复前共享来源用例因 8 个重复记录而失败；修复后 2 项通过，展开 3 个独特
  来源。同 UUID 的产物和证据仍分别提供，六个缺失引用仍全部被门禁拒绝。
- `cargo test -p omicsops-agent-core --lib mcp_result_projection -- --nocapture`：
  修复前成功结果仍有重复正文；修复后 4 项通过，失败、摘要、wrapper 预算、
  元数据和签名分页恢复都保留。fixture 投影 5826 → 3148 字节。
- `cargo test -p omicsops-agent-core model_replay::tests -- --nocapture`：
  原 checkpoint 重复结果回归先失败；修复后 6 项通过。
  fixture 上下文 6141 → 1498 字节，未覆盖的用户输入、委派和错误仍保留。
- `cargo test --workspace`：1503 项通过，16 项 ignored，核心 crate 183 项通过。
- `npm test`：992 项前端测试、22 项浏览器扩展测试通过。
- `npm run build`：通过；保留既有 Vite 大于 500 kB 的 chunk 提示。
- `npm run build:desktop`：通过，完成本地 Windows release 与 NSIS 构建，
  未安装或分发。linker 建库信息为 warning，未导致构建失败。
- `cargo fmt --all -- --check`、`git diff --check`：通过。没有纯格式文件修改。
- 独立代码审查：三项都未发现必须修复的问题。

## 实测限制与 smoke

上述数字是确定性样例的请求输入大小与记录数量，不是生产模型响应时间。
未执行真实 OpenCode、订阅模型、SSH/PBMC 或 Windows GUI smoke，原因是尚未
配置 acceptance/README.md 要求的一次性 profile 与验收环境；ignored 不算通过。

真实耗时对照须使用同一任务、模型、推理强度和自动审查配置，记录脱敏的
模型输入字节、执行/审查请求数量、工具数量、重试、推理 tokens 和总耗时。
分别验证小型 MCP 结果直接用于回答、大结果完整分页、共享来源覆盖多个条件，
以及原生 provider 压缩后继续/重开仍能引用原始证据。不要记录私有推理或凭据。
