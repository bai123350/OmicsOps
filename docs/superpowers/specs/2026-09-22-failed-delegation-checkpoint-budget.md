# 失败委派的检查点错误预算

日期：2026-09-22。范围：修复失败工具结果绕过检查点短视图，以及旧检查点已经
清空近期步骤后无法重新生成短视图的问题。沿用现有检查点、事件归档和结果分页
能力，不新增持久化模式、工具、模型配置或审批流程。

## 根因和观测

一次与界面报错匹配的本地运行快照中，模型上下文为 713416 UTF-8 字节，超过
宿主的 262144 字节预算。只记录体积与结构，不保存该运行的标识或科研内容：

| 项目 | 观测 |
| --- | --- |
| 完整上下文 | 713416 字节 |
| 序列化的 `unresolved_errors` | 508280 字节 |
| 单条检查点错误字符串（含 `agent.delegate: ` 前缀和原始图结果） | 504205 字节 |
| 检查点 `recent_steps` | 已为空 |
| 科学状态 | 100893 字节，同时出现在检查点与外层上下文 |

这些数值是同一快照的不同嵌套层级，不能相加解释总量。失败委派的图结果包含
子任务已收集的工具证据；旧检查点构造把失败 outcome 的完整 `model_content`
直接加入 `unresolved_errors`，绕过已有的 `event_view` 短视图。继续删除
`recent_steps` 已无作用。科学状态重复占用确实存在，但本次不改变其结构或
持久化职责。

错误来自 `AgentCoreV4::validate_execution_context` 的本地字符串字节检查，
发生在 provider 请求验证之前。这里的 262144 是宿主字节预算，不是从 OpenCode
响应得到的 token 上限；修改鉴权头、模型协议或目录窗口不能修复这条旁路。

## 行为与边界

检查点新建与旧检查点恢复共享一个错误视图构造入口。对于失败的 `ToolFinished`
事件，仅在当前执行上下文具备 `agent.read_tool_result` 能力时，复用现有
`event_view` 构造有界错误视图。保留失败标记、子任务状态、诊断摘要和原始结果
引用；大型委派图中的工具证据不再作为完整字符串重复塞入模型检查点。
按错误字符串嵌入检查点后的实际 JSON 字节数比较表示，只采用更小的视图；短错误
保留原文，避免把额外结构化数据和引用元数据带进不可裁剪的错误列表。

引用沿用原事件序号、事件 hash 和字段，读取仍须通过当前 project、conversation
和 run 的隔离检查及 UTF-8 分页边界。完整失败正文继续保存在原始事件中，可按
`next_offset` 分页恢复。视图不将失败改成成功，也不把摘要当成新的科研证据。
没有结果读取能力时保留原始错误；若不可缩减内容仍超限，继续明确暂停。
其他错误事件沿用既有语义。

旧检查点即使 `recent_steps` 已为空，也要比较持久化错误与重新构造的错误视图。
只有两者不同时才允许重新生成检查点；修复完成后的相同输入不得反复归档。
原始事件归档成功并持久化新检查点后，才采用新的模型上下文，并重新检查宿主
字节预算和完整模型请求预算。

原始事件及 hash 链、科学状态、冻结计划、完成标准、活动指导、权限、审批和
不确定派发状态均保留。继续运行恢复同一个 run；重建检查点不执行工具或重派
失败委派，不重写已有证据，也不把停止等待解释成取消远端计算。宿主预算、精确模型目录
匹配和 provider 协议保持现有规则；本次不处理科学状态去重或增加 Responses API。

## wisp-science / OpenCode 参考

只读检查的上游提交固定为 `6908592754978c8f39586c86e1b9dc9daaf135cc`。
Wisp 直接调用 HTTP API：Go 的 Base URL 为 `https://opencode.ai/zen/go/v1`，
Zen 为 `https://opencode.ai/zen/v1`。用户输入裸模型 ID 并选择协议，预设不固定
OpenCode 模型列表或模型到协议的映射。
[配置说明](https://github.com/xuzhougeng/wisp-science/blob/6908592754978c8f39586c86e1b9dc9daaf135cc/docs/model-configuration.md#L83-L98)

| 协议 | 在上述 Base URL 后添加的路径 | 认证 |
| --- | --- | --- |
| Chat Completions | `/chat/completions` | Bearer API key |
| Anthropic Messages | `/messages` | `x-api-key` 和 `anthropic-version` |
| Responses | `/responses` | Bearer API key |

对应实现为 [Chat](https://github.com/xuzhougeng/wisp-science/blob/6908592754978c8f39586c86e1b9dc9daaf135cc/crates/wisp-llm/src/openai.rs#L35-L76)、
[Messages](https://github.com/xuzhougeng/wisp-science/blob/6908592754978c8f39586c86e1b9dc9daaf135cc/crates/wisp-llm/src/anthropic.rs#L27-L50) 和
[Responses](https://github.com/xuzhougeng/wisp-science/blob/6908592754978c8f39586c86e1b9dc9daaf135cc/crates/wisp-llm/src/responses.rs#L22-L46)。
Wisp 默认发送自己的 User-Agent，并对 OpenCode `/zen` 路由发送稳定的
`x-opencode-session`；OmicsOps 已对精确 Go 端点发送自己的客户端标识和稳定
会话标识。鉴权头不影响本次发生在 HTTP 请求之前的本地预算错误。
[Wisp 请求标识](https://github.com/xuzhougeng/wisp-science/blob/6908592754978c8f39586c86e1b9dc9daaf135cc/crates/wisp-llm/src/provider.rs#L526-L559)

Wisp 在每次模型请求前计算包括固定工具开销的预算，先归档并裁剪工具噪声，再
按需生成语义摘要；失败时恢复原始消息。这里借鉴的是原始证据与模型工作视图
分离，不照搬其目录回退或模型 ID 尾段匹配，也不替换 OmicsOps 的 Host 授权。
[请求边界预算](https://github.com/xuzhougeng/wisp-science/blob/6908592754978c8f39586c86e1b9dc9daaf135cc/crates/wisp-core/src/context.rs#L450-L531)、
[归档与压缩](https://github.com/xuzhougeng/wisp-science/blob/6908592754978c8f39586c86e1b9dc9daaf135cc/crates/wisp-core/src/context.rs#L1793-L1907)

## 验证与当前状态

- 用超过 500 KB、包含多字节文本的模拟失败委派图覆盖新检查点及
  `recent_steps` 为空的旧检查点；检查短视图满足宿主和完整请求预算，失败
  状态与冻结计划保留，原始图可经结果分页完整恢复，事件 hash 链仍有效。
- 重复读取已修复检查点，不再产生重复归档；没有结果读取能力时保留原始错误，
  超限明确暂停，不调用模型。
- 12 条短错误各带约 7 KB 结构化诊断时，压缩后仍保留短错误正文并满足完整
  请求预算，防止精简视图反而扩大不可裁剪内容。
- 新增测试使用模拟 provider、
  内存事件存储和合成结果，不要求 SSH、真实数据或 API key。
- Windows 手工 smoke：加载出现该预算错误的原对话，使用已有“继续运行”恢复
  同一个 run；检查预算暂停得到重新评估、旧证据仍可读取、失败状态仍真实，且
  没有通过重建对话、扩大权限或重派既有计算来绕过恢复。当前未执行。

2026-09-22 实际执行：

| 检查 | 结果 |
| --- | --- |
| 修复前的大型失败委派回归 | 失败，529187 > 262144 字节；仅修复新检查点后，旧检查点仍以 528986 > 262144 失败 |
| 修复前的短错误膨胀回归 | 失败，完整模型请求超过预算 |
| `cargo test -p omicsops-agent-core checkpoint` | 7 通过，0 失败 |
| `cargo test --workspace` | 1289 通过，0 失败，12 ignored |
| `npm test` | 884 个前端测试和 22 个浏览器桥测试通过 |
| `npm run build` | 通过；保留现有大 chunk 提示 |
| `cargo fmt --all -- --check`、`git diff --check` | 通过 |
| `npm run build:desktop` | 通过；Windows release 应用及 NSIS 本地构建完成，未发布或安装 |

2026-09-24 恢复任务时核对了上述构建日志和本地可执行文件，确认 2026-09-22
的桌面构建已完成；本次补齐记录，不把恢复任务视为重新执行真实模型验收。

真实 OpenCode 模型、SSH 和科研流程端到端验收未执行；确定性回归、成功构建
或上游源码检查均不代表这些验收通过。
