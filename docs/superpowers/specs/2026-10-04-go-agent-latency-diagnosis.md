# OpenCode Go 文献调研耗时定位

## 本次实际运行

2026-10-04 对用户截图所对应的运行使用 SQLite 只读事务检查。只提取事件时间、
请求标识、工具名称、请求大小和 usage 计数，不导出用户提示词、模型推理正文、
原始工具输出、凭据或项目路径。统计区间从运行创建到用户取消；取消不表示已
派发的远端计算被取消。

| 项目 | 实测 |
| --- | ---: |
| 总运行时间 | 1,486.520 秒（24 分 46.520 秒） |
| 模型请求到最终 usage 的时间合计 | 1,327.899 秒（89.3%） |
| 工具批次执行时间合计 | 151.974 秒（10.2%） |
| 其他时间 | 6.647 秒（0.4%） |
| 模型逻辑请求 / 尝试 | 11 / 11，无重试 |
| 已完成模型回合 | 10；最后一轮被取消 |
| 已完成回合输出 token | 210,384 |
| 其中推理 token | 189,880（90.3%） |
| 最大单轮模型时间 | 351.367 秒 |
| 该轮输出 / 推理 token | 62,393 / 54,239 |
| 请求大小范围 | 128,157–281,563 字节 |
| 单轮输入 token 范围 | 43,031–92,469 |

usage 每个 attempt 只累计一次最终累计计数，不把 SSE 样本重复相加。工具使用
`tool_batch_finished.duration_ms`，避免将并行工具等待区间重复求和。模型区间
包含服务端等待、推理和输出；旧事件没有持久化首字节时间，无法进一步精确
拆开这三者。取消回合 usage 为 interrupted/unknown，缺失 token 不记为零。

本次冻结模型对应的已保存配置是 OpenCode Go `deepseek-v4.1-flash/max`。自动
审查虽然开启，但取消前没有提交完成提案，尚未进入 reviewer；本次没有模型
重试，所以不能将主要耗时归因于自动审查、超时退避或本地文件核验。

## 根因与调整

主要成本是 max 档持续生成的大量推理，而非工具执行速度。用户已明确选择
“日常调研使用 low，优先速度”。在确认该配置没有活动运行后，以事务和乐观
并发校验将此精确 Go 模型配置的 `reasoning_effort` 从 max 保存为 low，逐字段
确认其他内容没有变化。不改历史冻结运行、凭据引用或其他模型配置；复杂工作
仍可显式选择 max。

本项目既有预算映射为 low 16,384、high 32,768、max 65,536。此次未重写映射、
缩短超时、扩大输出预算或强行切换模型协议。选择未指定 effort 的“默认”并不
等同 low；DeepSeek 官方说明默认思考为 high。

此前 native replay 检查点去重适用于支持原生回放的 Responses 路径；Go 此模型
使用 Chat Completions，`supports_native_replay=false`，因此该项优化不会命中。
普通 JSON 上下文的近期事件只包含检查点之后的事件，本次不增加没有实际收益
的泛化检查点去重，也不新增模型推理正文持久化。

## 次要但可修复的放大因素

1. 四次 PubMed 调用将完整 MCP 包装器再次放入 `arguments`，缺少原工具所需
   query。现有泛化 object 校验与目标哈希校验均接受该形状，直到服务器本地
   参数解析才返回普通 isError，随后被计入服务器失败熔断。备用检索对应模型
   回合耗时 351.367 秒，工具批次耗时 137.333 秒。它们发生在错误之后，但不
   声称整个区间均可避免。
2. Runtime 成功结果同时保存 stdout/stderr 正文及 capture excerpt，模型视图
   又有同一正文的 model_content。数据超过 8 KiB 后整个 data 被隐藏，其中
   已由 Host 读取校验的 artifacts 大小和 SHA-256 也不可见。本次三个执行结果
   data 为 34,661 / 10,798 / 26,184 字节；去除正文副本后的元数据分别仅为
   1,405 / 764 / 918 字节。第一份已有四个捕获文件，却又出现四次 artifact.verify。
   核验批次仅耗 489 毫秒，但其前后的模型回合会继续产生成本。

修复在当前授权与原始证据边界内进行：参数形状预检只拒绝可证明不符的基本
schema 约束，返回精确原 schema 与未派发标记供模型纠错，不自动展开包装器、
代为改写批准参数或派发调用。未知 schema 构造仍交服务器验证，预检不是完整
JSON Schema 验证器。已知本地参数错误不计入服务器业务失败；真实业务错误
沿用现有熔断语义。

Runtime 模型投影在整体有界的前提下保留成功状态、执行身份、捕获元数据与
文件校验事实，正文只保留一份；超多或超大元数据显式标记省略并提供签名原文
引用。失败与无法识别的旧结果保持保守路径。捕获校验仅对应执行捕获时的内容，
文件变化、缺少事实或任务要求重新核验时仍需核验。

## 验证与限制

使用合成结果、内存 SQLite、假 MCP/运行器完成回归；不以真实模型、科学数据、
SSH 或网络为自动化测试前提。交付记录实际命令与通过、失败、忽略状态。
本次隔离 worktree 的确定性检查均 exit 0：

| 实际命令 | 结果 |
| --- | --- |
| `cargo test --workspace` | 1,509 passed，0 failed，16 ignored |
| `npm test` | 992 前端测试与 22 浏览器桥接测试通过 |
| `npm run build` | Web 生产构建通过 |
| `npm run build:desktop` | Windows release / NSIS 构建通过，未安装或分发 |
| `cargo fmt --all -- --check`、`git diff --check` | 通过 |

新增回归覆盖错误 MCP 包装器、必需字段与基础类型、复杂 schema 保守放行、
未派发参数错误与真实业务错误的熔断区别，以及 Runtime 输出转义后有界投影、
扩展元数据保留、capture 内容不一致和签名原文精确恢复。独立代码审查没有
剩余阻塞项。构建仅有既有 Vite 大 chunk 提示和 Windows linker 信息警告。

确定性检查之后，按 `acceptance/README.md` 显式运行：

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

在同一 Windows 用户会话中，仅选择上述已保存的精确 Go DeepSeek profile。
只读预检再次确认 effort 为 low；验收保留该 effort 与既有 keyring 引用，
使用临时项目和内存 Store。测试 1 passed / 0 failed（exit 0），执行耗时
6.79 秒；此前测试目标重新编译另耗 1 分 46 秒，未计入模型调用耗时。
生产 Agent 完成两轮模型调用、一次 `project.read`、正确 nonce 和 `RunCompleted`。
12 次非空临时 reasoning 观测均在对应结果之前，最大预览 251 字节，首次/末次
为首个模型请求后 2,405 / 6,016 毫秒。仅输出计数、字节数与时间，未记录正文。

配置更新已经保存，尚未对同一完整文献任务做 low/max 配对复测；代码修复和
输入缩减不能直接证明生产任务的耗时降幅。真实 SSH、科研 provenance 流程和
原文献任务的原生 UI 验收未执行。使用新构建重启应用并新建调研运行验证：模型
选择应为 low；错误 MCP 参数应显示可恢复的未派发错误；大执行结果应显示捕获
文件的大小与哈希。历史冻结运行不会因已保存模型配置更新而切换 effort。

## 官方依据

- [OpenCode Go 精确模型的 low/high/max 元数据](https://raw.githubusercontent.com/anomalyco/models.dev/dev/providers/opencode-go/models/deepseek-v4.1-flash.toml)
- [DeepSeek 思考强度与默认 high](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode/)
