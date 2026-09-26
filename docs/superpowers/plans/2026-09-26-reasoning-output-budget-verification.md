# 推理输出预算验证

## 修改和证据

`3b38084` 调整精确目录推理模型的输出预留，`3e720ad` 为隔离的 OpenCode Go
验收加入精确 deepseek-v4.1-flash 模型。实现由 GPT-6 Sol high 完成，GPT-6 Astra
high 只读复审未发现 P1/P2。

只读用户运行元数据确认保存配置为 max，精确目录输出上限为 384,000，而原实现
发出的上限为 16,384。截断结果的最终用量未知；没有推断它的实际推理 token 数。
回归先失败：high 请求实际 16,384 而期望 32,768，max 桌面请求实际 16,384 而期望
65,536。修复后核心 4 项及桌面请求 1 项定向测试通过。另验证同一推理强度下有效
额度变化会改变配置哈希，目录上限变化但有效额度不变时哈希不变。

预算仍受目录 output 和上下文限制，未知和非推理配置不提升；自动继续设置和
半截调用拒绝规则未改。历史运行不改写，新预算通过新运行重新冻结。

## 确定性检查

- `cargo test -p omicsops-core output_budget_tests --lib`：4 通过。
- `cargo test -p omicsops-desktop opencode_go_max_reasoning_budget_reaches_desktop_provider_request --lib`：1 通过。
- `cargo test --workspace`：1,356 通过，12 ignored，0 失败。
- `npm test`：944 项前端测试及 22 项桥接测试通过。
- `npm run build`：通过，保留 Vite 大块产物提示。
- `npm run build:desktop`：通过，release 编译 4m35s，生成本地程序与 NSIS 构建产物；
  未分发安装包，链接器只有创建库/对象的信息提示。
- `cargo fmt --all -- --check`：通过。

## 验收范围

真实命令（设置精确保存配置 ID，运行结束后清除进程环境变量）：

```text
cargo test --workspace agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

第一次执行失败，退出码 101，耗时 172.16 秒；OpenCode 在部分输出后中断响应流，
错误为 `stream ended after partial output: error decoding response body`，不是截图中
的 `truncated_output`。观察到 2 次含推理内容的尝试、418 个非空预览；这些元数据
不代表运行完成，也没有将失败记为通过。随后以全新一次性项目独立复测一次。

第二次执行通过，退出码 0，1/1，耗时 67.62 秒；同一 DeepSeek/max 配置下完成
project.read 和包含真实随机验证串的最终答复，并记录 RunCompleted。观察到 3 次
含推理内容的尝试、50 个非空预览，最大预览 1,341 字节；预览文本不落盘。
首次断流仍是已观察到的外部服务限制，复测通过不能证明它不会再次发生。

ignored 测试使用一次性目录、内存 Store 和只读保存配置，并从 Windows Credential
Manager 读取已有凭据；不修改用户数据库、原项目或冻结运行，不使用 SSH。随机
验证串不出现在请求中，必须经一次 project.read 得到，并在最终完成答复中返回。
测试仅输出推理预览的计数、字节长度和时间，不打印或持久保存推理文本及密钥。

这一小型模型—工具—模型验收不是原 miRNA 文献研究或长输出验收，不能证明以后
不会再发生截断。手工检查时关闭旧程序，从新构建启动，在原会话发送“继续”创建
新运行；历史失败记录保留。未运行原生 UI、SSH 或完整文献流程手工验收。
