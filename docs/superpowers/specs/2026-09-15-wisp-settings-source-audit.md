# Wisp Settings 源码对照与 OmicsOps 分批方案

日期：2026-09-15。状态：源码审计与架构方案；本文件不表示任何新增产品功能已经实现。

## 核对范围与版本

截图对应 Wisp Science v1.11.0，固定提交 `41913e4f83e20931462369a2b06bb86bd8c59fcc`；当前 main 为 v1.12.0，固定提交 `96f07cc937eaa966944bb370fd37c89cf2738b4e`。本文以截图版为基准，明确标注 main 才核对的细节。OmicsOps 基线为 `fd64b30`。

截图版设置包含完整 19 页：[导航定义](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L1763)分为 Workspace 九页和 Capabilities 十页。不能把 General 的几个字段或现有小弹窗视为完整对齐。上轮交付的语言持久化、模型预算等局部改动和隐私说明页是真实局部能力，但未完成这个整体设置体系。

主调查对两版本各页渲染分支逐段比较：General、Session、Environments、Storage、Usage、Appearance、Pet、Quick Actions、Workflows、Specialists、Memory、Plugins、Credentials、Channels、Permissions、Connections 共 16 页片段相同；Models、Browser、Skills 有差异。main 导航增加搜索并调整为四组。渲染片段一致不等于两个版本所有后端均一致。

## 19 页完整覆盖矩阵

本地路径均相对仓库根目录。“可复用”只说明存在真实调用或持久化，不表示已经有等价设置页。

| 页面与截图版源码 | Wisp 实际内容 / 核对边界 | OmicsOps 当前实现与缺口 |
|---|---|---|
| [General](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L1889) | 语言、工作区、恢复会话、发送键、选择浮窗、通知、更新；另嵌本地环境与网络面板 | `src/features/settings/SettingsPanel.tsx:194` 只有语言；逐字段见下表。 |
| [Session](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L2015) | 会话运行偏好；两版本渲染相同 | `src/features/settings/AgentSettings.tsx:11` 与 `src-tauri/src/agent_settings.rs:21` 已有迭代上限、自动继续、压缩、后续问题持久化。需逐字段整合，不能覆盖冻结运行配置。 |
| [Appearance](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L2864) | system/light/dark、配色、UI/code 字体字号、预览、自定义 CSS；两版本渲染相同 | 本地样式固定，`src/features/settings/settings.css:1`；无主题或 CSS 导入系统。需要实际视觉实现。 |
| [Pet](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L4210) | 启用、资源目录、选择保存；原生宠物窗口为独立能力 | 本地无宠物子系统。不可只增加无效开关。 |
| [Credentials](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L5917) | 固定科研服务及自定义环境凭据的设置/清除；后端秘密边界需按版本核验 | `crates/omicsops-adapters/src/credentials.rs:12/66` 有 vault，模型/SSH/MCP 表单可保存引用；无通用凭据管理 API。新页仅暴露引用和用途，不能回显明文。 |
| [Permissions](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L6306) | 项目审批设置，scope/kind/target 授权列表、单项/全部撤销 | `src-tauri/src/p1_commands.rs:656/680` 有 MCP 授权，`src-tauri/src/agent_v4.rs:3231` 有运行审批，`src/features/settings/BrowserSettings.tsx:274` 有撤销；现有 Privacy 为说明，不能替代真实授权管理。 |
| [Environments](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L2095) | 默认分析环境，新增/导入 SSH、导入 WSL，编辑/runtime 配置/探测/移除、撤销 SSH 信任；不是 Connections 页 | `src-tauri/src/agent_v4.rs:293` 探测，`src/features/workspace/RuntimeDialog.tsx:84/175` 展示与准备入口，`src/features/settings/SettingsPanel.tsx:367` SSH 配置。现有本地仅 system；准备入口不等于环境安装器。 |
| [Storage](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L2380) | 数据目录说明及 database/python/plugins/workspace/other 占用，全部或单项目范围选择；未发现清理/删除按钮 | `src-tauri/src/lib.rs:63/101` 固定应用数据与 DB，项目根独立；没有统一容量统计 API。只读统计即可作为对齐目标，不应扩大为清理或数据库迁移。 |
| [Usage](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L2507) | input/output/reasoning/cached token、工作区与会话分页、日/周/累计热图、模型占比、Skill/MCP 工具排行；不展示货币账单 | `src/features/workspace/ContextUsagePanel.tsx:7/24`、`src-tauri/src/agent_v4.rs:3552/3868` 有真实事件投影，区分 token 观察/字节预算/未知；缺跨会话聚合与活动/工具统计。价格系统不是此次对齐的必要条件。 |
| [Models](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L3059) | 截图版有模型设置；main 高级字段与 ACP 细节不得未经差异核对全部回填截图版 | `src/features/settings/SettingsPanel.tsx:138`、`src/tauri-api.ts:147` 已有 HTTP 模型配置/探测/发现/预算/推理档位。无 ACP 子进程提供方。 |
| [Quick Actions](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L4272) | name/workflow/description/enabled CRUD，依赖工作流模板 | `src/features/workspace/WorkspaceShell.tsx:791/799` 只有固定草稿动作/命令分派，无用户动作库。需新增持久化和实际触发链。 |
| [Workflows](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L4490) | 嵌入 workflow studio；main 图编辑器支持节点、串并联、specialist/model/approval | `src/features/workspace/WorkflowLibraryDialog.tsx:121/306`、`src-tauri/src/composer_workflows.rs:25/34` 有项目配方 CRUD、启停、步骤校验及上下文解析。是文本配方，不能宣称等价图执行引擎。 |
| [Specialists](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L4505) | 内置/自定义专家、instructions、模型和 skills 白名单；main 后端使用 settings JSON | `src/features/workspace/WorkspaceShell.tsx:1044` 专家按钮明确不可用。委派运行视图/模型配置不等于专家角色 CRUD。 |
| [Memory](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L4850) | 项目选择/启用、记忆文件 CRUD/清空、全局习惯 | `src-tauri/src/project_memory.rs:39/66/84` 真正读取/保存文件；`src-tauri/src/agent_v4.rs:6829` save_memory 执行；`src-tauri/src/p1_commands.rs:33/100` 搜索；`src/features/workspace/WorkspaceShell.tsx:1528` 检索 UI。已有文件真相源，无编辑删除/全局习惯设置。 |
| [Skills](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L5763) | Skills 页；main 增强项须保留版本区别 | `src/features/settings/SettingsPanel.tsx:219`、`src-tauri/src/skill_commands.rs:150/161/501` 已有导入/目录/启停/Agent 上下文。无等价 GitHub store、插件管理边界和删除全套流程。 |
| [Plugins](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L5336) | ZIP 或 HTTPS+SHA256 安装、可信状态、版本/启停/卸载；main 有插件目录及绑定 | 本地没有通用插件安装/生命周期；现有 MCP server 不能冒充插件包。 |
| [Browser](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L5599) | 浏览器配置；main 与截图版片段有差异 | `src/features/settings/BrowserSettings.tsx:150/232/256/274` 已有保存、会话准备、域名规则和撤销，可直接整合自身真实能力。 |
| [Connections](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L6375) | **MCP 连接器**：stdio/http、command/args/env、URL、none/oauth、headers、测试；不是 SSH | `src/features/settings/SettingsPanel.tsx:219`、`src-tauri/src/p1_commands.rs:485/502/713` 和 `crates/omicsops-mcp` 提供 stdio MCP。HTTP/OAuth 连接器尚缺，SSH 应归 Environments。 |
| [Remote Access / channels](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L6228) | relay/shared-folder 项目同步加 ChannelsPane；不能按英文标题推测为远程桌面服务 | 本地 SSH 文件上传/下载/项目同步选择是真实能力，但不等于跨设备同步或消息渠道。需单独协议、设备/渠道授权和秘密边界。 |

main 细节参照：[Appearance](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L2944)、[Models](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L3139)、[Workflows 图编辑器](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/agent_workflows.rs#L2792)、[Memory](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L4952)、[Plugins](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L5438)、[Channels](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L6349)。这些来源描述上游能力，不声明 OmicsOps 已支持。

### 已追到的调用边界

main [Environments](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L2175) 实际调用 `set_default_compute_resource`、`list_ssh_trust_edges`、`context_disposal_report`、`revoke_ssh_trust_edge`、`probe_compute_resource`、`remove_ssh_host`。这些证明页面连接实际命令；本轮尚未穷尽其后端存储 key 和各平台运行链。

main [Storage](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L2460) 打开时调用 `get_storage_usage`，前端按项目筛选；是只读统计，没有清理或保存动作。main [Usage](https://github.com/xuzhougeng/wisp-science/blob/96f07cc937eaa966944bb370fd37c89cf2738b4e/ui/src/settings_view.rs#L2587) 调用 `get_token_usage`，工作区详情用 `get_session_token_usage(projectId, offset, limit=20)` 分页；是观察统计，不是账单。以上三页渲染片段与截图版相同，后端逐表查询未在本轮全部追溯。

## General 逐字段与真实实施边界

[截图版 General](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/settings_view.rs#L1889)含保存/取消，在末尾嵌入 LocalEnvironmentPanel 与 NetworkSettingsView；更新检查有独立 invoke，不能把所有操作抽象成同一种本地开关。

| 字段 | 本地现状 | 建议可落地行为 / 所需补充 |
|---|---|---|
| 语言 | 已持久化，中英切换 | 保持旧 key 兼容；决定整页 Save/Cancel 时避免语言保存语义与其他字段冲突。 |
| workspace_dir | 本地每项目 `local_root`，创建时显式选择 | 先按项目模型定义默认目录设置；不得直接照搬重启文案或迁移 DB。上游含遗留默认项目逻辑，见下节。 |
| resume_last_session | 启动选首项目/首会话，不检查 user 消息 | 增加当前项目内有 user 消息的恢复候选查询，复用水合和竞态保护；关闭时进入该项目的新/空白会话态，不跳回全局项目库。应用启动恢复哪个项目另行定义；恢复不派发运行。 |
| send_with_modifier | `src/features/workspace/WorkspaceShell.tsx:358/886/1020/1032` 已持久化 | 提取共享偏好，General 与输入菜单即时一致；覆盖两模式/Shift+Enter/IME，明确 SideChat 是否一起生效。 |
| selection_popup_enabled | 无选中文本浮窗 | 先做消息正文限定的复制/引用到草稿真实动作，再提供开关；支持窗口级 Escape。 |
| notifications_enabled | 无原生通知桥接 | 新增原生通知服务、OS 权限状态、测试通知、真实事件去重；不从历史水合补发通知，正文不默认发送到系统。 |
| update_check_enabled / 检查按钮 / 版本 | 无 updater 源或插件 | 展示真实版本；无更新源时明确不可用，不能返回“已是最新”。有可信源后再建设检查/版本比较；检查不等于发布或自动安装。 |
| 本地环境 / 网络面板 | 探测散落，尚无等价网络设置 | 复用 system Python/R 探测；不能把探测成功等同依赖安装。代理等网络偏好需核对各实际 HTTP/SSH/MCP 客户端接入范围。 |

### General 保存、启动恢复与附属面板的补充证据

截图版 [sessions.rs:1740](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/crates/wisp-store/src/sessions.rs#L1740) 的 `latest_used_session_id(project_id)` 查询当前项目内 root/nonexploration 且符合 `SESSION_HAS_USER_TURN_SQL` 的会话，并按消息时间排序。恢复偏好控制项目内选择；关闭偏好不代表退回全局项目库。OmicsOps 应独立定义启动项目选择与项目内会话恢复，不能将两者混为一项。

截图版 [General 保存回调](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/main.rs#L5562) 将语言等设置持久化。语言使用 SQLite 的 `locale`，缺失时为英文，前端同时更新 locale/document language 并同步菜单；`resume_last_session`、`notifications_enabled` 缺省为 true。工作区保存验证绝对路径，不在保存时创建目录，下一次启动使用；这仍不构成应用数据库迁移。

发送快捷键和选择浮窗不是“只存 localStorage”。[prefs.rs:56](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/app_support/prefs.rs#L56) 使用 `wisp-send-with-modifier`（启用写 `1`，禁用移除）和 `selectionPopupDisabled`（禁用写 `1`，启用移除）；[main.rs:280](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/main.rs#L280) 在 hydration 后还将偏好写入 SQLite 的 `appearance_prefs`，并在 [main.rs:2390](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/main.rs#L2390) 回填已存值。OmicsOps 整合时应设计自己的单一偏好来源与兼容读取，不能机械复制两套状态。

`update_check_enabled` 通过独立的 `set_update_check_enabled` 立即保存，缺省 true，相关加载见 [lib.rs:4485](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/src-tauri/src/lib.rs#L4485)。main 调查发现系统通知在前台或关闭偏好时不发送、后台通知点击可导航到会话；更新检查在 Windows/Linux 读取 GitHub latest release 并比较版本，macOS 使用签名 updater。这里只记录源码分支，未执行真实跨平台通知、发布源或升级验收；OmicsOps 也没有因此获得可用更新源。

截图版 [NetworkSettingsView](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/network_settings.rs#L26) 分别配置 Model、MCP、Code 代理：system/空、direct/none 或自定义 URL，独立保存；[附加字段](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/network_settings.rs#L143) 包括 Conda mirror、Python index 与 CA bundle。[后端 network.rs](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/src-tauri/src/network.rs#L9) 将 `network_settings` JSON 保存到 SQLite，兼容旧 `proxy_url` 并即时应用；包镜像配置是 guidance，不是强制网络 allowlist。

截图版 [LocalEnvironmentPanel](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/ui/src/overlays.rs#L1182) 展示 Python/Rscript/uv/Node/npm/sci/pixi，调用 `detect_local_environment` 和 `save_local_environment_paths`，配置进入 SQLite 本地 execution context 的 `config_json`。它扫描 PATH/文件，不安装解释器，手工路径优先。后端逐行链尚待补齐。**OmicsOps 当前 AGENTS 明确规定本地执行只从 system PATH 启动 Python/Rscript、只接受 system 环境；自定义解释器路径不能作为普通设置悄悄加入。**如需支持，必须先明确调整项目约束，再设计执行解析、探测与验收；当前批次仅可复用现行 PATH 探测。

### 工作区目录不等于应用数据目录

截图版 [lib.rs:6893](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/src-tauri/src/lib.rs#L6893) 将 SQLite 放在固定 app_data 下的 wisp-science/wisp.sqlite；[lib.rs:6940](https://github.com/xuzhougeng/wisp-science/blob/41913e4f83e20931462369a2b06bb86bd8c59fcc/src-tauri/src/lib.rs#L6940) 用 legacy workspace_dir 回填/创建默认项目，并按活动项目等信息解析工作区。它不是迁移 SQLite 的开关。

OmicsOps 不能因看到“重启生效”就设计数据库、Skills、浏览器、lease 迁移。若新增“新项目默认目录”，必须明确只影响后续创建，不移动现有项目；若要修改既有项目根，则需要项目元数据、引用和正在运行任务的独立变更协议。最终选择应依据上游保存路径与产品范围再确定。

## 设置工作区整合

保留 `src/DesktopApp.tsx:1003` 的设置入口与底层工作区挂载，避免打开设置丢失草稿或打断运行。把当前 `src/features/settings/settings.css:1` 的 900×640 居中弹窗扩大为窗口内设置工作区，使用分组导航和独立滚动内容；对齐完整信息架构不要求一次实现 19 个新系统。

已有 Session、Models、Browser、Skills/MCP、SSH 设置可抽成对应内容页；Workflows 可复用配方库，Memory 可复用真实文件列表/检索，Usage 可复用会话观察。缺失系统在对照文档列明，不放无效开关或空白设置页。

所有可关闭层保留窗口 Escape 堆栈。导航、搜索入口与 `WorkspaceShell` 的 SettingsSection 类型需一起调整。根页与子编辑器必须测试立即 Escape 只关最顶层。

## 分批实施路线（全部 19 页，尚未实施）

用户已明确选择整个设置中心、按页面分批实现。以下路线覆盖全部 19 页；源码审计不是最终产品交付，每页需跟踪设计、实现、测试和提交状态。

1. 设置工作区与分组导航：整合既有页面，建立一致的保存、失败反馈与返回行为。不要以此声称完整功能对齐。
2. General 有界偏好：共享发送快捷键、恢复候选查询与启动导航、可用的选择工具条。工作区目录先明确项目语义；通知/更新需要原生接入独立交付。
3. 已有能力管理：Workflows 配方管理、Memory 文件检索与必要显式编辑 API、Usage 会话观察、MCP Connections、环境/SSH 页。
4. 安全与数据管理：引用型凭据目录、按真实 scope 展示/撤销授权、存储统计；任何删除、迁移和运行配置变化分别定义影响。
5. 新子系统分别设计：Quick Actions、Specialists、外观、通用插件、MCP HTTP/OAuth、渠道/跨设备同步及 Pet。每项需端到端调用与持久化，不能用 schema 或按钮代替实现。
6. 如需图工作流、全局习惯或跨会话用量统计，单独定义与现有文本配方、文件记忆、token 观察的兼容关系。上游 Usage 没有货币账单，不把计价系统加入对齐范围。

## 验证与交付要求

本文只做源码核对与方案，不运行产品行为测试、不声称新增功能通过。后续每批行为变化更新自动化测试，按仓库要求执行 `cargo test --workspace`、`npm test`、`npm run build`；原生插件、Tauri 组合和资源变更再运行 `npm run build:desktop`，依赖锁变化运行 `npm ci`。每批独立提交并报告实际结果。

窗口 Escape、Windows 路径、系统通知及原生目录选择需要明确 smoke 步骤。自动化不依赖真实 SSH、模型或密钥；真实模型/SSH 验收必须区分通过、失败和未执行。设置不得扩大模型/Skills/MCP 授权，不改变已冻结运行计划，不把凭据写入 SQLite/日志/上下文。不自行创建 tag、Release 或分发安装包。
