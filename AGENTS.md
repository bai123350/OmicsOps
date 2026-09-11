# AGENTS.md

## 项目定位

OmicsOps 是一个 Windows-first 的 Tauri 生物信息学桌面工作台，主要入口是
面向科研工作的对话式 Agent。它把研究目标、环境、计算、文献、Skills、MCP
工具和可核验的证据放在同一个项目上下文中。Python/R 可以在本机运行，也可以
通过 SSH 在远程主机运行；本地和 SSH 执行上下文同等重要。

一次对话应围绕研究目标澄清、环境检查、执行、验证和证据整理展开。项目中的
`Project`、`ExecutionContext`、`DataAsset`、`Run`、`Artifact`、`Paper` 和
`Decision` 是长期设计词汇，用于描述来源、依赖、运行过程和科研结论；它们不
要求每次改动都新增持久化表或完整实现全部愿景。

不要在一次变更中实现整个产品愿景。优先交付一个小而可持久使用的抽象、持久化
行为、工具、界面或可测试行为。

## 仓库布局

当前 workspace 的 crate 为：

- `crates/omicsops-core/`：领域模型、项目工作区、路径规则、脱敏和
  `src/sync.rs` 中的项目同步选择逻辑。
- `crates/omicsops-agent/`：Agent 与 provider 的基础消息、工具调用和完成
  契约；provider 定义在 `src/provider.rs`。
- `crates/omicsops-agent-core/`：Agent 编排、上下文视图、进度、计划、委派、
  验证和证据接口。
- `crates/omicsops-adapters/`：凭据、文档、内核、SSH、研究来源、Skills 和
  LLM 适配；provider 请求实现位于 `src/llm.rs`。
- `crates/omicsops-browser/`：受控浏览器桥接。
- `crates/omicsops-dto/`：供原生端和 Web 前端共享的纯数据 serde DTO，不依赖
  Tauri、UI、数据库或异步运行时。
- `crates/omicsops-knowledge/`：Skills、Memory 和 MCP 工具索引、搜索、冻结
  与授权。
- `crates/omicsops-mcp/`：按项目隔离的 stdio MCP 客户端运行时。
- `crates/omicsops-process/`：跨平台子进程启动策略。
- `crates/omicsops-protocol/`：Agent、运行时、工具、计划、事件和证据的数据
  契约。
- `crates/omicsops-runtime/`：本地/容器内核和运行时作业管理。
- `crates/omicsops-science/`：数据集、分析、产物、证据和 provenance 状态。
- `crates/omicsops-store/`：通过 sqlx 管理 SQLite；初始模式位于
  `crates/omicsops-store/migrations/init.sql`，幂等迁移逻辑位于该 crate 的 `src/lib.rs`。
- `crates/omicsops-tools/`：内置读写、编辑、搜索、grep 和 shell 工具。

`skills`、`acp`、`cli`、`sync`、`path` 和 `llm` 不是当前 workspace 的独立
crate。捆绑的 Skills 位于根目录 `skills/`，同步逻辑在
`crates/omicsops-core/src/sync.rs`；不要按旧目录名新增 crate。

桌面端位于 `src-tauri/`：`src-tauri/src/commands.rs` 定义 `AppState`、连接
和环境检查等命令；`src-tauri/src/agent_v4.rs` 负责桌面 Agent V4 的组合；
`src-tauri/src/dto.rs` 重新导出 DTO；`src-tauri/src/model_catalog_shared.rs`
提供模型目录共享逻辑。跨 UI 边界对象加入 `omicsops-dto`，并在
`src-tauri/src/dto_contract_tests.rs` 校验序列化契约，不手工复制另一套类型。
前端位于 `src/`，使用 React 19 + TypeScript；`src/types.ts` 保存前端类型，
`src/tauri-api.ts` 封装调用，`src/features/` 按项目、设置和工作区组织界面。
`docs/superpowers/specs/` 与 `docs/superpowers/plans/` 保存设计和实施计划，
涉及行为或边界变化时同步维护相关文档。

## 执行与数据边界

- 本地 `process_backend.rs` 只从 system `PATH` 启动 `python`/`Rscript`，且
  当前本地内核只接受 `system` 环境。SSH 执行可选远端 system 环境或项目绑定
  的 Micromamba 环境。探测到解释器只表示可找到可执行文件，不表示项目依赖已
  安装或工作流一定可运行。
- Windows 是首要支持目标；涉及 macOS 时必须明确说明行为，不能把未验证的
  路径或已发布安装包当作保证。除非通过 SSH/WSL 控制，代码和测试不得只假设
  Unix 行为。
- 对大型科学数据不要默认本地同步；优先保存远程引用、校验和与元数据，并清楚
  记录数据位置。不能声称所有数据永远不离开本机：模型、MCP 或外部服务可能
  接收提示词、结果或元数据，产品文档和界面应区分这些传输。
- 独立的一次性后台作业只有 Linux SSH 远端支持断线后的重连；交互内核不支持
  跨应用重启重连。后台作业当前没有自动轮询或取消命令，停止 Agent 也不会取
  消已经在远端运行的计算。

## 工程规则

- 凭据只能存入现有的 Windows Credential Manager/keyring 路径，不能写入
  SQLite。SQLite、事件、日志、模型上下文、导出包和 Git 中只能保存不泄密的
  引用或脱敏信息；SSH 私钥内容、密码和 API key 绝不能写入其中。
- 对长时间运行的计算，不要把延长 `shell` 工具超时作为主要解决方案；复用并
  按需演进结构化 run/job 生命周期，明确派发、查询、取消与恢复语义。连接中断
  或停止本地等待不能直接记为远端计算已取消，不确定的派发不能盲目重跑。
- 宿主负责执行能力与审批裁决；模型建议、Skills、MCP 和子 Agent 不得绕过
  审批、扩大授权或改写已冻结的运行与批准计划。科研结论须有可核验来源，摘要
  不替代原始工具证据，计划或生成的代码不算已执行结果。
- 保持模式向后兼容，迁移必须幂等。行为或边界改变时同步更新相关文档。
- 模型上下文/输出上限来自已编译的 models.dev 目录，按精确协议、HTTPS API
  host/端口和完整 model ID 匹配；OpenRouter 保留供应商前缀。不得恢复模型家族
  或前缀推断，家族 ID 不能匹配更长的同级 ID，未知网关不得推断目录能力。
- 不要仅因文件过长就拆分模块。只有在当前变更造成职责混合、重复编辑、必要
  依赖或明确维护问题时才重构，并在问题解决后停止。
- 所有可关闭的覆盖层、对话框、菜单和弹出框必须进入窗口级 Escape 堆栈，按
  视觉顶层到下层排序。根节点状态放在应用堆栈；组件局部状态可用会清理的作
  用域窗口监听器。不要仅依赖 DOM 冒泡或自动聚焦的局部键盘处理器。只有在内部
  状态必须先于父级消费 Escape 时，才使用局部处理并阻止事件传播。测试必须在打开后
  立即按 Escape、无需先把焦点移入内部，并确认一次按键只关闭最顶层而父级仍
  保持打开。
- 每次行为变化都要新增或更新测试。不要把真实 SSH 主机、GPU、SLURM 集群、
  WSL 发行版、API key 或网络访问作为自动化测试前提；使用纯解析测试、模拟命
  令运行器、临时目录和模拟 Tauri 命令。
- 如果 `cargo fmt --all -- --check` 仅因格式偏差失败，运行 `cargo fmt --all`，
  并将只有格式变化的修改放在单独提交中。
- 不要提交 `target/`、`dist/`、`node_modules/`、应用数据库、日志、真实样本、
  模型凭据、SSH 密码或私钥、服务器敏感输出，以及由本地开发配置生成的文件。

## 验证命令

先运行与改动最接近的检查，交付前运行默认完整检查：

```powershell
cargo test --workspace
npm test
npm run build
```

CI 在 Windows、Node.js 22 上依次运行 `npm ci`、Rust workspace 测试、前端测试
和生产 Web 构建。依赖锁文件发生变化时，必须用 `npm ci` 验证干净安装，而不
能只依赖现有 `node_modules`。

涉及 Tauri 配置、桌面组合、打包资源或安装行为时，再运行：

```powershell
npm run build:desktop
```

涉及真实 Agent Runtime V4、SSH、模型、R、Micromamba 或 PBMC 流程时，先完成
上述确定性检查，再严格按 `acceptance/README.md` 使用一次性环境并显式执行相
应 ignored 测试。记录实际命令和结果，明确区分“通过”“失败”和“未执行/被忽
略”；测试替身、成功构建、被忽略测试或局部 smoke test 都不等于生产端到端验
收。

## PR 期望

每个 PR 应说明：

- 要解决的用户问题和可观察的行为变化。
- 涉及的架构边界、协议或持久化影响，以及新增抽象的理由。
- 新增或更新的自动化测试和实际执行结果。
- UI、Windows、SSH、MCP 或远端流程改动所需的手工 smoke 步骤。
- 对凭据、审批、隔离、数据驻留、审计和恢复语义的影响。
- 已知限制、明确的后续工作，以及未执行的真实模型或 SSH 验收。

仓库目前没有正式发布流程。除非存在经过评审并落库的发布文档，不要自行创
建 tag、GitHub Release 或分发安装包；除非明确要求或准备发布版本，也不要仅为
普通行为改动创建版本说明。
