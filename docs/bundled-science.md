# 内置科学 Skills 与 MCP

来源为 [wisp-science](https://github.com/xuzhougeng/wisp-science)，固定提交
`3628a4209e494ba6fbef1095bb964782f7d2c430`，获取日期 2026-09-11。
此快照捆绑 26 个 Skill 包，以及 23 个实际原生科学工具域。它们随 Windows
桌面资源/可执行文件一起提供，无需克隆上游或安装 Python MCP 服务。

## 操作

对话左下角显示 `N skills · N MCP · N mem`。Skills 数量包含 Agent 可发现的
已启用包及其依赖；MCP 按已启用且有可发现工具的服务统计，不按工具总数统计；
Memory 是当前本地项目 `.omicsops/memory/` 目录第一层的 `.md` 文件数量，
不包含聊天消息、产物记录和研究笔记。目录不存在时为 0；读取失败显示错误。读取不创建目录，首次保存创建
目录。SSH 项目也统计本地项目目录，不查询远端。Skills/MCP 配置由应用共享，
数量不表示这些内容已全部装入本轮提示词。点击 Capabilities 可查看已安装、
未启用状态和共享范围，并进入管理；Settings（设置）打开原有设置。

切换对话、改变配置和新增消息/运行结束后会刷新数量；加载失败显示占位并允许
重试，不展示另一对话的旧数值。查看面板不会启动 MCP、联网或授予调用权限。

1. 打开设置的 Skills 与 MCP 页面。原有 9 个单细胞包继续保留，新包位于
   `wisp_science` 分类；默认启用 13 个科研技能，其他 13 个可手动启用。
   首次安装后的用户启停选择在重启后保留。
2. Agent 用 `search_skills` 找到已启用技能，再以返回的 UUID 调用 `use_skill`。
   默认结果包含指南、OmicsOps 兼容性说明和资源目录。读取脚本时传入例如
   `sections: ["Resource: runtime.py"]`，使用目录中的精确名称。
   脚本原文与参考资料完整返回并参与冻结哈希；单次最多 512 KiB，超限报错，
   不截断代码。修改过的已安装包必须重新导入，才能以新哈希使用。
3. 启动时自动注册全部 23 个内置科学 MCP，并从编译目录加载工具定义；新增服务默认启用，可被 Agent 检索。已有启停选择、自定义配置和凭据引用保留。旧版内置声明缺少工具目录时补齐目录，不自动开启被停用的服务。
4. 自动导入不启动进程、不联网，也不授予启动或工具执行权限。实际调用前仍需批准检查及调用；设置中保留添加入口，用于恢复缺失配置。原有 PubMed MCP 配置保留，因此全部默认启用时可能显示 24 个服务（23 个内置 + 1 个原有服务）。

## Skill 清单

默认启用：`analysis-workflow`、`audit-biomedical-paper-evidence`、
`figure-composer`、`figure-duplicate-audit`、`figure-style`、
`indication-dossier`、`journal-club-ppt`、`literature-review`、`paper-narrative`、
`pdf-explore`、`public-data-access`、`singlecell-qc`、`social-note`。

默认关闭：`agent-infini`、`browser-use`、`compute-env-setup`、`custom-theme`、
`customize`、`distill-concept-books`、`local-env-setup`、`pixi-environment-builder`、
`probe-compute-environment`、`remote-compute-ssh`、`self-awareness`、
`skill-creator`、`word-zotero-citations`。

包保留原始内容及所有附件；包外的宿主说明限定实际支持的操作。
Wisp 专有 `configure`、specialist/theme 导入、`.wisp` 发现机制等并没有因此实现。
Word/Zotero、InfiniSynapse/scimaster 及科研 Python/R 库仍是可选外部依赖。
加载 Skill 不安装这些依赖，也不执行 sidecar。需要的脚本须通过已有批准的文件
写入/运行工具在当前项目下准备，或显式载入持久内核；不能把本地 Skill 路径
当作 SSH 主机上的路径。

## 科学 MCP 域

| 域 | 主要数据源或操作 |
| --- | --- |
| biomart | Ensembl BioMart 注释、标识符转换 |
| biorxiv | bioRxiv、medRxiv 预印本 |
| cancer-models | cBioPortal、Cell Model Passports |
| cellguide | CELLxGENE CellGuide |
| chembl | ChEMBL 化合物、靶点、活性 |
| chemistry | PubChem、ChEBI、Rhea、BindingDB |
| clinical-genomics | CIViC、ClinGen、Open Targets |
| clinical-trials | ClinicalTrials.gov |
| drug-regulatory | openFDA |
| expression | GTEx、PanglaoDB |
| genes-ontologies | MyGene、UniProt、OLS、QuickGO、Reactome、KEGG |
| genomes | Ensembl、UCSC |
| human-genetics | GWAS Catalog、eQTL Catalogue、PheWeb |
| literature | OpenAlex、arXiv |
| omics-archives | GEO、ArrayExpress、MetaboLights、MGnify、PRIDE |
| protein-annotation | InterPro/Pfam、Human Protein Atlas、STRING |
| pubmed | NCBI、Europe PMC、开放全文与访问元数据 |
| regulation | ENCODE、JASPAR、UniBind |
| research-resources | Antibody Registry、Grants.gov |
| rna | Rfam 注释、比对与序列检索 |
| structures-interactions | PDB、AlphaFold、EMDB、Complex Portal、IntAct |
| variants | CADD、gnomAD、ClinVar、dbSNP |
| zinc | ZINC、SmallWorld、三维分区引用 |

设置中的工具数量来自已编译客户端的实际目录。每个进程只暴露和执行所选域的
工具；跨域名称、未知工具及不合法参数被拒绝。查询结果保留结构化来源信息，
错误保留 MCP `isError` 语义。RNA 序列检索和 ZINC 作业提交不声明为只读；
HTTP POST 失败不会自动重试，避免重复派发。

## 执行、数据和凭据边界

内置客户端仍需网络；外部数据库会收到查询词、标识符或显式提交的序列。
引用下载地址不等于数据已下载，也不等于实际分析已运行。默认保留大数据远端
引用；现有本地 system 与 SSH system/Micromamba 约束不变。

可选环境凭据只从固定白名单读取：`NCBI_API_KEY`、`NCBI_EMAIL`、
`NCBI_ADMIN_EMAIL`、`OPERON_CONTACT_EMAIL`、`OPENALEX_API_KEY`、
`OPENFDA_API_KEY`、`CIVIC_API_KEY`。MCP 配置仍使用现有 keyring 引用机制；
不要把密钥填入普通环境变量值或写入 Skill 示例中的配置文件。未传凭据时使用
上游匿名请求路径，服务限制、速率限制或付费要求由各数据库决定。

## 来源与许可

Skills 的原始文件哈希见 `skills/wisp-science/SOURCE.json`，原许可与第三方
声明随包分发。`crates/omicsops-bio/UPSTREAM.md` 记录原生实现的来源与适配。
用户明确同意直接引入后的 AGPL-3.0-only 组合许可，根目录 `LICENSE` 和 Cargo
许可声明已更新。单独声明 Apache/MIT 等许可的组件继续保留原声明；本次调整
不撤销此前版本已授予的许可。

上游还捆绑 Python/R kernel worker、浏览器扩展、ESR1/RNA-seq 示例与种子清单；
另有不属于应用捆绑目录的 community-skills。本次仅列出，未导入这些内容。

## 手工 smoke 与验收边界

在一次性 Windows 项目中确认所有 Skill 可见，关闭一个默认 Skill 后重启仍关闭；
启用带资源的 Skill，确认可以读取指定资源，加载本身不执行代码。
添加 `omics-archives` 和 `pubmed`，重复添加不增加记录；批准检查后只发现各自域
工具，未启用/未授权时 Agent 无法调用。配置可选凭据时确认仅保存 keyring 引用。
在受控联网条件下分别检索一个公开标识符并核对返回原始来源；SSH 场景额外确认
脚本仅在所选远端执行，且没有引用本地应用数据路径。

自动测试使用临时目录、临时 SQLite、模拟 HTTP/Tauri 和本地 MCP 进程，
不依赖真实 SSH、模型或公共数据库。构建与工具发现成功不代表所有公网端点、
可选依赖或科研工作流已通过端到端验收。实际检查结果记录在实施计划末尾。

## 自动回归的证据范围

从仓库根目录运行三个已捆绑 helper 的近端检查，再运行统一 Python discovery：

```text
python -B -m unittest discover -s scripts -p "test_bundled_skill_helpers.py" -v
python -B -m unittest discover -s scripts -p "test_*.py" -v
```

这些标准库测试只加载固定的 `paper-narrative`、`literature-review` 和
`figure-style` 源文件，隔离环境与 mock，检查 claim/path 配对、检索失败与零结果
的区别、DOI unknown、错误脱敏和加载副作用。图形替身只验证 `panel_crops`
返回的像素坐标，不安装 matplotlib/Pillow，不裁切或写入图片；不使用真实凭据、
网络、SSH、GPU 或 R。三组行为测试必须均可发现且非空，正常失败通过 unittest
结果与退出码传播。

| 检查 | PASS 能证明的范围 |
| --- | --- |
| bundle/hash | 捆绑资源完整、来源哈希与冻结内容匹配，不证明脚本可执行。 |
| helper unittest | 三个固定 helper 在受控输入与替身下的行为，不证明全部 26 个 Skills 或科学依赖可用。 |
| scripted V4 | 已执行的脚本模型/工具 fixture 所覆盖的运行时契约，不证明真实 provider 或科学结论。 |
| live acceptance | 仅实际显式执行的真实模型、SSH 或科研流程；未执行/被忽略不算通过。 |

本 helper 增量的本地测试通过与 GitHub CI 实际通过分别记录；CI 配置为 Windows、
Python 3.12 与 Node 22，修改配置本身不是 CI 运行证据。helper 输出不汇入 V4
模型调用计数或 suite hash。W08 的确定性 V4 场景 manifest、runner 和报告已交付，
使用与证据边界见[场景说明](testing/v4-scenarios.md)。本增量未执行真实模型、SSH、R、GPU、PBMC 或原生 UI 验收，
也不修改 Skills 源码、SOURCE pin、哈希或冻结规则。真实验收另见
[acceptance/README.md](../acceptance/README.md)。

## 离线合成示例 / Offline synthetic example

项目库的“离线示例 / Offline example”是独立的本地创建入口。选择已有本地父目录后，
应用创建唯一子目录，保存普通 Blank/Ready 项目和来源 marker；无需凭据、网络、
SSH、Python 或 R。唯一例子为 `synthetic-evidence-report-v1`，由本仓库原创，
采用 `AGPL-3.0-only`，附带原许可正文。它与上游 Skills、helper 和真实科学工作流
是独立的证据范围，不导入上游 transcript、thinking 或真实样本。

The Project Library's **Offline example** action creates a separate local child
folder under an existing parent. The original `synthetic-evidence-report-v1`
fixture uses `AGPL-3.0-only` and requires no credentials, network, model, SSH,
Python or R. Creation records source identity; viewing does not execute a workflow.

专用双语视图展示目标、合成表、预置结果与报告四段，以及七个文本文件：
`data/observations.csv`、`results/summary.csv`、`report.zh.md`、`report.en.md`、
`README.md`、`SOURCE.json`、`LICENSE.txt`。A 为 2、4，B 为 5、7；预置均值为
3 和 6、差为 3，未在本机执行，不能作为统计显著性或复现成功的证据。
SHA-256 只用于比较文件与预置内容的一致性，不证明科研结论。

The four sections and seven text files show synthetic observations A = 2, 4 and
B = 5, 7, preset means 3 and 6, and difference 3. These are already-written
arithmetic examples, not locally executed results or evidence of significance.
Hashes indicate file equality only.

启动、创建后、项目库和精确来源/用量跳转先查询 Store marker。已有 marker 时，即使
收据删除、损坏或目录不可用，也保留示例身份并显示不可用，不创建生产会话或派发
Agent/compute/SSH。身份查询失败阻断候选打开。普通当前项目可在可取消的来源
查询中继续显示，候选不会提前初始化；其先前授权工作可能继续。
全局设置只读取本地目录元数据。模型、MCP 和实际工作流仍可能传输信息，不能把
这一离线例子的无网络要求推广成产品的全部数据驻留保证。

Marker identification precedes candidate workspace initialization. Missing or
invalid receipts retain the synthetic identity with an unavailable view. Identity
errors never fall back to production initialization. A previously identified
ordinary workspace may remain visible during cancellable navigation; its earlier
authorized work may continue.

文件预览显示有界、脱敏的当前文本，不解析 HTML/Markdown，不加载远端图片、
链接、脚本、iframe 或字体。changed 明示为当前用户文本；missing/unavailable
禁用预览，不补写或覆盖。创建登记失败可在同面板重试，保持同一 request UUID；
关闭后重开是新尝试。创建与重启/文件状态的 Windows 原生 smoke 和真实科学验收
分别记录于 [acceptance/README.md](../acceptance/README.md)，模拟 UI 与构建不算通过。

Previews render bounded, redacted current text literally, including HTML and
Markdown syntax, without loading remote resources. Changed files are labelled as
current user text; missing/unavailable files cannot be previewed and are never
repaired automatically. A retry within one creation attempt retains its request
UUID; reopening the creation panel starts a new attempt. Mock tests and builds do
not establish native smoke or real scientific acceptance.

## 项目文件记忆

保存、检索、计数统一使用 `<项目目录>/.omicsops/memory/` 第一层 UTF-8 Markdown
文件；`.wisp/memory/` 不再读取。`save_memory` 接受简单的 `.md` 文件名和正文，
首次保存创建目录，原子写入，不覆盖已有文件，每个文件最多 256 KiB。
这是修改型工具，沿用宿主审批，Plan 模式不可执行；SSH 对话同样写本地项目目录。
`search_memory` 和界面记忆检索读取同一目录，维度为 `memory`，来源标为
`memory_file`。普通读取不创建目录，不联网，不将记忆内容认定为已验证科研证据。
手工编辑/删除文件后，下次检索及计数刷新反映磁盘状态。
手工放入的文件同样必须为 UTF-8 且不超过 256 KiB。任何文件读取失败时，检索
会报告具体文件名而非静默返回不完整结果；修复或移走该文件后重试。

旧消息、产物、研究笔记保留在数据库，不再作为 Memory 搜索结果，不自动转换为
记忆文件。旧 `.wisp/memory` 内容也不自动搬移；需要保留的笔记可放入新目录。
读取/保存使用现有常见凭据模式脱敏；不得向记忆文件提供 API key、密码或私钥。
