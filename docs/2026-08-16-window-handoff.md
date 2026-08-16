# OmicsOps Skills/MCP、单细胞工作流与工作区交互交接

> 更新时间：2026-08-16
> 当前分支：`codex/omicsops-science-workbench`
> 本窗口功能基线 HEAD：`7071be8 feat: render remote files as explorer tree`
> 上一份交接：`docs/2026-08-13-window-handoff.md`
> 项目定位：Windows-first、仅桌面端的生命科学研究工作台（Tauri 2 + React + Rust）

## 1. 交接摘要

本窗口在 2026-08-13 的 P1 工作台基础上，主要完成了五组工作：

1. 为设置页补齐可持久化、可检查、可逐工具授权的 stdio MCP 管理界面和 Tauri API。
2. 将临时自写的单细胞 Skills 全量替换为固定 GitHub commit 的高星开源 Skills，并确保启用后的 Skill、引用资料、脚本和依赖可进入 Agent 上下文。
3. 为 Skills 增加组学分类，当前单细胞内容统一收进“组学技能 → 单细胞组学”目录。
4. 增加会话删除和最近项目删除；项目删除只清理 OmicsOps 应用记录，绝不删除本地或远端科研文件。
5. 将远端文件平铺列表改成类似 VS Code Explorer 的可折叠目录树，同时保留完整路径下载、上传、刷新和同步状态。

此外，工作区中存在一组尚未提交的 PBMC/scRNA-seq v2 验收工作流增强，已经通过 Rust 静态契约测试，但尚未完成真实 SSH 端到端验收。下一窗口必须先区分“已提交产品功能”和“未提交验收 workflow”，不要把后者误描述为已经正式交付。

## 2. 本窗口提交

按时间顺序：

- `368ba91 feat: add MCP controls and bundled single-cell skills`
  - 增加 MCP server 配置、检查、启停和逐工具授权界面及后端接口。
  - 建立应用自带单细胞 Skill bundle 和启动安装流程。
- `69285a8 fix: source single-cell skills from K-Dense`
  - 删除上一提交中临时自写的单细胞 Skills。
  - 替换为 K-Dense-AI 固定 commit 的高星开源 Skills 快照。
- `ba61d19 feat: group omics skills and delete conversations`
  - 增加 Skill 分类字段和“组学技能 → 单细胞组学”分级展示。
  - 增加会话删除按钮、事务级清理和前端会话切换。
- `ece5308 feat: add recent project deletion`
  - 增加最近项目删除按钮、双语确认、活动任务保护和项目记录级联清理。
- `7071be8 feat: render remote files as explorer tree`
  - 将远端文件列表改成可展开/折叠的层级目录树。

以上五个提交均在当前分支，最新两个提交已经分别完成独立提交，没有混入 PBMC workflow 的未提交修改。

## 3. MCP 设置接口

### 3.1 当前已实现

设置页“技能与 MCP”现在支持：

- 保存本地 stdio MCP server：名称、启动命令和逐行参数。
- 编辑既有 server；保存配置本身不会启动任何进程。
- 在已打开项目上下文中显式批准一次 server 检查。
- 检查过程只执行：
  1. 启动 stdio 子进程。
  2. `initialize`。
  3. `notifications/initialized`。
  4. `tools/list`。
- 检查完成后显示 server 公告的工具名称与描述。
- server 检查成功后才允许启用。
- 每个工具单独批准或撤销调用权限。
- server 启用状态与工具批准清单分别持久化。
- 修改 command/args 或重新检查工具声明时，会撤销旧的工具授权，避免声明漂移后继续沿用权限。
- 实际调用要求同时满足：server 已启用、工具仍在公告列表、工具已批准。

持久化使用 `app_objects(kind='mcp_server')`；每次运行仍写入 `mcp_audit`。底层维持 30 秒请求超时、检查最多重试一次、工具调用不自动重放的约束。

### 3.2 重要安全边界

- server 默认停用。
- 保存配置不等于批准启动。
- 检查批准为一次性 UI 状态，检查结束后复位。
- 未明确批准时 MCP 子进程必须在启动前被拒绝。
- 目前只支持短生命周期 stdio server，不是持久进程池。
- 仍未支持 MCP resources、prompts、并发请求、长连接通知、OAuth/远端 transport 和完整 stderr 诊断。

### 3.3 已知文案不一致

`skills/single-cell/BUNDLE.json` 当前会默认启用 5 个打包 Skill，但设置页底部仍显示“技能与 MCP 均默认不启用”。MCP 默认停用是正确的，Skills 文案与实际 bundle 默认值不一致，下一窗口应修正文案或重新确认产品策略。

## 4. 高星 GitHub 单细胞 Skills

### 4.1 来源与固定快照

当前 bundle 不再使用本项目临时自写的 Skill。来源记录在 `skills/single-cell/SOURCE.json`：

- Repository：`https://github.com/K-Dense-AI/scientific-agent-skills`
- 固定 commit：`13385c7c4db02fdcc84a020752c07cce91ef780e`
- License：MIT，许可证副本为 `skills/single-cell/LICENSE.upstream.md`
- 观测 stars：33460
- 观测/下载日期：2026-08-14
- 安装方式：OpenAI `skill-installer` 的 GitHub 安装脚本

打包的 9 个 Skills：

1. `anndata`
2. `arboreto`
3. `cellxgene-census`
4. `pathway-enrichment`
5. `scanpy`
6. `scientific-visualization`
7. `scvelo`
8. `scvi-tools`
9. `statistical-analysis`

默认启用：`anndata`、`cellxgene-census`、`pathway-enrichment`、`scanpy`、`scientific-visualization`。

### 4.2 安装和 Agent 可见性

- Tauri 打包资源路径为 `skills/single-cell/`。
- 应用启动时把固定 bundle 安装到应用数据目录的 `skills` 根目录。
- 每个包按内容计算 SHA-256 并持久化版本、来源、启用状态和分类。
- 旧的自写 `bio-single-cell-*` 和 `bio-workflows-scrnaseq-pipeline` 包在 bundle 的 `replaces` 清单中，会被退休，避免同名/重复能力并存。
- Agent 上下文不只读取根 `SKILL.md`，还会读取 Skill 声明涉及的 references、scripts、assets、示例和依赖说明。
- 计划与每轮远端 Agent 动作继续保留 Skill 包哈希和章节引用审计。
- Agent system contract 明确要求根据真实数据和软件环境生成任务专用代码；打包脚本是可调整示例，不能因为存在 PBMC 示例就调用写死流程。

### 4.3 组学分区

`SkillPackage` 新增可选 `category`。当前 `BUNDLE.json` 将全部 9 个包归入 `single_cell`；设置页呈现：

```text
组学技能
└─ 单细胞组学
   ├─ anndata
   ├─ arboreto
   ├─ cellxgene-census
   └─ ...
```

没有分类的用户导入 Skill 继续显示在“其他科研 Skills”，不强制塞入单细胞目录。后续扩展空间转录组、Bulk RNA、蛋白组等时应通过 bundle/category 数据驱动增加分区，不要在 React 中逐个硬编码包名。

## 5. 会话与项目删除

### 5.1 会话删除

- 左侧会话列表每项有独立垃圾桶按钮，不与选择会话按钮嵌套。
- 删除前显示不可撤销确认。
- 当前 Agent 或当前会话运行活跃时禁止删除。
- 后端按 `project_id + conversation_id` 校验归属，并在事务中清理：
  - Agent events。
  - Tool calls。
  - Agent turns。
  - Messages。
  - Conversation。
- 删除非当前会话不会打断当前界面；删除当前会话后切换到下一条，没有剩余会话时自动创建空会话。

### 5.2 最近项目删除

- 最近项目卡片右侧有独立垃圾桶按钮。
- 双语确认文案明确说明：会话、运行、Notebook 和索引会被删除；本地目录和远端文件不会删除。
- 删除成功后立即从最近项目状态中移除卡片。
- 后端删除前拒绝以下活动状态：
  - Queued/Streaming/WaitingForApproval Agent turn。
  - 活动正式运行。
  - 活动 Kernel。
  - 活动同步传输。
- 项目事务会清理项目关联的 conversations/messages/turns/events/tool calls、approvals、artifacts、Notebook、sync entries、memory、Kernel session、run checkpoint、运行审计、step attempts、environment locks 和项目专属 plan 对象。
- 模型配置、连接、Skills、MCP server 等全局设置保留。
- 删除实现不调用文件系统删除 API；测试明确验证普通科研文件和 `.omicsops/project.json` 仍存在。

产品语义必须保持为“从 OmicsOps 删除项目记录”，不是“删除磁盘项目”。如未来要增加真正删除本地目录的能力，必须作为新的高风险流程单独设计，不能复用当前按钮悄悄扩大范围。

## 6. 远端文件 Explorer 树

远端文件面板已从完整路径平铺列表改成类似 VS Code 的目录树：

- 前端从 `RemoteFileEntry.relative_path` 构造层级节点，不改变后端远端扫描接口。
- 即使后端只返回文件、不显式返回父目录，前端也会补齐缺失父目录。
- 目录默认折叠，支持逐级展开/收起。
- 行内只显示当前层 basename；完整相对路径保留在 `title` 和下载参数中。
- 目录优先、同类按名称自然排序。
- Python/R/脚本、JSON/YAML、表格、图片和普通文档使用不同图标颜色。
- 文件大小继续显示；下载按钮在 hover/focus 时出现。
- 上传、刷新、同步任务、暂停/取消/重试能力没有改变。
- 使用 `role=tree/treeitem/group` 和 `aria-expanded/aria-level` 保留键盘与可访问性语义。

测试覆盖折叠初态、父目录补齐、嵌套展开、折叠隐藏和使用完整相对路径下载。

## 7. PBMC/scRNA-seq v2 验收工作流：未提交状态

以下文件当前仍在工作区，**不在 HEAD 中**：

- `crates/omicsops-runner/tests/workflow_assets.rs`
- `workflows/scrna-pbmc/analyze_pbmc.py`
- `workflows/scrna-pbmc/convert_h5ad_to_seurat.R`
- `workflows/scrna-pbmc/environment.yml`
- `workflows/scrna-pbmc/render_report.py`

### 7.1 已写入但未正式交付的增强

- 支持通过 `OMICSOPS_PROJECT_ROOT`、`OMICSOPS_PBMC_INPUT`、`OMICSOPS_PBMC_RESULTS` 指定隔离输入和输出，减少写死路径。
- 校验 10x `matrix.mtx/genes.tsv/barcodes.tsv` 存在且非空，并记录输入 SHA-256。
- 原始 counts 保存在 `layers['counts']`。
- 增加线粒体、核糖体和血红蛋白 QC 指标。
- 在任何细胞过滤前运行 Scrublet，保存 doublet score、预测结果、阈值和前过滤 QC 图。
- 使用 QC mask 与 doublet mask 的统一过滤，并导出全部细胞 QC 表。
- 对 filtered-only PBMC3k 数据显式记录 ambient RNA 限制：缺少 empty droplets 时不伪装执行 SoupX/DecontX，报告为 `not_applied_filtered_matrix_only`。
- 使用 Wilcoxon markers、cluster-level canonical marker scoring 和 score margin 做 cluster 注释；低置信度标为 `Uncertain (...)`。
- marker 集新增 Classical monocytes、FCGR3A monocytes、Dendritic cells 和 Platelets。
- 导出 cluster markers、cluster annotations、cell metadata 和 marker specificity 表。
- 新增前过滤 QC、doublet scatter、后过滤 violin、UMAP 和 marker dotplot。
- `qc-summary.json` 记录软件版本、随机种子、阈值、过滤计数、doublet 证据、ambient RNA 限制和注释置信度。
- h5ad → Seurat RDS 转换增强 PCA/UMAP dense embedding 的方向与细胞数校验。
- HTML report 展示嵌套 QC 元数据和新增图表。
- 环境新增 `nodefaults`、Scrublet 和明确 HDF5 版本约束。

### 7.2 当前验证边界

- `cargo test --workspace` 在这些工作区文件存在时通过，其中 `workflow_assets.rs` 的 4 项契约测试通过。
- 尚未确认在真实目标 SSH 主机上完成环境创建、下载、Scanpy、Scrublet、Seurat round-trip、图表和最终报告全流程。
- 没有证据表明真实 PBMC 端到端验收已经完成；不能仅凭静态字符串测试称为“完整验收通过”。
- 提交前应对脚本做真实隔离运行，核对 Scrublet 阈值、过滤后细胞数、cluster 注释合理性、Seurat reductions、产物哈希和报告引用。

## 8. 验证与构建状态

本窗口完成的验证：

- `cargo test --workspace`：全部本地 Rust 测试通过。
  - 显式依赖真实网络、SSH、WSL fixture 或真实模型的测试按既有配置忽略。
- `npm test -- --run`：8 个测试文件、46 项前端测试全部通过。
- `npm run build`：TypeScript 与 Vite 生产构建通过。
- `npm run build:desktop`：Tauri release 和 NSIS 安装包构建通过。
- `git diff --check`：通过，仅出现 Windows LF/CRLF 提示。

最新构建产物：

- EXE：`E:\Project\OmicsOps\target\release\omicsops-desktop.exe`
- EXE SHA-256：`8195C87E74393316E17BF0136A61D360A3BFAE3B3FDA4B1D99E9B687F1E54C92`
- NSIS：`E:\Project\OmicsOps\target\release\bundle\nsis\OmicsOps_0.1.0_x64-setup.exe`
- NSIS SHA-256：`91A0A58370A5B9C53B6127ACE1DF7443C955524030051BAF067510477F0BAB26`

重要：`omicsops-runner/src/scrna.rs` 通过 `include_str!` 编译嵌入 PBMC workflow，因此上述 release 是从含未提交 workflow 的脏工作树构建的测试产物，不能只 checkout `7071be8` 就得到相同二进制。提交/还原 workflow 后必须重新构建并更新哈希。

## 9. 当前 Git 状态与保护事项

当前 HEAD：

```text
7071be8 feat: render remote files as explorer tree
ece5308 feat: add recent project deletion
ba61d19 feat: group omics skills and delete conversations
69285a8 fix: source single-cell skills from K-Dense
368ba91 feat: add MCP controls and bundled single-cell skills
```

当前未提交状态：

- `.gitignore`：用户修改，增加忽略旧交接文档；不要覆盖或擅自提交。
- `acceptance/README.md`：`git status` 显示 modified，但当前 `git hash-object` 与 HEAD blob 相同，属于行尾/工作树状态异常；不要为了“清理状态”重写或 normalize。
- `crates/omicsops-runner/tests/workflow_assets.rs`：PBMC v2 未提交测试修改。
- `workflows/scrna-pbmc/analyze_pbmc.py`：PBMC v2 未提交分析脚本。
- `workflows/scrna-pbmc/convert_h5ad_to_seurat.R`：未提交 Seurat 转换增强。
- `workflows/scrna-pbmc/environment.yml`：未提交依赖增强。
- `workflows/scrna-pbmc/render_report.py`：未提交报告增强。
- `docs/2026-08-16-window-handoff.md`：本交接文档创建后为新文件，除非用户明确要求，否则不要自动提交。

不要提交 `target/`、`dist/`、应用数据库、API key、SSH 密码、私钥、真实样本或服务器敏感输出。

## 10. 重要文件入口

- 上一份交接：`docs/2026-08-13-window-handoff.md`
- GitHub Skill 来源与固定 commit：`skills/single-cell/SOURCE.json`
- Skill 默认启用、分类和替换清单：`skills/single-cell/BUNDLE.json`
- Skill 安装、分类和 Agent 上下文：`src-tauri/src/skill_commands.rs`
- MCP profile、授权与 stdio runtime：`src-tauri/src/p1_commands.rs`
- MCP/Skills 设置 UI：`src/features/settings/SettingsPanel.tsx`
- Agent 读取 Skill 和生成计划：`src-tauri/src/agent_commands.rs`
- 远端 Agent 动态代码生成约束：`src-tauri/src/commands.rs`
- 会话/项目命令与活动保护：`src-tauri/src/workspace_commands.rs`
- 项目级事务清理：`crates/omicsops-adapters/src/persistence.rs`
- 左侧会话删除 UI：`src/features/workspace/WorkspaceShell.tsx`
- 最近项目删除 UI：`src/features/projects/ProjectLibrary.tsx`
- 远端 Explorer 树：`src/features/workspace/RemoteFileTree.tsx`
- PBMC workflow：`workflows/scrna-pbmc/`
- 编译嵌入 PBMC workflow：`crates/omicsops-runner/src/scrna.rs`

## 11. 下一窗口建议执行顺序

1. 阅读本文件和 `docs/2026-08-13-window-handoff.md`。
2. 执行 `git status` 和 `git diff`，保护 `.gitignore` 与 PBMC workflow 的未提交修改。
3. 决定 PBMC v2 workflow 的归属：继续真实验收并独立提交，或明确保留为实验分支；不要与无关 UI 修改混提交。
4. 在隔离目录和可信 SSH 主机完成 PBMC v2 真实运行：
   - 输入 SHA-256。
   - Scrublet before-filter 证据。
   - QC/doublet unified mask。
   - cluster markers/annotation/specificity。
   - h5ad 与 Seurat RDS reductions。
   - HTML report 和全部图表。
5. 验证成功运行后的 Artifact、Notebook、MemoryFact 和重新进入会话后的历史恢复。
6. 对 MCP 选择无副作用 fixture 做真实配置 → 检查 → 启用 → 逐工具授权 → 调用，并验证命令变更撤权。
7. 修复“Skills 默认启用”与设置页底部文案不一致。
8. 对远端 Explorer 做真实大目录性能测试；必要时增加虚拟化或懒加载，但不能改变完整路径下载语义。
9. 若 PBMC workflow 提交或改动，重新运行 `cargo test --workspace`、`npm test -- --run`、`npm run build` 和 `npm run build:desktop`。
10. 对干净 HEAD 重新生成 EXE/NSIS 并记录新哈希，避免继续发布脏工作树构建。

## 12. 安全与产品语义

- Skills 只提供可审计的方法、依赖和代码示例；所有分析代码必须由模型结合当前数据动态生成，不能把特定 PBMC 操作写死为用户任务的唯一流程。
- GitHub Skill 必须保留 repository、固定 commit、license、bundle hash 和替换清单，不能静默跟随上游最新版。
- MCP 保存、检查、启用和逐工具批准是四个不同状态；不得合并成“配置后自动可调用”。
- 删除会话会清理应用内对话上下文；删除项目会清理应用内项目记录，但二者都不得删除本地或远端科研文件。
- 活动运行、Kernel、Agent turn 或同步期间不得删除所属项目。
- 远端文件树是已有选择性同步的展示层；不得因为改成目录树就隐式下载、递归同步或扩大远端访问范围。
- PBMC filtered-only 数据缺少 empty droplets 时，必须如实记录 ambient RNA correction 未执行，不能伪造 SoupX/DecontX 已完成。
- 最新安装包是测试构建，不是签名生产发行版；尤其要注意它嵌入了尚未提交的 PBMC workflow。
