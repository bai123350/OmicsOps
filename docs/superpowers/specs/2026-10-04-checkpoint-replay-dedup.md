# 压缩 checkpoint 与原生 replay 去重

## 问题与边界

原生 provider replay 已携带经过完整事件链、模型绑定和宿主结果校验的工具结果及公开进度。上下文压缩后，`checkpoint.recent_steps` 又保存这些事件的 JSON 模型投影字符串；原有去重仅处理 `recent_events`，同一内容因此再次进入模型请求。

本次只调整 `remove_replayed_context_events` 的模型视图。调用方仍先验证原生 replay 的完整来源、运行作用域和冻结模型绑定；事件、归档、持久化 checkpoint、执行、审批、重试和预算语义保持原样。

## 行为

- checkpoint step 必须能解析为 JSON，并与某个校验通过的签名来源事件的当前 `event_view` 完全一致；文本相似、复制 event hash 或仅有相同 call ID 均不足以删除。
- 工具结果还必须与原生 replay 中同 call ID 的 `ToolResult.output` 完全一致。其他运行的同名调用、缺少结果的部分 replay、改写的投影和完整性失败的来源继续保留。
- 公开 `ModelText` 必须已在经过验证的原生 replay 中以 `AssistantText` 表达。
- 普通叙述、旧格式 step、无效 JSON、未被 replay 覆盖的澄清与委派结果继续保留。`unresolved_errors`、科学状态、冻结计划、计算选择和活动用户指导不变。
- 精确投影不匹配时保守保留，兼容旧 checkpoint；不新增结果缓存、持久化格式或权限。

## 验证

签名事件链回归先证明现有实现仍发送重复 checkpoint step，再验证工具证据和公开进度只在 replay 中保留，未覆盖内容继续出现在上下文，原始事件及 checkpoint 字节与签名不变。

确定性 fixture 的上下文由 6,141 字节降至 1,498 字节，移除 4,643 字节重复内容；这是输入大小测量，不是模型响应时间或真实订阅端到端性能结论。

定向命令：`cargo test -p omicsops-agent-core model_replay::tests -- --nocapture`，6 个测试通过。真实模型、SSH 和订阅端到端验收未执行。
