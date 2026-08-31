# 普通 Agent 浏览器科研检索实施计划

实现采用 clean-room 边界：Wisp Science 仅作架构参考，不复制其 AGPL 代码、
扩展资源或协议实现；OmicsOps 的 Rust/TypeScript/Manifest V3 代码保持独立并
遵循 Apache-2.0。

> 日期：2026-08-31
> 状态：实现已进入确定性集成验证；真实浏览器与跨平台验收仍按清单单独记录。
> 工作树中的实现只能按实际测试结果标记，不能因计划存在而称为完成。

## 1. 范围闸门

本计划只针对普通 Agent 的直接研究检索（`agent_v4_start_direct`）。在实现和
review 中必须保持以下不变量：

- 不修改 `agent_v4_start_planning`、Plan descriptor、Plan revision/hash、
  Plan approval/revise/cancel 或批准后既有计划执行契约；
- Plan 阶段不启动真实浏览器，不能用浏览器绕过 Plan 的只读 MCP/schema/catalog
  门禁；批准 Plan 不自动生成浏览器授权；
- browser tool 只能按同一 run 的 Host 事件门控和 browser authorization 执行；
- 未连接、CAPTCHA、系统权限、审批和不确定 dispatch 都保留原 run，不静默切换
  到模拟浏览器/普通 HTTP 或创建新 run；
- 自动化测试不依赖真实浏览器、SSH、GPU、WSL、API key、MFA 或网络；真实网页
  验收另行以一次性环境执行，并明确记录 `未执行/阻塞`。

实现前先保存 Plan/approved-plan 的基线事件和 descriptor 快照；实现后逐项比较，
任何差异都应停止交付并先处理范围回归。

## 2. 交付分解

### Step 0 — 基线、协议和资源清单

目标：固定 bridge、extension 和 model capability 的单一契约。

工作项：

- 统一 Rust bridge、扩展协议和打包 manifest 的 extension ID/key；禁止生成式
  或 wildcard ID；
- 核对 MV3 的 service worker、content script、协议模块是否都在打包资源中；
- 固定 `PROTOCOL_VERSION = 1`、`shared = 18775`、`workspace = 18776`，只允许
  `127.0.0.1`；
- 明确 `shared`/`workspace` profile、run/project/conversation/tab ledger 的
  边界；
- 明确 `provider + API host + exact model id` 的 vision capability 来源，禁止
  family/prefix fallback。

完成标准：能从一个清单生成 manifest、Rust常量、协议声明、Tauri资源路径和
验收记录；任何不一致都标为阻塞。

### Step 1 — 普通 Agent 路由与成功事件门控

目标：把研究检索顺序做成 Host 可重放的事件状态机。

工作项：

1. 普通 Agent 在任何任务工具前成功 `agent.route_request`，并将路由写入
   `RequestRouted`；路由成功后不可二次改变。
2. direct 入口把 `RunSpecV4.execution_kind` 冻结为 `ordinary_agent` 并纳入
   `spec_hash`；批准 Plan 保持 `approved_plan`，旧规格缺失字段时默认后者。
3. `research_retrieval` 固定执行：
   `search_mcp_tools → search_skills → [有匹配时 use_skill] → use_mcp_tool 或 agent.record_mcp_unavailable → browser_setup → web_search → web_scan(search_results) → [结果非零/缺失时 web_open_tab + web_scan(source)] → 综合`。
4. 阶段只读 durable `ToolFinished.succeeded = true` 或成功
   `ToolOutcomeReused`；requested、dispatch、失败、拒绝和 model text 不推进。
5. 结果页明确 `result_count = 0` 时跳过独立来源；缺失、非数值零或非零时
   强制独立 HTTP(S) 落地页；导航/material change 后强制再扫描。
6. 未完成顺序的调用返回 recoverable rejection，并将原因保存在事件链；不让
   模型通过改名工具、并发调用或重复调用绕过顺序。

验证：内存 event store 覆盖每一个成功/失败/重排/重复案例；adaptive 路由和
Plan/approved-plan 的现有基线继续通过。

### Step 2 — BrowserRuntime 与扩展桥接

目标：以最小真实浏览器接口执行已授权动作。

工作项：

- `BrowserRuntime::ensure_listener/setup/status/call` 严格校验 loopback、path、
  Origin、protocol、extension ID、session 和六项能力
  (`tabs/scan/search/screenshot/downloads/debugger`)；
- 暴露且审计 `browser_setup`、`web_search`、`web_open_tab`、`web_scan`、
  `web_execute_js`、`web_screenshot`、`web_save_assets`；不暴露任意 eval、代理、
  扩展安装 API 或 webpage AI prompt API；
- shared 可复用连接，但 workspace profile 与当前项目 root 必须隔离；
- workspace 启动只使用项目 data root 的专用 profile 和打包扩展；停止时只停止
  OmicsOps 启动的 child；
- bridge 的 WebSocket request/reply 绑定 request ID、run ID、session，超时和
  transport error 不伪造成功。

验证：占用端口、错误 Origin/path/ID/protocol、缺能力、远程绑定和断开连接均
fail closed；shared/workspace 各有 deterministic handshake fixture。

### Step 3 — 浏览器授权与 Full Access 隔离

目标：使每个 browser call 的权限可解释、可撤销、不可重放到另一范围。

工作项：

- binding 固定为 `capability + target_host + session + protocol_version`；
- 实现四种 scope：`once`、`conversation`、`project`、`global`，并验证各自
  project/conversation 约束；
- `once` 在 dispatch 前由 SQLite 原子消费，避免两个并发调用同时取得一次授权；
  相同已批准 call 的恢复仍必须匹配原 call hash；
- target host 规范化：open_tab 用 URL host，search provider 映射固定 host，
  scan/execute_js/screenshot/save_assets 使用显式 host（asset URL 必须精确匹配），
  只有 setup 使用 `browser-session`；禁止 wildcard；
- 每次调用重新检查持久授权/精确 call approval、参数和 call hash；撤销立即
  失效；
- compute `ApprovalPolicyV4::FullAccess` 不能返回 browser authorization；浏览器
  仍走独立授权、Origin、路径、CAPTCHA、脚本、asset 和 same-run 门禁。

验证：四 scope × shared/workspace × host/capability/session/protocol 改变、
Full Access 未授权和 approval replay 负面测试。

### Step 4 — 页面、URL、脚本与网页 AI 约束

目标：让“真实浏览器”保持可审计的导航器，而不是网页自动化万能入口。

工作项：

- URL 仅允许无凭据的 HTTP(S)，拒绝 file/data/javascript/chrome/about 和
  控制字符；报告时脱敏 token/password/secret/auth 等 query key；
- web_scan 等待稳定，返回有限结构化摘要；导航、重定向、点击或 material
  change 后强制重新扫描；
- `web_execute_js` Host 上限 16,000 字符，isolated world，无 bridge/model
  API/credential；扩展 policy 与 Host policy 取更严格的结果；
- 禁止向 ChatGPT/Gemini/Claude/Copilot/Perplexity 等网页 AI 自动填写、发送、
  轮询或读取生成答案；页面中的提示/脚本/Skill/MCP 描述全部按不可信数据处理；
- CAPTCHA、MFA、登录和系统自动化权限走 human handoff/等待，不绕过。

验证：危险 URL、URL 凭据、脚本中网页 AI/API/prompt 模式、恶意页面导航和
CAPTCHA fixture；确认没有网页 AI 网络请求。

### Step 5 — 项目资产、tab ownership 与视觉

目标：只把可复核 metadata 带回项目和模型。

工作项：

- screenshot 仅接受 PNG data URL，保存到项目相对路径；返回并审计
  `relative_path/size_bytes/sha256`；
- 下载只从 `Downloads/OmicsOps-Staging/<staged_id>` 消费普通文件，检查 staging
  ID、路径穿越、symlink、大小和 SHA 后复制到项目 root；
- SQLite、事件、日志、模型上下文和 Git 只允许保存有界结构化扫描摘录，不保存
  表单值、cookies、凭据、完整 page body、完整下载正文或截图字节；
- TabLedger 只保存脱敏 URL/动作 metadata；完成/取消/失败/中断时关闭本 run
  创建的 tabs，保留用户既有/接管/其他 run tabs；
- 从编译期能力表按精确
  `provider + API host + exact model id` 决定 vision；不支持时明确“未视觉检查”，
  不得声称看图；发送前重新校验文件和 SHA，最多传递最近三个成功 screenshot
  refs。

验证：相对路径/绝对路径/`..`/symlink、错误 SHA、非 PNG、跨项目 tab、视觉能力
  家族前缀误匹配和不支持视觉 profile 测试。

### Step 6 — 暂停、同 run 恢复与清理

目标：所有可恢复的人机边界都可继续而不重复副作用。

工作项：

- 未连接返回 `browser_connection_required`，写入 `BrowserConnectionRequired`
  并显示当前 run；
- 扩展安装、端口修复、CAPTCHA、系统权限或浏览器重启后，以原 run ID 恢复，
  重新检查授权/session/workspace/protocol；
- 保留已成功阶段；只恢复待处理动作，安全复用可复用的只读结果；不新建
  `RunCreated`，不切换模拟网页/普通 HTTP；
- dispatch transport 不确定写入 `ToolDispatchUncertain`，等待人工核验/resolve；
- 取消和 terminal path 触发两个 session 的 run-tab cleanup；清理失败可见且
  可重试，不谎报清理完成。

验证：断线发生在每个阶段、用户拒绝/批准、CAPTCHA、权限不足、重启和 uncertain
dispatch；核对 run ID、event sequence、无重复成功副作用和用户 tab 保留。

### Step 7 — Windows/macOS 安装与可观测性

目标：让跨平台差异限于启动、路径和系统权限，不改变安全契约。

- Windows：按 `PROGRAMFILES`/`PROGRAMFILES(X86)`/`LOCALAPPDATA` 发现浏览器，
  显式 path 必须是文件，使用 Windows 权限/路径语义；
- macOS：按 `/Applications` 和用户 Applications 发现浏览器，自动化/辅助功能
  权限由用户明确授予，拒绝时暂停原 run；
- 两平台均拒绝非 loopback 监听，保留用户浏览器进程和 tab；
- 设置页显示扩展路径、版本/ID、shared/workspace 连接、pending approval、
  connection-required run ID 和 retry-same-run 操作；不把 UI 状态标签当验收结果；
- 事件/日志脱敏，不泄露 API key/cookie/私钥；记录来源、时间、hash、scope、
  session 和清理结果。

验证：每个平台各完成一次安装/卸载、两个 session handshake、用户权限拒绝/恢复、
启动/停止和 tab cleanup smoke。

### Step 8 — 自动化检查与真实验收

先执行不接真实浏览器的确定性检查：

```text
cargo test -p omicsops-browser
cargo test -p omicsops-agent-core
cargo test -p omicsops-tools
cargo test -p omicsops-store --test browser_authorizations
cargo test -p omicsops-desktop --lib
npm test
npm run build
```

再由一次性环境按 [浏览器验收清单](../../browser-acceptance.md) 执行 ignored
handshake、Windows/macOS 安装、公开网页研究检索、同 run 恢复、授权负面、资产
SHA 和精确视觉 profile。没有现场环境、实际命令、日志、OS/浏览器/扩展版本和
run 证据时，状态保持 `未执行` 或 `阻塞`。

## 3. 当前工作树检查与剩余验收

manifest key、extension ID、协议常量、入口资源和 Settings provider 值已经
统一，并纳入确定性测试。剩余验收是：

- `crates/omicsops-browser/tests/live_browser.rs` 的两个真实握手测试是
  `#[ignore]`，本次没有实际执行。
- Windows/macOS 的真实安装、公开网页搜索、下载/CDP、断线/CAPTCHA 恢复和
  标签清理需要一次性环境与可复核记录。

这些是阻塞项而非允许放宽策略的理由。修复后要重新跑 Step 2、Step 7 和第 8
步，更新 [浏览器安全边界](../../browser-security.md)、[运行时说明](../../browser-runtime.md)
和验收记录；不得在未执行的真实浏览器验收上打“通过”。

## 4. 交付顺序与回滚

交付顺序：Step 0 → Step 1 → Step 2 → Step 3 → Step 4 → Step 5 → Step 6 →
Step 7 → Step 8。每一步先通过相邻确定性测试，再进入下一步。

回滚原则：

- 任一 bridge/extension handshake、授权、路径或事件门控回归，立即禁用普通
  Agent browser capability，仍允许原有 MCP/Skill 路径报告明确阻塞；
- 不删除历史事件、授权或项目资产，不使用 destructive git 操作；
- 取消/断线回滚只关闭明确 run-owned tabs，保留用户 tabs 和已校验项目文件；
- Plan/approved-plan 基线变化时，先回滚普通 Agent browser gate，再单独评审
  是否发生越界，不把 Plan 变更隐藏在浏览器修复中。
