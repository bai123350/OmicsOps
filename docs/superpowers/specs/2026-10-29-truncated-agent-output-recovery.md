# Agent 工具输出截断的一次精简重生成

## 本次故障

截图中的 `truncated_output` 发生在主执行阶段。只读诊断元数据显示该次运行
9 分 27 秒，没有完成提案、确定性验证、ReviewerFinished 或 ModelRetrying。
续写设置未保存，按默认值 auto_continue=false；末次实际输出及推理用量未知。
当前保存配置的有效输出额度为 16,384，不能凭截断推断实际用了多少推理 token。

主执行在遇到截断后先检查可选 continuation_remaining；默认零导致立即退出，
没有进入 malformed JSON / unknown tool 共用的一次输出修复。工具响应的整体
重生成和用户选择的多次续写被混为一条路径。早期工具格式修复设计曾覆盖截断，
后续 Session 开关拆分使默认 Agent 缺少这次安全修复机会。

## Wisp 参考及适配边界

阅读固定版本 `7af98bb170b72f1e0a4d8ef4551b057df6034655`：

- [agent.rs:459](https://github.com/xuzhougeng/wisp-science/blob/7af98bb170b72f1e0a4d8ef4551b057df6034655/crates/wisp-core/src/agent.rs#L459)
  在 length/max_tokens 时拒执行整批工具调用，提示缩小并分段；其普通 Agent
  自动继续仍默认关闭，不能声称它默认无限恢复或覆盖所有 provider 协议。
- [project_reader.rs:758](https://github.com/xuzhougeng/wisp-science/blob/7af98bb170b72f1e0a4d8ef4551b057df6034655/src-tauri/src/project_reader.rs#L758)
  为只读 Reader 的截断/JSON 失败，从原 transcript 一次重生成更短输出。
  本次借鉴整体丢弃、精简指导与有界重生成原则，不复制源码。

不移植 Wisp 的合成 assistant/tool 历史、参数 excerpt 或输出预算提升策略。
OmicsOps 的原始工具证据、native replay、冻结模型配置和审批仍由宿主校验；
未执行的失败调用不得伪装成已有工具结果或已执行证据。

## 实现决策

仅对 persist_text=true 且当前请求 tools 非空的 Agent 请求，首次明确截断先
使用现有 output_repair_attempted 共享的一次修复。它与 malformed JSON 和
unknown tool 合计最多一次，不增加 Reviewer、tool-free 摘要或辅助请求的次数。
这次修复优先于可选续写，auto_continue 的默认值与保存值均不修改；后续的
显式续写仍按原运行级次数额度计数。

修复请求保留原 context、工具能力、图片和合法 replay，不附加被截断的正文、
残缺参数或推理。指导模型只输出一个更短且符合 schema 的完整调用，大写入
分块；若提交最终答案，则完整、简明且有已验证证据支持。不得拼接 JSON、假设
失败调用已执行或重跑此前成功工具。失败响应的全部调用、部分正文和 provider
continuation 不进入正式执行记录；只接受经过现有校验的新完整响应。

完整替代请求须再次通过真实 wire 预算准入；预算、取消、用户指导、原模型身份、
超时和宿主审批均保持。连续截断/混合格式错误不获得第二次隐式修复；仅在用户
明确打开续写时才使用其额外预算。纯文本无工具请求保持原开关政策，不改模型
目录、推理选项、配置输出额度、DTO 或持久化模式。Session 中英文说明需明确
一次工具输出修复与可选额外续写的区别。

## 验证范围

先复现默认工具请求截断直接退出，再验证一次完整调用只派发一次、连续失败及
混合错误共享修复槽、原上下文与 replay 保持、部分内容不落正式事件、停止/指导
阻止重试及完整 wire 预算拒绝。保留纯文本、可选续写、Reviewer 和宿主来源
完整性的已有回归。真实 API/SSH 与完整科研任务验收必须单独说明，离线夹具
和成功构建不代表用户科研任务已完成。

## 已执行验证

新增默认工具截断回归在旧实现先失败，明确不是网络或夹具错误。修复后
`cargo test -p omicsops-agent-core --lib tool_` 20 项通过，完整
`cargo test -p omicsops-agent-core` 280 项通过（273 项既有 + 7 项新增）。
原来的 tool-free 默认不续写策略保留；Session 夹具使用字面预期次数验证先共享
一次格式修复，再消耗显式运行级续写额度，跨逻辑请求不重置该额外额度。

桌面新增 `truncated_tool_repair_uses_desktop_wire_budget_before_a_second_request`
通过：真实 AgentCore.plan 驱动截断修复，第一请求可发送，新增 system 指导后
实际 Desktop/provider JSON 预算拒绝，第二次请求数为零，没有工具派发、正式
部分正文、replay 或完成记录。模拟 provider 不连接网络，也不读取凭据。
该测试不以 Rust ModelRequest 大小冒充最终 wire 请求大小。

`npm test` 通过：132 个 Vitest 文件、1588 项，另有扩展 22 项、发布脚本 37 项。
最后文案调整后 `npm run build` 再次通过，Session 设置相关 13 项再次通过。
全局格式检查、差异空白检查及独立源码复核通过。没有新增依赖或修改锁文件。

首轮完整 Rust 检查暴露一条订阅协议测试仍匹配旧的自动续写提示。将该用例
改为默认关闭续写，直接比较恢复前后的 input、tools、model、max_output_tokens
保持相同，且请求不包含 discarded partial；原请求次数与零工具派发断言保留。
更新后最终 `cargo test --workspace` 为 2136 通过、22 ignored、零失败。
真实模型/SSH、完整科研流程和安装升级验收未执行，用户运行记录不改写。

`npm run build:desktop` 通过，配置资源、Web 与原生二进制隐私检查通过，
生成 Windows x64 EXE（2026-10-29 22:49:12 +08:00）与 MSI（22:49:23）。
版本保持 0.1.0，未安装、分发、推送或更新 GitHub Release；现有失败历史保留，
安装本轮包并重启后须重新发起任务。构建仅有既有 Vite 大 chunk 和 Windows
linker 信息警告，没有构建错误。
