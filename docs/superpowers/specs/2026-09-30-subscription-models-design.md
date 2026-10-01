# 订阅模型接入设计

日期：2026-09-30（Asia/Shanghai）。分支：`codex/subscription-models`。

状态：用户于 2026-09-30 确认书面设计并授权实施；按功能验证后分别提交，实施中。

## 1. 用户目标与交付边界

用户要求在新分支中接入 Codex、Claude 和 OpenCode 的订阅，并指定 Wisp Science
作为实现参考。用户进一步确认：在 OmicsOps 现有聊天中选择订阅模型，保留现有
工具和审批；本次不把聊天交给拥有独立工具、会话和审批的外部 Agent。

用户已确认的方案为：Codex 订阅认证与 Responses；通过官方 Claude Code 程序
调用订阅模型并关闭其内置工具；复用 OpenCode Go，补齐 Responses 模型。

成功标准是三条路径都经过现有 Agent V4 的模型接口、工具注册表、宿主审批、
结果验证和证据持久化。模型返回的工具建议不算执行，只有宿主执行后的记录
才算工具证据。模型设置、普通聊天、规划/摘要、旁聊、委派与审核的客户端工厂
均应识别新配置，不能出现选择了订阅却静默使用 API key 模型的情况。

本次交付文本与宿主工具回合。新增传输默认不支持图像；只有后续对精确协议、
端点和完整模型 ID 建立图像预算并测试后才允许开启。既有图像路径不受影响。
本次不新增 ACP、SSH 远程 CLI 登录、CLI 会话导入、多账户自动轮换、订阅购买、
配额仪表盘、Zen 按量平台预设或科研工作流全套实现。

## 2. 参考证据与差异

固定参考提交：`ade254989da5d395c6334c893637f32c3ef61ba7`，不以变化中的 main
作为实施依据。参考项目与本仓库均声明 AGPL-3.0；借用源码时保留适用的版权
和许可信息，采用最小必要片段，不整批移植上游子系统。

- [Wisp codex_auth.rs](https://github.com/xuzhougeng/wisp-science/blob/ade254989da5d395c6334c893637f32c3ef61ba7/crates/wisp-llm/src/codex_auth.rs)
  展示 device/PKCE、刷新和订阅 Responses 路径。其令牌由调用方保管；OmicsOps
  使用已有 `CredentialVault`，不引入上游账户池和 CLIProxyAPI 文件扫描。
- [Wisp ACP 文档](https://github.com/xuzhougeng/wisp-science/blob/ade254989da5d395c6334c893637f32c3ef61ba7/docs/acp-agents.md)
  明确外部 Agent 自行拥有工具和认证。这不是用户选择的执行模式，不能直接
  用 ACP 接管 OmicsOps 会话作为本功能的替代实现。
- [Wisp 模型配置](https://github.com/xuzhougeng/wisp-science/blob/ade254989da5d395c6334c893637f32c3ef61ba7/docs/model-configuration.md)
  区分 Go 与 Zen。此处只扩展已有 Go 订阅路径。
- [OpenAI 认证](https://learn.chatgpt.com/docs/auth) 区分订阅与 API 计费，并说明
  device 登录和令牌刷新。公开文档不保证 OmicsOps 已获官方客户端认证。
- [Claude 认证边界](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use)
  不允许第三方收集或转接订阅令牌；用户可以自行登录未修改的 Claude Code。
  因此 OmicsOps 不实现 Claude.ai OAuth、不读取 Claude 凭据文件、不接收订阅 token。
- [Claude CLI](https://code.claude.com/docs/en/cli-reference) 提供登录状态、工具限制、
  配置隔离与结构化输出控制；[程序化调用](https://code.claude.com/docs/en/headless)
  说明 `--bare` 不使用订阅认证，因此该模式不能用于本设计。
- [OpenCode Go](https://opencode.ai/docs/go/#endpoints) 给出精确模型到协议的路由。
  Go 仍使用套餐 API key；已有客户端身份和稳定会话头继续使用 OmicsOps 自身标识。

以上服务文档核对日期为 2026-09-30。服务允许范围、账号权益与实际拒绝响应
保持可见；源码参考、接口文档和测试替身不等于真实订阅验收。

## 3. 接入方式选择

| 方式 | 优点 | 对本目标的影响 |
| --- | --- | --- |
| 分别使用订阅 HTTP、官方 CLI 模型适配和 Go HTTP（采用） | 可继续使用现有宿主工具；认证归属清晰 | 需要新增 Responses 和受限 CLI 适配 |
| 所有订阅均提取 OAuth token 后作为 HTTP 模型 | 传输外观统一 | 不符合 Claude 已核对的认证边界，不采用 |
| 将三种 CLI 作为 ACP Agent | 复用 CLI 原生会话和工具 | 改变工具所有权与会话绑定，不符合用户选择 |

## 4. 领域配置、客户端与兼容性

`ModelProviderKind` 新增 `open_ai_responses`、`open_ai_codex`、`claude_code`。
既有 `anthropic`、`open_ai_compatible`、`ollama` 的值和默认解释保持不变。
provider 显式决定协议与认证，不根据模型名、Bearer 字符串或 URL 猜测认证类型。

| provider | 配置端点 | 认证 | 实际传输 |
| --- | --- | --- | --- |
| open_ai_codex | 固定 https://chatgpt.com/backend-api | 每个配置的 keyring token bundle | /codex/responses |
| open_ai_responses | Go 使用 https://opencode.ai/zen/go/v1 | 已有每配置 API key | /responses |
| claude_code | 固定 claude-code://local，作为传输标记 | 官方 CLI 自行管理 | 本地原生可执行程序的 stdin/stdout |

CLI 配置增加可选 `cli_executable`：默认从 system PATH 解析官方原生程序，也
允许用户指定可执行文件绝对路径；不能包含参数。Codex 配置增加后端生成的
`subscription_account_ref`（不含身份信息的 UUID），与 keyring bundle 内部绑定
一致。两个字段缺省时不序列化，旧配置反序列化后原行为和执行哈希不变。
CLI 路径及 Codex 账户引用进入新配置的执行哈希；标签和 token 刷新不进入哈希。

跨 UI 的保存、登录、状态 DTO 定义在 `omicsops-dto`，由桌面端重新导出，前端
通过 `types.ts` 与 `tauri-api.ts` 使用。新增可编辑字段遵循现有 omitted/null
语义；账户引用由后端设置，不能通过普通保存请求伪造。provider 与专用配置
不匹配时拒绝保存。编辑既有配置继续保留已冻结的能力快照和可选字段。

在 adapters 新增 `ModelClient` 门面，分别封装既有 HTTP client、Codex 认证
包装与 Claude CLI client。门面提供当前桌面端使用的校验、预算、探测、文本
完成和 provider event 接口。CLI 生命周期和 OAuth 不塞入现有 HTTP 请求构造。
Responses 请求/解码是独立责任模块，可被 Go 和 Codex 共用；既有 HTTP 模块
只做必要接线，不因文件长度重构。

`commands.rs`、`model_commands.rs` 和 `DesktopModelPortV4` 改为经门面工厂
创建客户端；继续复用 `ProviderRequest`、`ProviderStreamEvent` 和
`ModelPortV4`。预算测量不执行网络或启动 CLI；真正发送前仍再次校验。

模型配置已以 JSON 存储，新增字段和 enum 不要求新增表。若实施发现确需 schema
变化，必须修订本设计并采用幂等迁移。旧配置、旧执行哈希和旧冻结运行必须有
回归测试；新 provider 记录不保证能被旧二进制读取，不做自动降级。

## 5. Codex 登录与凭据生命周期

第一版采用 device 登录，避免固定本地回调端口占用和浏览器 callback 子系统。
使用已核对的公开 Codex device 流程配置，并显式标识 OmicsOps 客户端；不绕过
账号关闭 device 登录、工作区限制或服务拒绝，不任意追加 OAuth scopes。
浏览器 PKCE、已有 CLI 凭据导入和多账户池留到单独的后续设计。

共享 HTTP 客户端必须编译启用 reqwest 的 `system-proxy`；Windows 系统代理是
默认网络路径的组成部分，环境代理仍按库的优先级生效。不能在关闭默认依赖
功能时意外退化为直连。本次不新增独立代理配置，固定 HTTPS 端点、证书校验与
拒绝重定向仍然生效；macOS 的依赖支持不表示已经完成该平台的实际登录验证。

状态机为 `pending -> authorized -> saved`，另有 `cancelled / expired / failed`。
发起登录返回有界的 `login_id`、固定验证地址、用户码和到期时间；前端只取得
这些显示字段与状态。后端按服务给定的间隔查询，处理 pending/slow-down，最多
等待 15 分钟。关闭登录面板、按 Escape 或取消会停止该 login_id 的后续轮询。
过期、应用退出后旧 login_id 失效；不能把一次性码作为可复用凭据。

设备码响应兼容官方 `user_code`/`usercode` 字段。认证适配器只识别固定的地区
拒绝、访问拒绝、网页验证、限流和协议错误类别；403/404 的有效 pending 响应
继续等待，明确拒绝与 HTML 页面不能冒充 pending。原始响应和未知错误仍丢弃。

令牌交换成功后仅在内存保留有界待保存凭据；用户保存模型配置时先写现有
keyring 的 `model/<profile_id>`，再写非敏感 profile JSON。新配置的数据库保存
失败时清理本次新建的 keyring 项；编辑必须保留旧秘密，失败时恢复，并报告
恢复失败，不能声称跨 SQLite/keyring 有原子事务。未保存、取消或过期的待保存
凭据须释放。access/refresh token、OAuth code、PKCE verifier、JWT 原文与账户
ID 不进入 DTO、SQLite、事件、日志、上下文或 Git，秘密类型不得派生泄密 Debug。

bundle 内含 token、到期时间、账户 ID 和对应的不透明账户引用。运行前在同一
账户锁内重新读取 keyring；临近到期刷新，刷新成功后持久化新 refresh token，
并确认账户不变。并发聊天、摘要与委派共用该账户锁，避免重复使用旋转令牌。
正常 token 轮换不改变执行哈希。登录另一账户创建另一模型配置；不允许借
重新登录替换活动运行的身份，也不允许把其它配置的 token 绑定到当前配置。

只能向精确 HTTPS auth.openai.com:443 认证端点和
chatgpt.com:443 的审核订阅路径发送对应凭据；拒绝 userinfo、query、fragment、
未知端口与重定向。生成请求带 Bearer 和账户头，不回退 api.openai.com。
刷新不确定时重新读取凭据，不能盲目重复 token 交换；invalid_grant、撤销和
授权失败显示重新登录提示。明确的生成 401 最多触发一次串行刷新和一次同账户
重试；其它部分流中断沿用有界重试规则，不根据重连推断原计算已取消。

退出仅清除该配置凭据，复用活动运行/队列的引用保护；不会退出独立 Codex CLI。
删除模型配置沿用现有删除事务与失败报告。保存成功不宣称订阅模型访问成功，
连接探测和完整工具回合分别验收。

## 6. Responses 请求与流事件

请求将宿主 system 层映射为 `instructions`，普通消息映射为 `input`；历史
工具调用与结果映射为 function_call / function_call_output，保留 call_id。
只发送宿主明确授予的 function tools，不注册供应商内置执行工具。Codex 的
`store:false`、流请求和订阅端参数限制单独塑形；Go 按其 Responses 协议发送。
不把 Chat Completions 的全部参数原样复制，也不默认请求服务端跨回合状态。

完整宿主历史每回合重放；如响应提供必须回放的 reasoning item，将其作为受
校验的 provider continuation 保存/回放，排除 reasoning 正文预览，并计算入
请求预算。若现有历史契约不能容纳此项，实施计划必须明确新增可选字段和旧
记录兼容测试；不能靠丢失必要 item 来声称完成多轮工具兼容。

SSE 解码支持拆包、UTF-8 分片、文本 delta、function 参数 delta、输出项结束、
usage、completed、incomplete 和 error。Completed 必须来自合法终止响应；
网络 EOF、[DONE] 或半段工具 JSON 不代替成功终止。call_id 与输出 index 逐项
校验，重复结束不能重复触发工具。服务拒绝、输出截断和失败保留可辨识分类。

文本、瞬时推理预览、调用建议与真实 usage 映射为既有 provider events。只有
经过完整参数解析的调用进入宿主 accumulator 和工具验证。已收到部分文本或
工具参数后不改用其它协议/供应商重发；重试不能把旧片段拼到新 attempt。

## 7. Claude Code 作为模型传输

用户在官方 Claude Code 自行安装与登录；OmicsOps 提供安装/登录指引和只读
状态检查，不收集登录 token、不启动自动安装或自动升级，也不代替用户退出。
删除该 profile 只删 OmicsOps 配置，不能删官方 CLI 登录状态。

Windows 第一版要求官方原生 `claude.exe`；`.cmd`、`.bat` 和包含 argv 的路径
拒绝并给出原生安装指引，不通过 cmd/PowerShell 拼接启动。其它平台可使用
原生可执行文件，但在没有实际验证前仅记录未验证，不声称已发布支持。

在与项目无关的空临时 cwd 中，通过 `omicsops-process::background_command`
启动一个有界的 print 请求。固定 argv 要求 safe mode、restricted mode、关闭
所有内置工具、拒绝 MCP 工具、严格空 MCP 配置、无用户/项目/local settings、
无浏览器集成、禁用 slash commands、无会话持久化和单轮输出。不得使用
`--bare`、`--continue`、`--resume`、bypassPermissions 或危险跳过审批参数。

上述配置仍可能受 managed settings/hooks 影响。预检必须核对实际 CLI 支持的
限制参数和无执行能力的启动状态；不能只凭版本号宣称安全。若 CLI 无法确认
禁用工具/MCP，或者 managed hooks 无法在该版本可靠关闭，则拒绝这条模型
路径并解释原因。不能为了兼容旧版本删掉限制参数；也不能宣称此进程是 OS
级沙箱。CLI 自身认证和缓存由其管理，OmicsOps 不保证其全部状态位于 keyring。

固定 settings 明确设置 `disableAllHooks:true` 并关闭 Claude.ai connectors。
[官方 hooks 说明](https://code.claude.com/docs/en/hooks)
指出非 managed 层不能关闭 managed hooks。生成前必须完成有效策略的只读
预检；存在仍生效的 managed hooks、无法枚举策略来源或策略格式未知时拒绝
生成，不等待 SessionStart/tool 事件发生后才认定边界成立。不得修改组织策略。

进程 env 由宿主构造，仅允许启动、系统、代理/证书和官方订阅登录所需项目；
去除 API key/token、apiKeyHelper、替代 Base URL、云 provider 开关、额外工具/
插件与调试注入变量，不在父进程修改环境。调用官方 `auth status` 的 JSON，
仅允许已登录的 Claude 订阅认证；API/云 provider、未知格式和未登录均拒绝。
执行前核对账户元数据，运行内观察到身份变化即失败；若 CLI 未暴露稳定身份，
须明确显示账户由 CLI 管理且不能冻结，不能作出固定账户恢复保证。

完整 `ProviderRequest`（system、角色历史、可用工具 schema）通过 stdin 作为
结构化文本提供，不出现在 argv、临时文件或调试输出。模型被要求返回严格
JSON envelope，仅允许 `{ "kind":"final", "text":"..." }` 或
`{ "kind":"tool_calls", "calls":[...] }`；call 每项包含唯一 call_id、已授予
tool_id 和对象参数 arguments，禁止额外字段和空 calls。第一版用模型文本
协议和宿主解析，不使用需要额外内部工具回合
的 CLI `--json-schema` 工作流；不得称其为原生 Anthropic function calling。

CLI 的 stream-json 作为传输记录解析：检查启动状态、模型身份、exit code、
result 是否为错误和完整 envelope。限制每行、累计 stdout/stderr、输入和
工具数量：stdin 至多 8 MiB、单行 stdout 至多 1 MiB、累计 stdout 至多 8 MiB、
累计 stderr 至多 64 KiB，每次 envelope 至多 16 个 calls；宿主剩余工具预算
可以更低。启动/认证状态检查每项最多 15 秒，生成沿用现有模型 attempt 的
idle/hard deadline。超过任一界限失败并回收进程。原始协议 JSON 不直接展示为最终
回答，也不写入证据。接收片段可更新活动状态，最终校验后再发出正文和工具
调用事件；因此第一版 Claude 不保证逐 token 的可见正文流。

call_id 重复、未授予工具、非对象参数、正文/工具混合歧义和非法 JSON 拒绝。
合法调用建议交回现有 V4 宿主工具注册表、参数验证与审批；工具结果由宿主
持久化，再进入下一次独立 CLI 模型请求。不向 CLI 导出 MCP 网关，也不给它
执行项目代码的能力。CLI 自己的工具执行事件一旦出现即视为边界失败，不计为
宿主工具成功。截图、隐藏 CLI 思考和工具建议不作为科学证据。

## 8. OpenCode Go 扩展

保留已存 Chat/Messages profile 的 protocol、model ID、keyring 引用、能力
快照和执行哈希，不自动迁移为 Responses。为官方路由表明确要求 Responses
的完整模型 ID 新增选择/保存路径；用户切换模型时同时明确采用匹配协议。
旧协议搭配这些模型仍拒绝并提示正确协议，不能同时试发多个端点。

实施时对固定核对日期的官方路由表和 models.dev 数据同时核对，形成完整 ID
映射；不按 `gpt-`、`grok-` 或家族前缀猜协议。Go 的精确 HTTPS host、443 端口、
路径、userinfo/query/fragment 校验与禁止重定向规则覆盖新 Responses 请求。
所有 Go 主/辅助模型请求发送自身 User-Agent 和当前 conversation UUID 的
`x-opencode-session`；重试稳定，独立会话不能共享身份。

模型查询失败保留错误，允许手填完整 ID。服务额度不足与平台启用的余额行为
按真实服务响应显示；OmicsOps 不自动改用 Zen 或 API 配置。

## 9. 能力目录、预算与请求选项

继续以编译的 models.dev 快照为能力来源，精确匹配协议、HTTPS host/port 和
完整 ID；Go 额外匹配完整 base path。Responses/订阅行只能由审核后的导入
规则生成，并保留原数据哈希；不能运行时把 api.openai.com 的同名模型能力
复制到 chatgpt.com 或未知网关。若没有可审计的订阅目录条目，采用现有未知
模型保守预算；不编造订阅额度、上下文或输出上限。

Claude CLI 传输不满足 HTTP 目录的精确匹配条件，第一版保持能力未知和现有
保守预算，可设置更小的用户预算，但不以同名 Claude API 模型推断最大能力。
模型 alias 只作为显式 CLI 请求值；不因此获得目录能力。工具可用表示宿主
envelope 桥接有实现，不表示外部 CLI 拥有已授予的工具。

measure/validate/send 对同一已塑形内容检查，计入 system、历史、tool schemas、
envelope 说明和 provider continuation。Claude 还受官方 stdin 大小约束，不能
通过丢失历史或写项目文件绕过。Responses effort 使用其审核字段；CLI 请求
不继承未验证的 effort/Fast 参数。旧 Fast 支持矩阵不自动扩大。

CLI 的输出预留用于宿主保守预算，不等于已向服务端设置相同 max_output_tokens；
字节上限也不是 tokenizer 上限。界面、测试和验收报告必须保留这个区别。

真实 usage 才记为 provider observation；CLI 缺少 token 计数则保持 unknown，
不能把零计数、进程时长或订阅计费误记为 token 使用量。CLI 可能聚合内部
请求，界面与审计注明该观测范围，不与单个 HTTP sample 重复累加。

## 10. 界面与登录 DTO

设置 Models 增加 Codex subscription、Claude Code subscription 两个入口，
Go 入口增加 Responses 选择。Claude 显示程序位置、登录状态和官方登录指引，
不显示 API key 输入框；Codex 显示设备登录、保存、取消和重新登录；Go 保留
套餐 key 表单。高级字段只展示该传输已实现的选项。

聊天模型选择器继续按 profile_id 选择，展示“Codex 订阅”“Claude Code 订阅”
和“OpenCode Go”，避免把 CLI 标为 API。模型列表 DTO 区分服务返回、已保存
模型与手填入口；无发现支持不能伪造账号可用模型列表。

登录 DTO 最小包含 begin/poll/cancel/finish，以及只读 subscription status。
pending login 的凭据永不返回浏览器；finish 只接受 login_id 和待保存的模型
字段。所有请求验证 provider、profile/account 绑定与状态，旧/跨配置 login_id
不能用。登录和 profile 更新的竞态按状态机拒绝，重复完成不能重复保存。

优先使用设置内嵌表单；如果设备登录成为覆盖层，必须注册窗口级 Escape 堆栈，
在打开后无需聚焦就能关闭最顶层，并保持父设置窗口打开，同时取消对应轮询。
异步完成不得在取消后重新打开覆盖层。传输提示说明模型服务可能接收提示词、
工具结果和元数据，不作“科研数据永不离开本机”的承诺。

## 11. 停止、失败、恢复与审计

三条路径保留 V4 attempt、model idle/hard deadline、停止标志和冻结运行校验。
网络 keep-alive、CLI system/usage/retry 事件不当作正文进展无限延长等待。
停止 CLI 请求必须终止并等待宿主启动的子进程树回收；Windows 行为使用明确
的子进程生命周期策略和测试，不能只 drop 一个包装进程即声称已终止。

停止模型请求不表示已派发的 Linux SSH 后台计算取消，保持既有 run/job 语义。
认证/执行身份、CLI 路径或模型配置不匹配时恢复失败，不能自动换模型或账号。
token 正常刷新、标签变化不使冻结执行失效；更换账户引用使其失效。

错误按未安装、版本/限制不支持、未登录、认证失败、额度/权限拒绝、输入预算、
输出结构、流中断、超时和取消分类。仅保留脱敏诊断，不能记录原始 stderr、
token 响应、认证文件或完整 CLI 启动 JSON。审核与审计仍引用宿主工具结果。

## 12. 确定性测试与真实验收

每次行为变化更新自动化测试；测试不得依赖真实订阅、网络、SSH、R 或 CLI 安装。

| 层 | 必须覆盖 |
| --- | --- |
| core / DTO / store | 旧 JSON 与哈希不变；新 provider/字段合法矩阵；omitted/null；新执行身份冻结；活动引用删除保护；keyring/数据库失败恢复 |
| Codex auth | 假时钟和 mock HTTP 的 pending/slow-down/过期/取消；一次完成；错误脱敏；并发刷新与轮换；账户不变；撤销；固定端点和重定向拒绝 |
| Responses | 实际请求塑形与预算；工具历史回放；逐字节 SSE 与 UTF-8；完整/截断/错误/EOF；重复 call_id；usage；取消；部分流不跨 attempt 拼接 |
| Claude CLI | 模拟进程的 argv/env/stdin/cwd；版本与启动限制；订阅/API 认证区分；managed hooks 拒绝；合法/错误 envelope；越权工具；大小限制；超时与 Windows 子树回收 |
| V4 composition | 主模型、规划/摘要、旁聊、委派和审核采用选中路径；宿主工具 nonce 回合；本地删除仍需现有审批；拒绝后工具未执行；停止与恢复不回退 |
| UI | provider 保存 payload；各登录状态；取消竞态；无 key/token 暴露；发现失败和手填；打开后立即 Escape 只关闭顶层并取消轮询 |
| catalog | 导入脚本与精确 host/port/path/protocol/full-ID 匹配；不继承同名模型能力；旧快照不隐式更新 |

先执行相关 crate / 前端局部检查，再执行仓库要求的完整命令：

```powershell
cargo test --workspace
npm test
npm run build
npm run build:desktop
cargo fmt --all -- --check
```

锁文件变化须 `npm ci`。fmt check 仅因格式失败时运行 fmt，纯格式变更独立提交。
新增目录导入规则还须运行现有 Python catalog 测试。不把构建生成的安装包分发，
不创建 tag/Release，不修改版本号或生成普通变更的版本说明。

真实订阅验收先扩展 `acceptance/README.md`，每条路径新增独立 ignored 用例，
使用一次性 Store/项目与已显式配置的凭据。只读 nonce 用例要求模型经宿主
读取一次随机文件并返回 nonce；另以临时文件验证删除审批拒绝后文件仍在。
Claude 还须验证真实官方 CLI 的工具/配置关闭组合、订阅认证及 Windows 停止。
限制组合未真实验证时，不宣称 Claude 的生产边界已验收。

Windows GUI smoke 覆盖登录、保存、重启后的选择、简单工具回合、删除审批、
Escape 和停止。真实 Codex、Claude、Go 分别记录命令、环境类别、通过/失败/
未执行，不输出秘密。已有 Go Chat nonce 结果不替代新 Responses 或 Claude
路径验收；本功能的 nonce 测试也不代表 SSH/PBMC/科学结论端到端验收。

## 13. 依赖顺序与已知限制

书面设计通过后再制定实施计划：先补共享协议与兼容配置，再补 Responses，
然后分别补 Codex auth、Claude CLI，最后整合 UI/V4 并完整验证。按可验证增量
提交，不要求一次新增整套科学领域模型。此顺序是架构依赖说明，不是已批准
执行的任务计划；不自动授权子 Agent 或并行实现。

Claude 文本 envelope 的可靠性和关闭限制的实际有效性必须经真实 CLI 验收；
无法满足宿主审批边界的版本/策略应停止并报告，不能换为自由执行的外部 Agent。
Codex 订阅接口、登录可用性与 Go 模型路由可能变化；拒绝保持可见，不伪装其它
客户端、不轮换账户避开额度，也不静默产生按量费用。

## 14. 本次文档评审与验证记录

本次仅阅读本地源码、固定参考提交和官方文档，编写设计。未运行产品测试、
真实登录、订阅生成、SSH、CLI 或安装包验收；未修改功能代码。

已完成设计自检：凭据落盘仅使用现有 keyring；旧 JSON/哈希兼容明确；Claude
的 CLI 限制和 managed hooks 有生成前拒绝条件；envelope 与资源上限明确；
新增目录遵循精确匹配；完整检查和真实验收分开记录。用户于 2026-09-30
确认此文件，现进入[书面实施计划](../plans/2026-09-30-subscription-models.md)评审与执行方式选择。

## 实施核对：Claude 执行资格（2026-10-01）

已核对 [CLI reference](https://code.claude.com/docs/en/cli-reference)、[hooks-guide](https://code.claude.com/docs/en/hooks-guide) 和 [managed-settings](https://code.claude.com/docs/en/managed-settings)。普通 settings 的 disableAllHooks 不能禁用 managed hooks；现有来源确认依赖会话内 /status。当前原生 runner 无法在 SessionStart 前完整验证有效策略，因此状态展示版本/订阅认证结果，同时返回 claude_policy_unverifiable，生产生成保持拒绝。不会修改组织策略、读取 CLI 凭据或将 mock runner 验证当作真实 CLI 验收。要启用生产生成，需要可核对的官方只读策略快照及相应真实验收。


## 实施与验证状态（2026-10-01）

Codex 设备登录/keyring 协调器、固定 Responses 传输、Go 精确路由及现有聊天接线已实现；工具、审批、冻结配置和事件证据仍由 OmicsOps 宿主裁决。Claude 配置/只读状态/受控进程及 envelope 已实现，生产生成继续拒绝，尚未达到三类订阅全部可用的目标。

完整确定性检查已通过：1471 Rust、970 Vitest、22 browser bridge、4 Python；Web/Windows 桌面构建与格式检查通过。新增四项真实订阅 ignored 测试、Windows GUI、macOS、SSH 与 PBMC 未执行。实际命令及逐功能提交见[实施记录](../plans/2026-09-30-subscription-models.md)；不将测试替身、打包成功或被忽略测试视为真实端到端验收。

最终审查修复后，完整确定性检查为 1480 Rust（16 ignored）、970 Vitest、22 browser bridge、4 Python；Web/Windows 桌面构建、格式和 diff 检查通过。独立审查发现的敏感参数持久化、续接闭合和错误分类问题已分别修复并提交，新增回归均先失败后通过。真实订阅与 GUI/其它执行环境验收未执行，Claude 生产限制保持。完整裁决、风险和审查范围见实施记录；分支保留本地，未发布。
