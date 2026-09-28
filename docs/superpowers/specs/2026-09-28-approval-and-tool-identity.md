# 审批诊断与稳定工具身份

## 目标与已知证据

用户选择 Local / RiskBased 后仍遇到 `runtime.execute` 审批，并在继续研究时遇到 `unknown tool omicsops_tool_18`。本次只修正可复现的审批判断或解释，以及 provider 工具身份边界；保留冻结能力、调用 hash、MCP 授权、运行恢复与原始证据。

当前代码已支持普通公开 GET 和项目相对路径结果写入，2026-09-25 的审批 spec 已记录旧截图代码通过当时分类器；本次为另一条真实调用。2026-09-28 08:49:27 UTC 的审批事件已只读关联，代码 2,970 字节，SHA-256 为 `a679f35bb76e437e510c98d5c982b3acc0763a11d8fc79bcc8a3125f91e0447c`。生产分类器仅解析该调用返回 false；AST 显示 `os.path.exists(name)`，其中 name 在顶层简单赋值为安全项目相对字符串。当前 path helper 只接受 join/getsize，故拒绝 exists。临时诊断副本仅将 exists 加入 getsize 同级的一参数元数据检查，完整真实调用的分类结果变为 true。该验证没有执行 Python、访问文献网络或修改数据库，原始代码未写入仓库或文档。

可直接从代码确认：`llm.rs::provider_tool_aliases` 根据当前工具列表序号生成别名，因此插入、移除或重排前置工具会改变后续工具的名字。`execution_request` 过滤 discovery、失败 MCP 包装器和关闭的可选能力；但当前注册表按 ID 排序，discovery 与 MCP 包装器排在带点号的工具之后，仅移除这两个工具不能证明当前运行发生了别名漂移。`canonical_tool_id` 对未知名字原样透传，core 随后把未知工具视为致命错误。正常流与非流回退均使用 request-scoped 解码；桌面没有把普通文本解析成工具调用的实现，`require_strict_json_fallback` 当前没有消费方。实际事件是否源于旧序号仍需运行证据，不能从错误名称单独断言。

参考模式来自固定提交的 Wisp Science：其[工具名和注册表](https://github.com/xuzhougeng/wisp-science/blob/10313c964528562bb5c674ae1001eade45934c8a/crates/wisp-tools/src/lib.rs#L160-L210)统一 schema、dispatch 与审批标识，其[工具结果处理](https://github.com/xuzhougeng/wisp-science/blob/10313c964528562bb5c674ae1001eade45934c8a/crates/wisp-core/src/agent.rs#L530-L650)把受控失败反馈给模型。这里只借鉴稳定身份与错误反馈模式，不复制 AGPL 实现；OmicsOps 在 provider 响应进入工具派发前拒绝未知名，并使用自己的有界修复路径。该参考不作为常规 Python/R 自动审批或只读并发的证据。

## 修复一：provider 工具身份与一次输出修复

1. 不合法的 provider 工具名改成可读 slug 加 canonical ID 的固定 SHA-256 摘要，最长 64 个 ASCII 字符，只使用字母、数字、下划线或连字符。保留不处于保留命名空间的合法原名。生成名使用保留前缀 `omicsops_tool_`；canonical ID 自身以该前缀开头时也编码，避免与生成名相撞。摘要与 slug 只由完整 canonical ID 决定，不因顺序、增删其他工具改变。检测当次请求的最终名字碰撞并拒绝构造请求，不用序号或追加下划线补救。无需新依赖，adapters 已有 sha2。
2. Request-scoped decoder 只接受本次公布的 provider 名，或本次公布工具的精确 canonical ID。解码成功后只向上层发 canonical ID。未知名、旧序号别名、未公布的 canonical ID 必须返回固定错误码 `unknown_provider_tool:`，且不可把候选名称、参数、代码写入错误文本。不得猜测数字索引、跨请求查旧映射、模糊匹配或扩展能力。
3. 在 adapter 边界拒绝整次模型响应，任何同响应中的工具都不派发。即使此前已经积累部分调用或 public preview，desktop stream 返回错误时也不得返回可执行 turn。核心仍对非 provider ToolPort 的未知调用保留防御检查。
4. `unknown_provider_tool:` 明确分类为永久 `InvalidResponse`，放在任何数字状态码、timeout 等字符串猜测之前。core 复用现有 `output_repair_attempted`，与 malformed JSON 共用一次输出修复额度。请求保留相同工具集合和证据，补充“上次响应未执行任何工具，只使用当前公布工具与完整 JSON”的修复指令；第二次同类错误终止并保留证据。不能用 transport retry 扩大重试，也不能重试已派发的副作用。
5. 旧数据库保存的是 canonical ID，正常历史无需迁移。历史已失败且没有 dispatch 的未知工具记录不能被推断为已执行；如另有已派发且结果不确定的操作，原恢复门禁继续生效。

## 修复二：安全相对路径的 exists 检查

在 `safe_os_path_call` 中只增加 `exists`，与 `getsize` 共享恰好一个参数的要求；参数继续必须满足现有相对路径证明。支持直接安全字面量、顶层简单赋值已证明的名称及安全 join 结果。`exists` 返回布尔值，不得把其结果登记为安全路径；`safe_relative_path_expr` 继续只接受返回路径的 join。不顺便增加 isfile/isdir 等未被实际调用需要的方法。

用脱敏最小等价代码验证常规 exists 无需新审批，并覆盖绝对路径、父目录、控制目录、未知和重绑名称、条件或缩进赋值、别名导入、裸方法引用、零/多参数及把布尔结果作为路径的反例。原有低风险规则和危险行为门禁保持；不新增 typed reason 接口、数据库表或 UI。不能自动批准旧 pending、覆盖旧拒绝、改写已有事件或切换 FullAccess。

## 耗时解释与边界

UI 总耗时是 run 首尾墙钟，包含模型、工具和用户等待。工具耗时通常从 dispatch 到结果，可能包含执行器锁等待；模型等待是当前 attempt 从请求事件开始的墙钟。104 秒工具和 51 秒模型不能单独证明 MCP 索引或核心 CPU 慢。进一步优化只依据事件拆分、请求字节数与工具输出大小；本次不靠提高超时或降低审批解决耗时。

本次只读事件统计截至 `run_cancelled`：86 条事件，总墙钟 574.453 秒（约 9 分 34 秒，截图是取消前的较早时点）。5 次模型请求均有匹配的最终或中断 usage 事件，耗时分别为 11.489、102.484、186.557、96.139、61.335 秒，合计 458.004 秒，约占总墙钟 80%。7 次已派发工具均有结果，逐调用耗时合计 104.445 秒，其中 runtime.execute 为 104.123 秒；search_mcp_tools 为 0.016 秒，其余项目读取、Skills/Memory 搜索均不足 0.1 秒。唯一审批等待 9.564 秒。工具可能并发，所以逐调用耗时合计不是严格互斥的墙钟分区。当前证据支持“模型等待为主要墙钟来源，运行时代码另占约 104 秒”，不支持“目录索引计算耗时数分钟”；模型供应商网络、排队、推理和生成的内部占比仍未观测。

## 验证

确定性测试覆盖工具顺序、插入和删除、保留前缀冲突、有效名称和长度；OpenAI-compatible / Anthropic / Ollama 的流式字节分块与参数增量，以及非流式响应恢复到 canonical ID；精确 canonical 兼容、未知和已移除工具拒绝；同响应好坏混合调用零派发；一次修复成功、第二次停止、与 malformed JSON 共用额度。审批测试保持 Local/SSH、旧批准/拒绝和 Plan 隔离。

交付前执行 `cargo test --workspace`、`npm test`、`npm run build`。桌面错误分类或接线变化需执行 `npm run build:desktop`。格式检查若失败仅因格式，按 AGENTS.md 单独提交格式变化。真实模型/SSH 测试仅在按 `acceptance/README.md` 配置一次性环境后显式执行，分别报告通过、失败、未执行；模拟测试与构建不等于真实端到端验收。
