# OmicsOps 本窗口开发交接

> 更新时间：2026-08-11  
> 当前分支：`codex/omicsops-science-workbench`  
> 功能基线提交：`9612c33 fix: add actionable model provider diagnostics`  
> 项目定位：Windows-first、仅桌面端的生命科学研究工作台（Tauri 2 + React + Rust）

## 1. 交接摘要

本窗口已把 OmicsOps 从原有五步远端执行向导推进为项目制桌面工作台，并连续完成阶段 1 的主要桌面壳层、阶段 2 的模型与 V2 执行链路、阶段 3 的安全选择性同步、阶段 4 的科研技能与文献源底座、阶段 5 的远端 Python/R 探索内核，以及 SSH、项目计算位置提示和模型测试诊断修复。

当前代码可以构建 Windows EXE 和 NSIS 安装包，但完整产品验收仍未结束。下一窗口应优先补齐真实 PBMC 端到端验收、科学文件预览与 Notebook、stdio MCP 运行时、专家真实调度和 Windows 安装升级矩阵。

## 2. 已完成任务

### 2.1 桌面壳层与项目体验

- 新生产入口使用 `DesktopApp`，提供项目库、科研工作台、统一设置和三栏布局。
- 支持空白、scRNA-seq、bulk RNA-seq、文献综述项目入口。
- 默认中文，可切换英文。
- 工作台已增加返回项目主页面的回溯按钮。
- 新建项目明确区分：
  - 仅本地工作区。
  - 本地工作区 + 远端 Linux 计算。
- 创建远端项目时展示连接名称、`用户@主机:端口`、本地目录和远端项目目录，避免用户无法判断计算位置。
- 连接、模型、技能/MCP、隐私和历史入口已集中到工作台设置。

相关提交：

- `91a31f7 feat: establish life science desktop workbench`
- `3348d24 fix: add project workspace back navigation`
- `a9a780e feat: clarify project storage and compute targets`

### 2.2 数据模型、持久化与 Agent 编排

- 增加 schema v3 工作台实体和读写接口，包括项目、会话、消息、Agent 回合、审批、产物、Notebook、技能、模型和同步记录。
- 增加 `omicsops-agent` 编排 crate，包含 Agent 会话、工具注册、审批门、专家调度契约和探索代码正式化接口。
- 正式任务继续复用 `AnalysisPlanV2`、`PolicyEnvelope`、V2 验证器、审批与执行器。
- 对话、Agent、内核和产物使用稳定事件桥接。
- 模型凭据和 SSH 凭据只保存到 Windows Credential Manager；数据库只保存引用。

### 2.3 模型提供方与对话执行

- 已实现 Anthropic、OpenAI-compatible、Ollama 三类适配。
- 已实现 SSE/NDJSON 流解析、文本增量、工具参数增量和结束事件规范化。
- 对话消息和 Agent 回合可持久化，模型生成的 V2 计划进入验证、展示和显式审批流程。
- 审批后可进入现有真实 SSH V2 运行入口。
- Ollama-only 项目在 Agent 回合与计划生成入口已有策略检查。
- 模型设置支持编辑、测试、测试中状态、成功信息和可操作的失败信息。
- Base URL 同时支持基础域名、`/v1`、`/v1/` 和 Ollama `/api`，避免重复拼接路径。
- 模型测试改为轻量非流式请求，并显示实际端点、模型、延迟和响应摘要。
- 增加“可用模型”查询；点击网关返回的模型可带入编辑表单，空密钥保存不会覆盖已有凭据。

本窗口真实诊断结论：

- 用户保存的模型名 `gpt-5.6Luna` 被其 OpenAI-compatible 网关以 `503 model_not_found` 拒绝，这不是密钥读取失败。
- 同一 Base URL 和同一凭据使用网关列出的 `gpt-5.6-terra` 已完成最小真实请求并通过。
- 网关当时返回的可用模型包括：`codex-auto-review`、`gpt-5.4`、`gpt-5.4-openai-compact`、`gpt-5.5`、`gpt-5.5-openai-compact`、`gpt-5.6-sol`、`gpt-5.6-terra`。
- 应用不会静默替换用户模型；用户仍需在设置中选择可用模型并保存。
- 实际 API 密钥未输出、未写入测试文件、未提交到 Git。

相关提交：

- `2479fee feat: connect model streaming to approved V2 runs`
- `9612c33 fix: add actionable model provider diagnostics`

### 2.4 远程 SSH 连接

- 已完善密码和私钥认证配置，并统一使用不透明凭据引用。
- 首次连接先探测主机指纹，不发送认证秘密；用户确认指纹后才执行认证诊断。
- 诊断结果展示延迟、Linux 信息、远端用户、HOME、SFTP、Python 和 R 可用性。
- 主机、端口或用户名变化会清除旧主机信任，防止错误沿用指纹。
- 项目只能绑定已存在的连接和绝对 Linux 远端路径。
- 本窗口使用用户提供的测试服务器完善过该链路，但用户名、密码等信息不得写入交接文件或仓库；继续从 Windows Credential Manager 使用已保存凭据。

相关提交：

- `79c6d72 feat: harden remote SSH connection setup`

### 2.5 文件同步与桌面生产构建

- 仅上传用户显式选择的文件，不扫描或隐式上传整个本地工作区。
- 已实现远端目录索引，并排除 `.omicsops` 控制目录。
- SFTP 上传后比较本地/远端 SHA-256，并写入 schema v3 同步记录。
- 下载使用临时文件、流式哈希校验和原子重命名。
- 本地存在不同内容时生成 `.conflict-N` 并列版本，不静默覆盖。
- 下载路径检查项目根目录、路径穿越和符号链接边界。
- 生产 Tauri 配置只嵌入 `dist`；开发地址仅位于 `tauri.dev.conf.json`。
- 构建后的 EXE 不再访问 `http://localhost:1420`，应用不为 UI 启动本地 HTTP 服务。

相关提交：

- `a8d58a2 feat: add verified selective project sync`

### 2.6 科研技能与文献数据源

- 内置并版本锁定 `scrna-qc`、`bulk-rnaseq-de`、`literature-review` 核心技能。
- 支持导入 Agent Skills 兼容目录，导入后默认禁用，需用户显式启用。
- 校验 `SKILL.md`、名称、版本、能力白名单、文件数量、包大小、路径穿越和逃逸符号链接。
- 技能包使用完整内容 SHA-256 和内容寻址目录；安装后再次校验。
- 同名技能仅允许启用一个版本。
- PubMed、Europe PMC、Crossref 已有统一检索记录、解析、分页、限流和本地缓存底座。

相关提交：

- `2fb7343 feat: add secure research skills and sources`

### 2.7 Python/R 探索会话

- 已实现项目级 Python/R `KernelSession` 状态机和远端 JSONL 驱动。
- 支持启动、执行、流式 stdout/stderr、中断、停止和产物捕获。
- 应用重启后将孤立运行会话标记为 `interrupted`。
- 仅允许从显式保存的代码单元重建；临时代码不进入恢复链路。
- 保存代码单元计算 SHA-256，并可生成版本化正式步骤提案。
- 正式化提案仍需进入主计划审批。
- 每项目限制最多三个活动内核，探索执行使用项目级信号量排队。
- 已定义文献、统计和代码审查专家结果契约与最多三个并行专家任务限制。

相关提交：

- `3d24ff3 feat: add persistent remote exploration kernels`

## 3. 未完成任务

### P0：下一窗口优先完成

1. **PBMC 真实端到端验收**
   - 使用 WSL SSH fixture 或隔离测试服务器完成：创建项目 → 上传/远端数据准备 → 对话生成计划 → 审批 → V2 正式运行 → UMAP/表格产物登记与预览 → 重启恢复。
   - 当前模型、计划、审批和 SSH 入口已接通，但尚无一次完整可复现的 PBMC 桌面闭环验收记录。

2. **科学文件真实预览器**
   - 补齐 PDF、CSV/TSV、Jupyter、H5AD 和 Seurat 元数据摘要。
   - Markdown、代码、图片、HTML、JSON、日志等也需确认由真实远端产物驱动，而不是示例数据。
   - `RemoteFileTree.tsx` 仍保留无远端数据时的 `demoFiles` 回退，应在生产闭环完成后移除或改成明确空状态。

3. **Notebook 自动记录与导出**
   - 自动收集假设、方法、观察、决策、证据、代码和产物链接。
   - 实现 Markdown、JSON 和项目打包导出 UI。

4. **stdio MCP 完整运行时**
   - 子进程生命周期。
   - JSON-RPC 收发。
   - 能力协商。
   - 每次工具调用审批。
   - 审计持久化与异常退出恢复。

### P1：核心功能补齐

5. **科研检索 UI**
   - 把 PubMed、Europe PMC、Crossref 结果卡片、来源信息和引用选择器接入对话与 Notebook。
   - 完成带可追溯引用的文献综述最终验收。

6. **专家真实协作**
   - 将文献、统计、代码审查专家接入真实模型请求。
   - 持久化专家任务和报告，并在主对话展示。
   - 保证专家写入、执行和远端提交仍回到主会话审批门。

7. **统一项目资源队列**
   - 当前探索内核已有项目级限制；正式 V2 运行还需与探索会话共享同一项目资源队列。

8. **同步任务管理**
   - 增加远端传输队列的暂停、取消、失败重试和断线续传。
   - 大文件保持按需下载，不引入全目录双向镜像。

9. **历史视图与旧入口清理**
   - 把 V1/V2 历史运行、计划、日志、审计的查看和导出完整嵌入新桌面壳层。
   - 删除旧向导的新任务入口。
   - 历史导出和迁移全部验收后，再清理旧命令兼容层。

10. **连接管理补充**
    - 增加删除连接流程。
    - 删除前展示受影响项目，并要求显式解绑确认。

### P2：Windows 发布硬化

11. **隐私与策略**
    - 增加脱敏诊断包导出。
    - 补全数据发送边界提示。
    - 对 Ollama-only 策略做全入口回归测试，而不仅是 Agent/计划入口。

12. **安装、升级与恢复矩阵**
    - 验证 NSIS/MSI 安装与升级、WebView2 缺失/旧版本、Credential Manager、schema 备份和应用重启恢复。
    - 当前只成功构建了 NSIS；尚未形成完整人工/自动验收矩阵。

13. **真实提供方覆盖**
    - OpenAI-compatible 已完成一次真实成功测试。
    - Anthropic 和 Ollama 仍需显式 opt-in 的真实连通测试。
    - 不应把真实网络和密钥测试加入默认 CI。

## 4. 当前验证结果

本窗口最后一次验证：

- `npm test`：7 个测试文件、24 项测试全部通过。
- `npm run build`：TypeScript 与 Vite 生产构建通过。
- `cargo test -p omicsops-adapters --test model_provider_contracts`：6 项通过。
- `cargo test -p omicsops-desktop --test command_contracts`：19 项通过。
- 真实 OpenAI-compatible 测试：
  - 保存的无效模型名可正确显示 `model_not_found` 和可用模型列表。
  - 临时覆盖为 `gpt-5.6-terra` 后测试通过。
- `npm run build:desktop`：通过。

尚需下一窗口补跑：

- `cargo test --workspace`
- WSL SSH fixture 全套 opt-in 测试
- PBMC 端到端验收
- Python/R 内核断线重建端到端测试
- Windows 安装、升级和重启恢复矩阵

## 5. 构建产物

- 独立 EXE：`E:\Project\OmicsOps\target\release\omicsops-desktop.exe`
- NSIS 安装包：`E:\Project\OmicsOps\target\release\bundle\nsis\OmicsOps_0.1.0_x64-setup.exe`
- NSIS SHA-256：`8FB266D77C1EE82A18617F3DF01FDB74971D1BC42BFF36D2842DBEB8E3A3C4BA`

构建时间为 2026-08-11，继续开发后应重新运行 `npm run build:desktop`，不要把旧安装包视为最新结果。

## 6. Git 状态与提交顺序

当前功能提交顺序：

1. `91a31f7` — 桌面生命科学工作台基础
2. `7da5bc0` — 桌面工作台实施计划
3. `2479fee` — 模型流、会话、V2 计划与运行连接
4. `a8d58a2` — 选择性同步与生产 EXE 本地地址修复
5. `3348d24` — 返回项目主页按钮
6. `2fb7343` — 科研技能与科研数据源
7. `3d24ff3` — 远端 Python/R 探索会话
8. `79c6d72` — SSH 连接安全与诊断完善
9. `a9a780e` — 新建项目计算位置与服务器信息说明
10. `9612c33` — 模型测试反馈、端点规范化和可用模型发现

工作区注意事项：

- `.gitignore` 仍有一项用户自己的未提交修改；不要覆盖、还原或误提交。
- 本交接文件创建后会成为新的未提交文件，除非当前窗口随后明确提交。
- 不要提交 `target/`、`dist/`、应用数据库、Windows 凭据或真实服务器密码。

## 7. 重要文件入口

- 总体实施计划：`docs/2026-08-11-omicsops-life-science-desktop-workbench.md`
- 桌面入口：`src/DesktopApp.tsx`
- 项目库：`src/features/projects/ProjectLibrary.tsx`
- 三栏工作台：`src/features/workspace/WorkspaceShell.tsx`
- 设置页：`src/features/settings/SettingsPanel.tsx`
- Tauri 前端 API：`src/tauri-api.ts`
- 模型适配器：`crates/omicsops-adapters/src/llm.rs`
- 模型命令：`src-tauri/src/model_commands.rs`
- SSH 与同步：`crates/omicsops-adapters/src/ssh.rs`、`src-tauri/src/sync_commands.rs`
- 科研源与技能：`crates/omicsops-adapters/src/research.rs`、`crates/omicsops-adapters/src/skills.rs`
- 探索内核：`crates/omicsops-adapters/src/kernel.rs`、`src-tauri/src/kernel_commands.rs`
- V2 运行与验收：`src-tauri/src/commands.rs`、`acceptance/README.md`

## 8. 下一窗口建议执行顺序

1. 先读取本交接文件和总体实施计划，检查 `git status`，保留 `.gitignore` 用户修改。
2. 运行 `cargo test --workspace`、`npm test` 和 `npm run build` 建立新基线。
3. 优先完成科学文件预览器与 Notebook，因为它们是 PBMC 闭环可见结果和方法记录的关键缺口。
4. 使用显式 opt-in 的 SSH fixture 完成 PBMC 与 Python/R 探索端到端验收。
5. 接着实现 stdio MCP 运行时和专家真实模型调度。
6. 最后进行 Windows 安装升级、诊断包、历史兼容清理和发布硬化。

## 9. 安全交接要求

- 不要在 Markdown、日志、测试快照、提交信息或终端输出中记录真实密码/API key。
- 真实模型和 SSH 测试必须由用户显式触发，默认测试使用固定响应或 fixture。
- 不要自动更改用户保存的模型；应列出网关模型并让用户确认保存。
- 不允许隐式全目录上传，也不允许下载时静默覆盖本地文件。
- 远端写入、执行和专家建议必须继续经过主会话策略与审批链。
