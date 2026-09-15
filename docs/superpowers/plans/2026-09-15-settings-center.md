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
| Models | 已有部分 | 完整现有 provider 管理，ACP 独立定义后接入 | B2 + B7 |
| Quick Actions | 固定动作 | 用户动作 CRUD、工作流绑定及真实调用 | B4，依赖 Workflows |
| Workflows | 文本配方 | 可用配方库设置页；图工作流另行细化其运行协议 | B2 + B7 |
| Specialists | 未实现 | 专家角色 CRUD、模型/工具约束、真实执行选择 | B7 |
| Memory | 文件读写/搜索 | 项目记忆列表/搜索/显式变更，习惯记忆独立作用域 | B4 |
| Skills | 已有部分 | 管理实际安装源、启停、详情/文件及安全删除边界 | B2 + B7 |
| Plugins | 未实现 | 验证安装源/包、生命周期、启停/移除及工具绑定 | B7 |
| Browser | 已有部分 | 现有配置/会话/域名/授权完整可用页 | B2 |
| Connections | stdio MCP | MCP stdio 管理；HTTP/OAuth 独立支持 | B2 + B7 |
| Remote Access | SSH传输不同义 | 项目跨设备同步及消息渠道，明确设备授权和恢复 | B8 |

批次顺序不强制串行：B1 先建立设置承载与 General 核心；B2 整合已有能力；B3 接原生偏好；B4 用户内容和视觉；B5 授权/秘密；B6 只读统计；B7 新扩展运行系统；B8 跨设备/渠道和 Pet。后续批次在开工前追加明确文件、协议、测试用例和提交任务，不将本 master 当作尚未设计子系统的代码规格。

## B1 文件与接口边界

- 前端所有者：`src/features/settings/SettingsPanel.tsx`、`src/features/settings/settings.css`、`src/features/settings/SettingsPanel.test.tsx`；按职责新增共享偏好 hook 及测试；`src/features/workspace/WorkspaceShell.tsx`、`src/DesktopApp.tsx`、`src/tauri-api.ts` 及对应集成测试。
- 后端所有者：`crates/omicsops-store/src/lib.rs`、`src-tauri/src/workspace_commands.rs`、`src-tauri/src/lib.rs` 与对应 Rust 测试。
- 返回既有 Conversation 的查询不新增复制 DTO；如果新增跨边界对象，必须放 `crates/omicsops-dto` 并更新 DTO contract tests。
- 两位实现者先锁定查询命令签名再接线；前端不得扫描所有消息模拟后端候选选择。
- 初始候选接口建议：`latest_used_conversation(project_id: Uuid) -> Result<Option<Conversation>, String>`；最终派发契约覆盖该建议并记录在本计划。

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
- [ ] 在 Store 用 `conversation_records`、`messages` 的真实 schema 写 EXISTS user 内容约束及消息时间排序；复用 `conversation_from_row`。正文存储格式按现有解码约定检查，不能把序列化空消息误算为 user 内容。
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

## 审阅与交付记录

- [x] 源码审计提交：`0a6ef68`。
- [x] 全 19 页范围及项目内恢复修正：`994203c`。
- [ ] B1 设置工作区与 General 核心：实现提交、检查结果与审阅结果在完成时登记。
- [ ] B2–B8：各批开工前追加可执行子计划，完成后逐页更新 master；未实现项始终保留未完成状态。
