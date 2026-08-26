# AGENTS.md

## 项目定位

OmicsOps 是一个 Windows-first 的 Tauri 生物信息学桌面工作台，也是面向科研项目的统一控制平面。一个项目应在同一上下文中组织本地计算、远程 SSH 服务器、GPU 主机、运行环境、文献工具、数据资产、分析运行、产物和论文，使研究者能够追踪它们之间的来源、依赖、执行过程和科研结论。长期稳定的产品对象包括 `Project`、`ExecutionContext`、`DataAsset`、`Run`、`Artifact`、`Paper` 和 `Decision`。

不要在一次变更中实现整个产品的愿景。更倾向于进行小规模的功能改进，每次只添加一项可持久使用的抽象、持久化表、工具、用户界面或可测试的行为。

## 仓库布局

- `crates/omicsops-agent-core/`：Agent代理循环、上下文管理、内存处理、数据来源跟踪功能。
- `crates/omicsops-tools/`：包括读写、编辑、搜索、grep 以及 shell 命令等内置工具。
- `crates/omicsops-store/`：使用的是 sqlx 库来操作 SQLite 数据库。迁移代码位于 `crates/omicsops-store/migrations/init.sql` 中；而幂等性的迁移逻辑则存在于 `crates/omicsops-store/src/lib.rs` 中。
- `crates/omicsops-runtime/`：支持管理式运行时环境,持久运行Python/R,并管理这些解释器的生命周期。
- `crates/omicsops-skills/` ：SKILL.md 中的发现功能以及 use_skill 工具。
- `crates/omicsops-dto/` ：为用户界面提供了共享的 serde 序列化对象，同时遵循了 Tauri 的调用/事件契约。该代码可以编译为 wasm32 和原生版本；只包含数据部分，不包含 Leptos/Tauri 的依赖库。 `src/dto.rs` 负责将其重新导出为标准类型，而 `src-tauri/src/dto_contract_tests.rs` 则负责将这些类型反序列化为后端负载，以纠正 serde 序列化方式中的差异。这里还会添加一些新的跨边界对象，而不是简单的手工复制。
- `crates/omicsops-skills/`：OpenAI、Anthropic、DeepSeek、Qwen、MiniMax、Ollama、LM Studio 等不同模型 API统一成一套 `Provider + Message + ToolCall + Completion` 接口。
- `crates/omicsops-mcp/`：MCP 客户端和 MCP Tool 适配层。
- `crates/omicsops-acp/`：连接外部 Agent 的 ACP 协议层。
- `crates/omicsops-cli/`：无 UI 的命令行 Agent 入口和测试入口。
- `crates/omicsops-sync/`：项目加密同步和 Relay 服务，项目跨设备同步。
- `crates/omicsops-path/`：统一寻找软件打包资源路径，统一管理程序资源文件路径。
- `src-tauri/`：桌面 shell、Tauri 命令、应用程序状态、SSH 主机注册表。`src/app_state.rs`拥有`AppState`/`SessionRuntime`/`ActiveProject`; `src/agent_turn.rs`拥有 send_message 转换管道、转换队列和 stop_agent；`lib.rs`保留命令注册、设置和共享助手。
- `src-tauri/src/model_catalog_shared.rs` ： `omicsops`的“模型规格数据库核心逻辑”：负责从 models.dev 提炼模型能力数据，并通过 `provider + API host + 精确 model id` 找到正确的上下文上限、输出上限、视觉能力、推理档位和价格；这些信息再被 Settings、`models.rs`、`omicsops-agent-core` 和 `omicsops-llm` 使用。
- `src/`：React 19 + TypeScript 前端。`src/types.ts` 保存前端类型，`src/tauri-api.ts` 封装 Tauri 调用，`src/features/` 按项目库、设置和工作区拆分界面。
- `skills/`：捆绑式科学工作流程。
- `docs/superpowers/specs/` 与 `docs/superpowers/plans/`：已批准的架构设计和实现计划。行为或边界变化时同步更新相关文档。

## 工程规则

- 明确指定 Windows 和 macOS 的行为。除非通过 SSH/WSL 环境进行控制，否则避免仅假设 Unix 系统的行为。
- 自动化测试中绝不应要求使用真实的 SSH 主机、GPU、SLURM 集群、WSL 发行版、API 密钥或网络访问权限。请使用纯解析测试、模拟命令运行器、临时目录和模拟的 Tauri 命令。
- 将密钥存储在现有的密钥环路径中，而不是 SQLite 数据库中。绝对不能将 SSH 私钥内容复制到 SQLite 数据库中。
- 对于长时间运行的计算，不要扩展现有计算。`shell`以工具超时作为主要解决方案。添加结构化的运行/作业抽象层。
- 对于大型科学数据，不要默认使用本地同步。尽可能将大型数据表示为带有校验和/元数据的远程引用。
- 保持模式向后兼容，迁移幂等，遵循现有规则。
- 模型上下文/输出上限来自已编译的 models.dev 目录，通过精确的模型 ID 匹配（网关）实现。`供应商/型号`ID 匹配基于尾部片段）。切勿重新引入前缀或家族匹配——家族 ID 不得包含更长的同级 ID。
- 不要仅仅因为文件过长就重构或拆分模块。必须有与当前变更相关的具体理由，例如职责混合导致重复编辑、必要的依赖项或测试边界，或者已评估的维护问题，并且一旦问题解决就停止。大型组合/根模块是可以接受的；不要追求任意的行数目标或推测性的抽象。
- 所有可关闭的覆盖层、对话框、菜单和弹出框都必须参与到窗口级的 Escape 堆栈中，并按照视觉上最顶层向下排序。根节点拥有的状态应位于应用程序堆栈中；组件局部状态可以使用作用域窗口监听器，该监听器会在清理时移除。不要依赖 DOM。`按下按键`处理程序接收到冒泡事件或`自动对焦`本地处理程序仅适用于内部状态必须在其父级之前消耗 Escape 键的情况；它必须阻止状态传播。测试必须在打开后立即按下 Escape 键，而无需先将焦点移入内部，并验证一次按下 Escape 键只会关闭最顶层，而其父级仍然保持打开状态。
- 每次行为发生变化时，都要添加或更新测试。
- 当用户可见的行为发生变化时，更新文档。仅在明确要求或准备发布版本时（参见“发布版本”）更新版本说明。
- 如果`cargo fmt --all -- --check`由于格式偏差而失败，请运行`cargo fmt --all`并将仅进行格式更改的更改放在单独的提交中。
- 凭据只保存到现有 Windows Credential Manager/keyring 路径。SQLite、事件、日志、模型上下文、导出包和 Git 中只能保存不泄密的引用或脱敏信息；SSH 私钥内容、密码和 API key 绝不能写入数据库。
- 不要提交 `target/`、`dist/`、`node_modules/`、应用数据库、日志、真实样本、模型凭据、SSH 密码、私钥、服务器敏感输出或由本地开发配置生成的文件。

## 验证命令

先运行与改动最接近的测试，再在交付前运行默认完整检查：

```powershell
cargo test --workspace
npm test
npm run build
```

CI 在 Windows 上使用 Node.js 22，并依次运行 `npm ci`、Rust 工作区测试、前端测试和生产 Web 构建。依赖锁文件发生变化时，使用 `npm ci` 验证干净安装，而不是只依赖已有的 `node_modules`。

涉及 Tauri 配置、桌面组合、打包资源或安装行为时，再运行：

```powershell
npm run build:desktop
```

涉及真实 Agent Runtime V4、SSH、模型、R、Micromamba 或 PBMC 流程时，先通过上述确定性检查，再严格按 `acceptance/README.md` 使用一次性环境显式执行相应 ignored 测试。记录实际运行的命令和结果，并明确区分“通过”“失败”与“未执行/被忽略”。

## PR 期望

每个 PR 应包含：

- 要解决的用户问题和可观察的行为变化。
- 涉及的架构边界、协议或持久化影响，以及新增抽象的理由。
- 已增加或更新的自动化测试和实际执行结果。
- UI、Windows、SSH、MCP 或远端流程改动所需的手工 smoke 步骤。
- 对凭据、审批、隔离、数据驻留、审计和恢复语义的影响。
- 已知限制、明确的后续工作，以及未执行的真实模型或 SSH 验收。

不要把测试替身、成功构建、被忽略的测试或局部 smoke test 描述为生产环境端到端验收。仓库目前没有正式发布流程；除非另有经过评审并落库的发布文档，不要自行创建 tag、GitHub Release 或分发安装包。
