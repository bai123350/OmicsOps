# 浏览器安全边界

> 状态：安全设计基线（2026-08-31）。这里的“必须”是 Host、桥接和扩展
> 的共同安全契约；静态检查、模拟测试或 ignored 测试都不能替代真实验收。

## 1. 安全范围

OmicsOps 浏览器能力是普通 Agent 的受控真实浏览器桥接。浏览器网页、扩展
返回值、Skill 文本、MCP 描述和搜索结果都属于不可信输入；它们不能改变 Host
的 capability、授权、研究阶段或项目路径。浏览器不是 Plan 工具，也不是
`Full Access` 的后门。

安全目标是：

- 只有用户明确授权的精确浏览器能力、目标 host、session 和协议版本才能执行；
- 只在回环的固定端口连接用户明确安装/启用的打包扩展；
- 不把 cookies、凭据、模型密钥、表单值、完整页面正文、下载正文或截图字节
  写入 SQLite、事件、日志或 Git；仅允许持久化有界结构化扫描摘录；
- 下载、截图和来源都绑定当前项目，能以大小、SHA-256、URL 脱敏值和时间复核；
- 浏览器连接、CAPTCHA、权限或不确定 dispatch 导致暂停时，保留同一个 run，
  不以静默降级或新 run 掩盖失败。

## 2. 信任边界与威胁控制

| 威胁/边界 | 强制控制 |
| --- | --- |
| 恶意网页、提示注入、伪造的“检索结论” | 页面文本只是工具输出；Host 只用成功事件推进阶段，综合时保留 URL/标题/时间/限制，不能把页面指令当作系统指令 |
| 网页 AI 或模型 API 被当作搜索工具 | 禁止自动向 ChatGPT、Gemini、Claude、Copilot、Perplexity 等网页 AI 输入提示词；`web_execute_js` 拒绝页面 AI/API 模式，扩展不转发模型凭据 |
| 端口劫持或伪造扩展 | 仅监听 `127.0.0.1` 的 `18775/18776`；严格检查 WebSocket path、`chrome-extension://<固定 ID>` Origin、协议版本、session、extension ID 和必需 capabilities |
| 跨项目/跨 run 泄露 | `workspace` profile、project root、run ID、tab ledger、授权快照分别绑定；共享连接不继承项目数据或授权；只关闭本 run 创建且仍归本 run 的 tabs |
| 审批重放、范围扩大、参数替换 | 授权绑定精确比较 capability、target host、session、protocol version；工具调用还绑定完整参数/call hash；Once 在 dispatch 前原子消费；拒绝 wildcard |
| 路径穿越、符号链接、覆盖用户文件 | 只接受项目相对路径；规范化并检查 canonical parent 在 project root 内；拒绝绝对路径、`..`、符号链接、非普通文件和工作区外目录 |
| 下载伪造或完整性损坏 | 只消费受控 staging ID；复制后计算 SHA-256 和大小，校验成功才登记产物；staging 元数据和 URL 脱敏 |
| 浏览器断线或副作用不确定 | 断线产生结构化等待事件并暂停同一 run；dispatch 不确定产生 `ToolDispatchUncertain`，需要人工核验/解析，不能盲目重试 |
| Full Access 越权 | `ApprovalPolicyV4::FullAccess` 只覆盖既有 compute 本地执行策略；浏览器仍须独立的持久化精确授权或本次 Host approval，不能绕过以下任一门禁 |

## 3. 扩展与桥接安装

打包扩展是 Manifest V3，当前 manifest 声明：

- 权限：`tabs`、`scripting`、`downloads`、`storage`、`debugger`；
- host permissions：仅 `http://*/*`、`https://*/*`；
- extension page CSP 的 `connect-src` 只允许
  `ws://127.0.0.1:18775` 和 `ws://127.0.0.1:18776`；
- `minimum_chrome_version` 为 109；
- 扩展页必须使用仓库随应用打包的静态资源，不拉取远程代码、脚本或配置。

用户必须在 OmicsOps 设置中主动选择安装/加载扩展并确认浏览器提示。安装
扩展本身不授予某个 domain、某个 run 或某个 MCP 的调用权限；域名访问仍由
Host 的浏览器授权和 URL policy 控制。卸载、停用、版本变化或 manifest/ID
变化后，已有连接必须重新握手，失败时暂停当前 run。

Tauri 优先从应用 `resource_dir/browser-extension` 取得打包资源，开发环境
才回退到仓库的 `browser-extension` 目录。安装说明必须显示实际扩展路径、
浏览器类型/版本、两个 session 的连接状态和错误原因；不能把“路径已显示”
当作“扩展已安装/握手已通过”。

桥接的握手约束是：

1. Rust listener 只绑定 `127.0.0.1`，shared 使用
   `ws://127.0.0.1:18775/v1/session/shared`，workspace 使用
   `ws://127.0.0.1:18776/v1/session/workspace`；
2. Origin 必须精确等于 `chrome-extension://joifljknpalpoppceknociillogolbnb`，
   path 必须对应期望 session；
3. hello 必须是 `protocol_version = 1`，extension ID/session 完全匹配，并
   同时提供 `tabs`、`scan`、`search`、`screenshot`、`downloads`、`debugger`；
4. 任一检查失败都拒绝连接，不接受 wildcard Origin、局域网/公网监听、未经
   批准的替代端口或未知扩展。

固定 ID 是打包 manifest 的发布契约，不应在部署时临时生成或从页面内容推断。
更新扩展时先验证 manifest/key/协议声明与 Rust 常量一致，再重新执行握手和
两平台 smoke。

## 4. 四种浏览器授权 scope

每次浏览器调用都必须得到 Host 的精确授权。授权记录由
`BrowserAuthorizationV4` 表示，binding 包含：

`capability + target_host + session + protocol_version`。

scope 只决定授权的生命周期和上下文，不能放宽 binding：

| scope | 记录的上下文 | 生命周期/匹配规则 |
| --- | --- | --- |
| `once` | 当前 `project_id + conversation_id` | 只匹配当前项目、会话和精确 binding；Host 在浏览器 dispatch 前以 SQLite `BEGIN IMMEDIATE` 原子消费，避免并发重放；相同已批准 call 的恢复仍受原 call hash 约束 |
| `conversation` | 当前 `project_id + conversation_id` | 当前会话内复用；换项目、会话、能力、host、session 或协议即不匹配 |
| `project` | 当前 `project_id`，`conversation_id = null` | 当前项目内复用；不能跨项目或改变精确 binding |
| `global` | `project_id = null`、`conversation_id = null` | 所有项目可匹配，但仍逐调用比较完整 binding；用户可随时撤销 |

批准卡对 `once`/`conversation` 绑定 project 和 conversation ID，对 `project`
只绑定 project ID，对 `global` 两者都为空；approval decision 与授权记录在同一
SQLite 事务提交，设置页不提供任意 grant 命令。`browser_revoke_authorization`
是显式撤销入口。授权 ID 由 canonical
`scope + binding + context` 的 SHA-256 得到；数据库中不得存任何密钥正文。

Host 的 target host 计算必须保持可审计：

- `web_open_tab` 使用 URL 的规范化 host（不含凭据、大小写归一）；
- `web_search` 对 `google`、`bing`、`duckduckgo` 分别绑定相应 host，工具 schema
  不接受未知/default provider；
- `web_scan`、`web_execute_js`、`web_screenshot` 和 `web_save_assets` 要求显式、
  规范化的 `target_host`；下载的每个 `source_url` 必须与该 host 完全一致；
- `browser_setup` 使用 `browser-session`，而 session 仍必须精确是 `shared` 或
  `workspace`。

`Full Access` 不会把上述授权变成全局授权，也不能跳过 per-call browser
approval、Origin/handshake、scope、URL、tab、CAPTCHA、脚本、路径、SHA 或
同 run 恢复门禁。浏览器授权与 compute approval 是两个独立的审计域。

事件链的业务终态仍不可恢复或追加普通工作。唯一例外是终态后最多一个
`BrowserTabCleanupRequired` 管理事件，用于确认仅关闭本 run 创建的标签；该
事件不携带网页正文、不改变终态，重复或其他终态后事件均拒绝。

## 5. URL、脚本与网页 AI 禁止

URL policy 只接受无首尾空白/控制字符、无用户名密码的 `http:` 或 `https:`
URL。`file:`, `data:`, `javascript:`, `chrome:`, `about:` 和带凭据 URL
必须拒绝；报告 URL 要对 `token`、`secret`、`password`、`authorization`、
`api_key`、`signature`、`code` 等敏感 query key 脱敏。

`web_execute_js` 是显式、调用方发起的受限操作，不是任意脚本沙箱或网络代理：

- Host 边界的脚本上限为 16,000 字符；扩展策略可以使用更严格的上限；
- 脚本在受控 tab 的 isolated world 执行，不获得 bridge 对象或模型 API；
- Host 和扩展都拒绝 cookie、Web Storage、IndexedDB、Credentials/Clipboard、
  Fetch/XHR/WebSocket/EventSource/Beacon 等敏感读取或页面网络 API；
- 命中网页 AI 名称、模型 API endpoint、`prompt()`/`confirm()` 等页面交互模式
  时拒绝；安全策略按 Host 与扩展策略的并集执行；
- 不允许用脚本向网页聊天框输入、提交、轮询或读取生成答案。普通搜索框、
  筛选框和翻页输入也必须先确认不是 AI prompt/生成式编辑器。
- 脚本求值结果一律丢弃，只返回执行状态；需要观察页面时必须重新调用
  `web_scan`，防止任意页面值进入 SQLite、事件或模型上下文。

如果页面要求 CAPTCHA、登录、MFA、自动化权限或用户选择，Host 追加
`BrowserHumanInterventionRequired` 并暂停同一个 run；不得尝试绕过验证，也
不得把页面 AI 当作替代来源。用户完成真实浏览器中的人工步骤后只能显式恢复，
该失败观察本身不能推进科研检索阶段。

## 6. 数据驻留、截图与模型视觉

桥接和扩展只保留完成操作所需的最小 metadata：tab/run/session ID、脱敏 URL、
标题、时间、动作状态、有限扫描摘要、来源定位和哈希。`web_scan` 的标题、
heading、链接和有界正文摘录可作为不可信科研证据进入事件链；表单值、cookies、
密码、API key、SSH 私钥、完整页面正文、完整下载正文和截图字节不得落入
SQLite、事件、日志、模型上下文或导出包。

截图必须先写入当前项目的项目相对路径，再返回 `relative_path`、`size_bytes`
和 `sha256`；SQLite/事件只保留这三个值（以及 `image/png`、来源和时间），
不保留 data URL 或图片正文。写入必须拒绝路径穿越和 symlink；替换已存在的
非普通文件也必须失败。

截图若要发送给模型，视觉能力只能按编译期精确表中的
`provider + API host + exact model id` 查询：

- 不得按 provider 家族、模型前缀、别名或模糊相似度继承 vision；
- 只有精确 profile 明确支持视觉时才读取项目文件并编码到模型请求；
- 精确 profile 不支持视觉时，可以保留已校验的截图引用，但必须告知模型和
  用户未做视觉检查，答案不得声称“看到了图”；
- 发送图像是独立的数据流转，应保留 model profile、API host、exact model id
  和能力判定的审计引用；凭据仍只存现有 keyring/credential vault。

`ModelImageRefV4` 只包含项目相对路径、媒体类型、大小和 SHA-256；Agent Core
当前将最近至多三个成功 `web_screenshot` 引用交给模型层。模型层在实际读取
前还要重新校验 root、普通文件、大小和 SHA，避免事件被篡改后发送错误文件。

## 7. tabs、共享连接与清理

扩展的 `TabLedger` 按 session 和 turn/run 记录经过脱敏的 URL、tab ID、窗口、
创建标记、触碰次数、阻断原因和关闭时间，不记录页面正文或凭据。run 创建
tab 时必须登记；导航到不允许 URL 要记为 blocked，不得偷偷改用另一个 tab。
其中用于 MV3 worker 重启恢复的 `chrome.storage.session` 副本进一步缩减为
session/run/tab ID、origin 和创建标记，不保存 URL 路径/query、标题或正文；
浏览器整体退出后该 session authority 不得自动提升为持久授权。

完成、取消、失败、断线清理或人工交接时，只关闭仍属于该 run 且由扩展创建的
tabs。run 开始前就存在的 tab、其他 run 的 tab、用户明确接管并保留的 tab 不
能被关闭。共享连接断开时不能清空另一个项目的 ledger；清理失败要留待处理
记录并向用户说明。

为覆盖 Manifest V3 worker 或桌面进程重启，持久清理摘要逐项记录 session、
run ID、tab ID 和 origin（不含正文/凭据）。恢复清理时扩展重新读取当前 tab：
不存在即确认已关闭，origin 一致才关闭，origin 不一致则按 tab ID 已复用处理并
拒绝。不能只因为重连后的内存 ledger 为空就宣称清理成功。

## 8. Windows 与 macOS 安全差异

- Windows 使用 Windows 路径和权限语义；默认发现范围为
  `PROGRAMFILES`、`PROGRAMFILES(X86)`、`LOCALAPPDATA` 下的 Chrome、Edge、
  Chromium 或 Chrome for Testing。显式路径必须指向存在的浏览器文件。停止
  进程只针对 OmicsOps 启动的 child，不能按进程名杀掉用户的其他浏览器。
- macOS 使用 `/Applications` 和用户 `Applications` 目录；浏览器自动化/辅助
  功能权限由系统控制。权限不足时显示系统设置指引、暂停同一个 run，并等待
  用户授权，不通过未公开 API 或提权绕过。
- 两个平台都只允许回环的 18775/18776；防火墙提示、扩展确认、浏览器 profile
  隔离和系统权限变化必须可审计。平台差异不能改变四种 scope、Full Access
  独立门禁、网页 AI 禁止、相对路径/SHA 或同 run 恢复。

## 9. 当前 checkout 的安全状态与阻塞项

固定 manifest key、扩展 ID、协议常量和入口资源已经统一，并由确定性 manifest/
handshake 测试覆盖。以下仍不是安全验收通过：

- 当前真实浏览器测试是 `#[ignore]`，需要显式的浏览器、扩展目录和一次性 profile；
  没有实际命令和日志的记录不得标记为通过。
- Windows/macOS 的真实下载目录、扩展权限提示、CAPTCHA、人机恢复、CDP 和标签
  清理尚需按验收清单手工执行；模拟测试不能替代这些现场边界。

发现安全阻塞时，默认行为是拒绝连接/暂停 run，而不是放宽策略。安全验收、
安装记录和未执行项模板见[浏览器验收清单](browser-acceptance.md)。
