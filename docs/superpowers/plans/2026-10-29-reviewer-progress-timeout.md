# Reviewer Progress Timeout Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development for the desktop progress bridge while the primary worker implements core supervision.

**Goal:** 多轮独立评审持续生成时不再被旧 180 秒整轮时限中断，静默、硬上限与停止边界保持可验证。

**Architecture:** ModelPortV4 增加向后兼容的进度方法；桌面端转发私有评审流，core 监督有效进度、取消及跨重试共享硬截止时间。沿用已有 V4 的 900 秒总上限，默认 180 秒 idle，不增加读取轮数或改变科研审核语义。

**Tech Stack:** Rust、async-trait、Tokio watch/select 与虚拟时间；模拟模型和本地构建。

**Spec:** `docs/superpowers/specs/2026-10-29-reviewer-progress-timeout-design.md`

## Constraints and review focus

- 不记录评审原文/推理、真实凭据或研究 payload；不执行真实 API/SSH。
- 格式重生成与传输重试共享整次 900 秒上限，重试噪声不延长 idle。
- Stop 在评审和退避期间及时生效，不误写完成记录、不重跑执行工具。
- 冻结的 reviewer profile、来源/预算/报告校验及现有轮数限制不变。
- 非流式旧端口维持兼容；持续内容也不能跨越绝对硬上限。

## Tasks

- [x] Core：加入兼容方法和失败回归，先复现多轮累计超时，再实现进度监督、共享硬上限与取消。
- [x] Desktop：模拟流式传输验证回调先失败，再转发冻结 reviewer 选择和内部事件；保留旧 review 入口。
- [x] 验证无进度、空内容/usage/retry、硬截止时间、跨重试、取消及错误报告边界。
- [x] 独立审查；运行 `npm run build`、`cargo test --workspace`、`npm test`、格式检查及 Windows 桌面打包/隐私检查。
- [x] 记录真实执行结果，按功能提交，不推送或修改 GitHub Release。
