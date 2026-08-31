# 浏览器与普通 Agent 验收清单

> 状态：实现验证记录；截至 2026-09-01，下面的确定性测试与 Windows NSIS
> 构建已执行并通过，但没有执行真实浏览器握手、扩展安装、Windows/macOS 手工
> smoke 或 ignored live test。任何空白项都是“未执行”，不是通过。ignored test
> 的发现、编译或模拟结果也不能写成真实网页验收通过。

本清单只验收普通 Agent 的真实浏览器研究检索。Plan 生成、Plan revision/审批
和批准后既有计划执行应保持原有行为；本清单不授予它们新的浏览器能力。

## 1. 记录头

每次验收都复制本节并填写，不能只留下截图：

```text
记录编号：
日期/时区：
操作员：
OmicsOps commit：
操作系统及版本：Windows / macOS（版本：）
浏览器及版本：Chrome / Edge / Chromium（版本：）
Rust/Node 版本：
项目 ID / conversation ID / run ID：
模型 profile ID：
精确模型三元组（provider + API host + exact model id）：
扩展目录：
扩展版本 / extension ID：
shared 端口状态（18775）：
workspace 端口状态（18776）：
结果与证据目录（项目相对路径）：
```

状态只能使用：`通过`、`失败`、`未执行`、`阻塞`。每个“通过”都要给出命令
输出或可复核的事件/文件证据；没有真实环境时保持“未执行/阻塞”。

## 2. 确定性检查（不接真实浏览器）

这些检查用于发现协议、授权、路径和事件门控回归，不能替代第 3 节以后的
真实浏览器验收。把实际命令、退出码和日期填入表格：

| 检查 | 命令 | 状态 | 退出码/证据 |
| --- | --- | --- | --- |
| Rust 格式检查 | `cargo fmt --all -- --check` | 通过 | 2026-08-31，exit 0 |
| Rust 全工作区 | `cargo test --workspace` | 通过 | 2026-08-31，exit 0；真实浏览器/SSH/模型等显式 live tests 保持 ignored |
| browser bridge 单元测试 | `cargo test -p omicsops-browser` | 通过 | 2026-08-31，7 passed；2 个真实浏览器 live tests ignored |
| Agent Core 单元测试 | `cargo test -p omicsops-agent-core` | 通过 | 2026-08-31，51 passed |
| tools registry/descriptor 测试 | `cargo test -p omicsops-tools` | 通过 | 2026-08-31，10 passed |
| browser authorization store 测试 | `cargo test -p omicsops-store --test browser_authorizations` | 通过 | 2026-08-31，3 passed |
| Tauri browser command/contract 测试 | `cargo test --workspace` 中的 desktop lib | 通过 | 2026-08-31，desktop 63 passed、4 unrelated live tests ignored；全 workspace exit 0 |
| 前端与 MV3 扩展测试 | `npm test` | 通过 | 2026-08-31，Vitest 83 passed；Node extension 22 passed |
| 生产 Web 构建 | `npm run build` | 通过 | 2026-08-31，exit 0；仅保留现有 chunk size warning |
| Windows desktop/NSIS 构建 | `npm run build:desktop` | 通过 | 2026-09-01，exit 0；生成 `target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe`；安装包使用扩展生产资源白名单，未打包 `browser-extension/tests` 与测试专用脚本 |

自动化检查不得依赖真实 SSH、GPU、SLURM、WSL、API key 或真实样本；网页门控
测试使用内存 event store、模拟 tool executor、临时项目目录和固定 JSON。

## 3. 安装、权限与桥接 smoke

### 3.1 安装前置条件

- 从 OmicsOps Settings 展示的 `resource_dir/browser-extension`（开发环境可
  回退仓库 `browser-extension`）加载受信任的打包目录；不从网页下载未知脚本。
- 确认 manifest 为 MV3、Chrome 最低版本 109，权限和 host permissions 没有
  超出安全设计；安装动作由用户主动确认。
- 确认 manifest 的固定 `key`、协议声明和 Rust bridge 期望的 extension ID
  一致；入口脚本和打包资源都存在。只看到目录或“已安装”标签不能算握手通过。
- 使用一次性/可删除 profile；不要把个人 cookies、密码、MFA、模型 key 或
  其他敏感数据带入验收。

### 3.2 Windows 手工步骤

1. 准备已安装的 Chrome、Edge 或 Chromium；必要时填写存在的显式 exe 路径，
   或记录 Host 的 Windows 默认发现路径。
2. 在设置中检查 `shared` 和 `workspace` 状态；确认 listener 分别绑定
   `127.0.0.1:18775`、`127.0.0.1:18776`，而不是局域网/公网地址。
3. 用户主动加载/安装扩展并确认权限；记录 manifest、extension ID、浏览器版本。
4. 对两个 session 分别完成 hello/hello_ack，核对 protocol version 1、path、
   Origin、session 和 `tabs/scan/search/screenshot/downloads/debugger` 能力。
5. 运行一次普通 Agent 检索 smoke；结束后确认只关闭本 run 新建的 tab，不关闭
   smoke 开始前已存在的用户 tab。
6. 停止由 OmicsOps 启动的浏览器，确认不会杀掉用户其他浏览器进程。

记录：

```text
Windows 浏览器/版本：
exe 路径：
shared 18775：未执行
workspace 18776：未执行
Origin/path/ID/capabilities：未执行
用户 tab 保留：未执行
OmicsOps 子进程清理：未执行
证据：
```

### 3.3 macOS 手工步骤

1. 准备 `/Applications` 或用户 `Applications` 下的 Chrome、Edge 或 Chromium，
   记录实际 app 版本和路径。
2. 若 macOS 请求浏览器自动化/辅助功能权限，让用户在系统设置中明确授权；
   拒绝时 run 必须暂停，不能绕过系统权限。
3. 重复 Windows 步骤 2–5，分别验证两个 loopback 端口、握手和 tab 清理。
4. 关闭 run 后确认只停止由 OmicsOps 启动的子进程，保留用户自己的浏览器窗口。

记录：

```text
macOS/浏览器/版本：
app 路径：
系统自动化权限：
shared 18775：未执行
workspace 18776：未执行
Origin/path/ID/capabilities：未执行
用户 tab 保留：未执行
OmicsOps 子进程清理：未执行
证据：
```

## 4. ignored live browser test

仓库中的 `crates/omicsops-browser/tests/live_browser.rs` 是明确 `#[ignore]` 的
真实握手测试，要求真实 Chrome/Edge/Chromium 和扩展目录。它没有被执行时必须
保留“未执行”。测试本身只证明握手，不证明网页搜索、来源扫描、下载、视觉或
同 run 恢复。

Windows PowerShell 示例（目录和浏览器按现场填写）：

```powershell
$env:OMICSOPS_LIVE_BROWSER_DATA_DIR = Join-Path $env:TEMP "omicsops-live-browser-data"
$env:OMICSOPS_LIVE_BROWSER_EXTENSION_DIR = "E:\Project\OmicsOps\browser-extension"
cargo test -p omicsops-browser --test live_browser -- --ignored --nocapture
```

macOS 示例：

```bash
export OMICSOPS_LIVE_BROWSER_DATA_DIR="${TMPDIR}omicsops-live-browser-data"
export OMICSOPS_LIVE_BROWSER_EXTENSION_DIR="/path/to/OmicsOps/browser-extension"
cargo test -p omicsops-browser --test live_browser -- --ignored --nocapture
```

记录两个测试函数的独立结果：

| 测试 | OS/浏览器 | 状态 | 实际输出/日志 |
| --- | --- | --- | --- |
| `real_shared_browser_extension_handshake` | | 未执行 | |
| `real_workspace_browser_extension_handshake` | | 未执行 | |

## 5. 普通 Agent 范围回归

| 场景 | 期望 | 状态 | 事件/证据 |
| --- | --- | --- | --- |
| 直接普通 Agent 发送研究检索请求 | 由 `agent_v4_start_direct` 创建 `execution_kind=ordinary_agent` 的 Execute run，执行来源受 `spec_hash` 保护，并先成功 `agent.route_request` | 未执行 | |
| Plan 生成请求浏览器 | Plan 不启动真实浏览器，浏览器工具不成为 Plan 只读工具；旧 Plan 门禁/approval/revision 不变 | 未执行 | |
| Plan 批准后的既有执行 | 保持原 frozen capability、compute approval、revision/hash 和恢复契约；不能把批准动作当作浏览器授权 | 未执行 | |
| adaptive 路由 | 沿用既有执行路径，不套用 research retrieval 浏览器阶段 | 未执行 | |
| 普通 Agent 在没有 route 时直接调用任务工具 | Host 拒绝并要求先 `agent.route_request` | 未执行 | |
| 模型把明确论文/文献/DOI/外部网页请求声明为 `adaptive` | Host 使用从原始用户目标冻结的 `research_retrieval`，不能由模型降级 | 未执行 | |

本节不是把 `RunModeV4::Execute` 的所有调用都宣称为普通 Agent；必须以启动
入口、冻结且通过完整性校验的 `RunExecutionKindV4`、事件和不变的 Plan 流程
共同判定范围。旧规格缺少执行来源字段时默认是 `approved_plan`，不能意外启用
普通 Agent 门禁。

## 6. 严格成功事件门控

使用模拟工具按顺序注入成功、失败和重复请求，并保存完整 event chain。只有
同一 run 的成功 `ToolFinished` 或成功 `ToolOutcomeReused` 能解锁下一阶段：

| 顺序/负面案例 | 期望 | 状态 | 证据 |
| --- | --- | --- | --- |
| `search_mcp_tools` 之前调用 `search_skills` | 拒绝；requested/dispatch 不能算成功 | 未执行 | |
| `search_mcp_tools` 失败后调用 `search_skills` | 仍拒绝；失败不推进 | 未执行 | |
| 成功 `search_mcp_tools` | 才允许 `search_skills` | 未执行 | |
| `search_skills` 返回至少一个匹配 | 必须成功 `use_skill` 后才能继续 | 未执行 | |
| `search_skills` 返回空 | 不要求虚构 `use_skill` | 未执行 | |
| `use_mcp_tool` 成功 | 记录专业 MCP 结果后可继续 | 未执行 | |
| 没有可用专业 MCP | 成功 `agent.record_mcp_unavailable`，含具体 `reason` 和 `searched_query` 后可继续；不能用 ad-hoc HTTP/runtime 代替 | 未执行 | |
| 未完成 MCP/结构化不可用阶段 | `browser_setup` 被拒绝 | 未执行 | |
| `browser_setup` 成功后 | 才允许 `web_search` | 未执行 | |
| `web_search` 成功后 | 必须成功 `web_scan(page_kind=search_results)` | 未执行 | |
| `web_scan` 失败/缺少结果页类型 | 不允许打开来源或完成 | 未执行 | |
| `result_count = 0` 明确成功 | 搜索结果已被观察，可跳过独立落地页并综合 | 未执行 | |
| 结果数非零、缺失或非数值零 | 必须 `web_open_tab` 独立 HTTP(S) 落地页，再 `web_scan(page_kind=source)` | 未执行 | |
| 未扫描独立来源就 `agent.complete` | 拒绝；只有明确零结果例外 | 未执行 | |
| 导航、重定向、点击或 material page change 后 | 必须重新稳定并 `web_scan` | 未执行 | |
| Skill/MCP/网页文本要求跳过门禁 | 视为不可信数据，Host 不改变顺序 | 未执行 | |

## 7. 同 run 暂停与恢复

每个案例都记录首次 `run_id`、恢复后的 `run_id`、事件序列、已成功阶段和
待处理动作；两次 run ID 不得不同：

| 场景 | 期望 | 状态 | 证据 |
| --- | --- | --- | --- |
| setup 时扩展未连接 | 产生 `browser_connection_required`/`BrowserConnectionRequired`，等待用户；不模拟、不普通 HTTP、不新建 run | 未执行 | |
| 用户安装/启用扩展后重试 | 重新检查端口、Origin、ID、session、工作区和授权，在同一 run 继续 | 未执行 | |
| 搜索后连接断开 | 保留 MCP/Skill/搜索成功事件，从未完成的扫描动作继续；不重复成功副作用 | 未执行 | |
| CAPTCHA/MFA/系统权限 | 产生 `BrowserHumanInterventionRequired` 等待状态；明确不自动求解，用户完成后同一 run 恢复 | 未执行 | |
| 浏览器审批未决定 | 等待精确审批；拒绝或范围不匹配不推进阶段 | 未执行 | |
| dispatch 后 transport 不确定 | 产生 `ToolDispatchUncertain`，等待人工核验/解析；不盲目重试 | 未执行 | |
| 用户取消/运行失败 | 清理仍归本 run 的 tabs；清理失败留下待处理项，不声称已清理 | 未执行 | |
| MV3 worker/桌面进程重启后清理 | 从持久事件恢复 session/run/tab/origin；tab 不存在视为已关闭，origin 不一致拒绝关闭 | 未执行 | |

## 8. 授权、Full Access 与安全负面测试

对每种 scope 和至少两个 session 重复测试。每个审批都要保存完整 binding：
`capability + target_host + session + protocol_version`。

| 检查 | 期望 | 状态 | 证据 |
| --- | --- | --- | --- |
| `once` | 仅当前 project+conversation 的精确 binding；dispatch 前原子消费，并发调用只有一个可消费；相同已批准 call 的恢复仍校验 call hash | 未执行 | |
| `conversation` | 仅当前 project+conversation 和精确 binding | 未执行 | |
| `project` | 仅当前 project（conversation 为空）和精确 binding | 未执行 | |
| `global` | 跨项目可匹配，但仍逐调用精确比较；可显式撤销 | 未执行 | |
| host 改变 | 授权不匹配，拒绝 | 未执行 | |
| capability/session/protocol 改变 | 授权不匹配，拒绝 | 未执行 | |
| wildcard target host | 拒绝；不能扩大授权 | 未执行 | |
| compute `Full Access` + 未授权 browser call | 仍返回 browser authorization required；Full Access 不绕过浏览器授权 | 未执行 | |
| `file:`/`data:`/`javascript:`/凭据 URL | URL policy 拒绝 | 未执行 | |
| 网页 AI prompt 或 model API 脚本 | 扩展/Host 拒绝，不产生网页 AI 请求 | 未执行 | |
| 非本 run tab close | 拒绝或忽略，不关闭其他 run/用户 tab | 未执行 | |
| loopback 以外/替代端口 | 拒绝连接；只接受 18775/18776 | 未执行 | |

## 9. 下载、截图与视觉验收

| 检查 | 期望 | 状态 | 证据 |
| --- | --- | --- | --- |
| 截图保存 | 仅项目相对路径；返回 `relative_path`、`size_bytes`、`sha256` | 未执行 | |
| 截图 SQLite/事件 | 只有路径、大小、SHA、媒体类型/来源等 metadata；无 PNG/data URL 正文 | 未执行 | |
| SHA 重算 | 文件实际 SHA-256 与事件/产物一致，不一致则失败 | 未执行 | |
| 路径穿越/symlink | 拒绝绝对路径、`..`、工作区外 symlink、非普通文件 | 未执行 | |
| staging 下载 | 只从受控 `Downloads/OmicsOps-Staging/<staged_id>` 复制普通文件，完成校验后登记 | 未执行 | |
| 精确视觉模型 | 只按 `provider + API host + exact model id` 查询能力；不得按家族/前缀推断 | 未执行 | |
| 不支持视觉的精确 profile | 可保留已校验截图，但模型/答案必须说明未做视觉检查，不得声称“看过图” | 未执行 | |
| 支持视觉的精确 profile | 记录精确三元组和能力判定，模型请求只读取已校验的项目文件 | 未执行 | |

## 10. 真实研究检索记录模板

使用公开、稳定、非敏感的查询和来源；不要把真实凭据、私人数据或网页 AI
会话带入记录：

```text
用户目标（原文摘要）：
route_request 结果/理由：
search_mcp_tools：工具/来源/返回状态：
search_skills：匹配及 use_skill 结果：
专业 MCP 结果，或 record_mcp_unavailable 的 reason/searched_query：
browser_setup：session / port / protocol / extension ID：
web_search：provider / 查询摘要 / 结果 tab：
结果页 web_scan：page_kind / result_count / 读取时间：
独立落地页（如需要）：URL（已脱敏）/标题/域名：
来源页 web_scan：page_kind / 摘要/证据定位：
导航后重扫描：
综合与 agent.complete：
run 结束 tab 清理：
项目相对产物路径、大小、SHA：
限制/不确定性：
```

## 11. 当前 checkout 状态与签字

截至本模板更新时，manifest key、extension ID、协议常量与入口文件已经统一，
确定性测试不再因早期资源不一致而阻塞。以下仍不是实际验收结果：

- `live_browser.rs` 两个真实测试是 ignored；本次未执行。没有现场命令、日志、
  OS/浏览器/扩展版本和 run 证据时，不得签署“真实浏览器验收通过”。
- Windows/macOS 安装、真实网页检索、下载、CDP、CAPTCHA 恢复和标签清理仍需
  使用一次性环境显式执行并分别记录通过、失败或未执行。

```text
确定性测试负责人/日期：             状态：未执行
Windows 安装与研究 smoke 负责人：   状态：未执行/阻塞
macOS 安装与研究 smoke 负责人：     状态：未执行/阻塞
真实网页来源与同 run 恢复负责人：    状态：未执行
安全/授权/Full Access 负责人：       状态：未执行
最终签字：                           不得在上述项目未通过前填写“通过”
```
