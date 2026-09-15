# Settings Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按页面分批交付全部 19 个可用设置页及其真实行为，依据 Wisp 截图版源码适配 OmicsOps。

**Architecture:** 复用现有 Tauri 命令、项目数据模型与设置实现，前端设置工作区统一导航。项目内会话恢复采用后端查询，偏好与运行真相源分离。跨执行、凭据、插件和渠道的新增子系统分别设计后落地，导航和说明不计为完成。

**Tech Stack:** Rust workspace、Tauri 2、SQLite/sqlx、React 19、TypeScript、Vitest。

**Spec:** `docs/superpowers/specs/2026-09-15-wisp-settings-source-audit.md`

## Global Constraints

- 用户已选择“整个设置中心，按页面分批实现”；不再等待范围确认。
- 设计使用 GPT-6 medium，代码使用 Sol high，读取可交给 Luna max；每批独立提交。
- Windows 是首要目标。本地 Python/Rscript 仅 system PATH、当前本地环境仅 system；不能通过设置绕开。
- 凭据只在现有 keyring/Credential Manager；SQLite、日志、模型上下文不得含明文秘密。
- 工作区设置不迁移 SQLite；更新源不存在时不伪造最新版本或发布渠道。
- 所有可关闭层加入窗口 Escape 堆栈，立即按 Escape 只关闭视觉最顶层。
- 所有行为变化有自动化测试；真实 SSH、模型、网络和系统安装不作为自动化前提。
- 每批完成最接近的检查；交付前执行 cargo test --workspace、npm test、npm run build。原生组合/资源变更增加 npm run build:desktop；锁文件变化执行 npm ci。
- 适配方案可保留明确能力限制，但不能将空页、无效开关或纯说明当作页面实现完成。

## Master tracker

状态“已有部分”只表示基线已有可复用功能。完成标准要求实现、验证、审阅和提交记录全部齐全。

| 页面 | 当前状态 | 可用页的交付范围 | 批次/依赖 |
|---|---|---|---|
| General | 已有部分 | 语言、共享发送偏好、项目内恢复、选择操作、通知、实际目录语义、真实更新状态及环境/网络入口 | B1 + B3 |
| Session | 已有部分 | 现有迭代/继续/压缩偏好完整管理与反馈 | B2 |
| Appearance | 未实现 | 实际主题、字体与字号应用；导入需明确安全边界 | B4 |
| Pet | 未实现 | 实际宠物展示、启停及资源管理 | B8 |
| Credentials | 已有 vault | 非敏感凭据目录、设置/替换/清除与引用一致性 | B5 |
| Permissions | 授权散落 | 真实 scope 授权列表及逐项撤销，不改冻结计划 | B5 |
| Environments | 已有部分 | system 探测、SSH 配置/信任/运行环境管理；遵守本地限制 | B2 |
| Storage | 未实现 | 真实应用/项目占用统计、范围筛选；不扩为清理 | B6 |
| Usage | 已有会话观察 | 跨会话 token/模型/时间/工具统计、项目与会话分页 | B6 |
| Models | 已有部分 | 完整现有 provider 管理；ACP 为可选扩展，不作为页面完成前提 | B2 + B7 |
| Quick Actions | 固定动作 | 用户动作 CRUD、工作流绑定及真实调用 | B4，依赖 Workflows |
| Workflows | 文本配方 | 可用配方库设置页及真实调用；图执行引擎为可选扩展 | B2 + B7 |
| Specialists | 未实现 | 专家角色 CRUD、模型/工具约束、真实执行选择 | B7 |
| Memory | 文件读写/搜索 | 项目记忆列表/搜索/显式变更，习惯记忆独立作用域 | B4 |
| Skills | 已有部分 | 管理实际安装源、启停、详情/文件及安全删除边界 | B2 + B7 |
| Plugins | 未实现 | 验证安装源/包、生命周期、启停/移除及工具绑定 | B7 |
| Browser | 已有部分 | 现有配置/会话/域名/授权完整可用页 | B2 |
| Connections | stdio MCP | MCP stdio 管理与真实检查/授权；HTTP/OAuth 为可选扩展 | B2 + B7 |
| Remote Access | SSH传输不同义 | 现有项目本地/SSH传输与同步管理，明确范围/状态/恢复；上游渠道平台不在基础页范围 | B8 |

批次顺序不强制串行：B1 先建立设置承载与 General 核心；B2 整合已有能力；B3 接原生偏好；B4 用户内容和视觉；B5 授权/秘密；B6 只读统计；B7 适合 OmicsOps 的专家/扩展管理；B8 项目传输与 Pet。后续批次在开工前追加明确文件、协议、测试用例和提交任务，不将本 master 当作尚未设计子系统的代码规格。

## B1 文件与接口边界

- 前端所有者：`src/features/settings/SettingsPanel.tsx`、`src/features/settings/settings.css`、`src/features/settings/SettingsPanel.test.tsx`；按职责新增共享偏好 hook 及测试；`src/features/workspace/WorkspaceShell.tsx`、`src/DesktopApp.tsx`、`src/tauri-api.ts` 及对应集成测试。
- 后端所有者：`crates/omicsops-store/src/lib.rs`、`src-tauri/src/workspace_commands.rs`、`src-tauri/src/lib.rs` 与对应 Rust 测试。
- 返回既有 Conversation 的查询不新增复制 DTO；如果新增跨边界对象，必须放 `crates/omicsops-dto` 并更新 DTO contract tests。
- 两位实现者先锁定查询命令签名再接线；前端不得扫描所有消息模拟后端候选选择。
- 已锁定接口：Store `latest_used_conversation(project_id) -> Result<Option<Conversation>, StoreError>`；Tauri `latest_used_conversation(project_id: Uuid) -> Result<Option<Conversation>, String>`；TS `latestUsedConversation(projectId): Promise<WorkspaceConversation | null>`。

### B1.1 设置工作区、分组与搜索

**Files:** 上述 SettingsPanel、settings.css、SettingsPanel.test.tsx；DesktopApp 仅调整开关/初始分组，不卸载工作区。

**Interfaces:** 保留 `SettingsSection` 导航入口、`initialSection` 与 `onClose`；可用页面注册表提供 id、组、双语标题、搜索词和 renderer。未实现页面在 master 跟踪，不注册成可用空页。

- [ ] 写测试：打开设置后立即 Escape 关闭；打开模型编辑子层后 Escape 只关编辑器；搜索中文/英文标题找到现有页；无结果有明确反馈且可清除；关闭后底层草稿保留。
- [ ] 运行 `npm test -- src/features/settings/SettingsPanel.test.tsx`，确认新增断言在旧布局/导航下失败。
- [ ] 把固定小弹窗改为窗口内设置工作区；左侧分组和搜索，右侧滚动，保持可见返回按钮与窗口 Escape。
- [ ] 使用已有 SettingsSection 路由进入模型/技能时仍定位到对应页；普通设置入口进入 General；搜索不清除正在编辑表单的值，离开表单时行为保持明确。
- [ ] 运行上述设置测试及相邻 overlay 测试，记录结果并提交。

示例行为断言：

```tsx
fireEvent.click(screen.getByRole("button", { name: "Configure DeepSeek" }));
fireEvent.keyDown(window, { key: "Escape" });
expect(screen.queryByLabelText("API key")).not.toBeInTheDocument();
expect(screen.getByRole("dialog", { name: "Workspace settings" })).toBeInTheDocument();
```

### B1.2 共享发送偏好

**Files:** 新增 `src/use-general-preferences.ts` 与测试，修改 SettingsPanel、WorkspaceShell、DesktopApp 相关参数与测试。

**Interfaces:** 单一共享状态包含 `sendWithModifier: boolean` 和 setter；兼容读取现有 `omicsops.composer.modifierSend` 的字符串 true/false。保持现有语言 hook 不回退用户语言。恢复偏好使用独立稳定 key，默认 true。

- [ ] 写测试：旧 key=true 时 General 显示 Ctrl/Cmd+Enter；在 General 修改后同一工作区发送行为立即变化；在输入菜单修改后 General 同步；重载保留；localStorage 抛异常时当前会话仍可操作。
- [ ] 运行新 hook 测试和 `src/features/workspace/ComposerIntegration.test.tsx`。
- [ ] 以根共享 hook 向设置/工作区传递同一状态；移除 WorkspaceShell 私有重复状态，避免 storage event 不能同步同窗口的问题。
- [ ] 保持 Shift+Enter 换行、IME 和 keyCode229 不发送；两种模式判断只影响快捷键，不影响发送按钮、审批或队列。
- [ ] 运行相关测试并提交。

键盘核心保持以下语义，实际接入复用已有组件：

```ts
const canSend = event.key === "Enter"
  && !event.shiftKey
  && !event.nativeEvent.isComposing
  && event.keyCode !== 229
  && (!sendWithModifier || event.ctrlKey || event.metaKey);
```

### B1.3 后端项目内最近使用会话查询

**Files:** Store、workspace_commands、lib 注册；Rust 测试与必要 API 接线由约定所有者操作。

**Interfaces:** 只读查询，参数项目 ID，返回既有 Conversation 或 null；不创建会话、不触发运行、不跨项目。

- [ ] 写真实临时 SQLite 测试：较新空会话不覆盖较旧有 user 消息会话；assistant/system-only 不候选；其他项目不候选；排序按最新消息活动时间而非会话重命名更新时间；确定性 tie-break；无候选返回 None。
- [ ] 额外校验分支/旁聊 frame 归属：只返回实际可打开的主会话记录；不把 side chat 当新会话。是否包含用户主动分支根据 OmicsOps 分支模型明确测试，不能照抄不存在的 Wisp exploration 标志。
- [ ] 运行 Store 定向测试确认失败。
- [ ] 在 Store 用 `conversation_records`、`messages` 的真实 schema 写 EXISTS user 内容约束及消息时间排序；复用 `conversation_from_row`。messages.content 直接保存 markdown；存在 role=user 即有效，空正文的附件-only 用户消息也应保留。
- [ ] 命令校验项目存在并转译错误，注册 Tauri handler；运行 store/desktop 定向测试后提交。

查询约束参考，不绕过当前 schema 与 frame 身份验证：

```sql
WHERE c.project_id = ?1
  AND EXISTS (
    SELECT 1 FROM messages u
    WHERE u.conversation_id = c.frame_id
      AND u.project_id = c.project_id
      AND u.role = 'user'
  )
ORDER BY (
  SELECT MAX(m.ts) FROM messages m
  WHERE m.conversation_id = c.frame_id AND m.project_id = c.project_id
) DESC, c.frame_id ASC
```

### B1.4 项目内恢复接线与验证

**Files:** DesktopApp、tauri-api、共享偏好、DesktopApp 集成测试。

**Interfaces:** 明确 requestedConversation（搜索/分支导航）优先于默认恢复；恢复设置控制项目内默认选择，不改变启动项目选择。开启无候选或关闭时进入当前项目空白会话；不得跳项目库或自动运行。

- [ ] 写测试：开启选择后端候选；关闭不打开历史；显式搜索导航仍定位指定会话；切项目时迟到请求不覆盖新项目；查询失败显示可重试错误且不伪装空结果；StrictMode 不重复创建空白会话。
- [ ] 运行相关 DesktopApp 测试，确认旧首会话选择失败。
- [ ] 复用 projectRequestToken、requestedConversation、conversationHydrating 与现有创建路径；将偏好在项目选择开始时捕获，切换偏好不立即抛弃当前编辑会话。
- [ ] 当确需空白会话时复用合适空记录或幂等创建，避免每次重渲染生成会话。保留会话加载、事件恢复与运行审批行为。
- [ ] 完成前端/Rust 定向检查，然后运行默认完整检查；桌面命令注册变化执行桌面构建。
- [ ] 由设计/审阅代理复核实现与本计划，记录实际通过/失败/未执行，更新 General 为“部分完成（B1）”，不标全页完成；提交本批。

## B2 复用真实能力形成独立可用页面

### 范围与导航契约

B2 的完成标准是七个页面都有真实管理动作和错误反馈，不是复刻所有上游扩展。采用现有稳定 section ID：`agent` 显示 Session，`remote` 显示 Environments，`models`、`skills`、`browser` 保持；新增 `connections` 表示 MCP、`workflows` 表示项目配方。所有入口和搜索使用同一注册表。Remote Access 后续用独立 `remote-access` ID，不能占用已有 `remote` 深链并改变其 SSH 含义。

- 前端 UI 所有者：`src/features/settings/SettingsPanel.tsx`、设置 CSS/测试、`src/features/workspace/WorkflowLibraryDialog.tsx` 与测试。
- 父级接线所有者：`src/DesktopApp.tsx`、`src/features/workspace/WorkspaceShell.tsx` 与集成测试；若和 B1 同时运行，等待 B1 完成这些共享文件后再修改。
- B2 不改 Store schema、凭据持久化、模型协议或审批策略。只有发现现有动作缺陷时，单独建立后端修复任务。

### B2.1 Session、Models、Browser 和 Environments

**Interfaces:** Session 直接渲染 `AgentSettings({ locale })`；Browser 直接渲染 `BrowserSettings({ locale })`；Models 继续使用现有 onSaveModel/onProbeModel/onListModels props；Environments 复用 RemoteSettings 的 connections、selectedProject、onSave、onTest、onConfirm、onBind。保持底层调用签名不变。

- [ ] 在 SettingsPanel 测试中验证分组标题不改变实际渲染器；Session 读取保存、模型保存失败、Browser 授权撤销仍调用原有接口。
- [ ] 将原“远端计算”导航重命名为“环境 / Environments”，页面保留 SSH 配置、连接测试、host key 确认和项目绑定。未选项目时仍能管理全局连接，只禁用依赖项目的绑定动作并说明原因。
- [ ] 为系统解释器探测提供现有 RuntimeDialog/环境入口，严格显示“可找到”状态；不增加假安装按钮或任意本地路径配置。
- [ ] 运行 `npm test -- src/features/settings/SettingsPanel.test.tsx src/features/settings/AgentSettings.test.tsx src/features/settings/BrowserSettings.test.tsx`，补一次 Settings→模型表单→立即 Escape 的集成断言，再提交。

页面交付保留的明确边界：Models 支持现有 HTTP/Ollama 协议，ACP 不属于基础页完成条件；Environments 支持现有 system 与 SSH，不承诺 WSL 导入或本地自定义解释器。Browser 沿用当前受控浏览器的域名与授权能力，不复制未经支持的上游字段。

### B2.2 拆开 Skills 与 MCP Connections

**Files:** `src/features/settings/SettingsPanel.tsx` 中现有 SkillsAndMcpSettings；可新增 `SkillsSettings.tsx`、`McpConnectionsSettings.tsx`，只有为了分离实际页面职责才提取；保留现有 BundledMcpPresets。

**Interfaces:** 最小改法给现有组件增加必需 `section: "skills" | "connections"`，只挂载对应内容，保持所有回调类型。若提取组件，Skills 只接 skillPackages/import/enable 及其 busy/error；MCP 接 mcpServers/save/inspect/enable/launchApproval/toolApproval/bundled presets 与 selectedProject，不互传无关凭据。

- [ ] 写测试：Skills 页不出现 MCP 启动命令表单，Connections 页不出现 Skill 导入；Connections 保存仍按原 command/args/env/cwd/timeout 构建请求；缺项目时授权检查给出项目提示而不是悄悄启动工具。
- [ ] 移动 BundledMcpPresets 到 Connections，现有 PubMed 凭据仍走 host keyring API；保留 launch approval 和逐工具 approval 控件。
- [ ] 保持所有“管理技能”搜索入口指向 skills，“管理 MCP”指向 connections；Privacy 内部按钮相应拆分，不能把 SSH 引导到 MCP。
- [ ] 完成 `SettingsPanel.test.tsx` 与 `BundledMcpPresets.test.tsx`；验证保存失败保留输入、凭据引用不变为明文，再提交。

有意义的隔离断言可使用：

```tsx
fireEvent.click(screen.getByRole("button", { name: "Connections" }));
expect(screen.queryByRole("button", { name: "Import skill" })).not.toBeInTheDocument();
// 在连接表单填写 command/args 后断言原 onSaveMcpServer 接到同一请求契约。
```

HTTP/OAuth MCP、插件包管理不是 stdio Connections 页完成条件；后续如增加需要独立协议与秘密管理设计，不展示没有执行端的 transport 选项。

### B2.3 Workflows 嵌入已有配方管理

**Files:** `src/features/workspace/WorkflowLibraryDialog.tsx`、`workflow-library.css`、现有测试；SettingsPanel 接入当前项目。

**Interfaces:** 给现有 `WorkflowLibraryDialogProps` 增加 `presentation?: "dialog" | "embedded"`，默认 dialog，继续复用 projectId/zh/onClose/onChanged。设置页传 embedded；对话框入口行为保持。embedded 不注册库自身 Escape 关闭层、不设置 aria-modal、不做全页 Tab trap，内部编辑器仍按 Escape 栈先关闭。

- [ ] 写测试：设置页能加载当前项目配方、创建编辑启停；切换项目时不残留旧项目数据；嵌入模式不存在第二个 modal；原对话框立即 Escape 仍关闭库，嵌入编辑器立即 Escape 只关闭编辑器。
- [ ] 在同一组件保留加载、保存、校验和 dirty edit 处理，仅条件化外层布局/focus trap/父级 Escape。不要复制 API hook 形成第二套配方状态。
- [ ] 无选中项目时显示可操作项目选择或回项目库入口，不能加载全局示例假数据。保存后调用 onChanged 刷新 composer 配方目录，确保真实发送能解析新模板。
- [ ] 运行 `npm test -- src/features/workspace/WorkflowLibraryDialog.test.tsx src/composer-workflow-api.test.ts src/features/settings/SettingsPanel.test.tsx`；补一条保存后 composer 能选择模板的集成测试，再提交。

配方提交后仍通过既有 Agent 执行/审批边界；它不是确定性 DAG 引擎。可用文本配方已足以交付 OmicsOps Workflows 页，图节点/串并联调度仅为可选扩展。

## 下一批可独立派发的只读后端边界

### Storage：本地元数据占用快照

目标是实际磁盘占用和路径说明，不增加删除、清理、迁移或远端扫描。新增 `src-tauri/src/storage_settings.rs` 与共享 DTO；Store 只提供现有项目根列表。原生命令 `settings_storage_usage(project_id: Option<Uuid>)` 接收项目 ID，不接受任意路径；路径由宿主已有 app_data/project 记录解析。

返回内容应包含分类项、bytes、完整性状态和扫描限制：应用 DB 及 WAL/SHM、受管 Skills/浏览器数据、选定项目目录；不将不存在的本地 Python 安装目录当成真实分类。共享/嵌套项目根必须去重，不将应用目录与项目目录重叠部分重复计入总计。默认全部项目视图只统计受管目录，用户选择单项目才扫描其显式根，避免启动时遍历海量科研数据。

扫描使用 symlink_metadata，不跟随 symlink/reparse 目录；有条目/时间上限，遇权限错误或限额返回 partial，不把未知部分当零。页面标明“逻辑文件字节数，部分扫描”而非声称精确物理磁盘分配。测试用临时目录覆盖已知大小、重复根、嵌套根、链接、缺失根、读失败与截断；不访问真实科研数据。完整实现步骤在该批派发前根据运行平台文件 API细化。

### Usage：只聚合可核验模型/工具事件

目标是跨会话 token、模型、时间及工具统计，不计货币。复用 `src-tauri/src/agent_v4.rs:3552` 的观察合并语义；新增独立查询/投影模块，避免逐会话在 UI 拉全部历史。Store 按项目与时间范围提供受限事件/观察读取；概览与会话列表分开，列表 limit 固定上界并有稳定 cursor/order。

返回字段区分已观察 tokens、缺失/中断次数、模型标识、按日计数、工具调用计数。Unknown 不能变成零；累计/增量 usage 必须按现有 attempt/sample 合并，不能重复求和；重试事件去重，不把 side chat 使用量悄悄并入主会话而失去归属。只累计有可靠身份和时间的事件，缺字段显式标记。

最小可先交付项目/会话分页总量与模型分组，页面显示真实范围；随后追加活动热图与工具排行。测试用构造事件覆盖重复 sample、累计 usage、重试、中断无 usage、跨项目、稳定分页边界、时区日界；不调用真实模型。确切 DTO 与 SQL 在读取现有事件 schema 后作为该批子计划锁定，不能先造前端图表返回示例数。

## Appearance 可并行独立任务

**现状约束：** `src/styles.css`、`src/features/workspace/workspace.css`、`src/features/projects/project-library.css` 和组件 CSS 使用大量固定色值/像素。只设置 root 变量无法生效；不得把仅 body 换色声明为全应用主题。

**文件所有权：** Appearance 实现者新增 `src/use-appearance.ts`、`src/use-appearance.test.tsx`、`src/features/settings/AppearanceSettings.tsx`、对应测试、`src/appearance.css`；可在 `src/main.tsx` 增加薄 Provider 并最后导入 CSS。不修改 B1/B2 所有者的 SettingsPanel、WorkspaceShell、settings.css。页面路由由前端所有者在组件提交后接入。

**最小接口：** `AppearanceProvider({ children })` 提供偏好与 setter；`useAppearance()` 返回 theme（system/light/dark）、uiFont（system/sans）、codeFont（system/mono）、uiScale（0.9/1/1.1/1.2）及各 setter；`AppearanceSettings({ locale })` 消费该 hook。偏好保存到独立 `omicsops.appearance.v1`，解析失败回默认，不读取任意 CSS/字体 URL。UI 明确显示“界面缩放”，不能把整体缩放伪称只改字体大小；自定义 CSS 导入不属于基础页。

- [ ] 写 hook 测试：持久化与非法值回退、system 主题跟随 matchMedia、切换显式主题后不受系统事件覆盖、卸载清理监听、localStorage 失败仍即时应用。
- [ ] 写组件测试：选择主题/字体/缩放更新同一个 Provider 状态，重挂载恢复；没有 Provider 时通过明确测试 setup，不能静默创建另一份偏好状态。
- [ ] Provider 在 html 上设置 `data-omicsops-theme` 与 UI/code font 变量；system 解析为实际 light/dark；CSS 给应用根设置字体与整体 zoom，并显式处理 100vh 主壳尺寸，防止缩放后内容裁剪。窗口 Escape 事件保持窗口坐标逻辑，不修改。
- [ ] 新 CSS 用 html data 属性提高特异性，覆盖项目库、侧栏、会话正文/代码块、输入表单、设置与对话框、popover、表格、错误/批准状态。保留成功/警告/危险语义与图像原色；禁止 filter/invert。未覆盖表面必须继续列为未验证，不能标完成。
- [ ] 运行 `npm test -- src/use-appearance.test.tsx src/features/settings/AppearanceSettings.test.tsx` 和 `npm run build`；浏览器实际检查 light/dark/system、四档缩放、项目库/工作区/设置/模型表单/审批浮层，确认对比度、滚动和不裁剪。JSDOM 属性测试不替代视觉验证。
- [ ] 独立提交组件/hook/CSS，再由 SettingsPanel 所有者加 appearance 导航并补立即 Escape/搜索测试。只有根应用与导航均接通、视觉核对通过后更新 Appearance 完成状态。

## 审阅与交付记录

- [x] 源码审计提交：`0a6ef68`。
- [x] 全 19 页范围及项目内恢复修正：`994203c`。
- [ ] B1 设置工作区与 General 核心：实现提交、检查结果与审阅结果在完成时登记。
- [ ] B2–B8：各批开工前追加可执行子计划，完成后逐页更新 master；未实现项始终保留未完成状态。
