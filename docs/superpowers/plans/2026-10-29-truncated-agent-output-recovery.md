# Truncated Agent Output Recovery Implementation Plan

**Goal:** 默认工具请求在输出截断后获得一次安全的精简完整重生成，保持用户的续写选择和宿主执行边界。

**Architecture:** 复用 core 已有 output_repair_attempted 单次槽位；工具截断与 JSON/未知工具共享。Session 只解释额外续写与修复的区别，DTO/默认设置不变。

**Spec:** `docs/superpowers/specs/2026-10-29-truncated-agent-output-recovery.md`

- [x] Core RED：默认工具调用截断、混合格式错误和重复失败；明确与运行级可选 continuation 的关系。
- [x] Core GREEN：仅工具开启且持久化正文的 Agent 请求修复一次，保留原上下文/replay，重新准入，不派发残缺调用。
- [x] 更新 Session 中英文说明；验证旧设置的默认值、保存和开关行为。
- [x] 回归取消/指导、实际请求预算、native replay、纯工具外摘要和 Reviewer 边界。
- [x] 独立复核；执行完整 Web、Rust、前端、格式检查及 Windows 打包/隐私检查。
- [x] 记录实际结果，按功能提交；不修改用户历史、不推送或更新 GitHub Release。
