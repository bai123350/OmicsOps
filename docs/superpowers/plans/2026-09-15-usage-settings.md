# Usage Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付跨项目/会话的真实token统计、模型分组、日/周活动和工具排行。

**Architecture:** Store受限分页读取 `agent_events_v4`；宿主按完整运行合并provider attempt，向UI返回聚合数据而非原始事件。增量分页扫描用固定快照边界，任何省略或缺失保持可见。

**Tech Stack:** SQLite/sqlx、Rust现有UsageTotalsV4、Tauri DTO、React/Vitest。

**Spec:** `docs/superpowers/specs/2026-09-15-wisp-settings-source-audit.md` 与 master Usage mini-slice。

## Global Constraints

- 不造token/模型/工具样例，不增加货币账单或价格推断。`null/None`与真实零不同。
- 不改变V4原始事件、运行计划或模型目录匹配。报表不读取请求正文、tool arguments、tool output或凭据。
- 所有UTC时间采用现有事件时间单位；API边界使用RFC3339，SQL转换仅在Store。日/周使用UTC并在UI明确显示，避免客户端固定偏移遇夏令时错误。
- 自动化使用构造事件及临时SQLite；不调用模型/SSH。修改DTO/Store/lib/types先协调当前owner。
- 页是实际管理查询的交付，不以只有一个ContextUsagePanel替代跨会话统计。

## 已核对源码及关键语义

`crates/omicsops-store/migrations/init.sql::agent_events_v4` 包含run_id、project_id、conversation_id、sequence、event_hash、value_json、occurred_at；event_hash唯一，主键(run_id,sequence)。`src-tauri/src/agent_v4.rs::context_usage_response` 将ModelRequestStarted和ModelUsageObserved按(logical_request_id,attempt_id)配对，为无usage的start合成Interrupted/Unknown观察。`crates/omicsops-protocol/src/context_usage.rs::UsageTotalsV4::from_observations` 已按sample_index去重，对cumulative取max、delta求和、混合语义返回unknown；重试attempt独立计数。

不能把input+output+reasoning+cache直接求“total”：reasoning/cache可能是子集。卡片分别呈现input/output/reasoning/cache_read/cache_creation/reported_total，total仅用provider报告。模型profile当前名称可作为显示标签，历史身份用profile_id+model_configuration_hash，不拿当前模型字符串重写旧记录。

## U1：受限Store查询和宿主投影

**Files:** 新增 `src-tauri/src/usage_settings.rs`；Store新增查询方法与临时DB测试（可按现有模块规范放 `crates/omicsops-store/src/usage.rs`，由lib导出）；共享DTO和contract tests、native handler注册。

```rust
struct UsageFilter { project_id: Option<Uuid>, from: Option<String>, until: Option<String> }
struct UsageScanCursor {
    // opaque serialized host-issued data; every value revalidated against filter
    snapshot_rowid: i64, snapshot_event_hash: String,
    after_run_id: Option<Uuid>, filter_hash: String,
}
struct UsageGroup { key: String, label: String, totals: UsageTotalsV4 }
struct UsageDay { date: String, attempts: u32, tools: u32, totals: UsageTotalsV4 }
struct UsageTool { tool_id: String, dispatched: u64, succeeded: u64, failed: u64, uncertain: u64 }
struct UsageAggregatePage {
    totals: UsageTotalsV4, projects: Vec<UsageGroup>, models: Vec<UsageGroup>,
    days: Vec<UsageDay>, tools: Vec<UsageTool>, next_cursor: Option<String>,
    scanned_runs: u32, omitted_runs: u32, unattributed_events: u32,
    snapshot_at: String, completeness: String,
}
struct UsageConversationRow {
    project_id: Uuid, conversation_id: Uuid, label: String,
    latest_activity: String, totals: UsageTotalsV4, incomplete: bool,
}
// settings_usage_page(filter, cursor: Option<String>) -> UsageAggregatePage
// settings_usage_conversations(filter, cursor: Option<String>)
//   -> { items: Vec<UsageConversationRow>, next_cursor: Option<String>, snapshot_at: String }
```

omicsops-dto当前已有omicsops-protocol直接依赖，UsageTotalsV4直接复用，不新增镜像类型或把投影业务逻辑搬进DTO。Rust internal `UsageEventPage` 只供Store→host，不暴露事件给UI。

读取算法必须按完整run分页，不能在attempt中间截断后把partial误标完整：初次在读事务捕获max(rowid)及对应event_hash为水位；后续页验证该水位的hash仍存在，否则返回“历史已变化，请刷新”。所有查询加rowid<=水位；新事件不插入既有快照。按run_id ASC选最多50个含相关事件的run，游标为last_run_id；过滤参数必须绑定在cursor摘要中，跨项目/日期游标拒绝。SQL使用参数绑定，不拼接UUID/日期。

每个run读取完整相关事件（ModelRequestStarted、ModelUsageObserved、ToolDispatchStarted、ToolFinished、ToolDispatchUncertain；只选择allowlist JSON字段或在host立即丢弃其他内容），顺序sequence ASC。单run相关事件上限20,000，页总事件上限100,000；在进入下一run前达到页上限便以已完成last_run_id返回下一页。单run超限整run跳过并omitted_runs+1，不返回该run的局部“精确总量”。UI提供日期/项目缩小范围并始终标partial；不静默吞掉记录。每页结束释放SQLite事务，后续页重新验证水位。

日期筛选按attempt归属时间：start.occurred_at优先，无start取该attempt最早observation；选中attempt后读取该run在快照内的全部samples再汇总，即跨日输出不把一次cumulative用量拆成两份。SQL先选范围内有start/observation的candidate run，host再按上述锚点过滤attempt。工具按首次dispatch时间过滤。未知时间、身份冲突、无法解析的历史记录只计unattributed_events并显示partial，不猜时间。旧版V3及更早没有可靠token事件的运行不构造token；页面显示“Agent V4持久化观察范围”。

对每个run先按(logical_request_id,attempt_id)分组，start无usage沿用unknown_observation_from_request语义；这一pure helper可从agent_v4移至context_usage纯模块并被两处复用，也可调用现有context_usage_response获得同一scope totals。不能复制一套不同去重算法。一个run内的同attempt若model_profile/config hash冲突，归入Unknown identity并增加不完整计数。跨run累加的是已经合并的UsageTotalsV4，不能把来自不同run的相同UUID在全局再次去重。写一个 `merge_usage_totals`：known都是None则None、有Some则求已知小计，incomplete和attempt计数checked_add；溢出返回partial且字段unknown，不饱和后伪称精确。

工具计数以首次ToolDispatchStarted的(run_id,call_id)为准，ToolRequested/BatchStarted不算实际调用，ToolOutcomeReused不算新调用。Finished/Uncertain只关联已存在dispatch；重复事件去重。保留完整tool_id；只能根据冻结run中实际工具元数据分MCP/builtin，不能凭字符串猜Skill调用。Skills是提示上下文不是可执行工具；无真实激活事件时不展示伪Skill排行，页面明确统计工具dispatch。日活动同时展示attempt数和dispatch数；模型占比仅用同一可靠计数字段，缺总量不绘饼图百分比。

会话接口独立分页，按真实相关事件最新活动时间DESC、conversation_id ASC，limit20，固定水位。读取选中20会话的run并复用相同投影，超限有incomplete。直接使用事件conversation_id（含独立frame），不把side chat合并至主会话；不存在可显示会话label时用ID和“旁聊/历史frame”身份，不能把使用量丢失或错误跳转到主会话。

- [ ] 构造失败测试：cumulative 10→20只算20，delta 10+20算30，重复sample只算一次；两个attempt重试分别计数；start无usage为Unknown且tokens None；provider明确Some(0)保留0；缓存/reasoning不额外叠加total。
- [ ] 临时Store测试跨项目隔离、timestamp相同稳定分页、快照后append不污染旧页、删除水位拒绝游标、游标换filter拒绝；跨午夜attempt只归属start一天但读完整samples；页边界不截断run；超大run明确omitted；同call重复dispatch与reused不会多算。
- [ ] 先运行 `cargo test -p omicsops-desktop usage_settings` 与Store usage测试确认失败，再实现查询/投影/DTO，重复运行至通过。测试中断言UI返回JSON不含原始tool参数或模型文本sentinel。
- [ ] 独立提交后端及contract tests，记录实际查询边界；不同JSON历史字段通过serde解析，不要求真实网络。

## U2：真实聚合页面与连续分页

**Files:** 新增 `src/usage-settings-api.ts`、`src/features/settings/UsageSettings.tsx` 及测试；根设置接入 `usage`。

**Interfaces:** `UsageSettings({ locale, projects, selectedProjectId, onOpenConversation })`。默认当前项目最近30天UTC；无当前项目允许全部项目。时间选项7/30/90天/全部；overview、项目分组、模型、活动日历/按周汇总、工具排行共用一次aggregate分页结果；会话表单独取20条。

- [ ] 测试所有卡片的None显示“未报告”，0显示0，known+incomplete显示“已观察小计”；错误不显示空数据成功态；项目/日期改变丢弃旧代次响应。
- [ ] 后端每页已完整聚合run，UI仅合并这些互斥页的totals/groups，不拉原始历史。自动逐页加载直到next_cursor=null，最多一次in-flight；显示已扫描run数和“统计中”。更换筛选/卸载取消本地等待并丢弃迟到结果；响应cursor已见则拒绝重复累加并显示可重试错误。页面重试保留最后成功页并只重发未合并cursor。
- [ ] 会话表“更多”稳定追加；切筛选清空分页；点击返回确切project/conversation身份。项目总览可点击筛选，模型分组显示历史profile/hash；周视图只是后端daily数据的UTC周聚合，不伪造小时分布。
- [ ] 工具表展示实际dispatch与成功/失败/不确定，Unknown工具保留原ID；日期无事件可呈真实0活动，但缺token仍未报告。超限/旧数据不可统计提示必须在总量旁可见。
- [ ] `npm test -- src/features/settings/UsageSettings.test.tsx`、设置导航测试、`npm run build`；图表可用轻量HTML表格/日历，不为小图引入大依赖。
- [ ] 运行默认完整检查及桌面构建；Windows本地测试DB打开设置、切项目/日期、会话分页，真实模型/SSH验收记未执行。页接通全部卡片/日周/工具后才更新master，不将U1单独标全页完成。
