# Credentials Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付真实 keyring 凭据目录、受限替换、自定义凭据创建及未引用凭据清除。

**Architecture:** 非敏感目录由实际模型、SSH、MCP 引用和受管条目合成；秘密只经现有 CredentialVault。宿主解析目标，不提供任意账户读取或枚举接口。

**Tech Stack:** Rust、Tauri 2、现有 Store JSON、React/TypeScript、Vitest。

**Spec:** `docs/superpowers/specs/2026-09-15-wisp-settings-source-audit.md`；延续 master 的 Credentials mini-slice。

## Global Constraints

- 用户已授权全设置中心按页实施；不再确认范围。设计 Astra medium，代码 Sol high，每页独立提交。
- 凭据只能保存至 `io.omicsops.desktop` 的现有 keyring service；不能进入 Store、事件、日志、导出或模型上下文。
- `PrivateKeySecret { path, passphrase }` 是当前 SSH 私钥配置格式，不能将它当普通 API key；UI 不加载历史 path/passphrase。
- 本计划只新增 app 代码一次；共享 `omicsops-dto`、`src/types.ts`、native `lib.rs` 由当前 templates owner 释放后串行接线，不能并发覆盖。
- 查询仅可输出引用、用途和存在状态。不得返回秘密、长度、首尾字符、摘要或原始 keyring 错误。
- 保留已有 mini-slice：被引用条目的删除返回依赖列表且零更改，不批量断开连接。替换会影响下一次凭据解析，不声称撤销服务端密钥或取消已运行计算。

## 已核对的真实入口

`crates/omicsops-adapters/src/credentials.rs` 的 `CredentialVault` 提供 set/get/delete，SystemCredentialVault 没有账户枚举。`src-tauri/src/commands.rs::normalize_connection_profile` 强制 `ssh/{uuid}`；SSH owner 保存校验身份和认证方式。`model_commands.rs::model_profile_from_request` 使用 `model/{uuid}`，Ollama 无凭据引用。`p1_commands.rs::configure_pubmed_preset` 使用 `mcp/{presetUuid}/NCBI_API_KEY`；其他 MCP `env_bindings[].credential_reference` 可以引用自定义非秘密账户。Store 已有 `list_model_profiles`、`list_connections`、`list_json`、`put_json`、`delete_json`，不新增凭据表。

## C1：目录与受限操作服务

**Files:** 新增 `src-tauri/src/credential_settings.rs`；修改共享 `crates/omicsops-dto/src/lib.rs`、`src-tauri/src/dto_contract_tests.rs`、`src-tauri/src/lib.rs`；必要时从现有 owner 提取验证函数，但不重写 owner 保存流程。

**Interfaces:** 所有新 DTO 在 omicsops-dto 定义并导出；TS camelCase/现有序列化风格以 contract test 为准。

```rust
// 以下为接口规格，derive/serde 在实现时按 DTO crate 规范补全。
enum CredentialTarget {
    Model { id: Uuid }, Ssh { id: Uuid },
    McpBinding { server_id: Uuid, name: String }, Managed { id: Uuid },
}
enum CredentialPresence { Present, Missing, Unavailable }
enum CredentialValueKind { ApiKey, Password, SshPrivateKey }
struct CredentialConsumer { kind: String, id: Uuid, label: String, binding_name: Option<String> }
struct CredentialEntry {
    target: CredentialTarget, reference: String, label: String,
    presence: CredentialPresence, value_kind: CredentialValueKind, consumers: Vec<CredentialConsumer>,
    can_replace: bool, can_delete: bool,
}
struct ManagedCredentialMetadata { id: Uuid, label: String, created_at: String }
struct CreateManagedCredentialRequest { label: String, secret: String }
struct ReplaceCredentialRequest { target: CredentialTarget, expected_reference: String, secret: String }
enum DeleteCredentialResult { Deleted, InUse { consumers: Vec<CredentialConsumer> } }
// Tauri commands with AppState:
// settings_list_credentials() -> Vec<CredentialEntry>
// settings_create_credential(request) -> CredentialEntry
// settings_replace_credential(request) -> CredentialEntry
// settings_delete_credential(id: Uuid) -> DeleteCredentialResult
```

目录实现接受 `&Store` 和 `&dyn CredentialVault`，原生命令只转调该服务。MCP list 使用现有 `McpServerProfile` 的 Store JSON kind，必须引用 owner 的常量，不重复拼接另一种 kind。以 account 去重合并 consumers；保留一个稳定可解析 target。仅已解析引用可 get；get 的字符串立即丢弃，仅转 presence，失败转 Unavailable。没有引用的 Ollama 不生成假条目。没有 key 的真实引用返回 Missing，仍可替换。

替换逐项重新查 owner：model 必须等于该 profile 当前 credential_reference；ssh 必须等于当前 authentication_reference 且通过 `parse_authentication_secret`；mcp_binding 必须匹配实际 server+name 的 credential_reference，不能通过用户输入任意 reference 创建访问。受管 target 的 account 固定为 `settings/{uuid}`。secret 非空、最多64KiB；API key 原样保存，不 trim 实际值，仅用 trim 判断空；SSH private-key path 非空及结构合法。存在多个消费者时 UI 在替换表单列出所有用途。请求的 expected_reference 必填，仅与宿主重解析结果比较，失配要求刷新；不能用它定位vault。

去重后的target选择canonical model/ssh owner优先、managed其次、mcp最后；value_kind来自canonical owner认证类型，不能由客户端设置。即使调用者直接发McpBinding alias指向ssh/model/managed账户，服务仍查该账户canonical owner并执行对应验证；ssh账户不存在owner时拒绝替换，不降为通用API key。若同账户存在不兼容canonical owner则can_replace=false，显示引用冲突并引导owner修复。测试覆盖MCP alias指向SSH private-key账户、伪造generic字符串被拒、合法JSON仍可保存，以及编辑途中MCP绑定变化触发expected_reference冲突且两个账户都未改变。

新建 label trim 后1–100 Unicode字符，最多100条受管条目；metadata 使用 Store kind `credential_entry_v1`。写 metadata 后写 vault，失败保留可识别 Missing 条目并返回固定“未保存秘密，可重试或删除”错误；不要删除可能已成功写入的凭据以假装跨 keyring/SQLite 原子事务。未提供成功响应时重试采用同 id 的 Replace，不再次新建。metadata 不存 secret，也不存凭据哈希。

删除只接受 Managed UUID，检查 metadata 存在，重新遍历全部模型/SSH/MCP引用；任何消费者返回 InUse。无消费者则 vault.delete，成功后删除 metadata；vault失败保留 metadata，metadata删除失败保留 Missing 可重试项。旧模型/SSH/NCBI账户不可从此通用命令删除，页面提供对应 owner 配置入口。自定义凭据 reference 可复制给 MCP 环境变量“凭据引用”；不得自动注入 shell、Python、R 或所有 MCP 环境。

同进程 managed 删除与 MCP 绑定保存必须共享短期异步 mutation mutex（放独立 managed state，由 native setup 管理），覆盖“检查引用→删除”和“检查受管引用→保存绑定”，避免检查后新增引用指向已删 secret。MCP 保存对 `settings/` 前缀新增严格 UUID+metadata存在校验；其他已有引用保持兼容。锁内不等待网络或用户授权。不要仅靠前端 busy 标志解决该竞态。

- [ ] 写临时 Store+MemoryCredentialVault 测试：两个MCP引用同账户只产生一条目录、包含两用途；缺值为Missing、get报错为Unavailable；DTO与错误全文不含测试 secret；非法/失效target触发零vault调用；合法替换保持owner配置不变；SSH错误JSON拒绝。
- [ ] 写创建/删除测试：重载metadata可见且Store全文不含秘密；set失败留下Missing；有引用删除返回InUse且vault未delete；vault删除失败不删metadata；metadata删除失败下次重试可完成；并发新MCP绑定与删除串行化，不能成功保存悬空受管引用。
- [ ] 运行 `cargo test -p omicsops-desktop credential_settings`，确认新增测试在实现前失败。
- [ ] 按上述接口实现目录/操作/命令，测试断言 `assert!(!serialized.contains("secret-sentinel")); assert_eq!(vault.get(&account)?, Some("secret-sentinel".into()));`，并检查Store的metadata序列化结果。
- [ ] 运行相同定向测试和 DTO contract tests；只提交本页文件，不能夹带 templates owner 修改。

## C2：可操作的 Credentials 页面

**Files:** 新增 `src/credentials-settings-api.ts`、`src/features/settings/CredentialsSettings.tsx` 及测试；共享 `src/types.ts`、SettingsPanel 导航由其 owner 接入。

**Interfaces:** `CredentialsSettings({ locale, onOpenOwner })`；owner 跳转目标 `models|remote|connections`，设置根保持打开。API wrapper与上述四命令一一对应，秘密仅存在当前 password 输入和调用参数中，不写 localStorage。private-key 替换收集新路径与passphrase，序列化成既有格式；不回显已有配置。

- [ ] 测试初次加载与刷新；Missing/Unavailable不同文案；替换失败保留当前输入用于重试但不渲染错误中的secret；成功、取消、切页、卸载清空输入；创建失败能对已保留的Missing条目重试替换。
- [ ] 实现非秘密用途列表、存在状态、创建、替换、未引用自定义项删除。被引用自定义项显示依赖及Connections入口，不出现可误导的一键删除。owner账户显示来源及真实替换表单；列表无 reveal/copy secret。
- [ ] 测试点击删除后出现具体条目确认层，立即按 Escape只关确认；编辑层同样进入窗口Escape栈；再次Escape才关闭设置。被引用响应必须保留原条目并展示最新用途，不能乐观移除。
- [ ] `npm test -- src/features/settings/CredentialsSettings.test.tsx`、设置导航测试和 `npm run build`；补一次mock Tauri命令断言传递的target是实体身份而不是任意account。
- [ ] 接线后运行默认完整检查和 `npm run build:desktop`。Windows smoke 使用临时无敏感测试值创建/重启/替换/删除受管项，检查UI不泄露；真实API和SSH验收标未执行。更新master并独立提交本页。

## 审阅标准

没有通用 get_secret 命令；枚举来自Store引用而非扫描Windows凭据；实际keyring写入可用；新增非秘密引用可在现有MCP配置消费；有使用者的删除无副作用；只改设置不扩大授权。Models/SSH/MCP owner缺乏的“解除绑定”不能用本页静默清空替代，此限制必须留在UI和交付记录。
