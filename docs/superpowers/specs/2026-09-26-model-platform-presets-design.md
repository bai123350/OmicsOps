# 多平台模型配置入口设计

日期：2026-09-26。范围：参考 Wisp Science 的平台接入边界，复用已有协议补齐配置入口。平台预设不等于新的模型协议，也不等于真实服务验收通过。

## 目标与参考差异

用户要求按参考项目补齐多平台接入，并在每个可验证增量完成后提交。固定核对参考提交 `f684c7995fc02eb4a7dec3ee4f01969416e0d64b`。Wisp `wisp-llm` 的 [provider.rs](https://github.com/xuzhougeng/wisp-science/blob/f684c7995fc02eb4a7dec3ee4f01969416e0d64b/crates/wisp-llm/src/provider.rs) 与 [lib.rs](https://github.com/xuzhougeng/wisp-science/blob/f684c7995fc02eb4a7dec3ee4f01969416e0d64b/crates/wisp-llm/src/lib.rs) 明确列出 OpenAI、DeepSeek、Qwen、MiniMax、Ollama、LM Studio 的 Chat Completions 兼容接入，以及 Anthropic Messages。参考 [model-presets.json](https://github.com/xuzhougeng/wisp-science/blob/f684c7995fc02eb4a7dec3ee4f01969416e0d64b/apps/macos/Sources/WispProjectBrowserUI/Resources/model-presets.json) 还包含 OpenCode、Kimi、GLM、Kimi Coding、GLM Coding。参考另有 OpenAI Responses 和 OpenAI Codex；前者是另一种线协议，后者还有订阅登录/令牌生命周期，不能当作普通 API key 预设。

本次交付上述平台与 Coding 变体的明确入口，保留 Anthropic、OpenCode Go、自定义 OpenAI-compatible，加入已被本地能力目录覆盖的 OpenRouter。Qwen 与 MiniMax 同时提供中国和国际端点。Responses、ChatGPT/Codex 订阅登录不在此次平台预设增量内；不新增 Gemini 原生协议，参考清单没有要求该协议。参考文档中的图像/视频生成角色不属于本次聊天平台配置范围。

## 平台与协议

| 配置入口 | 保存的 provider | 默认 Base URL | 本次行为 |
| --- | --- | --- | --- |
| OpenAI | open_ai_compatible | https://api.openai.com/v1 | 独立品牌入口 |
| DeepSeek | open_ai_compatible | https://api.deepseek.com/v1 | 保留已有入口 |
| Kimi | open_ai_compatible | https://api.moonshot.cn/v1 | 新增按量 API 入口 |
| Kimi Coding | open_ai_compatible | https://api.kimi.com/coding/v1 | 新增专属套餐入口，注明客户端适用限制 |
| GLM | open_ai_compatible | https://open.bigmodel.cn/api/paas/v4 | 新增按量 API 入口 |
| GLM Coding | open_ai_compatible | https://open.bigmodel.cn/api/coding/paas/v4 | 新增专属套餐入口，注明客户端适用限制 |
| Qwen 中国 | open_ai_compatible | https://dashscope.aliyuncs.com/compatible-mode/v1 | 新增入口 |
| Qwen 国际 | open_ai_compatible | https://dashscope-intl.aliyuncs.com/compatible-mode/v1 | 新增入口 |
| MiniMax 中国 | anthropic | https://api.minimax.cn/anthropic | 新增入口，使用官方 Messages 兼容 API |
| MiniMax 国际 | anthropic | https://api.minimax.io/anthropic | 新增入口，使用官方 Messages 兼容 API |
| Ollama | ollama | http://127.0.0.1:11434/ | 保留原生 Ollama API |
| LM Studio | open_ai_compatible | http://127.0.0.1:1234/v1 | 新增入口，默认本地无密钥 |
| OpenRouter | open_ai_compatible | https://openrouter.ai/api/v1 | 新增入口，保留完整供应商/模型 ID |
| Anthropic | anthropic | https://api.anthropic.com/ | 保留已有入口 |
| OpenCode Go | 按已审核完整模型 ID 选择 | https://opencode.ai/zen/go/v1 | 保留 Chat/Messages 路由与 Responses 拒绝规则 |
| OpenAI-compatible | open_ai_compatible | https://api.openai.com/ | 保留可编辑自定义入口 |

Qwen 端点依据 [阿里云 Base URL 文档](https://www.alibabacloud.com/help/en/model-studio/base-url)；地区密钥不可混用。MiniMax 依据 [国际文档](https://platform.minimax.io/docs/api-reference/text-anthropic-api) 与 [中国文档](https://platform.minimax.cn/docs/api-reference/text-anthropic-api)。LM Studio 的 [兼容工具接口](https://lmstudio.ai/docs/developer/openai-compat/tools) 使用 `/v1/chat/completions`，[默认服务器](https://lmstudio.ai/docs/developer/rest/quickstart) 不要求认证。资料核验日期为本文件日期。

Kimi 的两个平台与 key 来源依据 [官方平台说明](https://www.kimi.com/code/docs/kimi-code/faq.html) 和 [官方仓库接口表](https://github.com/MoonshotAI/kimi-code/blob/main/docs/AGENTS.md)；GLM 两个 API 路径依据 [ZCode 官方 endpoint 对照](https://zcode.z.ai/en/docs/qa)。Coding 入口明确提示使用专属套餐 key，并受服务方的套餐和客户端适用范围约束，OmicsOps 未被据此认证为官方支持客户端。不添加冒充 Claude Code、Kimi Code、Codex 的 User-Agent，不绕过认证、配额或服务方客户端限制。普通产品集成优先使用按量入口；拒绝响应保持可见。

上游预设的示例模型分别为 Kimi `kimi-k3`、GLM `glm-5`、Kimi Coding `kimi-coding`、GLM Coding `glm-5.2`；这些是参考内容，不是 OmicsOps 的能力声明，本次新入口仍留空让用户填写当前账户可访问的完整 ID。

新入口的模型输入允许任意完整 ID；不为新增品牌硬编码未经目录确认的默认模型或能力。既有 DeepSeek/OpenCode Go 模型建议保持原行为。MiniMax 复用现有 Anthropic wire adapter，不引入一个 MiniMax enum；Ollama 继续原生协议，不改写旧 profile。

## 最小实现边界

1. `src/features/settings/modelProviderPresets.ts` 保存静态展示配置与 exact endpoint 匹配辅助函数；`SettingsPanel.tsx` 消费预设生成表单。预设只填充 label、provider、base_url，不进入 SQLite，不参与执行身份，也不声明工具/视觉/上下文能力。配置标记按协议和标准化后的精确 endpoint 判断，不能因某个 Anthropic 协议 profile 存在就把 MiniMax 和 Anthropic 都标记已配置。域名、端口、路径、用户名、查询、fragment 均参与判定；允许所列端点的尾斜线以及原有官方 root/v1 等价写法。
2. 不改变 `ModelProviderKind`、`ProviderProtocol`、`ModelProfile`、`SaveModelProfileRequest` 的序列化值和结构，不新增表、不拆分 adapter。UI 仍保存现有协议、Base URL、完整 model ID。既有 profile 可原样编辑、保留凭据引用及能力快照。
3. 仅对 `OpenAiCompatible` + `http` + `127.0.0.1`/`localhost`/IPv6 loopback + 端口 `1234` + 根路径或 `/v1`（含尾斜线）允许无密钥。拒绝 username/password/query/fragment。此条件是已审核的本地 LM Studio 配置，不证明服务进程身份。其它端口、LAN/公网地址、协议仍沿用现有凭据要求。用户配置本地 token 时仍使用 keyring 和 Bearer；无 token 时不发送空 Authorization。无认证本地 client 禁止重定向，不能把本地认证例外传播到远程目标。
4. 后端保留非 Ollama 的凭据引用，确保可选 LM Studio token 仍沿用现有 keyring 生命周期；允许该引用暂无 secret。不往数据库存入占位密钥。不把其它 provider 的认证改成全局可选。
5. Qwen `/compatible-mode/v1` 已会追加 `chat/completions`；MiniMax `/anthropic` 已会追加 `v1/messages`，无需改写。GLM 仅对 `OpenAiCompatible` + HTTPS `open.bigmodel.cn:443` + 精确 `/api/paas/v4` 或 `/api/coding/paas/v4`（允许尾斜线）直接追加 `chat/completions` / `models`；不得再插入 `v1`。同样拒绝 username/password/query/fragment，不把此特例应用到未知 host 或路径。不新增先后试发多个生成请求的 fallback。保护未知 `/gateway` 配置原有追加 `/v1/...` 的行为。模型发现失败保持可见错误，不伪造平台返回的列表；手动输入模型始终可用。
6. 平台卡片多于单个窗口可见高度；点击配置入口、切换预设或编辑已保存配置时，表单滚动进入设置面板的可见区域。普通模型、地址和密钥输入不重复滚动，滚动不强制转移键盘焦点；Escape 仍由现有窗口级堆栈处理。

## 能力、恢复与安全

能力来源仍是 `src-tauri/src/model_catalog.json` 编译快照，经 `model_catalog_shared.rs` 按 exact protocol + HTTPS host/port + 完整 model ID 匹配；OpenCode Go 继续额外匹配 path。新增预设不触发目录刷新，不把一个地区或未知代理的模型能力复制给另一地区。保留 OpenRouter 供应商前缀。LM Studio 不继承云端同名模型能力，仍使用当前未知模型的预算策略。

本次不更新 models.dev 数据，也不更改已保存的 catalog snapshot、profile configuration hash、运行恢复判断、reasoning/fast 参数授权范围。云端使用既有 keyring API key 路径；模型和远程服务仍可能接收提示词、结果和元数据。宿主审批、MCP 授权、SSH、本地计算与证据语义不变。

本次实现的是配置与请求路由覆盖，不能凭兼容 API 声明厂商所有模型特性都可用。MiniMax 文档要求多轮原生工具会话保留完整 assistant 内容；现有 OmicsOps provider/Agent 交互没有为此次预设新增原生思考块回放。不得据此宣称已完成 MiniMax 全特性多轮验收。视觉输入仍受已有预算支持检查约束。

## 验证与交付

后端先覆盖本地认证允许/拒绝矩阵、可选 token、Authorization 省略、禁止本地重定向、各平台 endpoint；前端覆盖所有入口的准确保存 payload、精确已配置标记、编辑、地区切换和清空 credential、现有 OpenCode Go 特殊行为。使用纯函数、内存 keyring、模拟 Tauri 命令与本地 mock server，不能要求真实模型服务或 API key。

默认完整检查：`cargo test --workspace`、`npm test`、`npm run build`；本次后端涉及桌面 client 组合时再运行 `npm run build:desktop`。锁文件变化才需要额外 `npm ci`。新建可关闭层须遵循窗口 Escape 堆栈；优先复用现有内嵌表单，新增按钮不引入覆盖层。

交付记录区分确定性检查结果与未执行的真实云端/LM Studio 验收。Windows 手工步骤：打开各预设、检查 endpoint/协议、保存编辑、以安装好的 LM Studio 服务器进行模型查询和简单工具回合；云端使用对应地区 key 查询/测试，再新建对话。没有真实服务证据时明确记录未执行。
