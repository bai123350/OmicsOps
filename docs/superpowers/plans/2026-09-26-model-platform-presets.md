# 多平台模型配置入口实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 按参考项目补齐平台与 Coding 变体入口，复用现有协议，同时让默认本地 LM Studio 无密钥配置可工作。

**Architecture:** 平台仅为前端静态预设，持久化协议与模型身份保持不变。后端增加 exact loopback LM Studio 的可选认证，以及 GLM 两个精确官方 Base URL 的路径拼接，并通过回归测试保护其它端点行为。能力继续使用后端编译目录的 exact lookup。

**Tech Stack:** Rust、reqwest、Tauri、React 19、TypeScript、Vitest。

**Spec:** [多平台模型配置入口设计](../specs/2026-09-26-model-platform-presets-design.md)。执行方式沿用用户已指定的编写/检查代理分工；每个增量完成后单独提交，保留未跟踪的 `website/`。

## Global Constraints

- 不新增供应商品牌 enum、数据库表或凭据存储路径。
- 不修改冻结 profile hash/能力快照；不更新 models.dev 数据；不推断家族能力。
- OpenRouter 保留完整供应商/模型 ID；未知 gateway 不获得官方能力。
- 无认证例外只允许设计所列 LM Studio loopback HTTP:1234 root/v1；可选 secret 仍进 keyring。
- 不实现 Responses、Codex/ChatGPT 订阅 OAuth 或 Gemini 原生协议；不把预设当成真实服务验收。
- Coding 入口注明专属 key/客户端适用限制，不新增伪造官方客户端 User-Agent。
- 不改动不相关工作，不提交 website、构建产物、凭据、日志、真实数据。

## Review Focus

- 非默认端口/LAN/伪造 localhost 域名不能继承免密行为；任务 1 用拒绝矩阵覆盖。
- 本地无认证服务返回 30x 到远程地址时不能继续请求；任务 1 断言 redirect policy 与请求行为。
- MiniMax 与 Anthropic 共用协议时不能同时误标已配置；任务 2 用精确 endpoint 测试覆盖。
- 编辑旧 profile 不能清掉预算、reasoning/fast、委派绑定或触发目录刷新；任务 2 复用并补充现有回归。
- 模型发现错误/空列表不是保存失败；任务 2 保持可输入完整 model ID 和可见查询错误。

## Task 1: exact 本地认证与平台端点回归

**Files:**
- Modify/Test: `crates/omicsops-adapters/src/llm.rs`
- Test if needed: `src-tauri/src/model_commands.rs`
- 本任务不提交 docs；设计/计划随任务 2，最终验证由 root 单独记录提交。

**Interfaces:**
- Consumes: 现有 `ProviderProtocol`、`UnifiedModelClient::new`、`authenticate`、`provider_endpoint`、`provider_models_endpoint`。
- Produces: `is_local_lm_studio_endpoint(protocol: ProviderProtocol, base_url: &Url) -> bool`（adapter 内部辅助函数），供创建 client、认证及 redirect policy 共用。
- Produces: `is_glm_api_base_url(protocol: ProviderProtocol, base_url: &Url) -> bool`（adapter 内部辅助函数），仅匹配 spec 指定的 HTTPS `open.bigmodel.cn:443` 两条完整路径，用于生成与模型发现 endpoint 拼接。

- [x] 添加失败测试：`OpenAiCompatible` 的 `http://127.0.0.1:1234/v1`、`localhost`、IPv6 loopback、root/尾斜线变体可缺 credential；远程、其它端口、非 Chat 协议、含 userinfo/query/fragment 的 URL 仍被拒绝。非空 token 依旧接受。
- [x] 添加请求级测试：无 secret 不生成 Authorization，有 secret 为 Bearer；保存模型时只产生既有 credential_reference，无 secret 不写 keyring；禁止无认证 client 追随重定向。可使用 request builder 检查与本地 mock server，无真实网络。
- [x] 添加平台端点表测试：Qwen `.../compatible-mode/v1/chat/completions`；MiniMax `.../anthropic/v1/messages`；OpenRouter `.../api/v1/chat/completions`；Kimi `/v1/chat/completions`；Kimi Coding `/coding/v1/chat/completions`；GLM `/api/paas/v4/chat/completions`；GLM Coding `/api/coding/paas/v4/chat/completions`；LM Studio `/v1/chat/completions`。GLM 对应列表直接追加 `/models`。保护现有 OpenAI、Anthropic、Ollama、OpenCode Go 与未知 `/gateway/v1/...` 拼接；GLM 非官方 host、非443端口、相邻路径、userinfo/query/fragment不获得特例。
- [x] 运行 `cargo test -p omicsops-adapters llm`，确认失败来自预期新增行为。
- [x] 实现上述 helper，修改凭据必需判断与认证生成；本地例外启用 `reqwest::redirect::Policy::none()`。保留非 Ollama credential_reference，避免引入 DTO/schema 迁移；仅 GLM 精确匹配增加 endpoint 分支，其余保持原拼接。
- [x] 重跑目标测试；若 desktop 模型保存测试有变动，运行 `cargo test -p omicsops-desktop --lib model_`。
- [x] 审查 diff 后单独提交：`fix: support local LM Studio and exact GLM API endpoints`。仅暂存本任务代码/测试文件。

## Task 2: 平台预设入口与配置回归

**Files:**
- Create: `src/features/settings/modelProviderPresets.ts`
- Create: `src/features/settings/modelProviderPresets.test.ts`
- Modify/Test: `src/features/settings/SettingsPanel.tsx`、`SettingsPanel.test.tsx`
- Modify: 本次 spec/plan 的实际验证记录

**Interfaces:**
- Consumes: Task 1 的本地认证语义，已有 `FormState`、`ModelProfile`、`onSaveModel`。
- Produces: 静态 `modelProviderPresets`（id/label/provider/base_url）以及 `matchesModelProviderPreset(profile, preset): boolean`；UI 初始表单仍使用原来的 provider/base_url/label/model/credential 属性。

- [x] 为 spec 平台表的每个新增入口写测试：点击、检查默认 Base URL/协议、输入完整 model ID、保存，断言 payload 精确且无额外 preset_id。模型初值留空，既有 DeepSeek/OpenCode Go 默认保留。
- [x] 写匹配测试：相同协议不同品牌/地区不相互标记；标准端口和尾斜线等价；任意 subdomain、非默认端口、查询串、额外路径不匹配。保留官方 root/v1 的既有 profile 识别。
- [x] 写 UI 回归：LM Studio API key 标注可选且允许填 token；换成远程 URL 后不再显示本地免密说明；新增配置不继承另一个表单的 credential；OpenRouter ID 不删前缀；保存既有 profile 不触发 refresh_catalog。
- [x] 运行 `npm test -- src/features/settings/SettingsPanel.test.tsx src/features/settings/modelProviderPresets.test.ts`，确认预期失败。
- [x] 用独立数据文件实现预设及 matcher，在现有 provider-grid 加入入口，复用内嵌表单；不重构其余设置页面、不新增覆盖层。保留 Anthropic/OpenCode Go/Ollama/custom 入口和原测试定位。
- [x] 新增入口提示所用协议和地区，MiniMax 用 Messages；Kimi/GLM Coding 注明套餐 key 与服务方客户端限制、不增加冒充客户端的 UA。测试相关提示可见且保存 payload 不增加 UA。不要展示由品牌推断的能力或预算；Kimi/GLM 保持目录未知模型预算。
- [x] 重跑目标前端测试，运行 `npm run build`。
- [x] 审查 diff 后单独提交：`feat: add model platform configuration presets`，包含 spec/plan；不要暂存其它代理或用户文件。

## Final verification and record

- [x] 运行 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`，记录实际命令与退出结果。
- [x] 若锁文件改变，运行 `npm ci` 后重新验证前端。若 `cargo fmt --all -- --check` 仅因格式偏差失败，运行 `cargo fmt --all`，纯格式修改单独提交。（本次锁文件未变，格式检查通过。）
- [x] 检查 keyring-only、legacy profile/hash、已有 OpenCode Go routing/fast/reasoning/删除恢复测试未退化。
- [x] 记录真实模型/LM Studio Windows smoke 的通过、失败或未执行，不把 mock 请求或构建当作生产验收。
- [x] 将实际验证结果更新进文档并在对应实现提交或独立验证文档提交中保存。

## 执行记录

任务 1 已由提交 `0bb6f7e` 实现：仅对精确本机 LM Studio 地址允许空 key；无 key 时 GET/POST 不发送 Authorization，本地 client 不跟随 30x；GLM 两条官方路径直接追加 `chat/completions` / `models`。保留原 credential_reference，并让 Agent V4 与模型探测共用凭据读取路径。先观察到 LM Studio/GLM 新测试失败，再实现并通过 `cargo test -p omicsops-adapters --lib local_openai_auth_tests -- --nocapture`（5 通过）及 `cargo test -p omicsops-desktop --lib saved_local_profile_with_empty_keyring_reference_reaches_runtime_client -- --nocapture`（1 通过）。`cargo fmt --all -- --check` 通过。

任务 2 已实现 16 个配置入口与精确 endpoint 匹配，保留现有协议/profile 结构及 DeepSeek/OpenCode Go 特殊行为。前端新测试先观察到缺失入口及 URL 边界失败，随后 `npx vitest run src/features/settings/SettingsPanel.test.tsx src/features/settings/modelProviderPresets.test.ts --reporter=dot` 为 88 通过、0 失败；`npm run build` 退出 0。`npm test -- src/features/settings/SettingsPanel.test.tsx` 会先执行 package 脚本中的整个 Vitest 套件，红态时出现 12 个新增入口预期失败与 1 个未修改的 PluginsSettings Escape 测试失败；后续定向测试已全绿，完整套件由最终验证单独记录。

真实云端模型、LM Studio 服务、Coding 套餐 key、Windows 手工 smoke 未执行；上述结果仅为确定性自动化验证，不代表实际服务验收。

## 最终验证记录（2026-09-26）

实现提交为 `0bb6f7e`（本地认证与 GLM 路径）、`9425312`（平台入口与文档）、`7dc77ee`（打开配置表单时滚入视口）。独立审查发现的表单位于列表下方问题已在第三个提交修复并复核通过；没有剩余 P1/P2 发现。打开、切换及编辑配置各触发一次滚动，普通输入不触发滚动，不改变焦点或 Escape 堆栈。

| 实际命令 | 结果 |
| --- | --- |
| `cargo test --workspace` | 退出 0；1,339 通过，0 失败，12 忽略。后续提交只改前端和文档，没有再改 Rust。 |
| `npx vitest run src/features/settings/SettingsPanel.test.tsx src/features/settings/modelProviderPresets.test.ts --reporter=dot` | 最终定向测试 89 通过，0 失败。 |
| `npm test` | 最终版本退出 0；108 个测试文件、933 项前端测试和 22 项浏览器扩展测试通过。 |
| `npm run build` | 最终代码退出 0，TypeScript 与 Vite 生产构建通过。 |
| `npm run build:desktop` | 最终代码退出 0；Windows x64 release 编译及 NSIS 打包通过。仅本地构建，未安装或分发。 |
| `cargo fmt --all -- --check` | 退出 0；没有产生需要单独提交的格式修复。 |
| `git diff f083ed5..HEAD --check` | 通过。 |

依赖锁文件未变化，未触发 `npm ci` 要求。早期红态全套运行中的 PluginsSettings Escape 失败，在两次后续完整前端运行中均未复现；最终完整套件无失败。Vite 的大 chunk 提示为构建警告，未影响构建退出结果。

12 项忽略测试没有显式执行。真实云端 API、LM Studio 服务、Coding 套餐认证、SSH/科研流程以及 Windows 人工视觉 smoke 均未执行，不能将模拟请求、自动化测试或打包结果视为这些验收通过。手工检查步骤见设计文档。没有创建发布 tag、GitHub Release 或分发安装包；原有未跟踪的 `website/` 未修改、未提交。
