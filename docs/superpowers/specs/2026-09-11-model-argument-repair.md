# 工具参数契约与模型 JSON 纠错

## 实际故障

证据登记第一次调用把 `sources` 填成文献说明字符串，但宿主要求
`EvidenceSourceV4` tagged object。公开 schema 原先只有 array 类型，缺少
items 定义。随后模型响应中的工具参数 JSON 缺少分隔符，流组装器拒绝整轮
响应，错误被直接作为永久模型失败返回。

## 修复范围

- `science.record_evidence` 明确描述 literature 和 artifact 两种对象、kind
  标签及各自必填字段，给 conflicts_with 声明 UUID 元素类型。宿主原有验证
  仍有效，不把任意字符串自动转换为已验证证据。
- InvalidResponse 中明确的 malformed JSON arguments 与 truncated_output
  共享一次纠错机会。向模型说明上轮调用未执行，要求一个简短、符合 schema
  的完整 JSON 工具调用，并保留原上下文，不重复已完成的检索。
- 不修补或执行畸形 JSON。失败响应中的全部调用均不派发，部分公开文本不
  写入正式事件；纠错后仍失败则明确报告，不无限重试。请求预算、超时和取消
  检查继续适用。

## 验证

确定性回归先复现错误再修复：覆盖畸形 JSON 与截断输出的首次失败后恢复、
连续失败只重新生成一次、部分文本不提交及预览清理；工具契约测试覆盖两种
来源对象的必填字段。完整真实模型/SSH 验收另行执行，测试和构建不代表
真实科研任务已完成。

本次确定性检查：`cargo test --workspace` 423 通过、11 ignored；`npm test`
151 个前端测试和 22 个扩展测试通过；`npm run build` 通过。
