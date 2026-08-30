# 数据库 v4 备份与恢复

## 自动升级备份

OmicsOps 的应用数据库名为 `omicsops.db`，位于操作系统为应用标识 `io.omicsops.desktop` 分配的 Tauri `app_data_dir`。实际目录由操作系统和安装方式决定；不要根据用户名硬编码路径。需要定位时，先在系统的应用数据目录中查找该文件，并确认同目录属于 OmicsOps。

Store 首次打开 schema version 低于 4 的现有数据库时，会在任何迁移写入前创建一致性副本：

```text
omicsops.db.pre-store-v4.<timestamp>-<id>.bak
```

备份使用 create-new 语义；同名文件已存在时生成新 ID，不覆盖旧文件。Windows 与 macOS 都使用同一文件名规则，数据库路径可以包含空格、Unicode 和非 ASCII 字符。迁移成功后再次启动不会再为同一个 v4 库创建升级备份。

备份包含项目控制平面元数据、会话、消息、运行、事件、模型/Skill/MCP 配置引用和科研状态。凭据正文、SSH 私钥和 API key 不应进入数据库；它们仍保存在系统 keyring/Credential Manager。请按敏感科研元数据保护 `.bak`，不要提交 Git 或附到公开 issue。

## 迁移失败时

迁移在一个事务中执行。ID/计数/顺序、外键、SQLite 完整性、event hash chain 或扩展数据验证失败时，事务回滚，应用拒绝启动和写入。升级前 `.bak` 保留不变。

旧版 `agent_events_v4` 可能只在 `value_json` 中保存事件时间而没有独立的 `occurred_at` 列。v4 迁移会先从每条事件的 JSON 信封回填毫秒时间，再核对持久列、事件内容和 hash chain；不会用 `0` 或当前时间替代历史时间。旧 notebook、Agent run、context archive 和 scientific state 的兼容列也会在相关索引创建前补齐。任何 JSON、归属或 hash 不一致仍会使整个事务回滚。

先保存完整错误信息和失败库的只读副本；不要反复覆盖文件，也不要用 SQLite 工具手工删除约束或事件。可在报告中提供脱敏后的错误和 schema version，但不要提供凭据、真实服务器输出或未经脱敏的科研数据。

## 从 pre-store-v4 备份恢复

1. 完全退出 OmicsOps，并确认没有 OmicsOps 进程仍持有数据库。
2. 定位当前 `omicsops.db` 与目标 `.pre-store-v4.<timestamp>-<id>.bak`，核对它们位于同一个 OmicsOps 应用数据目录。若存在多份备份，选择所需升级尝试对应的文件，不要只按文件名猜测。
3. 将当前 `omicsops.db` **复制**为带日期的故障保留文件，例如 `omicsops.db.failed-20260828`。不要删除它；诊断 event/hash 或迁移问题时可能仍需要。
4. 将选定 `.bak` **复制**为 `omicsops.db`。保留原 `.bak` 不变，不要移动或原地编辑备份。
5. 启动同一版本的 OmicsOps。Store 会重新执行幂等 v4 迁移，并在提交前再次运行完整性、外键、归属和 event hash 验证。重新迁移也会创建一个新的、不覆盖旧文件的 pre-store-v4 备份。
6. 打开项目和会话，核对项目数量、消息顺序、最近运行、Agent/Plan 模式、计划 revision 与科研状态。只有这些检查通过后才清理故障副本；有审计要求时应继续保留。

若再次失败，停止重试并同时保留原始 `.bak`、失败库副本、应用版本与完整错误。不要把备份还原到另一个正在运行的 OmicsOps 实例或不同 schema 版本的应用。

## 数据库恢复不恢复科研文件

数据库是控制平面，不是本地/远端大型科研文件的默认备份。恢复数据库不会回滚、删除或重新下载项目目录、SSH/WSL/集群文件、容器镜像或远程数据资产；项目删除同样不会删除这些科研文件。恢复后若引用目标已变化，应把它作为数据驻留或 provenance 问题单独处理，不要通过删除数据库记录来“同步”文件系统。
