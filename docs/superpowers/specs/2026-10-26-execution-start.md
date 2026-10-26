# 恢复执行的持久运行状态

暂停后恢复执行时，旧桌面驱动直到结束才保存状态，期间 SQLite 仍显示 `waiting_for_input` 或 `waiting_for_approval`。依赖运行状态的行为会据此误判。

宿主持有原有 driver lease，完成环境/模型组合及活动运行注册后，调用 `Store::start_agent_execution_v4`。同一 SQLite 事务校验捕获的事件头、状态、冻结 spec 与审批锚、会话归属和锁、终态及停止请求，只更新状态为 `running`，保留其他 JSON 字段和原始事件。失败则撤销本次活动注册，不启动驱动。支持普通 Agent 与批准计划，不改变授权或重派计算。

测试使用内存 SQLite，覆盖两种模式的暂停/运行状态、来源变化、停止、终态和锚不匹配；不使用真实模型或 SSH。自动上下文压缩的集成验证记录在 `2026-10-22-automatic-context-atomicity.md`。恢复流程仍需要手工 smoke：等待输入或工具批准后恢复，确认运行状态及时更新；已停止运行不可重新启动。

实际验证：`cargo test -p omicsops-store --test execution_start` 两项通过；最终 `cargo test --workspace` 2053 通过、0 失败、21 ignored。Web 构建、Windows Release/NSIS 构建及 1518 项前端和 22 项扩展测试通过；真实模型/SSH 验收、安装和分发未执行。
