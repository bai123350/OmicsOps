<div align="center">

<img src="src-tauri/icons/128x128.png" alt="OmicsOps 图标" width="96" height="96" />

# OmicsOps

**面向科研与生物信息学的 AI 桌面工作台**

用自然语言推进研究，在本机或 SSH 服务器运行 Python / R，把数据、文献、计算与证据组织在同一项目中。

[![Release](https://img.shields.io/github/v/release/bai123350/OmicsOps)](https://github.com/bai123350/OmicsOps/releases/latest)
[![CI](https://github.com/bai123350/OmicsOps/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/bai123350/OmicsOps/actions/workflows/ci.yml)
[![License: AGPL-3.0-only](https://img.shields.io/badge/License-AGPL--3.0--only-blue)](LICENSE)

[下载桌面版](https://github.com/bai123350/OmicsOps/releases/latest) · [快速开始](#快速开始) · [使用文档](#文档与开发) · [问题反馈](https://github.com/bai123350/OmicsOps/issues)

</div>

## 把研究过程留在项目里

从「帮我调研一个方向」到「检查数据、运行分析、整理结果」，OmicsOps 让你围绕研究目标与 Agent 协作。你可以先审阅分析计划，再执行代码；也可以在运行中补充要求，随后检查结果表、图表、文献来源和执行记录。

它面向需要结合文献与计算的科研人员，尤其适合使用 Python / R、同时在个人电脑和远程服务器上工作的生物信息学用户。

| 你想完成的工作 | OmicsOps 提供的能力 |
| --- | --- |
| **用对话推进分析** | Agent 模式处理多步骤任务；Plan 模式先整理方案供你审阅，普通运行支持追加指导。 |
| **使用本机或服务器算力** | 连接本地 Python / R 或项目绑定的 SSH 主机；远端优先发现项目环境，Python 与 R 可分别选择解释器。 |
| **调研文献与科学数据库** | 检索 PubMed、Europe PMC、Crossref；通过内置科学 MCP 查询 GEO、UniProt、ChEMBL 等来源，保留可核验的标识和链接。 |
| **复用分析方法** | 按需加载 Skills 中的工作流指南，覆盖单细胞质控、文献综述、图表整理等任务；可导入技能并配置外部 MCP 工具。 |
| **管理文件与研究产物** | 在项目中组织会话、文件和分析产物；浏览本地或远端文件，预览 CSV / TSV 表格，查看执行过程与证据记录。 |
| **选择自己的模型服务** | 保存模型服务配置，按任务选择模型；可为新的普通 Agent 运行配置独立的只读子 Agent 模型。 |

科研包、模型服务和外部数据库各有配置要求。Skills 提供方法与操作指引，加载技能不会自动安装全部依赖。

## 下载与安装

**前往 [GitHub Releases](https://github.com/bai123350/OmicsOps/releases/latest) 下载桌面安装包。** 安装应用不需要 Rust、Node.js 或克隆源码。

当前版本：[v0.1.1](https://github.com/bai123350/OmicsOps/releases/tag/v0.1.1)。请选择与你的系统和芯片匹配的文件：

| 平台 | v0.1.1 下载 | 选择建议 |
| --- | --- | --- |
| Windows x64 | [EXE 安装包](https://github.com/bai123350/OmicsOps/releases/download/v0.1.1/OmicsOps_0.1.1_x64-setup.exe) · [MSI 安装包](https://github.com/bai123350/OmicsOps/releases/download/v0.1.1/OmicsOps_0.1.1_x64_en-US.msi) | 日常安装选 EXE；需要 MSI 部署时选 MSI。 |
| Linux x64 | [DEB 安装包](https://github.com/bai123350/OmicsOps/releases/download/v0.1.1/OmicsOps_0.1.1_amd64.deb) · [AppImage](https://github.com/bai123350/OmicsOps/releases/download/v0.1.1/OmicsOps_0.1.1_amd64.AppImage) | 按发行版和使用习惯选择。 |
| macOS Apple Silicon | [aarch64 DMG](https://github.com/bai123350/OmicsOps/releases/download/v0.1.1/OmicsOps_0.1.1_aarch64.dmg) | 适用于 Apple 芯片 Mac。 |
| macOS Intel | [x64 DMG](https://github.com/bai123350/OmicsOps/releases/download/v0.1.1/OmicsOps_0.1.1_x64.dmg) | 适用于 Intel 芯片 Mac。 |

Windows 是主要支持平台，Linux / macOS 目前为实验平台。Windows 安装包尚未做发行者签名，macOS 为 ad-hoc 签名、未公证；实机安装与科研流程验收状态以各版本说明为准。Linux 的凭据存储需要已解锁的桌面 Secret Service。

每个版本提供 `SHA256SUMS.txt` 和 `release-manifest.json`，用于核对文件及其源码版本。GitHub 的 **Source code** 压缩包是源码。应用内自动更新尚未实现，升级时请退出旧程序后手动安装新版；已有设备上的会话和模型配置会继续保留。

## 快速开始

### 1. 配置模型

安装并打开 OmicsOps，在设置中添加模型服务，填写服务地址和模型 ID，按服务要求配置你自己的 API key，并测试连接。模型服务的可用额度和费用由相应供应商决定。

### 2. 建立项目，选择计算环境

创建项目并选择项目目录，把研究任务、会话和产物放在一起。根据数据位置选择环境：

- **本地计算**：先准备需要的 Python / R 及科研包，确保应用能从系统 `PATH` 找到 `python` 和 `Rscript`。当前本地环境使用 `system`。
- **SSH 远程计算**：添加并信任 SSH 主机，将远端目录绑定到项目。打开运行时面板检测环境，优先选择项目目录中的 Python / R；两种语言可以来自不同的 `.envs` 子目录，远端 `system` 也可手动选择。

界面显示解释器可用，只表示可以执行它；分析需要的库、数据与权限仍需检查。

### 3. 描述研究目标

告诉 Agent 你的问题、数据位置、样本分组和希望得到的产物。需要先确认方法时切换到 **Plan** 模式；准备执行时使用 **Agent** 模式，并按提示处理审批或补充信息。

### 4. 检查并保留结果

查看执行过程、输出文件和文献引用，确认数据质量、统计方法与结论依据。模型解释需要与实际计算和原始来源核对，生成的计划或代码本身不代表分析已经完成。

## 可以这样开始一项研究

下面是任务表达示例，请替换为实际数据并准备对应依赖。

**文献调研**

> 帮我调研 CADD 变异有害性评分的相关研究。先明确检索范围，整理代表性方法、适用场景和限制，给出可核验的 DOI、PMID 或来源链接。

**单细胞数据质控**

> 检查项目中的单细胞数据。先确认输入格式、样本信息和 Python 依赖，再生成细胞与基因质控图，说明过滤阈值的依据，把图表和参数保存到项目中。

**服务器上的 R 分析**

> 使用项目绑定的 SSH 主机和 R 环境分析远端表达矩阵。先检查实验设计与依赖，再做差异表达分析，保存结果表、图和运行记录。大型原始数据保留在服务器上。

## 数据与执行由你掌握

- **使用自己的配置**：发行包由 GitHub Actions 从指定源码构建，不预置维护者的 API key、SSH 连接、模型配置、项目或聊天记录。重装后看到旧记录，可能是应用继续使用了该设备已有的数据。
- **凭据交给系统保管**：使用操作系统 keyring；Windows 使用 Credential Manager。凭据与项目数据分开存储。
- **按数据位置计算**：大型科学数据不默认同步到本机，可以保留远端引用和计算产物。使用在线模型、文献服务或 MCP 时，对话、代码、结果片段、查询及参数仍可能发送给所配置的服务。
- **执行有明确边界**：工具调用遵循当前运行的权限与审批规则，加载 Skills 或 MCP 不会自动扩大权限。执行记录与证据用于核验实际发生的操作。

Linux SSH 支持独立后台作业的后续查询；停止 Agent 不会取消已经运行的远端后台计算。交互式 Python / R 内核目前不支持跨应用重启重连。详细行为见 [技术说明](README-TECHNICAL.md)。

## 文档与开发

| 想了解什么 | 从这里开始 |
| --- | --- |
| 使用 Agent / Plan、选择环境 | [会话模式与环境选择](docs/agent-modes.md) |
| 使用内置 Skills 和科学数据库工具 | [内置科学工具说明](docs/bundled-science.md) |
| 查看运行机制、限制与开发命令 | [技术说明与开发指南](README-TECHNICAL.md) |
| 构建安装包、了解签名与发布规则 | [桌面构建与发布](docs/releasing.md) |
| 验证真实模型或 SSH 科研流程 | [真实环境验收](acceptance/README.md) |
| 提交问题或功能建议 | [GitHub Issues](https://github.com/bai123350/OmicsOps/issues) |

OmicsOps 使用 **Tauri 2 + Rust + React + TypeScript**。参与开发前请阅读 [开发约定](AGENTS.md)。欢迎提交问题复现、文档改进和聚焦的 Pull Request；分享日志或截图前，请移除凭据、私人路径和未公开数据。

## 许可与致谢

项目使用 [AGPL-3.0-only](LICENSE) 许可，第三方组件保留各自的许可与声明。

感谢 [Wisp Science](https://github.com/xuzhougeng/wisp-science) 提供科研工作台的设计参考，以及本项目采用的科学 Skills 和原生工具实现。捆绑技能的来源、固定版本与许可记录见 [第三方 Skills 说明](docs/third-party/skills/)。

本页的信息组织参考了 [Wisp Science](https://github.com/xuzhougeng/wisp-science)、[Cherry Studio](https://github.com/CherryHQ/cherry-studio) 与 [Jan](https://github.com/janhq/jan) 的项目介绍。
