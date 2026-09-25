# 生信 Agent 运行边界与生命周期设计

日期：2026-09-26。状态：**三项切片已实施、分别提交并通过审查；确定性验证与 Windows 打包通过，真实环境及手工验收限制见第 8 节**。

实施计划：[Agent Runtime Boundaries Implementation Plan](../plans/2026-09-26-wisp-agent-runtime-boundaries.md)。

源码核对：[固定提交的 Agent/Runtime 实现对照](2026-09-26-wisp-agent-runtime-source-audit.md)。该审计补充实际源码行号，区分可借鉴的远端身份校验与不照搬的取消收口语义，不扩张首期范围。

用户已选择首期重点：明确 Agent 的执行位置、隔离能力、审批和恢复语义。本文将其收敛为可独立交付的运行边界视图；服务器容器沙盒留作后续独立设计。本文不授权改变已冻结的运行、审批或现有计算行为。

## 1. 用户问题与首期结果

OmicsOps 已能在本机、SSH 和本机 Docker/Podman 中执行 Python/R，但执行配置、解释器探测、运行状态和隔离承诺散落在 prompt、工具描述及界面中。用户需要清楚知道“代码在哪里执行”“环境是否已检查”“停止 Agent 后远端任务是否仍运行”，Agent 也需要获得同一套宿主事实。

首期交付一个宿主生成的 `RuntimeBoundaryViewV4`，由 Agent 上下文与现有运行时对话框共享边界含义。用户看到执行位置、环境、计算代码的隔离/网络策略、审批方式，以及交互内核和后台作业各自的恢复限制。它描述配置和实现边界，不冒充存活探测、依赖验收或安全证明。

不新增服务器 daemon、云平台、作业表、对象检查器或新的模型执行工具；不迁移 Agent 编排；不改变本机 system Python/R 与 SSH system/Micromamba 支持范围。

## 2. 参考依据与现状

参考仓库固定为 `xuzhougeng/wisp-science` 的提交 `b242fcbd1867551889643bfb7ee734dc48f7f5f1`，避免将浮动主分支当成设计依据。

| 已核对的参考 | 可借鉴内容 | 本次取舍 |
| --- | --- | --- |
| [runtime manager 源码](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runtime/src/manager.rs) | RuntimeKey 按项目、scope、会话、执行上下文、语言管理解释器；RuntimeInfo 单独描述生命周期 | 保留 OmicsOps 已有 project/run/backend/language/environment 键，不复制另一套 manager |
| [runtime launcher 源码](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/src-tauri/src/runtime_launcher.rs) | 根据 ExecutionContext 启动 attached runtime，支持显式解释器配置和可替换命令执行器 | 学习配置与启动分层；本期不引入其解释器搜索、WSL 或解释器配置能力 |
| [runtime-aware context 提案](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/docs/superpowers/plans/2026-08-13-runtime-aware-agent-context.md) | 模型需要知道运行时事实，避免假定已加载对象或已有依赖 | 只做已知边界的有限上下文投影；对象 inspect 和主动 API 发现后置。该文档标为提案，不当作全部已实现 |
| [Agent Desktop Runtime 提案](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/docs/superpowers/specs/2026-09-10-agent-desktop-runtime-design.md) | 展示层生命周期与执行层生命周期分离 | 采用“关闭界面不等于终止计算”的语义；其 GUI/ChildSession 提案不作为服务器计算沙盒实现证据 |

OmicsOps 当前已有的基础必须复用：

- `crates/omicsops-protocol/src/lib.rs`：`ComputeSelectionV4` 冻结后端、环境、审批、网络策略和容器 image ID；`FullAuto`/`FullAccess` 只接受符合约束的容器选择。
- `crates/omicsops-runtime/src/lib.rs`、`jobs.rs`：按执行键隔离内存会话；不健康会话可替换；重复 job 身份不能重复执行；放弃等待不等于计算被终止。
- `src-tauri/src/agent_v4.rs`：延迟初始化计算资源、SSH 主机信任检查、冻结选择校验和项目规则加载；初始及初始化后 prompt 已包含 backend/environment/approval/network。首期整理和补全这些信息，不再增加一段重复配置叙述。
- `src-tauri/src/remote_jobs_v4.rs`、`remote_job_bridge.py`：Linux SSH 一次性后台作业、持久启动身份、丢失确认后不重派、重连查询及回执校验。无自动轮询或后台取消命令。
- `src-tauri/src/runtime_approval.rs`：普通 Agent 的宿主词法风险判断；该判断不是 Python/R 沙盒。后台派发仍需按现有策略审批。
- `src/features/workspace/RuntimeDialog.tsx`：已有运行时对话框与解释器状态。当前 `unverified` 文案主要针对镜像，但 local/SSH 配置目录同样会返回此状态，应按真实后端描述。

## 3. 可选方案

| 方案 | 用户收益 | 代价与范围 | 决策 |
| --- | --- | --- | --- |
| A. 统一运行边界视图 | 立即说明执行位置、检查程度、审批、停止与恢复限制；为后续沙盒提供明确能力契约 | 小范围 DTO、宿主投影、prompt 和既有 UI 接线；不新增执行机制 | **首期推荐，符合用户已选方向** |
| B. Linux SSH 容器执行 | 将服务端科研代码置于明确配置的容器边界 | 需远端引擎探测、镜像冻结、挂载/资源策略及独立作业恢复设计与真实验收 | 后续单独设计，不纳入首期 |
| C. 服务端 Agent 平台 | 桌面退出后继续完整 Agent 编排、多客户端连接 | 新身份认证、审批传递、服务状态、凭据和发布运维面，明显超出当前范围 | 暂不采用 |

## 4. 首期契约

### 4.1 唯一事实来源

`RuntimeBoundaryViewV4` 放入 `omicsops-dto`，保持纯 serde 数据。原有选择和枚举引用 `omicsops-protocol`，不另造具有不同含义的隔离级别。宿主投影函数可放入 `src-tauri/src/runtime_boundary_v4.rs`，其理由是 prompt、原生命令和 UI 需要共享一套边界映射；不为文件长度重构 Agent V4。

视图最小字段为：

- `source`：`draft_selection` 或 `frozen_run`；后者携带 `run_id`。
- `compute_selection`：已验证的完整选择，复用现有类型。
- `execution_location`：`local_host`、`ssh_host` 或 `local_container`。
- `isolation`：现有 `process`/`container`；同时给出稳定的限制代码供 UI 本地化，避免把等级名当安全结论。
- `interactive_lifecycle`：run 内复用；跨应用重启不可接回；中断不证明 SSH 远端终止。
- `detached_job_lifecycle`：`unsupported` 或 `ssh_linux_only`；后者明确可按原身份查询、停止 Agent 不取消、无自动轮询/取消、未知状态不重派。
- `verification`：该视图本身不探测解释器、依赖、远端存活或计算成功，固定标明 `not_checked_by_this_view`。既有探测结果单独展示，不将旧探测提升为当前事实。

网络和审批直接使用 `compute_selection` 中字段，不再复制一套可发生分歧的字段。可选的配置摘要必须来自该对象。首期不收集 CPU/GPU、内存、远端进程、环境变量或对象值，不引入“Starting/Ready/Busy”新状态机。

生命周期能力不是当前作业状态。例如 `ssh_linux_only` 只说明功能适用条件，并不确认当前远端就是 Linux、Python3 已安装或此时可派发作业。

### 4.2 草稿、运行和历史记录

新增一个只读原生命令（建议 `agent_v4_runtime_boundary`）承载 UI 视图，输入为带判别字段的二选一请求：

1. 草稿：`project_id + compute_selection`。宿主验证选择及其项目绑定，仅输出配置边界；不能据此授予执行能力。
2. 已有 run：`project_id + run_id`。宿主读取该项目拥有的 run 和冻结选择；忽略当前 composer 选择，不允许调用方同时覆盖选择。

历史 run 没有冻结 compute selection 时返回“历史运行未记录此配置”，不补用当前选择。失配、跨项目请求或无法读取时返回明确错误，UI 不保留另一 run 的旧视图。异步返回须按当前请求身份匹配，快速切换会话/选择时丢弃旧响应。

此命令只做本地校验、必要的 Store 读取和纯投影：不调用资源 `initialize`、不连接 SSH、不启动解释器/容器、不拉镜像、不创建环境。UI 不因打开对话框触发计算或新环境探测。

Agent prompt 在既有初始与资源就绪路径使用同一个投影/格式化函数，运行时输入只能来自冻结选择。初始化后仍保留真实项目规则及原有资源已初始化事实，不把该事实扩大为“科学依赖已就绪”。Prompt 文本有固定上限，不追加每个历史作业或重复全量事件；普通 candidate 与 compacted 两条上下文路径都保留该摘要并计入请求预算。

### 4.3 展示与实际保证

| 后端 | 应显示的边界 | 不得宣称 |
| --- | --- | --- |
| Local | 本机当前用户进程；仅 system PATH 的 Python/R；继承主机网络；项目 cwd | 项目外文件被 OS 强制封锁、依赖已安装、代码在安全沙盒 |
| SSH | SSH 账号权限的远端进程；system 或冻结项目 Micromamba 环境；继承远端网络 | SSH/Micromamba 自身形成强隔离，或 Stop 已终止远端后台任务 |
| 本机 Docker/Podman | 冻结镜像；计算容器 network none、rootfs 只读、capabilities 收紧、no-new-privileges、PID 数量上限；项目目录整体可写挂载 | 整个应用断网、项目数据只读、已有 CPU/内存配额、具备 VM 级或对抗恶意管理员的隔离 |

当前容器代码只设置 PID 上限，不笼统写“资源配额完整”。`network none` 仅约束该计算容器；主模型、MCP、浏览器等仍可能传输提示词、结果或元数据。当前所有后端均不承诺恶意代码不能攻击宿主。项目路径校验和回执摘要校验也不构成对恶意远端的可信证明。

现有 RuntimeDialog 增加紧凑的“执行位置 / 隔离与网络 / 审批 / 停止与恢复”信息。`available` 表述为“已找到解释器”，并说明依赖未由该状态验证；`unverified` 对本机、SSH、镜像分别给出准确描述。显示的是当前草稿还是某次运行的冻结配置必须可见。错误时展示无法读取及重试入口，不回退为安全或可用。

继续使用现有窗口级 Escape 堆栈。关闭对话框只是关闭展示，不触发 interrupt/cancel，也不改变运行选择。

### 4.4 审批、审计和恢复

视图仅解释现有审批策略，所有执行仍由宿主裁决。`RiskBased` 是尽力而为词法分类；视图不替任何调用作批准，不继承上一次批准；`FullAccess` 不扩大 MCP、浏览器或其他授权。原 run/call/hash/scope 绑定和拒绝决定保持不变。

复用 `RuntimeJobV4` 与 Store 的生命周期，不将 UI 的状态当持久事实。继续区分：提交成功 ≠ 计算完成；远端回执 ≠ 科研结论已核验；未知 ≠ 失败/取消；停止等待 ≠ 远端终止；恢复原身份查询 ≠ 自动重跑。

不新增表或写回历史 run，不修改旧 RunSpec 的序列化/hash，也不回填审批记录。视图由原始冻结信息生成，不作为新的执行证据。凭据仍只留在 keyring/Windows Credential Manager，视图、prompt、事件和测试样例不得包含密钥、SSH 认证内容或原始敏感服务器输出。

## 5. 可审实施切片

设计确认后按以下独立结果实施和提交；每项包含对应测试，不能先落空协议再宣称功能完成。

1. **宿主边界契约与只读查询**：加入 DTO、纯投影、草稿/冻结 run 查询及权限/旧数据测试；注册原生命令，补充 `dto_contract_tests.rs`。只读接口可独立被调用和验证，不触发资源初始化。
2. **Agent 上下文统一**：替换现有两处重复的选择说明，补上交互内核/后台作业及隔离边界；保留 lazy 初始化、SSH 规则观察门禁、既有审批和工具描述。用 fake factory 确认构造 prompt 不创建执行资源。
3. **运行时界面解释**：接入 `src/types.ts`、`src/tauri-api.ts` 和现有 RuntimeDialog/WorkspaceShell；草稿与当前/历史 run 正确分流，修正探测文案并处理异步过期响应。同步实施结果与验证记录。

不触碰 `website/`。不为此变更更新版本、创建 tag/Release 或分发安装包。

## 6. 验证与验收

确定性测试至少覆盖：

- 四种后端的边界矩阵；SSH 非 system 环境；容器冻结镜像及 network none；无效选择和 FullAccess/进程后端组合拒绝。
- 草稿与冻结 run 的来源标识；历史缺失选择不回填；跨项目/错误 run 拒绝；改变当前选择不能改变历史说明。
- 只读查询及 prompt 构造在 fake resource factory/命令执行器下调用次数为零，不连接 SSH、不执行 Python/R、不运行容器命令。
- 上下文压缩前后保留一致边界摘要，并由既有完整请求预算计入成本。
- 输出没有凭据；未检查不显示 ready；SSH Linux 能力不当作已探测平台；容器离线不推断模型/MCP 离线。
- DTO 序列化和 native/前端契约；现有 run/hash/审批兼容用例；独立后台作业派发、丢确认和重连查询回归测试继续通过。
- UI 四后端、中英文文案、缺失/失败状态、快速切换后的旧响应忽略、历史 run 冻结配置；打开即按 Escape 只关闭顶层，父级保持打开，未发出取消或中断请求。

先执行受影响 crate、DTO 和 UI 定向测试，交付前执行 `cargo test --workspace`、`npm test`、`npm run build`。原生命令及桌面组合变更后执行 `npm run build:desktop`。如修改依赖锁文件，补跑 `npm ci`；格式失败按 AGENTS.md 单独处理格式提交。

Windows 手工 smoke：检查本机/SSH/容器草稿说明，发起一次运行后修改 composer 配置并查看该 run 的冻结说明；打开/关闭对话框确认不启动计算。Linux SSH 后台作业的提交、断线、按原身份重连查询需在一次性环境中按 `acceptance/README.md` 验收，Stop 后仍运行必须如实展示。macOS 本期只保留共用数据契约，未执行的平台 smoke 不作支持保证。

## 7. 后续服务器沙盒设计入口

首期完成后可独立讨论 Linux SSH 容器后端：宿主仍持有审批与派发权，远端只执行已批准的不可变请求；请求需冻结主机指纹、远端 root、镜像身份、挂载、网络及资源限制，并沿用不确定派发不重跑和可核验回执。必须另行解决控制目录与科研可写目录的边界、输出提取、取消确认及引擎清理，不能简单把当前 SSH descriptor 改成 `container`。

这是后续方向，不是首期实现承诺。rootless 容器、VM、HPC/SLURM 等方案的保证、成本和环境要求需分别验证；环境管理、进程隔离和 SSH 传输均不冒称强安全沙盒。

## 8. 本次工作与验证记录

用户已确认设计及三项实施计划。第一项已完成并提交为 `d5f1ee0`：宿主只读边界 DTO、投影和草稿/冻结运行查询。定向验证通过：DTO 30 项、桌面 runtime_boundary 6 项、DTO 契约 42 项。

第二项已完成并提交为 `660f9a9`：Agent 初始化前后复用同一宿主摘要。定向验证通过：桌面边界 9 项及随后新增的真实本地 factory 集成测试 1 项、惰性资源 3 项、core 边界 2 项、完整预算 3 项、远端作业 2 项（另有真实 SSH 测试 1 项 ignored）。core 新增测试在原实现上即通过，桌面格式化函数缺失提供 RED 证据；没有修改 core 生产逻辑。

第三项已完成并提交为 `4f6cf9a`：原生 API、防过期查询 hook、草稿与冻结运行入口、运行边界双语说明和嵌套 Escape。未解析镜像或空镜像引用不发起查询；切换会话会清除旧对话框，修改草稿不会改变已查看的冻结运行。定向 UI 六文件测试 241 项通过；后续 DesktopApp 全文件 94 项通过，审查补充的冻结视图及四后端双语测试所在两文件 52 项通过。三个切片均经过独立审查，第三项的两处测试覆盖缺口已补齐并复审关闭。整分支 `98130fc..4f6cf9a` 终审通过，无需修复的 Critical、Important 或 Minor 问题；该结论不授权合并或发布。

2026-09-26 实施后的完整验证：

| 命令 | 实际结果 |
| --- | --- |
| `cargo test --workspace` | 1,324 通过，0 失败，12 ignored；针对最终 Rust 代码执行。 |
| `npm test` | 首次 906 通过、1 失败；冻结生产代码后复跑为 907 项前端及 22 项桥接全部通过；纳入审查新增用例后的最终完整运行为 916 项前端及 22 项桥接全部通过。 |
| `npx vitest run src/features/workspace/ComposerIntegration.test.tsx src/features/workspace/RuntimeDialog.test.tsx --reporter=dot` | 随后仅补充测试覆盖，两个受影响文件 52/52 通过；生产代码未再改变。 |
| `npm run build` | 通过，TypeScript 与 Vite 生产构建完成。 |
| `npm run build:desktop` | 通过，Windows x64 release 与 NSIS 打包完成；未发布、分发或安装产物。 |
| `cargo fmt --all -- --check`、`git diff --check` | 通过。格式化仅涉及本次新增 Rust 代码，无独立的既有代码格式变更。 |

首次前端全套失败位于未修改的 `SkillDetails` 嵌套 Escape 用例。精确命令 `npx vitest run src/features/settings/SkillDetails.test.tsx -t "closes the preview before details and the parent Escape layer"` 为 1 通过、10 项被过滤；之后完整复跑通过。失败未复现，现有日志不足以证明根因，没有据此修改该组件或宣称已修复。

构建仍提示 Web chunk 大于 500 kB，以及 MSVC 创建导入库的 linker stdout；Rust workspace 编译另有一次增量缓存无法复用的访问提示。对应命令最终均以 0 退出。依赖锁文件未变化，未重复执行 `npm ci`。`website/` 未改动。

真实模型、SSH、R/Micromamba、PBMC 验收未执行：当前进程未配置 `acceptance/README.md` 要求的一次性远端与模型验收环境变量。Windows 原生 GUI 手工 smoke 未执行，当前会话没有原生窗口交互工具；上述 jsdom 集成测试验证了草稿/冻结来源、切换与 Escape，但不替代原生端人工操作。macOS 未验证。ignored 用例、测试替身及成功构建均不等于生产端到端验收；服务器容器后端仍是第 7 节后续方向。
