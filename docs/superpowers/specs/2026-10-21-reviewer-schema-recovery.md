# 自动审核报告缺少必填字段时的恢复

## 故障与边界

OpenCode Go Chat `deepseek-v4.1-flash` 的实际运行记录显示，主模型提交的
`agent.complete` 含有 `schema_version`，随后完成提案及确定性验证均已落库，
确定性验证通过。自动审核阶段返回 `missing field schema_version`，宿主直接
把运行记为失败。主模型与审核模型使用同一已保存配置。调查只读取数据库中
必要的事件类型、字段名称及配置元数据，没有复制工具代码、科研内容或凭据。

审核工具的 JSON schema 原本已要求 `schema_version: 4`。问题是模型生成
合法 JSON 却遗漏字段时，`ReviewerReportV4` 反序列化错误没有被识别为可
重新生成的审核格式错误；现有恢复仅覆盖畸形 JSON 和输出截断。

## 修复

- 桌面审核入口明确完整报告形状，并用固定 `reviewer_report_schema:` 前缀
  分类反序列化及报告校验失败。诊断不回显模型正文，凭据存储方式不变。
- Core 将此类错误纳入已有的一次完整报告再生额度，与畸形 JSON、截断共享，
  不另加重试循环。原冻结请求、证据和运输重试限制保持不变。
- 不自动补版本、不接受部分报告、不关闭自动审核、不把科学错误 finding 改为
  成功。第二次格式错误继续失败，未知工具、未调用审核工具及认证错误不因此
  获得新的重试权限。
- 再生只重新请求独立只读审核，不重新派发已执行工具、不改写旧运行或审批。
  已有 `ModelRetrying` 事件记录这次恢复，无数据库迁移或新增执行授权。

## 验证范围

确定性测试覆盖缺少版本、错误版本、缺少字段、无效报告、不泄漏正文、一次
恢复后成功、第二次失败、三种格式错误共享额度、取消和永久错误边界，以及
自动审核开启时执行工具恰好一次并完成的完整 Agent 链路。

真实验收复用 `acceptance/README.md` 的 reviewer-enabled nonce 用例：只读
当前选定的 Go 配置，凭据由现有 keyring 读取，使用内存数据库、随机身份和
一次性本地目录，仅允许读测试文件。它验证当前模型的工具—审核—完成链路，
不能替代原 SSH 环境检查、原生界面或完整科研流程验收，也不能保证真实模型
恰好触发字段缺失；错误注入与恢复上限由确定性测试验证。

## 2026-10-21 验证结果

- `cargo test -p omicsops-agent-core reviewer_ --lib -- --nocapture`：22 项通过。
- 桌面审核 schema 回归：2 项通过，涵盖六类无效报告及合法报告。
- `npm run build`：通过；保留现有 Vite 大 chunk 提示。
- `cargo test --workspace`：2009 项通过，20 项 ignored，0 项失败。
- `npm test`：前端 1511 项及浏览器桥接 22 项通过。
- `cargo fmt --all -- --check` 与 `git diff --check`：通过。
- `npm run build:desktop`：通过，生成 Windows 可执行文件及 NSIS 安装包；
  未执行安装、发布或原生界面验收。
- 按验收文档设置当前选定配置的 `OMICSOPS_LIVE_MODEL_PROFILE_ID` 后执行
  `cargo test --workspace agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reviews_file_and_returns_nonce -- --ignored --exact --nocapture`：
  1 项通过，耗时 21.34 秒；实际模型为 OpenCode Go `deepseek-v4.1-flash`。
  此用例开启自动审核，断言成功读取一次临时文件、最终答案含随机 nonce，且
  `ReviewerFinished` 先于 `RunCompleted`。原 SSH 环境及原生界面未做真实验收。
