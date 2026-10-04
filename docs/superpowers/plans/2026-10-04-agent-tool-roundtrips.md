# Agent Tool Roundtrips Implementation Plan

**Goal:** 减少完整结果和小型 MCP schema 的不必要回读，保留证据和审批。
**Architecture:** 复用模型投影、目录分页和现有批量执行；不新增缓存或持久化模式。
**Spec:** ../specs/2026-10-04-agent-tool-roundtrips.md

## 执行步骤

- [x] 在 core 的结果投影测试中先复现完整 data 仍生成回读引用。
- [x] 在 context_views 目录测试中先复现小型精确 schema 不能直接获取。
- [x] 修改 context_views::event_view 和 mcp_directory_text；保留大结果引用及分页。
- [x] 更新 execution_request 工具指导，允许使用内联 schema，鼓励批量独立调用。
- [x] 运行定向和默认完整检查，独立审查边界与回归；记录实际验证及未执行验收。

已有 agent_v4.rs、tools/lib.rs、adaptive spec 和 website 工作区修改不属于本次
交付，保留原状。当前授权包含可逆的修复和检查；无发布、提交或真实账户操作。

## 执行记录

修复前的 successful_duplicate_result_projection、mcp_query_finds_full_description
和 selective_mcp_workflow 三个定向测试分别按预期失败：多余引用、缺少完整
内联 schema、无法在省去 schema 回读的流程中获取参数契约。
修复后完整核心回归通过，并追加了超大匹配 schema 的分页恢复回归。

独立审查未发现必须修复的问题；确认原始 schema、签名事件、同 run/hash
读取与宿主审批保持。未扩大通用只读缓存，因为文件和运行状态可能变化。
完整验证结果见 spec 中的实际验证记录。
