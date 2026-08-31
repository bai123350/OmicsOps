# OmicsOps 普通 Agent 浏览器科研检索设计

> 日期：2026-08-31
> 状态：实现设计基线；真实浏览器、扩展安装、Windows/macOS 和生产部署
> 验收仍未完成时，不能把本设计当作验收报告。

## 1. 摘要

普通 Agent 在用户明确授权后，可以通过本机受控的真实浏览器补足当前网页
证据。Host 先用结构化来源和方法知识，再在必要时使用真实浏览器；网页内容
始终是不可信输入，不能改变权限或阶段。

Wisp Science 只作为流程和浏览器桥接的架构参考。OmicsOps 的 Rust runtime、
协议、Manifest V3 扩展、资源和测试均为独立 clean-room 实现，不复制或改写
Wisp 的 AGPL 代码；OmicsOps 继续遵循仓库的 Apache-2.0 许可边界。

本设计的唯一新增产品路径是普通 Agent 的研究检索。它不改变 Plan 生成、
Plan revision/approval/revise/cancel，也不改变批准后既有计划执行的冻结
capability、compute approval 和恢复契约。Plan 不启动真实浏览器；批准 Plan
本身不授予浏览器授权。

## 2. 目标、非目标与术语

### 2.1 目标

1. 把论文、外部数据库和当前网页证据串成可审计的同一 run 来源链。
2. 强制 `MCP → skill/use_skill → 专业 MCP/结构化不可用 → 真实浏览器搜索 → 结果扫描 →（有结果时）独立落地页 → 综合` 的顺序。
3. 只有成功的持久化事件才能推进阶段；断线、CAPTCHA、权限和审批都能在
   同一 run 中暂停/恢复。
4. 用精确 capability/host/session/protocol 授权真实浏览器，且让 compute
   `Full Access` 永远不能绕过浏览器边界。
5. 下载和截图只留下项目相对路径、大小、SHA-256 等最小可复核 metadata；
   视觉输入严格按 `provider + API host + exact model id` 的能力查找。

### 2.2 非目标

- 不在 Plan 中增加浏览器、Network、写入或任意脚本能力；
- 不自动向网页聊天机器人/网页 AI 发送提示词，不将网页 AI 作为 MCP 或搜索
  结果替代；
- 不提供任意 HTTP 客户端、网络代理、任意 JS/eval、扩展安装 API、凭据读取或
  绕过 CAPTCHA/MFA 的自动化；
- 不把用户既有浏览器 tabs、个人 profile、cookies 或其他项目数据纳入 run；
- 不把真实浏览器 ignored test、模拟浏览器或静态检查描述为生产 E2E 验收。

### 2.3 术语

- **普通 Agent**：由 `agent_v4_start_direct` 直接启动的 Agent run。
- **研究路由**：成功的 `agent.route_request` 返回
  `research_retrieval`；`adaptive` 保持既有路径。
- **阶段成功**：同一 run 中持久化的 `ToolFinished` 且 `succeeded = true`，或
  等价的成功 `ToolOutcomeReused`。请求、dispatch、失败或模型文字都不算。
- **专业 MCP**：已发现、已启用、允许启动、schema/catalog 可绑定且由 Host
  调用的结构化 MCP 工具。
- **独立落地页**：搜索结果之外的 HTTP(S) 来源页面，必须单独打开并扫描。
- **同 run 恢复**：恢复请求复用原 `run_id`、项目、会话和事件链，而不是创建
  新 run 或重放已成功的副作用。

## 3. 总体架构

```text
普通 Agent（direct）
        │ agent.route_request
        ▼
Agent Core：事件门控 + same-run recovery + completion gate
        │
        ├─ search_mcp_tools / search_skills / use_skill
        ├─ use_mcp_tool 或 agent.record_mcp_unavailable
        └─ browser_setup → web_search → web_scan → web_open_tab/web_scan
                │
                ▼
        Tauri DesktopToolExecutorV4
                │ capability/session/run 授权
                ▼
        BrowserRuntime ── loopback WebSocket ── MV3 Browser Bridge
                │                         18775 shared / 18776 workspace
                ▼
        用户的真实 Chrome/Edge/Chromium
```

职责边界：

- Agent Core 只从 durable event chain 推导研究阶段，并在 `agent.complete` 前
  进行顺序门控；它不解析网页或保存图片正文。
- Desktop/Tauri Host 负责精确浏览器授权、session/workspace 绑定、连接状态、
  视觉能力判断、项目路径和 tab 清理。
- `BrowserRuntime` 负责 listener、握手、命令 request/reply、浏览器启动和
  安全资产落地；它不独立持久化页面扫描、cookies、凭据或截图字节。
- MV3 扩展负责受控 tab 操作、稳定后扫描、截图和下载 staging；它不拥有
  OmicsOps 模型凭据，不生成网页 AI prompt。

## 4. 模式边界与工具暴露

### 4.1 Plan 不变

Plan 仍使用 `RunModeV4::Plan` 的只读 descriptor 和现有 revision-scope
approval。浏览器工具 `browser_setup`、`web_search`、`web_open_tab`、
`web_scan`、`web_execute_js`、`web_screenshot`、`web_save_assets` 不得在
Plan descriptor 中出现，也不得通过 `use_mcp_tool` 间接绕过。Plan 的 MCP
只读动态授权仍要求 server/tool approval、catalog SHA、schema SHA 和
`readOnlyHint` 条件。

批准 Plan 后的执行仍按原冻结 spec/capability、approval hash、compute policy
和恢复语义处理；本设计不新增“批准即浏览器授权”的转换。普通 Agent 的浏览器
研究门控不能被复用来改变已批准计划的行为。

`RunSpecV4.execution_kind` 明确区分 `ordinary_agent` 与 `approved_plan`。
direct 入口冻结前者并把它写入 `spec_hash`；Plan 批准入口冻结后者。为兼容旧
规格，缺失字段只默认成 `approved_plan`。Agent Core 只有在规格完整性通过且
执行来源为 `ordinary_agent` 时才启用研究阶段门控与普通 Agent 提示词。

### 4.2 普通 Agent

普通 Agent Execute run 在调用任何任务工具前先成功路由：

```text
agent.route_request(research_retrieval)
  → search_mcp_tools
  → search_skills
  → [有匹配时 use_skill]
  → use_mcp_tool 或 agent.record_mcp_unavailable
  → browser_setup
  → web_search
  → web_scan(page_kind=search_results)
  → [result_count 非零/缺失时 web_open_tab + web_scan(page_kind=source)]
  → 综合 + agent.complete
```

route 是 Host 对本轮原始用户目标的冻结判定。模型仍须调用
`agent.route_request` 形成可重放事件和说明理由，但模型参数不能把 Host 已判定
的科研检索降级为 `adaptive`。Host 的确定性信号至少覆盖论文/文献、
PubMed/PMID/DOI、期刊/引用、外部数据库、最新资料、显式网页和跨来源证据；
本地文件编辑或仅处理已有数据的任务保持 `adaptive`。

`adaptive` 路由不使用这套研究阶段门控。路由一旦成功写入
`RequestRouted`，本 run 不接受第二次路由；没有路由时，任何任务工具都先被
Host 拒绝。

## 5. 严格成功事件门控

`research_workflow_rejection` 只读取当前 run 的事件：

1. 找到最近成功的 `RequestRouted`；
2. 顺序检查成功 `ToolFinished`/`ToolOutcomeReused`；
3. 对每个阶段只允许当前所需 tool，其他请求写入 recoverable rejection；
4. 失败调用不推进，也不因模型再次声称成功而推进；
5. 只有阶段全部完成时才允许 `agent.complete`。

具体规则：

- `search_mcp_tools` 失败不能解锁 `search_skills`；
- `search_skills` 返回非空匹配时，至少一次成功 `use_skill` 是硬门槛；返回空
  时不要求虚构 Skill；
- `use_mcp_tool` 成功或 `agent.record_mcp_unavailable` 成功（二者至少其一）
  才能进入浏览器；不可用记录必须包含可审计 reason 和 searched query；
- `browser_setup` 成功后才能 `web_search`；搜索成功后必须有
  `web_scan(page_kind=search_results)`；
- 结果扫描明确给出数值 `result_count = 0` 时可直接综合；缺失、非数值零或
  大于零时，必须打开并扫描独立来源页；
- 每次导航、重定向、点击或其他 material page change 后必须再次稳定并扫描；
- 任何成功阶段不能被另一个 run 的事件、另一个项目的共享连接或一个旧的
  approval 代替。

`agent.complete` 还必须满足现有 completion schema、冻结 criteria、deterministic
verification 和 reviewer 规则。浏览器阶段完成不等于科学结论正确；综合答案
必须保留来源、不确定性和限制。

## 6. 浏览器桥接协议

### 6.1 Session 与端口

只允许两个回环 endpoint：

| session | endpoint | 语义 |
| --- | --- | --- |
| `shared` | `ws://127.0.0.1:18775/v1/session/shared` | 可跨项目共享连接，但不共享项目数据/授权 |
| `workspace` | `ws://127.0.0.1:18776/v1/session/workspace` | 当前工作区隔离 profile 和数据边界 |

端口冲突、非 loopback 绑定、远程 host、path/session 不匹配都 fail closed。
`PROTOCOL_VERSION = 1`；hello 必须携带精确扩展 ID 和
`tabs/scan/search/screenshot/downloads/debugger` 全部 capabilities。Rust
bridge 还严格检查 `Origin = chrome-extension://<固定 ID>`。

### 6.2 Host 工具接口

当前 `crates/omicsops-tools` 的浏览器 descriptor 是 Network effect，输入
边界如下：

| tool | 必要输入/输出约束 |
| --- | --- |
| `browser_setup` | `session` 为 `shared`/`workspace`；可选 `launch_if_needed`；只在 Execute |
| `web_search` | `session`、非空 `query`、可选 `provider`；返回结果 tab identity；不发送网页 AI prompt |
| `web_open_tab` | `session`、HTTP(S) `url`；登记当前 run 的 tab |
| `web_scan` | `session`、`tab_id`、`page_kind`（`search_results`/`source`）；页面稳定后返回结构化扫描 |
| `web_execute_js` | `session`、`tab_id`、脚本；Host 上限 16,000 字符，执行前过网页 AI 与敏感 API policy；隔离执行并丢弃求值结果，之后必须重扫 |
| `web_screenshot` | `session`、`tab_id`、项目 `relative_path`；结果只有路径/大小/SHA metadata |
| `web_save_assets` | `session`、staged assets；只能复制安全 staging 普通文件并校验 SHA |

### 6.3 Tauri/BrowserRuntime 接口

`src-tauri/src/browser_commands.rs` 提供设置、状态、setup、授权 list/grant/
revoke；`BrowserRuntime` 提供 `setup`、`status`、`call`、`save_screenshot`、
`save_staged_asset` 和 `close_run_tabs`。调用帧带 `run_id`，reply 必须以
`request_id` 配对；未连接、握手失败和 timeout 不能伪造成功结果。

## 7. 暂停、恢复与 tab 生命周期

### 7.1 状态与事件

浏览器未连接时，Host 返回 `error_kind = "browser_connection_required"`，
Agent Core 写入 `BrowserConnectionRequired` 并进入等待用户状态。CAPTCHA
返回 `human_intervention_required`，写入
`BrowserHumanInterventionRequired` 并明确不自动求解。用户解决扩展、端口、
系统权限或 CAPTCHA 后，UI 以原 `run_id` 重试；Host 再检查 session、
workspace、授权、protocol 和 tab ownership。

普通的可恢复浏览器失败写入失败 `ToolFinished`，但阶段不推进；模型可在同一
run 改变动作。dispatch 后无法确定是否已执行时写入 `ToolDispatchUncertain`，
必须等待人工核验/`ToolDispatchResolved`，不盲目重复网络动作。

同 run 恢复必须满足：

- 已成功的阶段保留 durable evidence，不能重新执行以制造“新起点”；
- 待处理动作沿用原项目/conversation/run context，并以安全幂等规则决定是否
  复用结果；
- 不创建第二个 `RunCreated`，不切换到模拟网页/普通 HTTP，不把暂停说成完成；
- 取消、失败、完成或中断时只关闭当前 run 创建且仍归它的 tabs；用户明确保留
  的 tab 和 run 开始前的 tab 不关闭；清理失败记录待处理项。

### 7.2 连接启动

`BrowserRuntime::setup` 先确保回环 listener，再按 `BrowserConfig.auto_launch`
和 `launch_if_needed` 连接/启动。workspace 启动使用 data root 下的
`browser/workspace-profile` 和打包扩展；shared 可附加共享连接。workspace
没有验证扩展握手时必须停止临时进程并 fail closed。

## 8. 授权模型

浏览器授权独立于 compute approval。精确 binding 为：

```text
capability + target_host + session + protocol_version
```

四种 scope：

| scope | project/conversation 绑定 | 成功调用后的生命周期 |
| --- | --- | --- |
| `once` | 当前项目 + 当前会话 | dispatch 前原子消费，避免并发重放；恢复仍受原 exact-call approval/call hash 约束 |
| `conversation` | 当前项目 + 当前会话 | 当前会话内复用 |
| `project` | 当前项目，conversation 为空 | 当前项目内复用 |
| `global` | 两者为空 | 可跨项目复用，但仍逐调用精确比较 |

`web_open_tab` 的 target host 来自规范化 URL host；`web_search` 的显式 provider
映射到 Google、Bing 或 DuckDuckGo 的固定 host；scan/execute_js/screenshot/
save_assets 要求显式 target host，且 asset source URL 必须精确匹配；只有 setup
使用 `browser-session`。wildcard host 不合法。

每次调用都重新检查持久授权或生成 exact-call approval。Full Access 只能影响
原有 compute local execution policy，不能绕过浏览器 scope、binding、连接、
Origin、网页 AI 禁止、CAPTCHA、路径或 SHA 校验。

## 9. 数据、下载与视觉

### 9.1 数据驻留

事件和 SQLite 只保存最小审计信息：run/project/conversation/session、脱敏 URL、
标题、有界结构化扫描摘录、动作状态、来源定位、相对路径、媒体类型、大小和
SHA-256。表单值、cookies、密码、API key、SSH 私钥、完整 page body、完整下载
正文和截图字节不得进入 SQLite、事件、日志、模型上下文或 Git。

### 9.2 项目资产

`save_screenshot` 只接受 `data:image/png;base64,...`，写入当前项目 root 下的
项目相对路径。路径要通过 safe-relative、canonical parent、symlink 和 regular
file 检查；返回 `{relative_path, size_bytes, sha256}`，校验失败不登记。

`web_save_assets` 只能读取 `Downloads/OmicsOps-Staging/<staged_id>` 的普通
文件，拒绝 staging symlink/非法 ID，再复制到项目相对路径并计算 SHA。大文件
正文保留在项目资产，不默认同步到其他设备。

### 9.3 精确模型视觉

成功截图引用由 `ModelImageRefV4` 表示：项目相对路径、`image/png`、大小和
SHA-256。Agent Core 当前只把最近至多三个成功截图引用交给模型层。模型层发送
前再次校验文件和 SHA。

视觉能力必须从编译期能力表以精确
`provider + API host + exact model id` 查询。不得按厂商、家族、前缀、别名或
相似 ID 继承；精确 profile 不支持视觉时，只能提示“已保存/校验但未视觉检查”，
不能声称看过图。凭据只通过现有 keyring/credential vault 读取。

## 10. 扩展与网页 AI 安全

扩展为 MV3，manifest 当前声明 `tabs`、`scripting`、`downloads`、`storage`、`debugger`，
HTTP(S) host permissions 和只连两个 loopback WebSocket 的 CSP。权限较宽不等于
Host 授权较宽；页面内容和扩展输出仍是不可信输入。

脚本执行在受控 tab 的 isolated world、没有 bridge/model API/凭据；Host 16,000
字符上限和扩展网页 AI policy 都必须通过。任何命中网页 AI 名称、chat/completion
API、`prompt`/`confirm` 或聊天框提交的操作都拒绝。系统不得自动填写、发送或
轮询 ChatGPT、Gemini、Claude、Copilot、Perplexity 等网页 AI。

## 11. Windows/macOS 设计

Host、端口、授权、事件和数据契约跨平台一致：

- Windows 从 `PROGRAMFILES`、`PROGRAMFILES(X86)`、`LOCALAPPDATA` 发现 Chrome、
  Edge、Chromium/Chrome for Testing；显式 path 必须是文件；只停止 OmicsOps
  启动的 child。
- macOS 从 `/Applications` 和用户 `Applications` 发现浏览器；系统自动化/辅助
  功能权限由用户授权，权限不足显示指引并暂停原 run；不能提权绕过。
- 两平台仅绑定 127.0.0.1，不能把 18775/18776 暴露到局域网或公网，不能以杀掉
  其他浏览器进程代替 tab cleanup。

## 12. 验收、可观测性与当前限制

验收必须分成：确定性事件/授权/路径测试、Windows 安装与 handshake、macOS
安装与 handshake、公开网页搜索/结果扫描/独立来源、断线/CAPTCHA 同 run
恢复、截图/SHA/视觉 capability、Full Access 负面测试。每一项记录 OS、浏览器
版本、扩展 ID/version、端口、project/conversation/run ID、精确模型三元组、
实际命令/日志和项目相对证据路径。

截至本文更新时，manifest key、扩展 ID、Rust/JavaScript 协议常量、入口资源和
Settings provider 值已经统一，并有确定性测试。真实浏览器测试仍是 `#[ignore]`，
没有真实命令结果可签署。当前仍需现场确认：

- Windows/macOS 扩展安装、权限提示、shared/workspace 握手、下载目录和 CDP；
- 公开网页搜索、独立来源、断线/CAPTCHA 同 run 恢复与标签清理；
- 当前 executor 对 `NotConnected` 会写入 `BrowserConnectionRequired`；CAPTCHA
  会写入 `BrowserHumanInterventionRequired` 并在 UI 中提供同 run 恢复。真实
  浏览器仍须现场确认这些分支与 transport/command 错误，不得把模拟事件测试或
  `recoverable` 字段本身当作已完成的人机交接。

这些限制只说明当前工作树不能宣称安装/握手/网页验收通过，不改变本设计的
fail-closed 要求。可执行的记录格式见
[浏览器验收清单](../../browser-acceptance.md)，实现步骤见
[实现计划](../plans/2026-08-31-browser-research-agent-plan.md)。
