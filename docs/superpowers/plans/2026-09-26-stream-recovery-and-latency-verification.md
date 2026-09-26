# 断流恢复与上下文性能验证

## 实现与审查

- `25e9e44`：固定缺终止事件错误归为可重试传输故障，拒绝执行部分响应。
- `689abc2`：历史 provenance.code 正文按需读取，完整快照先持久化。
- `a030818`：独立格式提交。

参考仓库检索由 GPT-6 Luna max 完成，架构与复审由 GPT-6 Astra high 完成，
两项实现由 GPT-6 Sol high 完成。复审无剩余 P1/P2；追加了优化归档失败时原候选
超预算和强制压缩的保守边界测试。没有改变用户保存的 max 推理强度。

## 回归证据

断流分类测试先失败：真实 AdapterError 包装字符串被归为 InvalidResponse。
修复后，本地 SSE 返回已完整的工具参数但无 terminal 时仍失败；同一 call ID 的
第二响应正常结束后只返回第二份参数，两次请求体相同，没有拼接旧文本。core
设置 max_model_retries=1，恰好尝试两次后停止，产生一条重试事件，部分文本与工具
未落为成功结果。认证、截断输出及无效参数不误入新增分类。

性能回归先失败于缺少模型投影，修复后相同合成科学状态与预算下，context 从
18,216 字节减至 2,294 字节。完整分页恢复与非 code 字段保持不变。覆盖 UTF-8
阈值、作用域/哈希/状态匹配、同快照复用、状态变化刷新、保留近期结果、无 reader、
关闭压缩、归档失败回退和原文仍超预算时的目录视图兜底。

只读实际故障快照的组成对比为 152,313 字节 → 67,374 字节（后者尚未包含新增
引用元数据），减少 55.8%。这不是完整模型请求减少比例或耗时加速比例。实际
31 分钟运行只有约 16.7 秒是工具批次，成功输出约 93% 为推理 tokens；保留 max
意味着推理延迟仍可能很长。

## 已执行定向检查

- `cargo test -p omicsops-desktop terminal --lib`：9 通过。
- `cargo test -p omicsops-agent-core missing_terminal_stream --lib`：1 通过。
- `cargo test -p omicsops-agent-core code_ --lib`：5 通过。
- `cargo test -p omicsops-agent-core --quiet`：155 通过。
- `cargo test --workspace`：1,364 通过，12 ignored，0 失败；ignored 不算真实验收。
- `npm test`：944 项前端测试和 22 项桥接测试通过。
- `npm run build`：通过，保留大块构建产物提示。
- `cargo fmt --all -- --check`：通过。
- `npm run build:desktop`：当前未通过最终产物替换。release 库编译完成后，Windows
  因两个正在运行的 OmicsOps 实例锁定 target/release/omicsops-desktop.exe 而拒绝删除
  旧文件（os error 5）。未强制终止应用，以免丢失用户输入；需退出后重新构建。

## 手工流程与限制

完整确定性检查通过后，桌面打包因旧程序锁定受阻期间，执行了隔离的真实模型
验收（沿用已保存的精确 DeepSeek/max 配置，仅进程环境选择其 ID）：

```text
cargo test --workspace agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

退出码 0，1/1 通过，耗时 171.95 秒。模型成功读取一次性文件并返回其中随机验证串，
记录 RunCompleted；观察到 2 次包含推理内容的尝试、147 个非空预览，未记录推理
正文或凭据。该真实测试没有故意制造 EOF，断流安全恢复由上述本地 SSE 和 core
回归验证；此时长不是原 31 分钟任务的前后对照，也不是性能加速证明。

从新构建程序启动后，在原会话发送“继续”创建新运行；历史失败事件保留。确认
无终止事件时出现有界重试，仍可取消；恢复后的工具仅执行一次。代码快照增长
后，确认科学证据和最新工具结果仍可供模型使用，必要时可读取完整代码。

没有自动重跑原研究任务，没有修改用户数据库/冻结运行/凭据，没有执行 SSH
或原生 UI 手工验收；一次性小模型验收也不能替代原 miRNA 长流程验收。
