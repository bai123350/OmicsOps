# Store 与 Agent 会话生命周期架构

## 范围与来源边界

本设计将 SQLite 所有权集中到异步 `omicsops-store`，并把 Agent/Plan 模式、计划修订和规划期工具权限建立在同一持久化边界上。机制层面参考了 [`wisp-science@8d57fb3`](https://github.com/xuzhougeng/wisp-science/tree/8d57fb34808f0073218e0b07108d146745169a3b) 的对象组织方式；参考项目采用 AGPL-3.0，本仓库的 schema、迁移和实现均为 Apache-2.0 下的独立原创代码，不复制参考 SQL 或源码。

本次只建立可持久使用的 Store 和会话生命周期抽象。publication、exploration、ACP、undo 等尚无产品入口的表只保留 schema、约束、索引和迁移验证，不添加占位 CRUD 或 UI。

## 持久化边界

`omicsops-store` 使用 `sqlx::SqlitePool` 提供全异步 API，拥有项目、会话、消息、模型、Skill、MCP、Agent V4、Scientific V4、同步和退休 runtime 数据。`omicsops-adapters` 只负责 LLM、SSH、研究来源等外部系统，不再拥有 SQLite 连接或 repository 实现。

v4 schema 在 `crates/omicsops-store/migrations/init.sql` 中独立定义。`projects` 是规范化主体，OmicsOps 特有字段存入一对一扩展；会话映射为 `frames` 与 `conversation_records`，根会话满足 `root_frame_id = id`；消息独立保存并由会话内 sequence 约束顺序。`env_snapshots` 及 OmicsOps 模型、Skill、MCP、Agent V4 和 Scientific V4 扩展随 schema 一同创建。

桌面应用通过共享 `omicsops-dto` 暴露跨边界对象。DTO crate 只包含 native/wasm 均可编译的数据类型；Tauri helper 负责把 Store 内部值验证并映射为共享 DTO，前端不复制另一套后端结构。

## v3 到 v4 的切换

文件库以 `PRAGMA user_version = 4` 标识当前 schema。打开旧库时，Store 在启动迁移事务前创建同目录且不覆盖的备份：

```text
<db>.pre-store-v4.<timestamp>-<id>.bak
```

随后在单个 SQLite 事务中暂存重名旧表、创建 v4 schema、映射旧数据，并验证项目/扩展一对一关系、会话与消息归属、ID 双向集合、计数、消息顺序、同步条目、context archive run 归属、Agent event hash chain、Scientific state、`integrity_check` 和 `foreign_key_check`。全部验证成功后才提交；任一步失败都会回滚，应用启动失败且不会接收写入。成功迁移后只写 v4 表，不双写旧表；再次打开 v4 库不会重复迁移或创建第二份升级备份。

备份与人工恢复流程见 [数据库 v4 备份与恢复](../../database-v4-backup-recovery.md)。数据库中的项目删除只删除控制平面记录，不删除本地项目目录、远端科研文件或大数据引用。

## 会话模式与一致性快照

`SessionAgentModeV4` 只有 `agent` 和 `plan` 两个 wire value。模式按 `conversation_agent_mode:{conversation_id}` 写入 `settings`；没有该键的旧会话默认 Agent。模式不属于项目级或窗口级瞬时状态。

桌面恢复一个会话时，Store 在一个读事务中返回 `ConversationAgentStateV4`：ownership、mode、active lock、latest plan revision 和 latest Agent run 来自同一 SQLite snapshot。Tauri 在返回前校验最新 run JSON 的 run/project/conversation/status 身份。前端以 project、conversation 和递增请求序列三重校验异步响应，旧会话的迟到响应不得覆盖当前会话。

active plan revision 的状态为 `generating`、`revising` 或 `pending`。它只锁定所属会话；此时普通消息、再次 direct/planning start、模式切换和会话删除被拒绝，批准、请求修改和取消仍可使用。其他会话保持独立。

## 计划修订与原子批准

`proposed_plans` 每行表示一个 revision。revision identity/content——ID、owner、run、revision number、structured plan、Markdown、hash 和 created time——不可变；status、feedback 和 updated time 是允许变化的生命周期元数据。请求修改会原子地记录 feedback、把当前 pending revision 转为 revising，并追加可验证的 revision-request event；新的 plan 生成下一条 revision，旧内容不被覆盖。

只有最新 pending revision 可被批准。批准事务同时验证 project/conversation/run 归属、状态、revision、plan hash 和冻结 spec，写入审批与模式事件，把会话切回 Agent，然后提交。只有提交成功后桌面层才启动执行。取消将 run/revision 终止并解除锁，但会话保持 Plan，方便用户重新发起规划。

## Plan 工具权限

规划循环对宿主声明为 `ToolEffectV4::ReadOnly` 的内置工具自动放行，并允许 `agent.request_input` 与 `agent.propose_plan` 完成规划协议。runtime、mutation、network 和 delegation 默认拒绝；动态授权也不能绕过只读 effect 检查。

MCP 工具必须同时满足：server 已启用、启动已批准、当前 catalog/schema hash 与冻结值一致、当前工具声明的 `annotations.readOnlyHint` 恰为 `true`。字段缺失、为 `false` 或任一 hash 变化都 fail closed。没有持久工具批准的目标使用绑定 exact server/tool/arguments/catalog/schema 的单次审批，规划暂停，决定后恢复，并在真正调用前再次复核。

`readOnlyHint` 是第三方 server 提供、宿主无法验证的提示。批准表示用户信任该 server 对该次调用的只读声明，不等价于 OmicsOps 对无副作用的保证；审批信息与用户文档必须持续展示这一边界。

## 凭据与数据驻留

API key、SSH 密码和私钥继续只通过现有 Credential Manager/keyring 路径解析。SQLite、event、log、模型上下文、导出包和 Git 只保存不泄密的引用或脱敏信息。Store 备份会包含数据库中的全部控制平面数据，因此应按科研项目元数据的敏感级别保护，但它不应包含凭据正文。
