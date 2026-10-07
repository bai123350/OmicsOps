# Wisp Science Twelve Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完整交付用户确认的 W01–W12 首次范围，包括真实宿主行为、界面、回归和可核验的实际检查结果。

**Architecture:** 13份逐项计划覆盖12项功能，W08分为V4场景与Skill helper两个独立部分，共50个可验证任务。共享本地文件读取和纯文本检查有唯一所有者，Store/协议/宿主/UI按既有边界演进；共享入口串行编辑。

**Tech Stack:** Windows-first Tauri 2、Rust/sqlx/SQLite、React 19/TypeScript/Vitest、Python 3.12 stdlib unittest、现有SSH/SFTP和V4运行时。

**Spec:** [已确认总设计](../specs/2026-10-04-wisp-improvements-design.md)、[源码对照和首次范围](../specs/2026-10-04-wisp-science-improvement-backlog.md)。设计基线为 `161199f`；用户于2026-10-06确认 `8515da8` 全部计划并选择子Agent逐项实施与审阅。逐任务推进，完成审阅后才勾选；子计划中描述写计划阶段的待审阅文字以此执行授权为准。

## Global Constraints

- 用户已选择全部12项首次范围，不缩成前三项或静态界面；PDF/Office、远端取消、文件恢复写入、独立探索执行和实际项目迁移export/import仍为后续。
- 使用 `E:\Project\OmicsOps` 的 `codex/wisp-science-improvements` 分支；原修改已存 `e3b20a5`，清单 `ba04a9f`，设计 `161199f`。不另建聊天/工作树，不push/发布/安装构建产物。
- 凭据只走Credential Manager/keyring；公开数据为安全引用/脱敏投影。模型/MCP传输仍按现有配置，不宣称所有数据永远不离开本机。
- 宿主裁决执行/审批；保留frozen authority、unknown dispatch身份和不重跑语义。窗口关闭、shelve、Stop或放弃等待不证明远端计算取消。
- 跨UI Rust类型只在omicsops-dto，协议纯类型可重导出；同步宿主serde合同和TS调用。迁移幂等、旧数据可读，不建立第二套运行/科学/会话状态。
- 每项行为有真实RED/对应GREEN或正确旧helper的基线PASS；自动测试只用mock/tempdir/fake timers/scripted model，不依赖真实SSH/模型/R/GPU/SLURM/网络。
- Overlay全部window Escape stack，打开立即按Escape一次只关最顶层；async结果捕获scope/generation。所有任务先读AGENTS.md、对应设计及该计划的Interfaces。
- 不因文件长拆模块；不新增旧目录名crate；不提交target/dist/node_modules/数据库/logs/真实样本/秘密/本地生成配置。
- 默认完整检查 `cargo test --workspace`、`npm test`、`npm run build`；桌面组合/Win32/资源另 `npm run build:desktop`；锁变更先 `npm ci`，纯cargo fmt修改独立commit。

## Review Focus

1. W03作业与W04内核检查同时进入RuntimeDialog：来源不同、只读，不初始化缺失内核；立即Escape只关闭子层（W03 Task4、W04 Task5）。
2. shelve/open/new与草稿、示例识别交叉：收起保留文字，精确打开仍读当前visibility，示例始终先识别marker而零Agent初始化（W01 Task2–3、W07 Task4、W12 Task4）。
3. 共享Windows读取与hash scope：W02 prefix不能变成W10/W11 full；reference-only/敏感内容不会因入口不同而持久化（W02 Task1–3、W10 Task2、W11 Task2）。
4. 压缩后旧工具来源与稿件/诊断：手动压缩不改科学状态/原事件，完整final replay准入、sequence/hash引用仍可取回（W05 Task2–4、W06 Task2/4、W09 Task1–2）。
5. Python discovery与V4 suite报告共享CI：W04新测试自动发现，V4失败非零但保留完整安全报告，默认cargo零写产物，旧required checks均保留（W08两份计划Task3）。

---

## 子计划与功能完成条件

每份子计划有独立文件地图、准确接口、五项Review Focus、测试断言和RED→实现→GREEN→commit。这里负责顺序与整合，不重复子计划实现内容。

| 功能 | 计划 | 任务数 | 交付可观察行为 | 当前状态 |
| --- | --- | --- | --- | --- |
| W01 | [文字草稿](2026-10-04-wisp-w01-composer-drafts.md) | 3 | 跨项目/会话恢复文字，旧回执只清理原revision | 已实现、3任务审阅通过 |
| W02 | [表格预览](2026-10-04-wisp-w02-table-preview.md) | 4 | 本地/SSH CSV/TSV有界逻辑记录、来源/hash/truncation | 已实现、4任务审阅通过 |
| W03 | [远端作业](2026-10-04-wisp-w03-remote-jobs.md) | 4 | metadata列表、手动status与明确启用的有限退避 | 已实现、4任务审阅通过 |
| W04 | [内核观察](2026-10-04-wisp-w04-kernel-inspection.md) | 5 | 活着的Python/R metadata，不隐式启动或执行对象方法 | 已实现，5任务审阅通过；真实R/SSH/GUI未验收 |
| W05 | [手动压缩](2026-10-04-wisp-w05-context-compaction.md) | 5 | 暂停ordinary run真正原子保存可恢复checkpoint | 进行中，1/5任务审阅通过 |
| W06 | [运行诊断](2026-10-04-wisp-w06-run-diagnostics.md) | 4 | run/tool/失败筛选、精确来源定位、脱敏HTML/JSON | 待实施 |
| W07 | [离线示例](2026-10-04-wisp-w07-offline-example.md) | 4 | 双语合成工作区，重启/坏receipt仍隔离于生产Agent | 待实施 |
| W08a | [V4场景](2026-10-04-wisp-w08-scenarios.md) | 3 | 五场景manifest/hash/实际调用计数和安全报告 | 待实施 |
| W08b | [Skill helpers](2026-10-04-wisp-w08-skill-helpers.md) | 3 | 固定本地helper离线行为回归与统一Python CI | 已实现、3任务审阅通过；CI未执行 |
| W09 | [稿件证据](2026-10-04-wisp-w09-publication-evidence.md) | 4 | 保存revision准备度与reference-only四entry证据ZIP | 待实施 |
| W10 | [文件基线](2026-10-04-wisp-w10-file-baselines.md) | 4 | 显式安全小本地文本immutable snapshot/有界diff | 待实施 |
| W11 | [迁移预览](2026-10-04-wisp-w11-migration-preview.md) | 3 | 空默认选择、类别COUNT、文件状态/hash与rebind/omissions | 待实施 |
| W12 | [会话可见性](2026-10-04-wisp-w12-conversation-visibility.md) | 4 | 持久可逆收起/恢复，分页前过滤，精确引用保持 | 待实施 |

W08只有两部分都通过才算完成。W02/W04两种worker/source的deterministic协议回归均必需，单独本地入口不满足范围；真实远端/模型验收独立记“未执行”时不冒充端到端通过。

## 唯一共享接缝和文件所有者

| 接缝 | 生产者 | 消费者/整合规则 |
| --- | --- | --- |
| `src-tauri/src/project_file_read.rs` 的 ValidatedProjectFile/metadata/read_prefix/read_complete | W02 Task1 | W02 Task3、W07 Task3、W10 Task2–3、W11 Task2；不复制Win32/path框架 |
| `omicsops-dto/src/workspace_files.rs` 四基础类型 | W09 Task1 | W10 Task1扩展baseline/diff，W11消费；W02使用独立project_table DTO |
| `workspace_file_content.rs` 的 inspect_small_workspace_text | W10 Task2 | W11 Task2，仅纯UTF8/extension/敏感判断；不依赖baseline表/blob |
| `RuntimeDialog/WorkspaceShell` | 主实施者按W03→W04顺序 | 子作业层和内核观察各自生命周期、同window堆栈；其它UI入口随后串行 |
| `DesktopApp/ProjectLibrary` | 主实施者按W01→W07→W12→W11顺序 | draft store、统一识别示例、visibility直接getter、卡片迁移dialog不互相覆盖 |
| `lib.rs/types.ts/tauri-api/dto_contract_tests` | 当前任务拥有其新增字段/命令 | 同时只一个任务编辑注册/重导出；宿主不另复制Rust DTO |
| `.github/workflows/ci.yml` | W08b Task3先Python；W08a Task3后scenario/upload | 保留npm ci/workspace/npm/build；W04测试自动discovery，不重复Python步骤 |

W07只需W02 Task1叶子，不需要table/SSH界面；W10/W11只需W09 Task1的基础契约，不需W09 UI。为简化当前shared checkout，下面默认完成整个前项后再进入后项。W12在W11前落地，使后者“包括已收起会话”的集成测试使用真实schema，而非假设未来表。

## 实施顺序与记录

审阅批准本总计划时，选定执行方法一次；实施中不逐项再向用户询问已授权功能。按所选执行技能执行各任务与review，不在共享入口同时派发多个写任务；可以并行做不冲突的只读查证与验证。

- [x] 1. W01 Task1–3：版本草稿、应用导航、全发送回执。
- [x] 2. W02 Task1–4：共享handle、parser/DTO、source/command、真实表格UI。
- [x] 3. W03 Task1–4：metadata、身份status、退避panel、Runtime入口/边界说明。
- [x] 4. W08b Task1–3：helper正确行为基线、错误传播、统一Python CI。
- [x] 5. W04 Task1–5：非启动contract、两套worker、weak registry、Runtime观察UI。
- [ ] 6. W05 Task1–5：旧hash兼容、candidate/final准入、atomicStore、宿主租约、UI/恢复。
- [ ] 7. W06 Task1–4：validated readonlychain、safeprojection、export、筛选/定位UI。
- [ ] 8. W07 Task1–4：marker事务、合成fixture、safeview、所有项目选择入口。
- [ ] 9. W08a Task1–3：五scenario manifest、控制行为、stable report/CI upload。
- [ ] 10. W09 Task1–4：基础DTO/context、readiness、post-dialog包、保存revisionUI。
- [ ] 11. W10 Task1–4：immutableStore、safecontent/blob、diff、explicit选择UI。
- [ ] 12. W12 Task1–4：visibility/schema/CAS、discovery/directget、collection、banner/cache。
- [ ] 13. W11 Task1–3：只读metadata、boundedfiles/digest、project-card预览。
- [ ] 14. 全分支独立审阅和最终交付验证，修复任何未解决的范围/行为问题。

完成任务后才勾选对应子计划checkbox；记录提交、RED实际失败原因、GREEN实际数量、完整检查与手工/真实验收状态。更新此表及原清单的实现状态，不把计划存在或旧基线PASS记为功能完成。失败先查根因和对应spec，修复当前功能所需最小边界；范围外云服务/授权/真实主机信息不足不阻止确定性实现。

## 最终验证与交付

逐任务先跑最接近检查；最终全分支重新执行以下命令，保留真实exit/count和ignored，不能只引用最早旧修复基线。

```powershell
cargo fmt --all -- --check
cargo test --workspace
python -B -m unittest discover -s scripts -p "test_*.py"
npm test
npm run build
npm run build:desktop
git diff --check
```

锁变化额外 `npm ci`。如果fmt仅格式偏差，`cargo fmt --all`后的纯格式变化单独提交。W08a另按子计划设 `OMICSOPS_EVAL_REPORT_PATH`，检查五场景JSON及故障非零，产物只在target；不得把故障fixture提交。

Windows合成数据smoke覆盖：导航/延迟发送、CSV/BOM/截断、jobs默认手动/stop本地查询、未启动kernel的观察、pause compact保留问题/旧来源、诊断定位/保存取消、无凭据示例重启/丢receipt、历史稿件包校验、小文件baseline/diff、shelve/restore/旧引用、卡片迁移空选择。真实模型/SSH/R等仅按 `acceptance/README.md` 一次性环境显式ignored验收；未执行就记录未执行。CI未实际运行时说本地通过/CI未执行。

最终交付列出12项实际行为、提交/检查证据、Windows手工与真实验收状态及明确后续。没有未解决review问题且全部首次范围实际满足后才能报告功能完成；不创建发布tag、Release或分发安装包。

## Plan Self-Review and Handoff

总设计12项各有计划，W08两部分共覆盖而非替代；50任务各有测试cycle，共享类型/读取/CI职责只有一个生产者。五项交叉Review Focus已绑定已有子计划测试；依赖顺序不引用未来未定义接口；计划正文主要是签名/断言/决策，无实现body或占位决策。当前文档检查与实施检查分开记录。

2026-10-06检查工作区时另有未提交的local runtime failure recovery/reviewer regeneration改动，涉及core、agent_v4、acceptance及其独立设计/测试；不是本计划任务的实现。它们保持原样，计划提交只stage此组Wisp文档。开始实施前重新读取这些共享文件的最新状态，保留新增错误/科学状态行为；尤其W05纯prepare/W06只读诊断/W09只读报告不调用会进行reconciliation写入的science snapshot路径。本计划源基线是身份参考，不作为恢复旧文件内容的指令。

用户已选 **Subagent-driven**：12项跨文件读取、冻结运行、原子压缩和持久化，每任务由子Agent实施及独立审阅，最后全分支审阅。继续全部12项，不重复范围或执行方式确认。

W01已实现：`afb1af3`/`ff8595b` 存储与Hook，`ae439c7`/`6312f60` 导航和删除清理，`9295ab2`/`bb3bbc7` 回执及真实调用方接受语义。3个任务均完成独立审阅及必要复审，无未解决发现。最终 `npm test` 为114文件/1,138前端与22扩展测试通过，`npm run build`通过（既有大chunk提示）；Rust/desktop及全分支最终审阅仍待后续整合。真实模型/SSH与Windows native smoke未执行。
