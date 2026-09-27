# 审批误报与 MCP 目录读取优化验证

## 已完成修改

- `f26f7ba`：完整 MCP 目录继续冻结，模型只读取工具名称索引；支持按关键词筛选并精确取 schema，压缩后保留已验证的目录引用。
- `7ce9a71`：RiskBased 对显式 `os.path.join/getsize` 和已证明的项目相对路径不再因 `import os` 误报。未知路径、动态访问、危险能力和不确定绑定仍需批准。
- `676f197`：按仓库约定单独提交格式变化。

保留用户明确选择的 DeepSeek `max`，不改输出额度、超时、冻结授权或原有审批记录。
历史已请求的单次审批仍按原决定处理；新的预检规则用于后续调用。

## 回归与审查

先复现安全路径被拒绝及压缩后目录引用丢失，再完成实现。Astra high 审查、Sol high
编码，Luna max 查阅 GitHub 参考源码。审查发现的长查询页元数据溢出、条件/分号
赋值、嵌套作用域和复杂名称绑定问题均已补充回归。最终无剩余 P1/P2。

247 工具的合成目录，在相同分页约束下，旧描述预览目录为 34,219 字节/7 页，
新名称目录为 6,346 字节/2 页。完整原始目录和单个工具 schema 仍可完整恢复。
这些是目录数据量和分页数，不能换算成整个研究任务的耗时改善比例。
参考仓库与本项目实现差异见对应设计文档，未声称参考项目保证固定完成时间。

## 已执行确定性检查

- `cargo test -p omicsops-desktop --lib runtime_approval`：6 通过。
- `cargo test -p omicsops-agent-core mcp_ --lib`：10 通过。
- `cargo test --workspace`：最终 1,372 通过、12 ignored、0 失败。
- `npm test`：944 前端测试、22 桥接测试通过。
- `npm run build`：通过。
- `cargo fmt --all -- --check`：通过；先运行 `cargo fmt --all` 并单独提交格式变化。
- `npm run build:desktop`：通过，生成新的 `target/release/omicsops-desktop.exe` 及本地 NSIS 构建产物，未发布。

Web 构建保留大 chunk 提示；Windows linker 输出创建库的信息被标为 warning，
构建退出码为 0。未修改依赖锁文件、用户数据库、凭据或未跟踪的 website 目录。

## 真实验收与手工范围

确定性检查通过后，执行：

```text
cargo test -p omicsops-desktop agent_v4::go_live_acceptance_tests::live_opencode_go_agent_reads_file_and_returns_nonce -- --ignored --exact --nocapture
```

使用只读选择的精确 OpenCode Go `deepseek-v4.1-flash`/`max` 已保存配置与 keyring
凭据，在一次性项目中运行。退出码 0，1/1 通过，耗时 129.61 秒；成功读取随机
验证串并在最终答复返回，记录完成事件。观察到 2 次有内容的模型请求、387 次
非空预览，未输出或保存推理正文及凭据。这是小型真实模型/工具链验收，不是
原调研的速度对照，也没有使用真实 MCP 目录验证整段文献流程。

原始 miRNA 长流程、SSH 和原生界面手工验收未执行。
手工复查：用新构建启动新运行，选择“帮我批准”；项目相对路径元数据脚本应直接
执行，危险命令和未知路径仍出现审批。文献工具搜索应返回匹配索引，可改关键词、
清空筛选或取精确 schema；压缩后继续使用相同目录引用，不重复完整目录发现。
