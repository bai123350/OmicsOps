# Wisp Science 12 项首次范围总设计

日期：2026-10-04。状态：已明确需求范围，具体设计待审阅；尚未开始产品代码实现。

## 目标和已确认范围

用户要求把 Wisp Science 中值得借鉴的能力纳入 OmicsOps，并实现项目清单。原有修改已在 `e3b20a5` 保存；当前分支为 `codex/wisp-science-improvements`，清单已在 `ba04a9f` 纳入仓库。用户随后明确选择“先完整实现 12 项首次范围”。因此交付目标覆盖全部 W01–W12，不以前三项、静态界面或设计文档代替功能完成。

目标是让科研用户能保留会话输入、观察已运行计算、查看科学文件、核对诊断与证据，并在没有外部凭据时体验一个明确标记的示例。所有新增能力保留 OmicsOps 的项目作用域、宿主审批、冻结运行、可核验来源和不确定派发不重跑的边界。

“首次范围”以[清单各项的建议切片](2026-10-04-wisp-science-improvement-backlog.md)为准：W09 包含只读准备度报告与 reference-only 结构化包；W11 只包含可选择的迁移预览，实际导出/导入为下一切片。PDF/Office/Notebook 预览、远端取消、独立探索执行、文件恢复写入、包含原始数据的证据包和跨设备活跃运行恢复不属于本轮。

## 实现策略与取舍

选定策略是分别补齐现有架构中的 12 个小能力，依赖共享的领域/协议/Store 接缝，但每项拥有自己的 UI、回归与提交。这样需要更多精确的身份和失败测试，却能复用已有研究数据与安全规则。

两个备选方案不采用：整体移植上游管理器会形成第二套会话、内核、运行与证据状态；集中做一个巨大改造则会把互不依赖的持久化、文件读取与 provider 准入绑在同一个不可单独验证的交付中。单纯复制界面也无法实现作业查询、对象检查和原子压缩的用户行为。

本轮不新增独立 crate、不引入云服务，不为了文件长度重排现有模块。只有三个功能共同需要的本地有界文件读取与 Windows reparse/handle 检查抽取一个聚焦宿主模块。各项设计按可独立交付的章节拆分到以下文件，实施计划随后同样按功能拆分。

## 功能与完成标准

| 项 | 本轮可观察交付 | 主要设计章节 | 必须证明的边界 |
| --- | --- | --- | --- |
| W01 | 切项目/会话找回各自文字草稿；接受回执只清理已提交版本 | [草稿、文件与作业](2026-10-04-wisp-composer-files-jobs-design.md#w01会话文字草稿) | 不跨作用域携带附件/引用；不落库 |
| W02 | 本地与 SSH CSV/TSV 有界只读表格 | [草稿、文件与作业](2026-10-04-wisp-composer-files-jobs-design.md#w02有界-csvtsv-表格预览) | 完整逻辑记录解析；prefix/full 摘要和截断说明 |
| W03 | detached SSH 作业列表、手动刷新和用户启用的有限退避查询 | [草稿、文件与作业](2026-10-04-wisp-composer-files-jobs-design.md#w03ssh-后台作业列表与查询) | 原 frozen job/root/fingerprint；无重新派发或取消 |
| W04 | 已启动 Python/R 内核的有界对象元数据 | [运行时设计](2026-10-04-wisp-runtime-design.md) | 现有 session 串行协议；不隐式启动或调用任意对象方法 |
| W05 | 暂停 ordinary run 的手动上下文压缩可真正提交并恢复 | [运行时设计](2026-10-04-wisp-runtime-design.md) | 完整请求准入；事件头/科学状态/指导一致；失败回滚 |
| W06 | run/tool/失败筛选、sequence/hash 定位和脱敏 HTML/JSON 导出 | [诊断、示例与回归](2026-10-04-wisp-diagnostics-example-design.md) | 只读校验来源事件链；导出安全投影而非原 payload |
| W07 | 显式创建一个双语离线合成/录制示例工作区 | [诊断、示例与回归](2026-10-04-wisp-diagnostics-example-design.md) | 无模型/SSH/网络；不伪造生产 Run/审批/证据 |
| W08 | manifest 驱动的确定性 V4 场景报告与捆绑 Skill helper 行为回归 | [运行时设计](2026-10-04-wisp-runtime-design.md)、[诊断、示例与回归](2026-10-04-wisp-diagnostics-example-design.md) | suite hash/count/status；失败非零；mock 外部库/网络 |
| W09 | 保存 revision 的准备度报告及 publication.md/manifest/checksums 包 | [证据与工作区设计](2026-10-04-wisp-evidence-workspace-design.md) | reference-only、保存快照、omissions；不等同复现通过 |
| W10 | 显式选中小本地文件的不可变基线和差异 | [证据与工作区设计](2026-10-04-wisp-evidence-workspace-design.md) | 有界读取、完整哈希、敏感正文不保存；SSH/大文件只引用 |
| W11 | 显式选择文件与项目材料的迁移清单预览 | [证据与工作区设计](2026-10-04-wisp-evidence-workspace-design.md) | 不复制共享 DB/凭据/权限；列出缺失与重绑定能力 |
| W12 | 持久化可逆收起会话、独立集合和恢复 | [证据与工作区设计](2026-10-04-wisp-evidence-workspace-design.md) | 可见性与运行/分支状态分开；分页前过滤；旧引用仍可解析 |

## 架构和依赖

- DTO 只进入 `omicsops-dto`，不依赖 Tauri、数据库、UI 或异步运行时；同步维护宿主合同测试与前端调用类型。
- 宿主命令核对 project/conversation/run/session 与 frozen identity，前端过滤不能替代所有权检查。
- 新的持久化属于 Store，迁移幂等、旧数据可读；UI 可见性不改变内部引用解析和授权查询。
- W04 需要非启动的 live runtime lookup 与本地/SSH 两套 worker 协议；W05 需要 core 的候选完整请求生成与 Store 原子提交。这两项不能通过启用现有按钮完成。
- W02 建立的 `project_file_read` 被 W10/W11 复用；W09 的首版引用包不依赖 W10 原始字节，不为导出读取 live scientific files。
- W08 的 runner 独立实现，随后加入与 W03–W06 新行为相关的确定性场景；它不替代 Store、协议或 UI 测试。
- W01/W06/W12 和 helper 回归可独立推进。W07 只依赖 W02 的本地文件检查叶子接口，不依赖表格或 SSH UI；示例身份与 Project 同事务登记，启动和精确来源入口都先识别示例。共享 `DesktopApp`、`WorkspaceShell`、命令注册与 DTO 合同由主实施者串行整合，避免并行覆盖。

建议交付顺序是 W01；W02/W03；W08 helper；W04/W05；W06/W07 与 V4 runner；W09/W10/W11/W12。该顺序是依赖与风险排序，全部 12 项仍属于本轮任务。实现过程中遇到已证明的接口冲突，只做服务于当前功能的调整，并同步相应设计与测试。

继续使用已授权创建的当前分支和 checkout。无需另建聊天、发布 tag、GitHub Release、安装或分发构建产物。普通功能按项提交；如果 fmt 发现已有纯格式偏差，按仓库规则独立提交格式修复。

## 共同失败与数据边界

凭据仍只存在现有 Credential Manager/keyring 路径。SQLite、日志、事件、模型上下文和导出均只接受安全引用/脱敏投影；小文件基线发现凭据样式正文时降为引用，不能把秘密放进快照。

任何等待停止、窗口关闭、断连或 shelve 都不能记为远端计算已经停止。不确定的 job dispatch 保留原身份；面板只查询。inspect 只查询活着的 session，不能因为缺失而启动/重建解释器。manual compact 不改变冻结审批、工具证据或 scientific state，也不把摘要当成新执行结果。

导出文件由宿主生成，原始事件链先校验；JSON 使用版本化 schema，HTML 只输出转义文本且无脚本/远端资源。导出来源与 hash scope 显式记录。准备度报告、provenance 完整性与成功 build 都不代表实际复现。

全部新弹层、集合、菜单进入窗口级 Escape 堆栈。测试必须在打开后立即按 Escape，不先聚焦内部，并证明一次只关闭顶层。所有长等待与异步 UI 请求都捕获作用域/generation，晚到响应不能改写另一个项目/会话。

## 验证与交付证据

每项行为新增/更新测试，并按失败测试、实现、对应通过测试的顺序推进。自动化只使用临时目录、模拟命令/transport、scripted model、fake timers 和模拟 Tauri；不依赖真实 SSH、API key、GPU、科学服务器或网络。

按项运行最接近的检查，交付前重新执行：

```powershell
cargo test --workspace
npm test
npm run build
```

新增宿主组合、worker/资源打包、Windows 文件 handle 或示例资源后运行 `npm run build:desktop`。依赖锁文件变化必须执行 `npm ci` 验证干净安装。`cargo fmt --all -- --check` 和 `git diff --check` 也须通过。

真实 runtime/model/SSH 验收只按 `acceptance/README.md` 在一次性环境中显式执行对应 ignored tests。未执行要明确记录，不能用 mock、ignored 计数、安装包 build 或局部 smoke 声称端到端通过。

交付更新每项实现状态、提交、对应测试和手工 smoke。最终汇报明确实现的 12 项、实际执行的检查，以及真实模型/SSH 验收是否执行；不能仅链接设计文件声称完成。本轮前面已通过的 1,514 个 Rust 测试和 1,014 个前端/扩展测试只属于旧修复基线，不能代替新功能验证。

## 设计审阅检查

本总设计和各子设计应满足：12 项均有可观察完成标准、输入身份与预算、失败语义和测试；未把首范围扩展成产品愿景；没有占位任务、隐含外部授权、第二套 DTO 或凭据持久化。具体实施计划必须在这些设计获审阅后再生成，并逐项绑定同一完成标准。
