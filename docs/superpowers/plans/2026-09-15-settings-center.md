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

2026-09-21：19个目标页面及额外Privacy均已实现接通，基础适配范围闭合。以下各页的旧“最终全套待跑/验收待做”指早期批次状态；最终确定性检查、桌面构建与真实环境未执行项统一以[验证记录](../specs/2026-09-21-settings-verification.md)为准。原计划中的详细任务框保留设计过程，当前交付状态以本表和验证记录为准。

状态“已有部分”只表示基线已有可复用功能。完成标准要求实现、验证、审阅和提交记录全部齐全。

| 页面 | 当前状态 | 可用页的交付范围 | 批次/依赖 |
|---|---|---|---|
| General | G1目录预检、G2选择操作及21a514f缩放修复、G3原生通知0127562均已接入提交；真实OS通知验收未执行 | 语言、共享发送偏好、项目内恢复、选择操作、通知、实际目录语义、真实更新状态及环境/网络入口 | B1 + B3 |
| Session | 基础行为已核对；完整检查见验证记录 | 现有迭代/继续/压缩/后续问题偏好完整管理与反馈 | B2；baseline-review |
| Appearance | 616dbae 已提交及接入；已做指定表面视觉检查 | 实际主题、字体与界面缩放；自定义CSS导入不在基础范围 | B4 |
| Pet | 5bd7213 + 85c78ac 已提交并接入 | 内置窗口内伴侣、外观与启停；运行状态目前仅有可选组件接口，未连接实际运行；自定义资源/独立桌面窗口不在基础范围 | B8 |
| Credentials | 34afb53 + 26c4438 已提交接入，定向验证通过；完整检查见验证记录 | 非敏感引用目录、受限替换、自定义keyring创建及未引用项删除；列表快照串行化 | B5；独立credentials计划 |
| Permissions | 8eb938e + 70e9917 已提交并接入 | 真实 scope 授权列表及逐项撤销，不改冻结计划；竞态修复已测 | B5 |
| Environments | c306e0a 已补system探测、新SSH入口、busy/绑定反馈及切项目竞态；定向通过；完整检查见验证记录 | system真实探测、SSH 配置/信任/运行环境管理；backend目录不等于解释器探测 | B2；baseline-review |
| Storage | 6745ee3 + 85c78ac 已接入；901bb76 布局修复及有数据fixture视觉通过；完整检查见验证记录 | 真实应用/项目占用统计、范围筛选；不扩为清理；fixture不证明真实扫描 | B6 |
| Usage | 57dfbc4 整页接入、13d9c57 长内容布局已提交验证；完整检查见验证记录 | 跨会话 token/模型/UTC日周/工具统计、项目与会话分页 | B6；独立usage计划 |
| Models | 基础行为已核对；完整检查见验证记录 | 完整现有 provider 管理；ACP 为可选扩展，不作为页面完成前提 | B2 + B7；baseline-review |
| Quick Actions | 9f2ea7a + 4051103 + 3931cad 已提交接入，定向验证通过；完整检查见验证记录 | 用户动作 CRUD、工作流绑定及可见草稿插入，不自动提交 | B4，依赖 Workflows |
| Workflows | fcbc589 已提交并接入 | 可用文本配方库设置页及真实调用；图执行引擎为可选扩展 | B2 + B7 |
| Specialists | 9f2ea7a + 4051103 + 3931cad 已提交接入，定向验证通过；完整检查见验证记录 | 项目角色模板CRUD、启停与可见草稿插入；独立专家运行、模型/工具白名单为可选扩展 | B7 |
| Memory | 基础CRUD/导航已提交；2837101 + 1cd6088 补齐待处理草稿保护和文件名筛选；完整检查见验证记录 | 项目记忆文件列表、按文件名筛选与显式增改删；无正文全文搜索或全局习惯作用域承诺 | B4 |
| Skills | S1/S2及S3 UI 9fc6824、native 343e06a均已接入；24项原生定向测试及生命周期复审通过；真实原生手工验收未执行 | 实际安装源与启停、有界详情/文件读取、受管删除、持久化清理重试及运行/依赖保护 | B2 + B7；baseline-review |
| Plugins | b851013 + 1748cc9已实现接入；native 9、契约1、前端/API 8通过，恢复逻辑复审通过；原生手工验收未执行 | 本地声明包验证/安装/更新/启停/安全移除及所有权绑定 | B7；plugins-general计划 |
| Browser | ce37b5f 已修首次加载失败后的保存门控，定向验证通过；完整检查见验证记录 | 现有配置/会话/域名/授权管理，失败不能以默认值覆盖未知配置 | B2；baseline-review |
| Connections | b5c3156 + 60c4b69 完整页修复已提交，定向与输入焦点检查通过；完整检查见验证记录 | MCP stdio 管理与真实检查/授权，环境变量编辑保留与输入焦点已修复；HTTP/OAuth 为可选扩展 | B2 + B7；baseline-review |
| Remote Access | 6c2a40d + 49743f5 已提交并接入 | 现有项目本地/SSH传输与同步管理，明确范围/状态/恢复；上游渠道平台不在基础页范围 | B8 |

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

## 后续页面的最小真实功能与可派发切片

以下任务不修改正在整合的 SettingsPanel/DesktopApp/WorkspaceShell，也不占用 Storage 实现者的共享 DTO/native 注册文件。先由独立实现者提交组件、API 模块、后端纯函数和测试，再由对应所有者统一接线。只有接线完成且真实操作通过才更新页面完成状态；单独组件提交只是中间交付。

### Pet：窗口内研究伴侣

**文件：** 新增 `src/features/settings/PetSettings.tsx`、`src/features/workspace/ResearchCompanion.tsx`、`src/use-pet-preference.ts` 及测试/CSS。根所有者随后在工作区挂载伴侣并注册设置页。

**接口：** 偏好 `enabled: boolean`，只存 `omicsops.pet.enabled`；`ResearchCompanion({ status: "idle" | "running" | "needs_attention", onOpenCurrentRun })` 展示内置 SVG/CSS 伴侣及来自当前运行的状态，不生成或冒充科研建议。设置页实时预览与开关实际控制工作区显示；隐藏不影响运行。

- [ ] 测试关闭后不渲染、重载保存、状态从 props 更新、点击查看运行只调用显式回调；reduced-motion 下停止装饰动画。
- [ ] 实现可访问的静态/轻动效组件，不创建原生独立桌面窗口、不下载资源、不轮询模型；可关闭浮层进入窗口 Escape 栈，避免遮挡输入和审批。
- [ ] 定向测试、构建、实际检查小窗口与120%缩放；根集成后才登记 Pet 页可用。上游自定义资源目录/桌面游走宠物不在基础范围。

### Quick Actions：工作流快捷插入

**文件：** 新增 `src/features/settings/QuickActionsSettings.tsx`、`src/quick-actions-api.ts`；后端 `src-tauri/src/quick_actions.rs` 与临时 Store 测试。共享 DTO 与注册在 Storage 所有者完成后由主代理一次接入。

**接口：** 项目拥有 `QuickAction { id, project_id, name, description, workflow_id, enabled }`；list/save/delete 命令按项目校验，存 Store JSON kind `quick_action_v1`，上限100条/name100字/description500字。保存校验引用配方属于当前项目；插入时再次确认存在且启用。前端回调 `onInsertWorkflow(workflowId)` 使用既有 ComposerReference workflow 解析，不自动提交。

- [ ] 写后端测试：跨项目引用拒绝、悬空引用返回明确不可用、编辑不改变owner、删除只删除动作不删除配方、启停持久化。
- [ ] 写UI测试：新增/编辑/禁用/删除调用真实API，按钮显式插入一次；被禁用/删除的配方不可插入；未选项目能选择项目而不是创建假全局动作。
- [ ] 在独立组件中完成CRUD与错误反馈，最后由工作区所有者接入 onInsertWorkflow，保留草稿和已有引用，数量限制仍由既有composer边界裁决。

### Specialists：明确标识为角色模板

**文件：** 新增 `src/features/settings/SpecialistsSettings.tsx`、`src/specialists-api.ts`、`src-tauri/src/specialist_templates.rs` 及测试。

**接口：** 项目角色模板 `SpecialistTemplate { id, project_id, name, description, prompt, enabled }`，Store JSON `specialist_template_v1`；list/save/delete。prompt 非空、UTF-8字节上限16KiB、name100字；执行前使用既有秘密内容校验。回调 `onInsertPrompt(text)` 把带可见模板名称的文本插入当前用户草稿，不作为隐藏系统提示、不自动选工具或修改审批。

- [ ] 测试跨项目隔离、输入限制、启停删除、保存失败保留编辑；插入保留原草稿，只产生一次用户可见文本，不调用发送/启动 API。
- [ ] 预置内容可作为可复制示例，不冒充已执行的子Agent；用户可编辑创建自己的科研角色模板。
- [ ] 页面显式“角色模板，插入后由你发送”，工作区旧 disabled Specialist 菜单由整合所有者换成真实模板选择。模型白名单、独立专家运行、并行编排属于可选后续，不能声明本批已实现。

### Plugins：受管声明包，不重复包装已有 MCP 注册

现有 `src-tauri/src/bundled_mcp_commands.rs:51` 启动即注册全部内置 MCP，`add_bundled_mcp_server` 幂等补登记。因此仅把“添加内置MCP”改名“安装插件”不算交付。基础包应确有新安装清单和受管文件，同时只复用允许的声明能力。

**文件：** 新增 `src-tauri/src/integration_packages.rs`、`src/integration-packages-api.ts`、`src/features/settings/PluginsSettings.tsx`、包安装测试。先锁定本地包格式，再接注册；不与 skills/MCP owner 并行修改同一旧文件。

**最小格式：** 本地目录中的版本化 manifest，声明 package id/version、包内相对 SKILL.md 文件及其 SHA256、已编译 MCP preset ID 引用。禁止自定义二进制、安装脚本、任意命令、绝对/父目录/链接路径及未经支持远程 URL。清单和字节摘要决定安装内容，包不是新的审批主体。

**生命周期：** 先只读检查并给出文件/绑定预览，再用户点击安装；安装到独立受管目录，持久化安装清单和状态后才登记可发现的技能/内置连接。默认不授予启动或工具权限；失败返回具体阶段、保留可重试状态，不宣称已安装。移除只停用并删除本包拥有且摘要未被用户修改的文件/绑定；其他连接/技能与冻结运行引用保留，不能批量删除共享对象。已有安装按同摘要幂等；不同版本先明确更新差异，不盲目覆盖。

- [ ] 先实现 manifest解析/路径与hash验证/受管临时目录安装的纯函数测试：路径穿越、symlink、篡改hash、重复安装、写失败和用户修改文件保护。
- [ ] 用模拟注册器测试声明登记，不起进程、不授权；后端状态查询证明安装结果，UI列表/详情/移除接真实命令。
- [ ] 本页与 Skills/Connections 的区别是包来源、版本和所有权生命周期；具体技能启停、连接检查与授权仍转到对应页面。ZIP/远程仓库/市场和任意脚本插件不在基础包范围。

### Remote Access：项目 SSH 文件传输与同步

**文件：** 新增 `src/features/settings/RemoteAccessSettings.tsx` 与测试。复用 `src/tauri-api.ts` 的 listSyncEntries/pauseSyncTransfer/cancelSyncTransfer/retrySyncTransfer、uploadSelectedFiles/downloadProjectFile；父级传选定项目和现有文件选择入口。

**接口：** 组件以 projectId/绑定的connection与remoteRoot为范围，显示真实传输条目、方向/路径/状态、显式上传/下载和状态允许的暂停/取消/重试。列表异步返回必须按项目代次丢弃迟到结果；同一条目操作期间禁重复。

- [ ] 测试无SSH绑定时提供Environments绑定入口，不能假装通道在线；切项目不泄漏旧列表；上传只接项目内选择，下载冲突保留实际返回状态。
- [ ] 测试暂停/取消/重试失败保留原记录并显示可重试错误，刷新读取真实状态；取消文件传输绝不能标记Agent/远端计算取消。
- [ ] 页名说明“项目SSH传输”，显式记录与Wisp的relay/shared-folder/消息渠道不同；不宣称跨设备账户同步。现有选择性数据传输本身是可用页面，不要求实现飞书/微信平台。

## Memory、Credentials、Permissions、Usage 的后端小切片

| 切片 | 实际基础与最小新增 | 必须通过的测试 / 不能伪造的边界 |
|---|---|---|
| Memory 文件管理 | `project_memory.rs` 已有files/read/save且save不覆盖；新增按projectId+安全basename的list/read/create/update/delete命令，update/delete带expected SHA256，写入用同目录临时文件与原子替换；只访问 `.omicsops/memory` | 跨项目/路径穿越/保留名/链接拒绝，256KiB限制，摘要冲突不覆盖，删除不扫整个目录，写入沿用脱敏。UI显示文件真相源，不声称全局习惯或自动推理记忆。 |
| Credentials 目录与受限写入 | vault已有get/set/delete但禁止向UI回传get值。先从模型/SSH/MCP引用及内置NCBI配置生成非秘密使用目录；修改通过既有owner保存路径，新自定义凭据使用受管命名空间并只返回存在状态 | 未知/其他命名空间拒绝，秘密不进Store/错误/事件；删除有使用者时返回依赖而非破坏共享引用；不要实现任意keyring账户枚举。 |
| Permissions 汇总与撤销 | 已有MCP launch/tool审批和browser授权查询/撤销；新增合并DTO或前端独立查询，保留真实类型及scope；调用现有撤销函数，不能写通用grant表替代真实授权 | MCP撤销启动时同时停用并清工具授权、session invalidate；browser撤销按ID；失败不假刷新为已撤销；冻结运行快照不被此设置改写。新授权仍通过原有审查入口。 |
| Usage 分页/汇总 | 使用现有ModelUsageObserved与attempt合并；先项目范围概览+稳定会话分页+模型分组，后追加日计数/工具排行 | 有限查询、重复sample/累积报告去重、重试与缺失保持可见、附件/旁聊归属不混淆、跨项目隔离；未知不变0，不造货币价格。 |

每个切片先由后端代理在新模块/测试定义契约，再安排 DTO/注册 owner 接入，避免和当前 Storage 并行编辑同一共享文件。审阅必须看真实调用与持久化结果，组件mock通过不能替代后端验证。

## 审阅与交付记录

### 剩余页已锁定的派发规格

- [Windows最终手工smoke清单](../specs/2026-09-16-settings-windows-smoke.md)：19目标页及额外Privacy、窗口/隔离/草稿、测试keyring、通知、Skills和Plugins；步骤不等于通过记录，待实现项不得勾选。

- [六个基础页调用核验](../specs/2026-09-15-settings-baseline-review.md)：Session/Models基础功能已连接；Environments/Skills/Browser/Connections的具体缺口与最小测试要求已列明。代码阅读不是最终验收。

- [Credentials独立计划](2026-09-15-credentials-settings.md)：真实keyring目录/创建/替换/未引用删除；canonical owner、防alias绕过、expected reference及MCP删除竞态已定义。
- [Usage独立计划](2026-09-15-usage-settings.md)：固定事件快照、完整run分页合并、项目/会话/模型/日周/工具；不估账单、不把缺失当0。
- [Plugins与General剩余切片](2026-09-15-plugins-general-settings.md)：本地声明包安装/更新/启停/安全移除与恢复；General真实目录选择起始位置、正文选择动作、原生通知、版本/缺源状态及环境网络实际入口。语言/发送/恢复沿用B1。

以上独立计划细化并优先于同页较早的mini-slice接口；适配边界仍保留。没有将说明、mock或孤立组件登记为整页完成。

### 本轮已提交里程碑和验证范围

- 主代理报告：Credentials `34afb53` 原始native定向11 tests、frontend/API/SettingsPanel 64 tests、Web build及desktop build通过；`26c4438` 凭据快照串行化修复后Credentials 7 tests通过。该记录不等于真实服务端密钥或SSH验收。
- 主代理报告：Memory `2837101` 待处理草稿保护、`1cd6088` 文件名筛选已提交，最终定向11 tests通过；Browser `ce37b5f` 保存baseline门控定向4 tests通过。
- 主代理报告：Quick Actions/Specialists工作区插入接线 `4051103`、卡片间距修复 `3931cad` 已提交，templates整合153 tests及Web build通过；这是用户可见草稿插入，不是独立专家运行。
- 主代理新增视觉检查：该批dist/Vite preview、headless Edge 720×600、显式dark与OS light相反、uiScale1.2，逐一实际点击当时18个导航（含额外Privacy，Usage/Plugins尚未进入该dist）；所有 `.settings-layout > main` 的clientWidth=scrollWidth=410，document width=viewport720。无Tauri宿主/未选项目，只证明导航和小窗页面框架无横溢；不能证明有数据时完整页面或原生行为。随后Storage数据fixture验证见下项。
- 主代理报告：Connections `b5c3156`完整页修复，native MCP 25/25、Settings+API 61/61、Web build通过；`60c4b69`变量名逐字符失焦修复，Settings 59/59、Web build通过。根Edge 720×600使用keyboard.type('NCBI_EMAIL')实际得到完整值，焦点仍在env name 1；此检查证明真实键盘输入行为，不替代真实MCP进程/服务验收。
- 主代理报告：Usage native `ff03b2b` + `b7aace8`，Store最终5/5、native 7/7通过；整页 `57dfbc4` 的API 2 + Usage 9 + Settings 60 + Desktop 77 = 148/148及Web build通过。
- 主代理报告：Storage `901bb76` CSS修复，定向3/3及Web build通过；根已查看有数据fixture截图，1440×1000与720×600、dark、uiScale1.2且OS light，内容clientWidth/scrollWidth分别956/956和410/410，无横溢。当前有效图为 `C:/Users/jindong/.codex/visualizations/2026/09/15/01a0a4d8-424b-7122-b3e4-925d0df0a256/settings-storage-ui-fixture-verified.png`，旧 `storage-ui-fixture.png`已过时。mock响应只证明布局，不证明真实磁盘扫描。
- 主代理报告：共因长内容布局 `13d9c57`，Usage 9 + Storage 3 + Settings 61 = 73/73、Web build及Rust workspace全套通过。根actual dist、Edge 720×600、dark120%验证：Storage 15分类main clientHeight/scrollHeight为436/1830，各卡clientWidth=scrollWidth；Usage main高度436/1424，6个统计section均clientWidth=scrollWidth，均已滚至底部。截图目录 `C:/Users/jindong/.codex/visualizations/2026/09/15/01a0a4d8-424b-7122-b3e4-925d0df0a256/` 下 `settings-storage-long-content-verified.png`、`settings-usage-long-content-verified.png`；旧 `settings-usage-ui-fixture.png`为缺陷图。这些数据fixture证明布局，不是原生统计端到端验收。
- 主代理报告：General G1 native `b67f2c2`，7/7及desktop build通过；UI `75bdf7e`，72/72及Web build通过。根mock native fixture的found/missing呈现正确，main高度436/1100、宽410/410，截图同目录 `settings-general-native-fixture.png`；不是真解释器验收。目录失效宿主预检/ProjectLibrary notice后由 `8db2719`补齐，native 8、frontend 12及Web build通过。G2 `a3e4c7b`全部接入，164 tests及Web build通过；G3通知待做，General仍部分。
- 主代理报告：Environments `c306e0a` 已接system probe、新SSH入口、字段busy、绑定反馈及防旧绑定拉回项目；Settings 64、Settings + Desktop 142/142及Web build通过。
- 主代理报告：Skills S1 `d7ba5e9` 来源receipt切片，adapter 2/2、native 16/16、fmt通过；父junction修复 `f18f45b`已提交，S2 native `831a659`有界详情/文件预览已提交，native 11 tests通过；UI/S3管理仍进行。基础功能剩余General G3通知、Skills管理、Plugins，不能将局部提交标为整页完成。
- 主代理报告：General G2根实际Edge dist + mock Tauri消息UI检查通过：旧草稿保留、引用追加、未调用发送mutation、立即window Escape关闭选区toolbar、textarea.setSelectionRange不触发toolbar。这是UI mock验证，不是模型/SSH端到端验收。
- 上述新增提交之后，最终 `cargo test --workspace`、`npm test`、`npm run build` 和必要desktop build仍待统一重跑。某次Vitest阶段711项通过后，npm参数被末尾node --test解析导致整体命令失败；不能记录成完整 `npm test` 成功。

- 主代理报告：Memory/Remote Access导航 `49743f5`，相邻142 tests及build通过；Permissions竞态修复 `70e9917`，61 tests及build通过。
- 主代理报告：templates native `9f2ea7a` 后执行新鲜 `cargo test --workspace`，exit 0；真实SSH/model/PBMC ignored验收仍未执行。后续凭据/用量修改后须重新运行最终完整检查。
- 主代理报告：前端79 files、664 tests及22 tests通过；Browser低对比修复 `45b4404` 后headless Edge 1440×1000、dark、uiScale1.2，6个Browser标签computed color均为rgb(242,244,247)，立即Escape关闭设置。Connections/Browser布局clientWidth=scrollWidth=1200（CSS缩放前）无横向溢出。
- 已实际打开General/Connections/Browser/Permissions/Memory/Remote Access/Storage/Pet导航；此次预览无Tauri宿主、无选定项目，只证明UI导航/布局，不证明原生命令、真实项目数据或网络运行验收。
- 当前有效截图：`C:/Users/jindong/.codex/visualizations/2026/09/15/01a0a4d8-424b-7122-b3e4-925d0df0a256/settings-browser-dark-verified.png`。较早review截图已被替代；其他表面或平台不据此视为视觉全部通过。

- [x] 源码审计提交：`0a6ef68`。
- [x] 全 19 页范围及项目内恢复修正：`994203c`。
- [x] B1 设置工作区/共享快捷键：`64ad747`；项目候选查询：`470906d`；恢复接线：`3779c6c`。定向检查已通过（主代理记录），完整检查待整合；General 仍缺其余字段。
- [x] Appearance 组件、持久化与根接入：`616dbae`。主代理已检查 Edge 大小窗口/dark 120% 并修布局对比；完整检查和最终导航整合仍待记录，不能据此标全部设置完成。
- [ ] B2–B8：各批开工前追加可执行子计划，完成后逐页更新 master；未实现项始终保留未完成状态。
