# Subscription Models Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 OmicsOps 现有聊天中使用 Codex、Claude Code 和 OpenCode Go 订阅，继续由宿主执行工具、审批和证据整理。

**Architecture:** 新增 Responses 适配、Codex keyring 认证协调器和关闭执行能力的官方 Claude CLI 模型适配，以 `ModelClient` 门面接入已有 V4。工具历史从验证过的事件投影为结构化回放，必要的供应商续接项有明确来源；旧 provider 与冻结运行保持兼容。Claude 生成前检查有效限制，未知策略拒绝；旁聊保留单次派发。

**Tech Stack:** Rust 2024、Tokio、reqwest/rustls、sqlx SQLite、既有 keyring、Tauri 2、React 19、TypeScript、Vitest；Windows 原生进程树管理。

**Spec:** [订阅模型接入设计](../specs/2026-09-30-subscription-models-design.md)，用户于 2026-09-30 确认书面设计。参考 Wisp 固定提交 `ade254989da5d395c6334c893637f32c3ef61ba7`。

状态：用户已授权实施与逐功能提交，当前采用 Native 执行；勾选框与文末记录跟踪实际进度。

## Global Constraints

- 使用已建 `codex/subscription-models` 分支；保留未跟踪的 `website/`，只暂存本任务文件。
- 新增 provider 为 `open_ai_responses`、`open_ai_codex`、`claude_code`；旧三个 serde 值、缺省字段和旧执行哈希不变。
- Codex 固定 `https://chatgpt.com/backend-api`；Go 固定 `https://opencode.ai/zen/go/v1`；Claude 传输标记固定 `claude-code://local`。
- 新增文本/宿主工具传输默认不支持图像，不扩大旧 Fast 支持矩阵，不推断家族/同名模型能力。
- Codex 使用 device 登录，最多等待 15 分钟；令牌只进现有 `model/<profile_id>` keyring 项，不返回 UI。
- 不读取 Claude/Codex CLI 凭据文件，不引入账户池，不冒充其它客户端，不自动切换按量 API 或 Zen。
- Claude Windows 第一版只接受原生 `claude.exe`；拒绝 `.cmd`、`.bat`、shell 命令和任意自定义 argv。
- Claude 不使用 `--bare`、`--continue`、`--resume`、bypassPermissions 或危险跳过审批参数。
- Claude stdin 至多 8 MiB、单行 stdout 至多 1 MiB、累计 stdout 至多 8 MiB、累计 stderr 至多 64 KiB、每次 envelope 至多 16 个 calls。
- Claude 启动/认证状态检查每项最多 15 秒；生成采用原有 idle/hard deadline；有效 managed hooks 无法关闭/无法枚举时生成前拒绝。
- 跨 UI DTO 放在 `omicsops-dto` 并做桌面序列化契约测试；JSON 兼容变化不新增表，不迁移旧 profile。
- 停止模型不表示 SSH 已派发计算取消；无法确认的 token 交换/生成派发不得盲目重复。
- 不提交构建产物、凭据、原始日志、实际样本或本地开发配置；不发布安装包、tag、Release 或普通变更版本说明。
- 实施先运行确定性检查，再按 `acceptance/README.md` 显式运行 ignored 验收；通过、失败、未执行分别记录。

## Review Focus

- 账户 A 的待保存登录不能覆盖配置 B，数据库失败不能遗留新 keyring 项或毁掉旧秘密；任务 4/5 固定失败与竞态测试。
- 被截断或摘要化的历史不能把孤立工具结果绑定给同名调用；任务 2 验证 call_id、事件来源、配置哈希及原证据引用。
- CLI 自动更新或配置改变后，缓存的成功预检不能继续授予调用资格；任务 6 每次生成重新核对 executable 指纹和有效策略。
- 旁聊首次请求已被服务接收但回包丢失时，不能因 401/流 fallback 自动再派发；任务 8/9 固定单次派发测试。
- Escape 后才到达的授权成功不能保存凭据或重新打开登录面板；任务 5/11 验证取消终态与异步 UI 竞态。

---

## File Structure and Dependency Order

新文件的类型签名由所属任务定义，后续任务只消费，不复制另一套 DTO 或工具执行器。

| 路径 | 责任 / 所属任务 |
| --- | --- |
| `crates/omicsops-core/src/workspace.rs` | 显式 provider、可选配置和执行哈希（1） |
| `crates/omicsops-dto/src/subscription_models.rs` | 登录/状态/模型发现 DTO（1） |
| `crates/omicsops-protocol/src/model_replay.rs` | 可持久化回放与来源契约（2） |
| `crates/omicsops-agent-core/src/model_replay.rs` | 验证过的事件到模型回放投影（2） |
| `crates/omicsops-agent/src/provider.rs` | provider 请求的可选回放及续接/活动事件（2） |
| `crates/omicsops-adapters/src/responses.rs` | Responses 塑形、SSE、终态、usage（3） |
| `crates/omicsops-adapters/src/codex_auth.rs` | OAuth、秘密 bundle、串行刷新（4） |
| `src-tauri/src/subscription_models.rs` | 登录状态机、keyring/profile 提交与只读状态（5） |
| `crates/omicsops-process/src/managed_child.rs` | 受宿主管理的后台子进程树（6） |
| `crates/omicsops-adapters/src/claude_code.rs` | CLI 预检、stdin/stream-json/envelope（6/7） |
| `crates/omicsops-adapters/src/model_client.rs` | HTTP/Codex/CLI 客户端门面（8） |
| `src-tauri/src/commands.rs`、`model_commands.rs`、`agent_v4.rs` | 桌面客户端工厂与 V4 接线（9） |
| `scripts/import_model_catalog.py`、编译目录及 Go 配置 | 精确路由与有来源的能力条目（10） |
| `src/features/settings/SubscriptionModelForm.tsx` | 专用订阅表单/取消生命周期（11） |
| `src-tauri/src/agent_v4/subscription_live_acceptance_tests.rs` | 独立一次性 opt-in 验收（12） |

新增 adapters/process 模块对应新的 OAuth、CLI 和协议职责，不搬迁无关旧代码。
本文的 `ProviderRequest` 指 agent 契约，wire 请求显式写作 `llm::ProviderRequest`；
`RequestBudget`、`RequestBudgetMetrics` 与 `ModelProbeResult` 复用现有 llm 类型。
各测试步骤中的断言变量由该步骤列出的 fixture/调用生成，不是新增生产接口。
任务顺序：1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 → 12。
CLI 预检无法满足设计时，保留明确错误与已完成增量，修订方案；不能将该路径
替换为自由执行的 ACP 或宣布整项完成。

## Task 1: provider、配置身份与 UI 契约兼容

**Files:** Modify/Test `crates/omicsops-core/src/workspace.rs`、`crates/omicsops-dto/src/lib.rs`、`src-tauri/src/dto_contract_tests.rs`、`crates/omicsops-store/src/model_profiles.rs`、`src-tauri/src/model_commands.rs`、`src/types.ts`；Create `crates/omicsops-dto/src/subscription_models.rs`。

**Interfaces:**
- Produces: `ModelProviderKind::{OpenAiResponses, OpenAiCodex, ClaudeCode}`；`ModelProfile.cli_executable: Option<String>` 与 `subscription_account_ref: Option<Uuid>`，缺省不序列化。
- Produces: `validate_subscription_profile_fields(profile: &ModelProfile) -> Result<(), String>`（core）；普通保存不能设置账户引用，Codex 无绑定只能处于未认证配置状态，生成时拒绝。
- Produces DTO: `BeginCodexLoginResponse { login_id: Uuid, verification_uri: String, user_code: String, expires_at: DateTime<Utc> }`；`CodexLoginState::{Pending, Authorized, Saved, Cancelled, Expired, Failed}`；`CodexLoginStateResponse { login_id, state, expires_at, error_code: Option<String> }`。
- Produces DTO: `FinishCodexLoginRequest { login_id: Uuid, profile: SaveModelProfileRequest }`；`SubscriptionModelStatus { provider: String, authenticated: bool, masked_account_label: Option<String>, cli_version: Option<String>, error_code: Option<String> }`。
- Produces DTO: `ModelDiscoverySource::{Provider, ConfiguredOnly}`；`ModelDiscoveryResult { models: Vec<String>, source: ModelDiscoverySource, can_refresh: bool }`。保留原有 Vec 模型查询命令，任务 9 新增有来源的查询命令。
- `SaveModelProfileRequest.cli_executable` 用 `Option<Option<String>>` 延续 omitted/null 语义；三个旧 provider 的未知新增配置拒绝，不让 HTTP 保存误写订阅字段。

- [x] 写失败测试 `subscription_profiles_preserve_legacy_hash_and_json`：旧 fixture 哈希逐字相同；新字段缺省不会出现；CLI 路径/Codex 账户引用改变新哈希，标签不改变。 关键断言：`assert_eq!(legacy_hash_after, legacy_hash_before); assert_ne!(cli_path_changed_hash, cli_hash); assert_eq!(label_changed_hash, legacy_hash_before);`
- [x] 写失败测试 `subscription_dto_omitted_null_and_binding_are_explicit`：CLI omitted 保留/null 清空；普通请求携带账户引用不能绕过后端绑定；DTO 只允许登录所需 user_code，不含 token/authorization_code/verifier/原账户 ID。 关键断言：`assert_eq!(omitted.cli_executable, old.cli_executable); assert_eq!(cleared.cli_executable, None); assert!(!dto_json.contains("refresh_token"));`
- [x] 写失败测试 `subscription_profile_delete_respects_frozen_references`：活动 run/queue/side chat/review 引用仍阻止删除；Claude 无 credential_reference，删除不调用 CLI logout。 关键断言：`assert!(active_delete.is_err()); assert_eq!(cli_logout_calls, 0);`
- [x] 运行 `cargo test -p omicsops-core subscription_`、`cargo test -p omicsops-dto subscription_` 和 `cargo test -p omicsops-store subscription_`，确认失败来自新增要求；先记录原有基线异常。
- [x] 实现上述类型、合法矩阵和哈希；为现有 exhaustive matches 增加显式暂不可派发分支。编译所需初始化器机械补 `None`，不要在本任务提前启用协议或登录。
- [x] 重跑局部命令及 `cargo test -p omicsops-desktop --lib subscription_`，应全部通过；检查旧 DTO/profile tests。
- [x] 审查并只暂存本任务文件，提交 `feat: define subscription model profiles and contracts`。

## Task 2: 结构化历史和可验证的续接来源

**Files:** Create `crates/omicsops-protocol/src/model_replay.rs`、`crates/omicsops-agent-core/src/model_replay.rs`；Modify/Test 对应 `src/lib.rs`、`crates/omicsops-agent/src/provider.rs`、`crates/omicsops-agent/Cargo.toml`、`crates/omicsops-agent-core/src/context_views.rs`、`crates/omicsops-store/src/lib.rs`（事件回归）；Test `crates/omicsops-agent/tests/provider_replay.rs`。

**Interfaces:**
- Produces protocol: `ModelReplayItemV4::{AssistantText { text: String }, ToolCall { call: ToolCallV4 }, ToolResult { call_id: String, output: String }, ResponsesReasoning { id: String, encrypted_content: String }}`。
- Produces protocol: `ModelReplayBindingV4 { model_profile_id: Uuid, configuration_hash: String }`；`ModelProviderContinuationV4 { items: Vec<ModelReplayItemV4> }`；`ModelReplayRecordedV4 { logical_request_id: Uuid, attempt_id: Uuid, binding: ModelReplayBindingV4, continuation: ModelProviderContinuationV4 }`；`AgentEventKindV4::ModelReplayRecorded { replay: ModelReplayRecordedV4 }`。
- Produces core: `project_model_replay(project_id: Uuid, conversation_id: Uuid, binding: &ModelReplayBindingV4, events: &[AgentEventV4]) -> Result<Vec<ModelReplayItemV4>, String>`；验证每个来源 run 的完整事件链哈希和顺序、目标 project/conversation、成功模型 attempt 与匹配宿主结果，复用现有有界 view 与原证据引用。调用方按已验证的会话运行顺序提供事件，不跨会话搜索同名调用。
- `ModelRequestV4.replay` 默认空并省略序列化；`ModelTurnV4.provider_continuation` 默认 None 并省略。`ProviderRequest.replay` 消费同一 protocol 类型；agent 新增 protocol 依赖，protocol 不反向依赖 agent。
- `ProviderStreamEvent` 新增 `Continuation { continuation: ModelProviderContinuationV4 }` 与 `ContentProgress { bytes: u32 }`；续接仅在合法成功终态发出，进展只代表非空 assistant 内容。
- 每个续接最多 16 个 items、累计 1 MiB，单个 encrypted_content 至多 256 KiB；正文 reasoning/未知字段不持久化。

- [x] 写 `model_replay_rejects_orphans_and_wrong_bindings`：相同 call_id 的跨 profile/attempt/project 数据不能关联；无 ToolRequested 的结果不能回放；已拒绝调用只采用宿主记录的拒绝结果，不伪造执行。 关键断言：`assert!(cross_project_replay.is_err()); assert!(orphan_replay.is_err()); assert_eq!(denied_execution_calls, 0);`
- [x] 写 `model_replay_survives_restart_without_reexecuting_tools`：旧事件链哈希保持；新事件重载后按顺序回放；工具执行器调用计数不增加；结构截断不能留下孤立 result。 关键断言：`assert_eq!(reloaded_replay, saved_replay); assert_eq!(executor_calls_after, executor_calls_before);`
- [x] 写 `model_replay_drops_plaintext_reasoning_and_measures_ciphertext`：拒绝未知/过大 item；模型通用 context view 不含 opaque continuation 正文；请求预算仍计入密文。 关键断言：`assert!(!context.contains("PRIVATE_REASONING_SENTINEL")); assert!(with_ciphertext.serialized_request_bytes > without_ciphertext.serialized_request_bytes);`
- [x] 运行 `cargo test -p omicsops-agent-core model_replay`、`cargo test -p omicsops-agent --test provider_replay`、`cargo test -p omicsops-protocol model_replay`，观察预期失败。
- [x] 实现类型/投影；仅给拥有匹配已冻结 binding 的新传输回放，旧 HTTP 请求不新增 wire 字段。机械更新所有 `ModelRequestV4`/`ModelTurnV4` 初始化器，空值保持旧行为。
- [x] 重跑以上测试及 `cargo test -p omicsops-store model_replay`，应通过；核对旧事件签名与未知 enum 的明确失败。
- [x] 提交 `feat: preserve validated model tool replay and continuation`。

## Task 3: Responses 塑形、SSE 与 HTTP 执行

**Files:** Create `crates/omicsops-adapters/src/responses.rs`、`crates/omicsops-adapters/tests/responses_contracts.rs`；Modify/Test `crates/omicsops-adapters/src/lib.rs`、`llm.rs`；必要时为同目录协议 fixture 新增文件。

**Interfaces:**
- Produces: `ResponsesEndpointKind::{CodexSubscription, OpenCodeGo}`；`responses_endpoint(kind, base: &Url) -> AdapterResult<Url>`，分别只产生审核的 `/codex/responses` 和 `/responses`。
- Produces: `build_responses_request(kind, base: &Url, model: &str, request: &ProviderRequest, budget: Option<RequestBudget>, effort: Option<&str>) -> AdapterResult<llm::ProviderRequest>`。
- Produces: `ResponsesStreamDecoder::for_request(request: &ProviderRequest)`、`push(&mut self, chunk: &[u8]) -> AdapterResult<Vec<ProviderStreamEvent>>`、`finish(&mut self) -> AdapterResult<Vec<ProviderStreamEvent>>`。
- Produces: `ResponsesAuthorization::bearer(token: String, account_id: Option<String>) -> Self`（私有秘密字段，无泄密 Debug）；Go 的 account_id 必须 None，Codex 必须来自认证快照。
- Produces: `ResponsesHttpClient::new(kind: ResponsesEndpointKind, base: Url, model: String, budget: Option<RequestBudget>, session_id: Uuid) -> AdapterResult<Self>`；`async stream_once(&self, request: ProviderRequest, authorization: ResponsesAuthorization, on_event: impl FnMut(ProviderStreamEvent) + Send) -> AdapterResult<()>` 只有一个生成派发。此任务不依赖尚未实现的 OAuth 类型；普通 attempt 编排由任务 8 门面接入现有策略。
- 采用既有 tool alias 规则并检测冲突；完成后按原工具 ID 回传。续接只含审核的 opaque reasoning 与必要输出项，不保留原始响应。

- [x] 写 `responses_shapes_native_tool_history_and_exact_endpoints`：instructions、input、function schema、call_id/results 顺序；Codex store=false/stream 参数受限；Go 自身会话头；伪造域名/端口/query/redirect 拒绝。 关键断言：`assert_eq!(codex_body["store"], false); assert_eq!(wire_call_id, host_call_id); assert!(foreign_endpoint.is_err());`
- [x] 写 `responses_requires_completed_terminal_and_complete_arguments`：逐字节 UTF-8、交错函数 index、重复 call_id、非法/截断参数、incomplete/error、无合法 completed 的 EOF/[DONE] 均不得成功。 关键断言：`assert!(truncated.finish().is_err()); assert_eq!(completed_tool_calls.len(), 1); assert_eq!(duplicate_execution_calls, 0);`
- [x] 写 `responses_preserves_usage_and_attempt_boundaries`：真实 usage 不重复相加；必要续接回放后预算增大；中断后的片段不拼入下次 attempt；once 不使用非流 fallback。 关键断言：`assert_eq!(merged.output_tokens, Some(17)); assert_eq!(once_generation_count, 1);`
- [x] 运行 `cargo test -p omicsops-adapters --test responses_contracts`，确认预期失败。
- [x] 实现请求/解码/固定端点 HTTP client；对本地 mock server 显式注入测试 transport，不能开放生产固定端点的任意 URL 例外。新传输不发送图片，不注册供应商内置工具，不重试其它协议。
- [x] 重跑契约测试和 `cargo test -p omicsops-adapters --test provider_usage --test model_provider_contracts`，应全部通过。
- [x] 提交 `feat: add bounded Responses model transport`。

## Task 4: Codex device auth 与并发刷新

**Files:** Create `crates/omicsops-adapters/src/codex_auth.rs`、`crates/omicsops-adapters/tests/codex_auth_contracts.rs`；Modify `crates/omicsops-adapters/src/lib.rs`；借用参考源码时保留许可证说明。

**Interfaces:**
- Produces: 秘密类型 `CodexCredentialBundle { access_token: String, refresh_token: String, expires_at_ms: i64, account_id: String, account_ref: Uuid }`（只在 vault 内序列化，无泄密 Debug）；`CodexAccessSnapshot::into_responses_authorization(self) -> ResponsesAuthorization`，其它字段私有。
- Produces: `CodexDeviceChallenge { device_auth_id: String, user_code: String, verification_uri: Url, interval: Duration, expires_at_ms: i64 }`；`CodexDevicePoll::{Pending { next_poll_after: Duration }, Authorized { bundle: CodexCredentialBundle }}`，均不是 UI DTO。
- Produces: `CodexAuthClock::now_ms(&self) -> i64`；异步 trait `CodexAuthTransport` 可注入 HTTP 与假时钟：`async begin_device(&self) -> AdapterResult<CodexDeviceChallenge>`、`async poll_device(&self, challenge: &CodexDeviceChallenge, cancelled: Arc<AtomicBool>) -> AdapterResult<CodexDevicePoll>`、`async refresh(&self, bundle: &CodexCredentialBundle) -> AdapterResult<CodexCredentialBundle>`。poll 在任何授权码交换前检查取消标记。
- Produces: `async CodexCredentialCoordinator::access_snapshot(&self, reference: &str, account_ref: Uuid) -> AdapterResult<CodexAccessSnapshot>`；`async refresh_after_unauthorized(&self, reference: &str, account_ref: Uuid, rejected: &CodexAccessSnapshot) -> AdapterResult<CodexAccessSnapshot>`。每 reference/account 串行锁内重读 vault，提前五分钟刷新；401 对照已拒绝快照，避免并发重复刷新。
- Produces: `CodexCredentialCoordinator::new(vault: Arc<dyn CredentialVault>, transport: Arc<dyn CodexAuthTransport>, clock: Arc<dyn CodexAuthClock>) -> Self`；clock 只读时间，等待由 Tokio 负责，测试用暂停时钟推进。
- 生产构造器固定已核对 auth endpoint 与 client 配置、拒绝重定向；测试构造器只用于 mock transport。网络错误返回安全 code，不输出响应正文。

- [x] 写 `codex_device_poll_obeys_interval_expiry_and_cancel`：pending/slow-down、15 分钟上限、乱序响应、取消后无 token exchange；授权码和 verifier 不进状态 DTO。 关键断言：`assert!(expires_at_ms - started_at_ms <= 15 * 60 * 1000); assert_eq!(exchanges_after_cancel, 0);`
- [x] 写 `codex_refresh_is_serialized_and_keeps_account_binding`：并发两个访问只刷新一次；第二次先重读新 token；五分钟阈值、keyring 写失败、账号改变和 invalid_grant 明确失败。 关键断言：`assert_eq!(concurrent_refresh_count, 1); assert_eq!(refreshed.account_ref, original.account_ref); assert!(changed_account.is_err());`
- [x] 写 `codex_auth_unknown_exchange_is_not_replayed`：token exchange/旋转 refresh 网络结果不确定后，重读 vault，未发现已写入新秘密则标记重新登录，后续等待者不重复交换；明确生成 401 最多一次刷新；敏感 fixture sentinel 不出现在 error/Debug/序列化 UI 字段。 关键断言：`assert_eq!(uncertain_exchange_count, 1); assert!(!safe_error.contains("SECRET_TOKEN_SENTINEL"));`
- [x] 运行 `cargo test -p omicsops-adapters --test codex_auth_contracts`，确认预期失败。
- [x] 实现上述接口与秘密边界；锁共享到所有模型门面实例，不因每次工厂创建而重建独立锁。不要读取实际用户 keyring 或 CLI auth.json 作为测试前提。
- [x] 重跑 auth 测试、任务 3 的认证/端点测试，应通过。
- [x] 提交 `feat: manage Codex subscription device auth and refresh`。

## Task 5: 原生登录状态机与 profile 保存事务边界

**Files:** Create `src-tauri/src/subscription_models.rs`；Modify/Test `src-tauri/src/commands.rs`、`lib.rs`、`model_commands.rs`、`model_deletion.rs`、`dto_contract_tests.rs`；为现有 AppState 初始化器机械补共享 coordinator。

**Interfaces:**
- Consumes: 任务 1 DTO 和任务 4 coordinator、既有 CredentialVault/Store/profile 删除保护。
- Produces: `SubscriptionLoginManager`（AppState 中 Arc），最多 8 个 pending login；状态 terminal 不可回退，过期清理内存秘密，应用退出停止轮询。
- Produces Tauri commands: `subscription_begin_codex_login(profile_id: Option<Uuid>) -> BeginCodexLoginResponse`；`subscription_poll_codex_login(login_id: Uuid) -> CodexLoginStateResponse`；`subscription_cancel_codex_login(login_id: Uuid) -> CodexLoginStateResponse`。
- Produces async commands: `subscription_finish_codex_login(request: FinishCodexLoginRequest) -> ModelProfile`；`subscription_model_status(profile_id: Uuid) -> SubscriptionModelStatus`；`subscription_disconnect_codex(profile_id: Uuid) -> ModelProfile`。State 参数及 Result 包装遵循现有 tauri command 约定，helper 可注入 vault/store 测试。
- 后台 poll 消费服务间隔；UI poll 只查询状态，不触发重复 HTTP。保存先 keyring 后 Store，编辑失败恢复旧秘密；账户变化建立另一 profile，不借 finish 改写活动配置。
- disconnect 复用活动引用保护，清除此 profile 的 vault bundle/账户引用，保留未认证配置；Store 失败恢复旧秘密，恢复失败明确报告。操作不调用官方 Codex/Claude CLI logout。

- [x] 写 `subscription_login_cancel_wins_over_late_authorization`：取消终态、过期、重复完成、应用 manager drop；第九个 pending 明确拒绝；释放临时凭据。 关键断言：`assert_eq!(final_state, CodexLoginState::Cancelled); assert_eq!(vault_writes_after_cancel, 0); assert!(ninth_pending_login.is_err());`
- [x] 写 `subscription_login_cannot_cross_profile_or_replace_active_account`：login A/finish B 拒绝；旧绑定可重新登录同账号；账号改变产生新的 profile_id/account_ref，旧活动 run 不被重绑。 关键断言：`assert!(finish_other_profile.is_err()); assert_ne!(changed_account_profile.id, old_profile.id); assert_eq!(active_run_binding_after, active_run_binding_before);`
- [x] 写 `subscription_save_compensates_keyring_database_failures`：新 row 失败清理新秘密；旧 row 失败恢复旧秘密；恢复失败如实报告；不能产生 credential 到 SQLite 的 fixture 命中。 关键断言：`assert_eq!(vault_after_failed_edit, vault_before_edit); assert_eq!(new_secret_count_after_failed_insert, 0);`
- [x] 写 `subscription_login_disconnect_is_scoped_and_guarded`：活动引用拒绝退出；成功只清除该 profile 绑定及秘密；其它 profile/CLI 登录不变；Store 失败恢复原 bundle。 关键断言：`assert!(active_disconnect.is_err()); assert_eq!(other_profile_secret_after, other_profile_secret_before); assert_eq!(cli_logout_calls, 0);`
- [x] 运行 `cargo test -p omicsops-desktop --lib subscription_login`、`cargo test -p omicsops-desktop --lib subscription_save`，观察失败。
- [x] 实现状态机、注册命令和 profile 保存 helper；活动引用保护同时用于退出/删除，CLI profile 删除不退出独立 CLI。普通 key 表单无法往 Codex bundle 写入裸 API key。
- [x] 重跑上述命令及 `cargo test -p omicsops-desktop --lib dto_contract_tests`，应通过。
- [x] 提交 `feat: expose isolated subscription login lifecycle`。

## Task 6: Claude 执行资格与 Windows 子进程回收

**Files:** Create `crates/omicsops-process/src/managed_child.rs`、`crates/omicsops-process/tests/managed_child.rs`；Modify `crates/omicsops-process/src/lib.rs`、`Cargo.toml`；Create/Modify `crates/omicsops-adapters/src/claude_code.rs`、`tests/claude_code_contracts.rs`、`src/lib.rs`、`Cargo.toml`；依赖变化包含 Cargo.lock。

**Interfaces:**
- Produces process: `BackgroundLaunchSpec { program: PathBuf, args: Vec<OsString>, cwd: PathBuf, env: BTreeMap<OsString, OsString> }`；`ProcessInput = Box<dyn AsyncWrite + Unpin + Send>`、`ProcessOutput = Box<dyn AsyncRead + Unpin + Send>`。
- Produces process: `ManagedBackgroundChild::spawn(spec: BackgroundLaunchSpec) -> io::Result<Self>`；`take_stdin(&mut self) -> Option<ProcessInput>`、`take_stdout(&mut self) -> Option<ProcessOutput>`、`take_stderr(&mut self) -> Option<ProcessOutput>`；`async wait(&mut self) -> io::Result<ExitStatus>`、`async terminate_and_wait(&mut self) -> io::Result<ExitStatus>`。Drop 同步关闭 job 以终止树；正常/显式取消还等待回收，异步调用取消后需有独立 reaper 完成等待。
- Windows 使用 Job Object 的 kill-on-close，子进程进入 job 后才开始执行，防止 assign/spawn 竞态；需要的 Win32 binding 使用锁文件已有 windows-sys 0.61.2 的 target dependency。原 background_command 行为不变。
- 非 Windows 对新增 Claude/managed-child 路径返回明确 Unsupported；保留旧进程路径。此计划只验收 Windows 原生 Claude，不扩大 macOS 支持保证。
- Produces adapters: `ClaudePreflight { executable_fingerprint: String, version: String, auth_kind: ClaudeAuthKind, account_fingerprint: Option<String>, restrictions_verified: bool }`；`ClaudeAuthKind::{Subscription, Api, Unknown}`。
- Produces: async trait `ClaudeProcessRunner`：`async inspect(&self, executable: &Path) -> AdapterResult<ClaudePreflight>`（只读进程/有效策略检查，每项限 15 秒）；`async spawn(&self, spec: BackgroundLaunchSpec) -> AdapterResult<ManagedBackgroundChild>`。测试 runner 记录生成次数，返回受控 helper 或预检 fixture；生产 runner 枚举失败不能伪造 restrictions_verified=true。
- Produces: `async inspect_claude_preflight(executable: &Path, runner: &dyn ClaudeProcessRunner) -> AdapterResult<ClaudePreflight>`；策略来源枚举失败、managed hooks 生效、未知 CLI 格式拒绝生成。
- 必须在任何生成 SessionStart 之前完成有效策略预检；仅检查启动事件太晚。缓存不能跨 executable 内容/策略变化，生成时重检；不修改组织策略或认证存储。

- [x] 写 `claude_preflight_rejects_unverifiable_policy_and_non_subscription_auth`：.cmd/.bat/命令字符串、未知版本输出、Api/Unknown/未登录、managed hooks 和来源枚举失败；全都断言 generator spawn 次数为 0。 关键断言：`assert!(unknown_policy_preflight.is_err()); assert_eq!(generator_spawn_count, 0);`
- [x] 写 `claude_preflight_invalidates_changed_binary_and_policy`：相同路径替换文件、CLI 更新、只读策略变化后旧检查失效；fingerprint 不包含 raw account/token。 关键断言：`assert_ne!(new_fingerprint, old_fingerprint); assert!(changed_policy_generation.is_err());`
- [x] 写 `managed_child_terminates_entire_tree_without_console`：使用 Rust 测试可执行文件的受控 helper 模式生成父/孙进程，取消后全退出；不依赖 Python/Claude/SSH；Windows 无可见 console；另测父进程先退出、drop、重复终止和 PID 重用防护。 关键断言：`assert_eq!(remaining_owned_processes, 0); assert_eq!(visible_console_windows, 0);`
- [x] 运行 `cargo test -p omicsops-process --test managed_child` 和 `cargo test -p omicsops-adapters --test claude_code_contracts preflight`，观察预期失败。
- [x] 实现进程生命周期与 fail-closed preflight。有效 managed 来源无法通过已核对的只读方式确认时返回可说明错误，不能把“文件不存在”当作不存在服务端/注册表策略；不要为了通过测试删掉此条件。
- [x] 重跑上述命令与现有 process tests；记录 Windows 行为，检查无系统全局 env/安装/登录变化。
- [x] 提交 `feat: constrain Claude subscription execution and child lifecycle`。

## Task 7: Claude stdin、stream-json 与宿主 envelope

**Files:** Modify/Test `crates/omicsops-adapters/src/claude_code.rs`、`tests/claude_code_contracts.rs`。

**Interfaces:**
- Consumes: task 6 runner/preflight/process 与 task 2 ProviderRequest；不向 CLI 导出 MCP/tool executor。
- Produces: `ClaudeCodeClient::new(profile_id: Uuid, executable: PathBuf, model: String, runner: Arc<dyn ClaudeProcessRunner>) -> AdapterResult<Self>`；`build_invocation(&self, request: &ProviderRequest) -> AdapterResult<ClaudeInvocation>`（纯函数，无进程/网络）。
- Produces: `ClaudeInvocation { launch: BackgroundLaunchSpec, stdin: Vec<u8> }`；动态 prompt/schema 只在 stdin，工作目录为受控临时目录，不取科研项目 cwd。
- Produces: `parse_claude_envelope(value: &Value, tools: &[ProviderToolSpec]) -> AdapterResult<Vec<ProviderStreamEvent>>`；`async ClaudeCodeClient::stream_once(&self, request: ProviderRequest, on_event: impl FnMut(ProviderStreamEvent) + Send) -> AdapterResult<()>`。
- 固定参数包含 `-p`、`--restricted`、`--safe-mode`、`--tools` 的真实空 argv、`--disallowedTools mcp__*`、strict 空 MCP、空 setting sources、禁用 slash/no-chrome/no-session-persistence、`--max-turns 1`、`--output-format stream-json --verbose --include-partial-messages`；inline settings 关闭 hooks/connectors。其它限制与预检一致，不能只保留名字而漏掉值。
- env 从白名单构造，去除 API/token/helper/替代端点、cloud 与插件/debug 注入；固定系统提示说明输入 ProviderRequest 的 system 权威与历史为数据。动态上下文/工具 schema 只走 stdin。
- envelope 为 spec 的 tagged final/tool_calls；不得执行 CLI 原生工具；单次调用没有静默 repair/fallback 重派发，普通 V4 的显式 attempt 策略由宿主控制。

- [x] 写 `claude_invocation_is_tool_free_subscription_only_and_stdin_only`：空 argv 真的为空；JSON schema 不进 argv；引用/换行/中文/反引号/`$()` 原样进入 stdin；不会成为 shell 代码；恶意父 env 被移除。 关键断言：`assert_eq!(tools_argv_value, ""); assert!(invocation.stdin.windows(3).any(|w| w == b"$()")); assert!(!argv_has_dynamic_prompt); assert!(!invocation.launch.env.contains_key(OsStr::new("ANTHROPIC_API_KEY")));`
- [x] 写 `claude_stream_rejects_invalid_final_and_native_execution`：合法 final/calls 转换；未知/重复工具、extra fields、混合/空 calls、is_error、非零退出、缺 result、工具执行事件拒绝；原始协议 JSON 不出现在最终正文。 关键断言：`assert!(native_tool_event_result.is_err()); assert!(extra_field_envelope.is_err()); assert_eq!(host_execution_count, 0);`
- [x] 写 `claude_stream_bounds_io_and_counts_only_content_progress`：分别越过 8 MiB/1 MiB/8 MiB/64 KiB/16 calls 边界；system/usage/空 fragment 不产生 ContentProgress，真实非空内容才产生；预算测量不 spawn。 关键断言：`assert!(stdin_8_mib_plus_one.is_err()); assert!(stdout_line_1_mib_plus_one.is_err()); assert!(stderr_64_kib_plus_one.is_err()); assert_eq!(keepalive_content_events, 0);`
- [x] 写 `claude_stop_and_identity_change_cannot_produce_success`：await 被取消仍回收进程树；auth 检查超 15 秒拒绝；账户指纹变化失败；无稳定身份时公开限定恢复保证；usage 缺项保持 unknown。 关键断言：`assert_eq!(processes_after_cancel, 0); assert!(changed_identity_result.is_err()); assert_eq!(missing_usage.output_tokens, None);`
- [x] 运行 `cargo test -p omicsops-adapters --test claude_code_contracts`，观察失败。
- [x] 实现调用/NDJSON parser/envelope/bounds；最终校验后才发工具调用/正文，记录真实 usage，绝不写完整 stdout/stderr 或凭据。CLI 输出预留只用于预算，不宣称服务端 max_output_tokens 已设置。
- [x] 重跑完整 CLI contracts 与 process tree tests，应通过；审查禁止参数/环境矩阵。
- [x] 提交 `feat: adapt Claude subscription output to host model events`。

## Task 8: 可替换 ModelClient 门面与单次派发

**Files:** Create `crates/omicsops-adapters/src/model_client.rs`、`tests/model_client_contracts.rs`；Modify `src/lib.rs`、`Cargo.toml`（消费共享 DTO）；最小修改 `llm.rs` 必要接口可见性，不搬迁已有 HTTP 逻辑。

**Interfaces:**
- Produces: `ModelClient::{Http(UnifiedModelClient), Codex(CodexModelClient), Responses(OpenCodeGoModelClient), Claude(ClaudeCodeClient)}`；`ModelClient::from_profile(profile: &ModelProfile, services: Arc<ModelClientServices>) -> AdapterResult<Self>`。
- Produces: `ModelClientServices { vault: Arc<dyn CredentialVault>, codex: Arc<CodexCredentialCoordinator>, claude: Arc<dyn ClaudeProcessRunner> }`；Codex/Go wrapper 各自持有 ResponsesHttpClient、该 profile 的凭据引用/绑定及共享 services，私有字段不泄密 Debug。两 wrapper 在发送边界取得 ResponsesAuthorization，不把 OAuth 接进任务 3 类型。
- 与现有调用同名：`with_session_id(self, Uuid) -> Self`、`with_request_budget(self, RequestBudget) -> Self`、`request_budget(&self) -> Option<RequestBudget>`、`with_reasoning_effort(self, Option<String>) -> AdapterResult<Self>`、`with_fast_mode(self, Option<bool>) -> Self`、`has_image_budget(&self) -> bool`、`validate_request(&self, &ProviderRequest) -> AdapterResult<()>`、`measure_model_request(&self, &ProviderRequest) -> AdapterResult<RequestBudgetMetrics>`。
- Async: `stream_with_provider(&self, request: ProviderRequest, on_event: impl FnMut(ProviderStreamEvent) + Send) -> AdapterResult<()>`、`stream_with_provider_v4` 与 `stream_with_provider_once` 使用同一签名；`probe(&self) -> AdapterResult<ModelProbeResult>`、`list_models(&self) -> AdapterResult<Vec<String>>`、`discover_models(&self) -> AdapterResult<ModelDiscoveryResult>`。
- once 不允许生成 401 刷新后重发或非流 fallback；在派发前可以刷新已过期凭据，派发后仅返回明确错误。CLI 不伪造服务可用模型列表，返回 ConfiguredOnly/can_refresh=false。

- [x] 写 `model_client_routes_without_credential_or_protocol_inference`：三旧 provider 仍构造旧 HTTP client；新字段只选择对应 backend；图像和未支持 Fast/effort 拒绝；validate/measure 无 HTTP/process 派发。 关键断言：`assert_eq!(validation_http_calls, 0); assert_eq!(measurement_process_calls, 0); assert!(unsupported_image.is_err());`
- [x] 写 `model_client_once_does_not_retry_accepted_request`：mock 服务已接收后连接关闭或 401，生成计数=1；过期 credential 的前置刷新不算重复生成；无 paid fallback。 关键断言：`assert_eq!(accepted_then_disconnected_generation_count, 1); assert_eq!(generation_401_count, 1); assert_eq!(paid_fallback_count, 0);`
- [x] 写 `model_client_probe_and_discovery_preserve_source_and_unknown_usage`：探测成功不代表工具回合；CLI 发现来源为 ConfiguredOnly；服务错误不伪造成列表；旧 Vec 包装兼容。 关键断言：`assert_eq!(cli_discovery.source, ModelDiscoverySource::ConfiguredOnly); assert!(!cli_discovery.can_refresh);`
- [x] 运行 `cargo test -p omicsops-adapters --test model_client_contracts`，观察失败。
- [x] 实现门面和 scoped services；默认预算与真实 payload measurement 一致，token/原始CLI output 不进入 Debug。新 backend 每个 callback 保留 session/attempt 边界。
- [x] 重跑门面测试与 adapter 全部测试，应通过。
- [x] 提交 `feat: unify subscription and API model client routing`。

## Task 9: 桌面 V4、辅助调用、恢复与审批接线

**Files:** Modify/Test `src-tauri/src/commands.rs`、`model_commands.rs`、`agent_v4.rs`、`side_chat.rs`、`session_reviews.rs`、`follow_up_questions.rs`、`context_compaction.rs`、`agent_commands.rs`、`lib.rs`；Create `src-tauri/src/agent_v4/subscription_contract_tests.rs`；Modify/Test `crates/omicsops-agent-core/src/lib.rs` 续接持久化入口。

**Interfaces:**
- 将 `unified_model_client_for_profile` 改名为 `model_client_for_profile(state: &AppState, profile: &ModelProfile) -> Result<ModelClient, String>`，测试 helper 使用同一门面注入 services；更新全部调用方。
- `DesktopModelPortV4.client: ModelClient`；prepare_request 传任务 2 的 replay；stream 收集 Continuation 回 ModelTurnV4，映射 ContentProgress 到 Responding 活动。
- core 仅在成功完整 model turn 且工具建议合法后，将续接与当前 logical_request_id/attempt_id/binding 绑定并持久化；取消/失败 attempt 不产生可回放副作用。后续 request 从通过事件链哈希校验的记录投影，不能解析旧 context 文本来制造原生 call_id；原 context 中同一工具回合不再重复伪装为普通消息输入。
- 新增 Tauri `list_model_profile_model_discovery(profile_id: Uuid) -> ModelDiscoveryResult`，保留原 `list_model_profile_models` 的 Vec 返回值。

- [x] 写 `subscription_v4_uses_selected_backend_for_every_model_role`：ordinary/plan/summary/side-chat/delegated/reviewer/compaction/clarification 采用冻结选择，独立 profile 不共享 Claude invocation 或 Codex 账户；无默认模型 fallback。 关键断言：`assert_eq!(observed_profile_ids, expected_frozen_profile_ids); assert_eq!(default_fallback_calls, 0);`
- [x] 写 `subscription_v4_replay_is_durable_only_after_validated_turn`：failed/cancelled/越权调用不写 continuation；重启恢复不重复 ToolFinished；配置/账户引用/CLI 路径变化拒绝，标签/token 刷新允许。 关键断言：`assert_eq!(failed_attempt_continuation_count, 0); assert_eq!(recovered_tool_execution_count, 0);`
- [x] 写 `subscription_v4_local_delete_requires_host_approval`：三 backend 的 read nonce 通过宿主；删除临时文件建议进入现有 approval，拒绝后文件仍存在、执行器计数=0；计划模式不获得写权限。 关键断言：`assert!(temporary_file.exists()); assert_eq!(denied_delete_execution_count, 0); assert_eq!(nonce_read_execution_count, 1);`
- [x] 写 `subscription_v4_stop_does_not_cancel_remote_job_or_extend_on_keepalive`：CLI/HTTP 请求停止且冻结远端 job 状态不变；仅真实非空内容推进 idle，system/usage/retry 不无限延长；旁聊生成计数=1。 关键断言：`assert_eq!(remote_job_after_stop, remote_job_before_stop); assert_eq!(side_chat_generation_count, 1); assert_eq!(keepalive_deadline_extensions, 0);`
- [x] 运行 `cargo test -p omicsops-desktop --lib subscription_contract_tests` 和 `cargo test -p omicsops-agent-core model_replay`，确认预期失败。
- [x] 实现工厂/辅助调用/请求历史/event 接线；复用工具 registry、删除判定、plan approval 与 run/job 生命周期，不新增 CLI 直连项目能力。
- [x] 重跑上述命令，以及 desktop 的 `model_`、`side_chat`、`session_review`、`local_deletion` 过滤回归，应通过；逐项核对各模型调用的 retry/deadline。
- [x] 提交 `feat: integrate subscription models with host Agent V4`。

## Task 10: Go 精确 Responses 路由与目录导入

**Files:** Modify/Test `scripts/import_model_catalog.py`、`scripts/test_import_model_catalog.py`、`src-tauri/src/model_commands.rs`、`model_catalog_shared.rs`、`model_catalog.json`、`crates/omicsops-core/src/workspace.rs`（请求选项校验）；保留已有目录源哈希。

**Interfaces:**
- 导入脚本的 Go protocol map 新增官方明确的 Responses 完整 ID；保存校验与前端任务 11 消费同一审核列表，禁止家族匹配。
- `exact_model_capabilities` 继续按显式 provider、HTTPS host/port、完整 ID 和 Go path 匹配；Codex 无可信独立条目时返回 None，不复制 OpenAI 普通 API 行。
- 已存在的 Go Chat/Messages 配置不改写；旧协议保存 Responses-only ID 拒绝，显式新 provider 才允许。reasoning/Fast 仅采用已审核 wire 字段。

- [x] 写 `test_go_responses_uses_exact_reviewed_ids_and_source_limits`：源 fixture 含官方完整 ID 与同前缀 sibling，前者生成对应协议行，后者不获路由；source_sha256 为实际 raw source 哈希，重跑确定性相同。 关键断言：`self.assertEqual(actual_source_hash, hashlib.sha256(raw_source).hexdigest()); self.assertNotIn(unreviewed_sibling_id, responses_ids)`
- [x] 写 `subscription_catalog_never_inherits_api_or_cli_alias_capabilities`：Codex 同名 ID/未知 host/额外端口/CLI alias 无目录行时 None；新 Go 行精确匹配，不更新已有 profile 的能力 snapshot/hash。 关键断言：`assert_eq!(codex_unmatched_capabilities, None); assert_eq!(unknown_gateway_capabilities, None); assert_eq!(saved_snapshot_after, saved_snapshot_before);`
- [x] 运行 `python -m unittest discover -s scripts -p test_import_model_catalog.py` 与 `cargo test -p omicsops-desktop --lib subscription_catalog`，观察失败。
- [x] 按实现时再次核对的官方路由和 models.dev 源生成目录；不要手编能力数值或改旧条目的来源哈希。没有审计源则保守未知并记录，保留手填完整模型 ID。
- [x] 重跑 Python 测试与 desktop 的目录/profile 协议测试，应通过；检查 Go 请求的 UA/session/header/redirect 矩阵。
- [x] 提交 `feat: enable exact OpenCode Go Responses model routing`。

## Task 11: 订阅设置与现有聊天模型选择器

**Files:** Create `src/features/settings/SubscriptionModelForm.tsx`、`SubscriptionModelForm.test.tsx`、`src/tauri-api.subscription-models.test.ts`；Modify/Test `src/types.ts`、`tauri-api.ts`、`DesktopApp.tsx`、`features/settings/SettingsPanel.tsx`、`SettingsPanel.test.tsx`、`modelProviderPresets.ts`、`modelProviderPresets.test.ts`、`model-form.css`、`features/workspace/ApiModelPicker.tsx`、`ApiModelPicker.test.tsx`。

**Interfaces:**
- API wrapper camelCase 函数对应任务 5/9 的精确命令名；状态只能取 DTO，finish 成功触发现有 profile 列表刷新。
- `SubscriptionModelForm` props：`provider: "open_ai_codex" | "claude_code"`、`profile: ModelProfile | null`、`onSaved(profile: ModelProfile): void`、`onCancel(): void`、`zh: boolean`；复用现有设置状态，不新增独立 Agent binding。
- picker 使用有来源的 discovery API；既有 Vec wrapper 留给未迁移调用。CLI 非 API 标签，configured-only 不假装服务发现失败；完整 ID 手填始终可用。

- [x] 写 `subscription_form_saves_provider_specific_payloads_without_secrets`：Codex begin/authorized/finish/disconnect 正确；Claude 无 key 框，只读 auth 状态；Go 三协议准确 payload；opaque binding 不由表单输入。 关键断言：`expect(savedPayload).not.toHaveProperty("subscription_account_ref"); expect(savedPayload).not.toHaveProperty("refresh_token"); expect(claudeKeyInput).toBeNull();`
- [x] 写 `subscription_form_escape_cancels_only_top_layer`：打开后立即 window Escape，不聚焦内部；只关闭顶层、父设置仍开；cancel 只调用一次；late authorized/result 不保存/重开。 关键断言：`expect(parentSettings).toBeVisible(); expect(cancelLogin).toHaveBeenCalledTimes(1); expect(finishLogin).not.toHaveBeenCalled();`
- [x] 写 `subscription_picker_retains_selected_profile_and_discovery_source`：错误/空/ConfiguredOnly 区分；手填 ID、重启加载、禁用不支持 effort/Fast；换 Go ID 明确换协议；旧 profile ordinary edit 不刷新能力。 关键断言：`expect(selectedProfileIdAfterRestart).toBe(selectedProfileIdBefore); expect(configuredOnlyRefresh).toBeDisabled();`
- [x] 运行 `npm test -- src/features/settings/SubscriptionModelForm.test.tsx src/features/settings/SettingsPanel.test.tsx src/features/workspace/ApiModelPicker.test.tsx src/tauri-api.subscription-models.test.ts`，确认预期失败。
- [x] 实现内嵌表单/原生调用/列表刷新；需要覆盖层时用现有窗口 Escape stack，cleanup 取消轮询。显示订阅登录/CLI安装与模型服务传输说明，不泄露 scheduler、tool alias 或原始协议。
- [x] 重跑局部前端测试、`src/tauri-api.model.test.ts` 和 `npm run build`，应通过；现有平台入口和旧保存 payload 不变。
- [x] 提交 `feat: add subscription model settings and chat selection`。

## Task 12: 完整回归、真实 opt-in 验收与交付记录

**Files:** Create `src-tauri/src/agent_v4/subscription_live_acceptance_tests.rs`；Modify `src-tauri/src/agent_v4.rs`（模块注册）、`acceptance/README.md`、本 spec/plan 的实际执行记录。

**Interfaces:**
- 新增 ignored 测试 `live_codex_subscription_reads_nonce`、`live_claude_subscription_reads_nonce`、`live_go_responses_reads_nonce` 与 `live_claude_subscription_windows_stop`；复用已有 Go acceptance 的临时 Store/随机 nonce/只读原 profile 装载方式。
- 显式使用 `OMICSOPS_LIVE_SUBSCRIPTION_PROFILE_ID`；按测试分别验证精确 provider/base，keyring 从当前用户 session 读取；Claude CLI 使用用户已自行登录的官方原生程序，不打印身份或秘密。
- 另写 deterministic 删除拒绝测试并在真实 GUI smoke 中用临时文件手工确认；nonce 不在 prompt，主机执行一次 `project.read`，最终回答有 nonce 和事件证据。

- [x] 写 `subscription_acceptance_preflight_rejects_unsafe_or_missing_configuration`：未设置 profile_id、不匹配 provider、非空原项目目录、无 keyring/CLI登录均在生成前拒绝；检查用例只触及临时数据。关键断言：`assert!(missing_config.is_err()); assert!(non_temporary_project.is_err()); assert_eq!(preflight_generation_count, 0); assert_eq!(original_db_after, original_db_before);`
- [x] 运行 `cargo test -p omicsops-desktop --lib subscription_acceptance_preflight`，确认预期失败；实现 harness，并重跑至通过。ignored 的编译/被忽略状态不算真实通过。
- [x] 按顺序执行 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`cargo fmt --all -- --check`，记录退出码与实际 counts；依赖锁文件变化额外 `npm ci`，Rust 锁文件按正常 Cargo 验证。
- [x] fmt 仅因格式偏差失败时运行 `cargo fmt --all`，将纯格式变化单独提交。桌面可执行文件被正在运行应用占用时记录阻塞，不强杀用户应用。
- [x] 确定性检查通过且操作者已显式提供一次性验收配置后，分别执行下方精确 ignored 命令，逐项保存通过/失败/未执行及 elapsed；缺少配置时保留未执行。不得自动复用全部既有用户账户或修改用户数据库。

```powershell
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_codex_subscription_reads_nonce -- --ignored --exact --nocapture
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_claude_subscription_reads_nonce -- --ignored --exact --nocapture
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_go_responses_reads_nonce -- --ignored --exact --nocapture
cargo test -p omicsops-desktop --lib agent_v4::subscription_live_acceptance_tests::live_claude_subscription_windows_stop -- --ignored --exact --nocapture
```

- [x] 记录 Windows GUI smoke：device登录/取消/保存、Claude原生登录状态、Go协议切换、重启选择、read回合、删除拒绝、立即 Escape 与停止。未执行的 GUI/macOS/SSH/PBMC 明确标记；不把 nonce 当科研流程验收。
- [ ] 检查全分支秘密/日志/配置边界、git diff whitespace、无 website/产物暂存；按用户选择的执行方法做最终代码审查并处理实际问题，更新文档，提交 `test: verify subscription model integration and acceptance boundaries`。

## Coverage and Execution Handoff

| Spec 章节 | 计划任务 |
| --- | --- |
| 1–3 目标、证据、接入选择 | 全局约束，1/6/8/9/12 |
| 4 配置、门面、兼容 | 1/2/8/9 |
| 5 Codex 认证生命周期 | 4/5/8/11 |
| 6 Responses 与多轮历史 | 2/3/9 |
| 7 Claude 模型传输 | 6/7/8/9/11/12 |
| 8 Go | 3/8/10/11 |
| 9 能力、预算、usage | 2/3/7/8/9/10 |
| 10 UI/DTO | 1/5/9/11 |
| 11 停止、失败、恢复、审计 | 2/4/5/6/7/8/9 |
| 12 验证 | 每任务 red/green + 12 |
| 13–14 范围、限制、记录 | 全局约束、6/12、当前评审 |

已完成计划自检：覆盖表无缺口；跨任务签名与 DTO 命名一致；五项 Review Focus
均绑定测试；没有未定义的执行器/凭据存储或省略的单次派发约束。当前未运行
上述产品检查、ignored 或 GUI smoke，所有步骤需实施时记录实际结果。

待用户评审计划并选择执行方式：

1. Native：当前会话逐任务实现，阶段测试/提交，最后独立审查整个分支。推荐此法，
   因为回放契约、模型门面和桌面工厂相互依赖，连续实现便于保持接口一致。
2. Subagent-driven：每任务由独立实现代理与独立评审代理完成后进入下一任务，
   最后审查整个分支；上下文与评审开销更大。

用户于 2026-09-30 授权实施代码并要求每完成一个功能提交一次；本会话采用 Native。
实施与实际检查记录逐步补充，未执行真实订阅/SSH/GUI 验收前不宣称验收通过。

## Execution Record

Task 1：新增 provider、可选 CLI/账户执行身份、纯 DTO 与保存合法矩阵；旧 JSON/哈希
保持，编辑 omission 保留 CLI，null 清除；Claude 删除复用现有活动引用保护且不动 CLI 登录。
核心/provider/DTO/CLI编辑测试先失败后通过。Store 删除逻辑未改变，复用已有
run/queue/side-chat/review 保护测试并增加 Claude 无凭据 fixture。
确定性检查：cargo test --workspace（1412 passed，12 ignored），npm test
（957 Vitest + 22 browser tests），npm run build，cargo fmt --all -- --check 均通过。
基线首次 SkillDetails Escape 测试失败，未改其源码；后续完整前端回归通过。
真实模型、CLI登录、SSH、GUI smoke 和桌面打包尚未执行。

Task 2（2026-10-01）：加入共享类型、宿主事件回放投影、provider 续接/活动事件；
旧请求的空字段省略。校验完整链、project/conversation/profile/hash/attempt、
宿主 call/result 一致性和续接大小；通用 context view 隐去续接。孤立结果测试
先失败后修复；SQLite 重载保持原 head/hash、重复 append 幂等。
cargo test --workspace（1418 passed，12 ignored），额外 Store 回放测试、
cargo check --workspace --all-targets、cargo fmt --all -- --check 均通过。
此增量仅准备契约；真实 wire 密文预算在任务 3 验证，持久化发送接线在任务 9。
任务 3–12 未完成。

Task 3：Responses 请求、固定端点、SSE 与单次 HTTP 传输完成；4 个新增契约测试通过，cargo test --workspace 1423 通过/12 ignored，provider_usage/model_provider_contracts 通过，fmt check 通过。以实际序列化 input（含 opaque reasoning）计量；1 MiB 行/8 MiB 流/256 KiB 参数/16 调用限额；无重定向、图片、内置执行工具或协议 fallback。

Task 4：固定设备登录、私有 vault bundle、共享刷新锁完成；6 个新增认证契约测试通过。完整 Rust 1429 通过/12 ignored，fmt check 通过。401 刷新代次避免相同 token 的重复刷新；网络等待后复查 vault，已退出的凭据不恢复；不确定轮换/写入失败要求重新登录。

Task 5：设备登录后台状态机与六个 Tauri 命令完成；8 个新增行为测试、10 个 subscription 定向测试通过。完整 Rust 1437 通过/12 ignored；共享 credential mutation lock 防止刷新覆盖退出；守卫提取供保存/删除共用，同账户 token 更新不改变冻结身份。数据库失败恢复新/旧秘密，恢复失败明确要求重新登录；退出数据库失败也恢复秘密。

Task 6：Windows suspended spawn / Job Object / handle-list / owned handle reaper 与 Claude 预检完成；完整 Rust 1445 通过/12 ignored，fmt check 通过。生产 CLI 有版本/订阅状态检查，但不能完整确认 managed policy，restrictions_verified=false 且生成 runner 拒绝启动。该限制不等于 Claude 生产端到端通过；后续 envelope 测试只使用受控 runner。官方 CLI 文档与 hooks-guide 明确 managed hooks 不能被普通 disableAllHooks 覆盖，managed-settings 仅提供会话内 /status 来源确认；未调用实际用户 CLI。

Task 7：Claude stdin 调用、严格单回合 envelope 与 NDJSON 解析已实现。预算边界和 stderr 管道阻塞均先复现失败再修复；完整 Rust 回归 1450 通过/12 忽略，最终 Claude 合约 9 通过，fmt 通过。取消与进程树由 Task 6 的 Windows 测试覆盖；成功与身份变更使用隔离测试进程。未调用真实 CLI/订阅模型，生产策略门禁仍有效。

Task 8：显式 ModelClient 门面与共享凭据协调器已实现；旧 HTTP provider 保留原请求逻辑，新 provider 不按模型名推断或付费 fallback。门面 5 个合约通过，adapters 全部测试通过（真实 SSH 测试忽略），fmt 通过。Go 发现请求独立限时/限流且禁止重定向；Codex/Claude 仅返回 ConfiguredOnly。单次派发遇到 401/断流不重发，普通 Codex 最多一次无输出的 401 刷新重试。

Task 9：主对话、Plan、总结、旁聊、委派、reviewer、压缩与澄清入口均接入 ModelClient；新模型发现命令保留来源。完整 Rust 回归 1462 通过/12 忽略；最终桌面 subscription 合约 3 通过，core 续接/闲置合约 3 通过，Responses 合约 5 通过，fmt/diff check 通过。三 backend 的临时 nonce 读取/删除拒绝/恢复测试使用 HTTP 替身或隔离 Windows 测试进程，非真实订阅验收。停止 HTTP 请求后，真实临时 Store 中已派发远端 job 的运行状态保持不变；未连接 SSH。
续接仅投影当前 run 的完整验证事件链；其它运行的对话历史仍作为有界文本证据。临时委派节点独立保存调用内续接，主运行只保存已验证节点结果。MCP 请求只允许依据同一已验证事件链中的冻结目录补齐宿主绑定字段；其它参数差异拒绝。心跳/usage/空片段不推进 idle；opaque reasoning 不进入普通上下文。

Task 10: Six exact Go Responses IDs appended from models.dev source SHA 664e5052595cb2464666cbe71b26917f6b903d967ce9bd0d39e347a3c5aa84bc; old rows and top source SHA remain unchanged. Missing catalog/protocol tests observed RED then GREEN. Python 4 passed; desktop model_ 38 passed/1 ignored; facade 5 and Responses 6 passed; fmt/diff check passed. Effort is validated per exact ID, unknown IDs keep options unset. Source-advertised vision remains in catalog but host subscription image transport remains unsupported. No real subscription generation executed.

Task 11: Subscription settings and existing chat picker are connected. Codex device sign-in requires explicit Save after authorization; cleanup cancels each challenge once, including late begin/poll results. Claude has native executable setup, read-only saved-profile status and visible production policy gate; no key input. Go switches all three exact protocols and displays Muse training disclosure. Discovery retains its source; ConfiguredOnly cannot refresh and full-ID input remains available. Ordinary edits omit catalog refresh, account bindings and untouched budget/effort/delegation fields. Settings 87, desktop application 95, final subscription form 6, picker 9, preset 2, API wrappers 5 and routing 1 tests passed in their targeted runs; final Web build passed. No GUI or actual subscription sign-in executed.

Task 11 follow-up: Native subscription links now invoke a narrow symbolic-resource command for exactly the Codex device page, Claude official setup and Go privacy page. Windows uses a balanced COM apartment and ShellExecuteW open verb, without shell argv or arbitrary URLs. Other platforms and opener failures show a copyable fixed URL; native GUI opening is unexecuted. Wire/native tests observed RED then GREEN; frontend resource/lifecycle wrappers 10 passed. No new npm dependency or lock change. Implementation follows the Windows ShellExecuteW API contract (https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecutew). Complete checks are being repeated after this correction.

Task 5 follow-up: Generic credential inventory now identifies Codex keyring references as read-only subscription_session values. Replacement is refused even through a legacy MCP alias to the same reference; login/refresh/disconnect remain the only application mutation paths. Native overwrite test and frontend labeling test observed RED then GREEN; 12 credential/DTO and 8 frontend credential tests passed. This response-only enum addition has no SQLite migration or profile hash impact. Final complete checks are repeated after this correction.

Task 12（2026-10-01）：一次性订阅 nonce/Windows stop 的四个 ignored 测试已加入；配置预检测试先失败后通过，原数据库只读且临时 fixture 字节保持不变。未提供显式一次性配置，因此四项真实订阅验收及 Windows GUI/macOS/SSH/PBMC 均未执行；Claude 两项仍会在策略门禁拒绝。临时 nonce 不是科研端到端验收。
最终确定性检查（包含原生链接和只读凭据修复）：cargo test --workspace 退出 0（1471 通过、16 ignored）；npm test 退出 0（970 Vitest、22 browser bridge）；npm run build、npm run build:desktop、cargo fmt --all -- --check 均通过。桌面构建完成 Windows x64 NSIS 打包，仅作为构建检查，未发布或分发。Python 使用 python -B -m unittest discover -s scripts -p test_import_model_catalog.py，4 项通过；git diff --check 通过。npm 锁文件未改变，无需 npm ci；Cargo 锁依正常 workspace 检查。现有 Web chunk-size 和 Windows linker 提示不影响成功结果。最终独立代码审查及其修复记录待补充。

Final review fix 1: Native continuation now applies the existing host browser guard before persistence. A prohibited response is rejected whole without rewriting opaque state. Sentinel persistence test observed RED (sensitive URL stored) then GREEN; no rejected arguments in durable/published events or replay. Full regression after the complete correction pass remains required.

Final review fix 2: Before a native replay request, the complete verified event chain is reconciled into durable host ToolFinished decisions tied to the original call and source hash. Clarification uses the actual InputRequested/UserInputAnswered pair; completion uses its submitted proposal and actual verification/reviewer decision. A proposal without ToolRequested or ToolDispatchStarted receives an explicit failed proposal_not_dispatched closure and is not executed. Requested ordinary calls still require normal recovery; uncertain dispatch remains fenced. Four new deterministic regressions observed RED-to-GREEN, including Store reload, failed verification/reviewer correction, parallel approval denial and crash boundaries. All agent-core tests pass; full workspace after the correction pass remains required. This reuses existing protocol outcomes without new SQLite schema.
