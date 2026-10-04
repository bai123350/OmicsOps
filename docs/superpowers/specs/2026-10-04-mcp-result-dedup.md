# MCP 结果的模型视图去重

## 问题与行为

桌面端的 `use_mcp_tool` 把原始 MCP 返回值序列化为 `model_content`，同时在
`data.result` 中保存同一返回值，并在 `data` 中附上服务器、工具、catalog/schema
hash 和 audit ID。原有投影只识别 `model_content` 与整个 `data` 相同的情况，
因此小型 MCP 结果仍会完整进入模型上下文两次。

在现有成功结果去重规则中增加一个精确条件：仅当工具 ID 为 `use_mcp_tool`、
`model_content` 与 `data.result` 的 JSON 序列化字符串逐字相等，且完整 `data`
仍在现有 8192 字节内联预算内时，投影删除重复的 `model_content`。
模型直接使用完整的 `data.result`，全部 Host 元数据与 provenance 保留。
完整内联结果不生成冗余回读引用。

失败结果、不同摘要、仅语义相同但序列化字符串不同的结果，以及其他工具的
嵌套数据保持原行为。完整 `data` 超预算时仍使用原有省略、引用和分页规则，
不能只按 `data.result` 的大小决定去重。

## 边界

这次变更仅作用于 Agent Core 的模型投影。签名事件、SQLite、MCP 调用、
审批、catalog/schema 绑定、凭据、冻结执行范围、推理强度和自动审查均不改变。
原始 `model_content` 仍可使用同 run 的 sequence/hash 读取，超大结果能按
`next_offset` 完整恢复。无需迁移，也不新增缓存或模型请求。

## 验证

四个纯内存回归测试覆盖：

- 成功结果只内联一份，原始结果、全部 Host 元数据、provenance 和签名事件保留。
- 失败、独立摘要、不同 JSON 字符串和其他工具的嵌套结果不被去重。
- 返回值本身满足预算、完整 wrapper 超预算时继续提供原始内容和回读引用。
- 超大 UTF-8 结果通过签名引用分页读取后与原始结果逐字一致。

`cargo test -p omicsops-agent-core --lib mcp_result_projection -- --nocapture`
先得到预期失败：成功结果仍含重复的 `model_content`；最小修复后 4 项通过。
该确定性 MCP fixture 的模型投影从 5826 字节降至 3148 字节，减少 2678 字节。
这只说明本 fixture 的输入体积变化，不代表真实模型耗时已按比例降低。
`cargo test -p omicsops-agent-core --lib`：183 项通过；`rustfmt --edition 2024
--check crates/omicsops-agent-core/src/context_views.rs` 和本次文件的
`git diff --check` 通过。

交付前仍需运行 workspace、前端测试与 Web 构建。真实模型/SSH 验收和 Windows
GUI smoke 不由这些纯内存测试替代。

手工 Windows smoke：在一次性文献项目中获取小型 MCP 记录，确认模型能直接
使用完整记录并引用对应 audit；获取超大结果时确认仍能分页恢复。沿用同一
模型、推理强度和自动审查配置，比较脱敏的模型输入字节、请求次数和总耗时。
