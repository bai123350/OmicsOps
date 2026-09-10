# 浏览器运行时（普通 Agent）

> 状态：2026-08-31 的实现边界与验收基线。本文描述必须保持的行为，
> 不表示真实浏览器、扩展安装、跨平台或生产部署已经验收通过。

## 1. 范围与不变边界

浏览器检索是普通 Agent 的执行能力：用户直接发送消息，由
`agent_v4_start_direct` 建立普通 Agent run 后，Host 才可以按本页编排真实
浏览器。它服务于论文、外部数据库和当前网页证据的检索与交叉核验。

本功能只改变普通 Agent 的研究检索路径，不改变：

- `agent_v4_start_planning` 的 Plan 生成、Plan 只读工具门禁、revision/hash
  绑定、批准/修改/取消流程；
- Plan 被批准后的既有计划执行契约、冻结 capability、compute approval 或
  恢复语义；
- 任何把网页 AI 聊天框当作检索来源、向网页 AI 自动发送提示词或把普通
  HTTP 抓取当作真实浏览器的旁路。

Plan 阶段不启动真实浏览器，浏览器工具也不因 Plan 或批准动作而自动加入。
本页的研究检索门控只适用于普通 Agent；不要把 Plan 的只读批准或批准后的
计划执行描述成浏览器检索授权。

## 2. 按需浏览器调用

2026-09-10 起，普通 Agent 不再要求 route、MCP、Skill、浏览器固定先后顺序，
也不因已有专业 MCP 结果而强制再跑浏览器。参见
[自适应循环设计](superpowers/specs/2026-09-10-adaptive-ordinary-agent.md)。

模型可按任务选择专业来源、网页搜索或直接打开已知 URL。浏览器必须已连接且
获得宿主授权；导航或页面变化后，先等待稳定并 web_scan 再依赖页面内容。
来源应可定位，Skill 是方法指导，失败、派发和模型文字不能充当成功证据。
这些能力选择不改变下述目标、连接、权限、下载、恢复与隔离边界。

## 3. Host 与 Tauri 接口

`crates/omicsops-browser` 的 `BrowserRuntime` 保存连接和两个 session 的状态，
不独立持久化页面扫描、cookies、凭据或截图字节。当前 Tauri 命令由
`src-tauri/src/browser_commands.rs` 暴露：

| 命令/接口 | 作用 |
| --- | --- |
| `browser_get_settings` / `browser_save_settings` | 读取或保存 `BrowserConfig`，并返回 shared/workspace 状态和扩展路径 |
| `browser_status(session)` | 返回 session、端口、监听/连接状态、协议版本、扩展 ID 和能力集合 |
| `browser_setup(session, launch_if_needed)` | 确保回环 listener，并按设置连接或启动浏览器 |
| `browser_list_authorizations` / `browser_revoke_authorization` | 列出或撤销浏览器授权；新增授权只能由工具批准卡与 approval event 原子创建，设置页没有任意 grant 接口 |
| `BrowserRuntime::call(session, run_id, method, payload)` | 发送带同一 `run_id` 的桥接命令 |
| `save_screenshot(project_root, relative_path, data_url)` | 校验 PNG data URL，写入项目相对路径并返回大小/SHA-256 |
| `save_staged_asset(project_root, staged_id, relative_path)` | 从安全的下载 staging 区复制普通文件并返回大小/SHA-256 |
| `close_run_tabs(session, run_id)` | 仅请求关闭该 run 拥有的 tabs |

Host 暴露给普通 Agent 的稳定工具 ID 为：

`browser_setup`、`web_search`、`web_open_tab`、`web_scan`、
`web_execute_js`、`web_screenshot`、`web_save_assets`。

它们都是受 Host 授权约束的网络效果；不是任意 JavaScript、任意文件写入、
代理配置、扩展安装或网页 AI prompt 工具。`web_scan` 的 `page_kind` 只允许
`search_results` 或 `source`，`web_open_tab` 只允许不含凭据或敏感 query key
的 HTTP(S) URL。`web_execute_js` 在隔离 world 执行，禁止凭据/存储/剪贴板和
页面网络 API，且丢弃求值结果；动作后必须用 `web_scan` 取得结构化观察。

## 4. 连接、端口与 session

桥接只绑定回环地址，端口是固定协议的一部分：

| session | WebSocket endpoint | 用途 |
| --- | --- | --- |
| `shared` | `ws://127.0.0.1:18775/v1/session/shared` | 受 Host 管理、可供多个项目使用的共享连接 |
| `workspace` | `ws://127.0.0.1:18776/v1/session/workspace` | 当前工作区的隔离 profile/连接 |

端口冲突、远程监听、路径不匹配、Origin 不匹配、协议版本不匹配、扩展 ID
不匹配或缺少必需能力时，连接必须拒绝并报告可解释的原因；不能静默换用
其他端口、远程代理或未经批准的连接。

桥接协议当前为 `PROTOCOL_VERSION = 1`。握手必须同时匹配期望的 session、
固定扩展 ID 和下列能力：`tabs`、`scan`、`search`、`screenshot`、
`downloads`、`debugger`。连接成功后，命令帧携带 `request_id`、同一 run 的
UUID、method 和对象 payload；reply 的 `request_id` 必须对应等待中的请求。

`shared` 与 `workspace` 是连接边界，不是授权继承边界。每个 run 仍保存自己
的授权快照、tab ledger、来源和审计事件；共享连接不得使一个项目读取另一项目
的页面、下载或上下文。

## 5. 暂停、恢复与结束

浏览器连接是 run 的一部分。连接未就绪、用户需要完成 CAPTCHA/系统授权或
浏览器命令失败时，Host 必须保留已完成阶段和待处理动作，并把 run 置于可解释
的等待/失败状态；不伪造结果，不切换到模拟网页、普通 HTTP 抓取或新 run。

典型的同 run 恢复流程是：

1. 浏览器未连接时，产生失败的浏览器工具结果和
   `BrowserConnectionRequired`（`error_kind =
   "browser_connection_required"`），UI 显示连接指南和当前 run ID。
   页面检测到 CAPTCHA 时则产生
   `BrowserHumanInterventionRequired`（`error_kind =
   "human_intervention_required"`），同一 run 暂停且明确说明 OmicsOps 不会
   自动求解 CAPTCHA。
2. 用户安装/启用正确扩展、解决 CAPTCHA 或补充系统权限后，Host 重新检查
   session、端口、Origin、协议、工作区和授权，再从该 run 的事件链继续。
3. 已经成功的只读阶段只复用持久化结果；待处理动作用相同 `run_id` 和新的
   `call_id`（或安全的幂等结果复用）继续。不得让用户“重新开始”来掩盖未完成
   的研究，也不得把暂停回答成完成。

浏览器 Network 调用在 dispatch 后发生传输不确定时，必须遵守现有
`ToolDispatchUncertain`/人工核验语义，不能盲目重试可能产生副作用的动作。
取消、失败或成功结束时，Host 请求清理该 run 创建的 tabs；清理失败要记录
待处理项。用户在交接时明确保留的 tab 从 run-owned 集合移除，run 开始前存在
的 tabs 永远不被清理。

运行结果终态写入后，协议只允许再追加一次管理性
`BrowserTabCleanupRequired`，供自动关闭失败或关闭策略禁用时显示确认卡；它不
改变 run 的成功/失败终态。任何第二个清理提示或其他终态后业务事件仍按损坏的
事件链拒绝。

清理事件的每个 tab 摘要都绑定自己的 `session + run_id + tab_id + origin`。
Rust Runtime 会跨 WebSocket 断线保留最后一次经校验的摘要；即使桌面进程或
Manifest V3 service worker 重启，确认操作也会从持久事件恢复目标。扩展只在
`chrome.storage.session` 中的最小所有权账本仍证明该 tab 归属，且当前 origin 与
记录一致时关闭；tab 已不存在视为已关闭，账本丢失或 ID 被其他 origin 复用时
拒绝，避免误关用户标签。session 副本不含 URL 路径/query、标题、正文或凭据。

## 6. 下载、截图与模型视觉

下载和截图都落在当前项目工作区内：

- 目标必须是非空项目相对路径；拒绝绝对路径、`.`/`..`、路径穿越、工作区外
  符号链接、非普通文件和未经授权的共享目录。
- `web_screenshot` 只接受 PNG data URL。保存后返回并审计
  `relative_path`、`size_bytes`、`sha256`；SQLite 只保存这些元数据、媒体类型、
  来源和时间，不保存截图正文。
- `web_save_assets` 只接受批准 host 下的 HTTP(S) `source_url`；扩展生成不可预测的 staging ID 并下载到
  `Downloads/OmicsOps-Staging/<staged_id>` 读取普通文件，再复制到项目相对路径；
  staging ID、文件名、URL 和 SHA 都要经过校验/脱敏。
- `web_scan` 的有界结构化摘要（标题、heading、链接和有限正文摘录）作为科研
  证据进入事件链；表单值、cookies、凭据、完整页面正文、完整截图和下载正文
  不进入事件或 SQLite。截图/下载只保存来源定位和哈希/产物引用。

截图如果要送给模型，必须通过编译期的精确模型能力表判断视觉能力，键为
`provider + API host + exact model id`。禁止按 provider 家族、模型前缀或模糊
匹配推断 vision；同一厂商的相邻 model id 不能互相继承能力。Host 只在该精确
profile 标记支持视觉时读取并编码截图，否则向模型说明“已保存并校验，但未做
视觉检查”，最终答案不得声称模型看过图片。`ModelImageRefV4` 只携带项目相对
路径、`image/png`、大小和 SHA-256；当前执行上下文最多取最近三个成功截图引用。

## 7. Windows 与 macOS

两平台共用上面的 Host 阶段、端口、授权、事件和数据契约，差异只在启动/附加、
进程管理、路径和系统权限：

- Windows：显式浏览器路径必须是存在的文件；未填写时按 Windows 的
  `PROGRAMFILES`、`PROGRAMFILES(X86)`、`LOCALAPPDATA` 发现 Chrome、Edge、
  Chromium 或 Chrome for Testing。workspace 启动使用项目数据根下的
  `browser/workspace-profile` 和打包扩展；结束时只停止 OmicsOps 启动的子进程，
  不能通过杀掉用户其他浏览器进程来清理。
- macOS：按 `/Applications` 和用户 `Applications` 目录发现支持的浏览器；
  若用户未授予浏览器自动化/辅助功能权限，显示可操作提示并暂停同一个 run，
  不能绕过系统权限。路径使用 macOS 语义，不能把 Unix 路径规则套到 Windows。
- 两平台：只接受 `127.0.0.1`，拒绝将 `18775/18776` 暴露到局域网/公网；
  URL、脚本、下载、截图、SHA、网页 AI 拦截和 Full Access 边界一致。

## 8. 当前 checkout 的实现状态与未验收项

这些是当前工作树的实现事实，不是生产验收结果：

- manifest 的固定 `key`、Rust bridge、扩展协议和测试已统一到 extension ID
  `joifljknpalpoppceknociillogolbnb`；manifest 引用的 service worker、content
  script 和 session 入口均已纳入打包资源。
- Rust 与 Settings UI 都只使用 `default/google/bing/duckduckgo`；Agent 工具调用
  必须使用 setup 结果给出的显式 `google/bing/duckduckgo` provider。
- 扩展的 Node 确定性测试和 Rust bridge 模拟测试可以验证协议与策略，但不能
  代替 Chrome/Edge 实际安装、Origin、下载目录和 CDP smoke。
- 当前 Tauri executor 对 `NotConnected` 会产生
  `browser_connection_required`/`BrowserConnectionRequired`；其他 transport、
  command 或 CAPTCHA 文本错误会分别落为 `browser_command_failed` 或
  `human_intervention_required` 的 recoverable outcome；后者会追加
  `BrowserHumanInterventionRequired` 并进入可见的同 run 暂停。真实浏览器上的
  CAPTCHA 恢复仍须按验收清单执行，不能把模拟事件测试描述为现场验收。

真实网页、安装、跨平台和 ignored live test 的状态统一以
[浏览器验收清单](browser-acceptance.md) 为准；未执行的项目必须保持
“未执行/阻塞”，不得写成“已验证”。
