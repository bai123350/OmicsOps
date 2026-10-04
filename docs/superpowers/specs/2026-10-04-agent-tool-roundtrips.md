# 减少 Agent 工具往返

## 问题与目标

用户的 OpenCode 文献检索截图显示 49 个步骤、33 分 21 秒；可见工具本身仅需
0.1–0.3 秒。这不足以确定服务端耗时，但多余的工具决策会增加模型往返。
当前成功结果即使已把完整 data 提供给模型，去重 model_content 后仍附带回读引用；
MCP 搜索则只给出名称，即使匹配的完整 schema 很小也需要另一次读取。

参考 wisp-science 的按需工具加载方向，独立实现以下小范围优化：

- 完整内联的成功结果只保留一份 data，不为相同 model_content 生成回读引用。
- MCP 目录在完整匹配项及 schema 的转义 JSON 不超过 4096 字节时，附带
  inline_tools；否则沿用名称目录、精确 schema 读取与分页。
- 明确指导模型批量提出互相独立的读取，按已有证据推进；只回读缺失内容，
  已有 schema 可以直接使用。实际文献证据、来源获取和审批仍然必须执行。

## 边界与验证

仅改变模型投影和工具指导，不改变签名事件、数据库、凭据、工具审批、执行范围
或远端运行语义。完整原始结果始终可通过同 run、序号和 hash 校验后读取。
schema 必须来自原始目录，不能截断、猜测或替换目标；大目录继续分页。
MCP 的副作用锁和网络工具审批不变：同轮批量提出请求减少模型往返，
不保证 MCP 网络派发并行。

## 参考依据

固定参考提交 `93bc3be03dd0856c7408a1d23f9b1ac02546b71e`：

- [Registry::search_mcp_tools](https://github.com/xuzhougeng/wisp-science/blob/93bc3be03dd0856c7408a1d23f9b1ac02546b71e/crates/wisp-tools/src/lib.rs#L300)
  在发现结果中提供匹配工具的完整 schema，并保留真实目标的审批。
- [MCP 结果投影](https://github.com/xuzhougeng/wisp-science/blob/93bc3be03dd0856c7408a1d23f9b1ac02546b71e/crates/wisp-mcp/src/result.rs#L13)
  避免重复的结构化 JSON，同时保留事实和失败语义。

本项目保留完整冻结目录，以有界 inline_tools 补充模型视图；没有照搬参考的
top-k 目录限制或顺序执行循环，也没有改变既有同 run/hash 读取边界。

## 验证要求

使用纯内存事件和模拟模型验证内联结果无冗余引用、精确 schema 可在同一结果
取得、超大 schema 退回分页、UTF-8 转义预算与完整原始记录可恢复。
运行 cargo test --workspace、npm test、npm run build；如桌面组合改变，追加
npm run build:desktop。真实 OpenCode 耗时需按 acceptance/README.md 的一次性
环境复测，构建和模拟测试不能证明截图中的 33 分钟已降到某个时间。

## Windows 实际验证（2026-10-04）

- `cargo test -p omicsops-agent-core --lib`：首轮修复后 174 项通过；完整
  workspace 检查包含新增的大 schema 回退测试，核心合计 175 项通过。
- `cargo test --workspace`：1495 项通过，16 项 ignored；ignored 不算真实验收。
- `npm test`：992 项前端测试、22 项浏览器扩展测试通过。
- `npm run build`：通过；保留既有大于 500 kB 的 Vite chunk 提示。
- `npm run build:desktop`：通过，完成本地 Windows release 和 NSIS 构建；
  未安装或分发。linker 建库信息以 warning 输出，没有导致失败。
- `cargo fmt --all -- --check`：初次仅新增测试格式失败，随后执行
  `cargo fmt --all` 并重新检查通过；没有额外的纯格式文件修改或提交。
- `git diff --check`：通过。
- 模拟文献流程验证：原有名称目录路径需要 4 次执行模型调用；匹配的完整
  schema 内联后为 3 次，并保留真实工具证据及 RunCompleted。此数字不含
  固定的模拟 reviewer，也不代表真实模型耗时降低 25%。
- 真实 OpenCode/SSH 验收和 GUI smoke：未执行。所需一次性模型 profile、
  subscription disposable/profile 和 SSH/PBMC 环境未配置。

手工 Windows smoke：用一次性文献项目搜索具体 MCP 工具，展开结果确认
inline_tools 带完整参数 schema；允许模型直接调用目标工具，观察无额外 schema
回读且审批仍生效。对大目录/大 schema 验证分页读取和最终论文证据仍可恢复。
在相同 OpenCode 模型、推理强度与自动审查配置下比较前后执行模型请求数量、
工具数量、重试、推理 tokens 和总耗时，只记录脱敏计数和时间。
