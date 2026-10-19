# Wisp 首次范围工程交付记录 — 2026-10-19

当前源码：`c9a5338d70ad14207a7a58eb7d37793498e9b840`；tree `af173b7086f34cd4d5c29cb8ccf03fda0abe495e`。12 项首次范围、50 个任务的 FULL/必要 SCOPED 链已关闭。根协调者在 global progress 第378行接受最终源码整合及其确定性证据，保留以下限制。本文的协调记录截止该行；文档审计、暂存与提交的实际结果独立记录；本段不预先宣称它们完成，也不替代实际验证。

[完整原始裁决、理由与成本](2026-10-19-wisp-rulings.md) · [证据、精确指纹与位置索引](2026-10-19-wisp-evidence-index.md) · [总实施计划](../plans/2026-10-04-wisp-improvements.md)

## 当前结果与证据边界

|范围|实际结果|相位与限制|
|---|---|---|
|源码任务|50/50 独立 FULL/必要 SCOPED 关闭；SOURCE50 文档节点 bcec075|不等于原生或真实科研端到端验收|
|最强整分支审阅|原 gpt-6-astra ultra 读完55,473行/173路径，保存原报告后中断；另一 fresh ultra 完成实际定向读取及缺失尾部|两位读者的覆盖分别归属；frozen bcec075 原始 With fixes/OPEN 判定保留|
|首次最终修复|source54196dce、纯格式a993efa：WB-I1/I2/原I3、M1及M5标题修订经 scoped 判定 ADDRESSED|该轮引入 NB-I1；初始 scoped OPEN 报告仍按其 SHA 保存|
|有界后续修复|实际 c9a5338 五路径；targeted review 判 NB-I1 ADDRESSED、无新增 actionable Critical/Important/Minor；根协调者接受|完整初步diff→实际提交字节桥，不把预提交读取说成未来批准；不是第二次整分支审阅|
|源码与证据审计|a993 frozen1300项、c9有界3653项独立断言均0失败；记录基线2123项0失败|有界/cached/记录环境子集和终态送达归属限制明确；本文不冒充其源码审计，也不声称最终文档审计已通过|
|最新正常前端|128文件、1441 Vitest +22 Node，通过；final5 manifest42cedda…72d10|两个1440/1失败、诊断1441通过以及早期1439/1均各自保留，不累加、不推断原因|
|最新 Rust/fmt/Web|1938通过/18ignored、90 summaries；fmt0、standaloneWeb0|在第五个 fixture 编辑前执行。1120其他RAW及3dist内容桥保持；native beforeBuild tsc/Vite验证 final5，而不是谎称Rust/Web在第五编辑后重跑|
|最终原生|final5 native terminal0，release/NSIS BUILD ONLY|现有缓存、483顶层dep-info边界；没有 launch/install、完整extern重建或原生键盘验收|
|较早工作树/干净归档|SOURCE50工作树1935/18明确 ADOPTED PRE-REVIEW；bcec075干净归档1916/17、实际npm ci/Python48/W08故障恢复|旧归档仅是当时精确提交；没有新的clean-latestHEAD、CI或冷缓存保证|
|保留输入|Python48、npmci304added/305audited、W08 normal5→fault4/1非零→restored5|262相关输入桥、tar1118/manifests/sidecars/logs/proof snapshots支持带日期证据；原执行目录与3原输出在审计时缺失，未推断谁、何时或为何移除|

## 明确保留的残余与未执行项

WB-M2 producer cursor>10000、M3发表证据来源身份展示、M4压缩JSX、M6安全fixture身份重复、M7警告/性能、M8预取与总量资源政策，以及 M5真实deferred-diff/分页回归债务仍开放。N01自动压缩归档后的非原子部分效果/无rollback限制仍在。带日期 npm audit 的12个受影响包（4low/3moderate/3high/2critical；8dev/4production-low）未解决；不是12条独立advisory，也未完成可达性或运行时利用性审计。

早期 PublicationEvidence 等待子层的1439/1、后续 revision_changed/raw 的两次1440/1以及历史 Settings Escape 失败仍为 UNKNOWN。第五路径仅加强fixture异步就绪契约；后续成功与插桩运行没有证明历史失败原因、无害性或生产race已修复。

GUI physical STOP 自10月11日保持。旧W06部分smoke、先前尝试文件面板且返回 QUEUED/实际显示未核实的事件均如实保存；没有把整段历史说成无人调用UI工具。原生手工、安装器、真实模型/SSH/R/Micromamba/PBMC/GPU/SLURM及实际CI Node22/Python3.12未执行；本地Node24/Python3.10、mocks/build/ignored不是这些验收。没有merge/push/release/tag/分发/清理授权。

七个外部dirty/untracked路径与两个working-only initializer被保留。较早acceptance RAW归一化缺陷在测试后经817条unowned行/18条外部行精确恢复；不是所有阶段七个整文件RAW从未变化，也不是恢复后又跑了测试。当前有界五路径阶段的七个RAW则保持不变。所有相位、成本和原始证明见下列历史材料及索引。

## 历史任务、验证与决策材料

下文沿用已独立审计的历史结构和原始报告。各阶段曾写 OPEN、pending 或 candidate 的语句只描述当时冻结的源码/准备快照；当前结论以上述源码、独立 targeted/provenance结果及根第378行裁决为准。原报告不被改写成批准后来的代码。所有原始理由、成本、失败和读者归属保留。

## 用户可见的十二项范围与当前源码门禁

| Feature | 独立审阅 SOURCE | 可观察行为与必须保留的界限 |
| --- | ---: | --- |
| W01 对话草稿 | 3/3 | 版本所有权、明确 branch 目标、ACK 后导航/重试；原生 GUI/真实模型未验。 |
| W02 表格预览 | 4/4 | 本地/SSH 有界读取、原 CSV/TSV、引用和公开单元投影；真实 SSH/GUI 未验。 |
| W03 远端作业 | 4/4 | 持久元数据、显式查询、离线收据与窗口有限轮询；断连不表示远端取消，真实 SSH 未验。 |
| W04 内核检查 | 5/5 | 本地/SSH 只读有界检查、非启动注册与 UI；实际 R/macOS/SSH 活体验收未验。 |
| W05 上下文压缩 | 5/5 | prepare、手工 CAS commit、host 承认及窗口内重试；自动路径 N01 非原子残余仍在。 |
| W06 运行诊断 | 4/4 | 有界诊断、验证原事件链、固定 HTML/JSON 导出、UI；旧 HEAD 隔离 native 仅局部通过。 |
| W07 离线示例 | 4/4 | Windows 安全创建/收据/marker 与惰性只读 UI；原生创建/重启/收据与零派发未验，Linux/macOS 创建不可用。 |
| W08 Skills 辅助与场景 | 6/6 | 固定 helper/discovery、五情景解析/hash、报告与故障路径；“签名”只是事件 hash 链，本地 Python 3.10 不等于 CI 3.12。 |
| W09 发表证据 | 4/4 | 精确已保存 revision 的只读上下文/readiness/ZIP/UI；`original_file_status=not_checked`、`reproduction_status=not_executed`，无真实科学复现。 |
| W10 文件基线 | 4/4 | immutable manifest、受限捕获/blob、list/get/diff 与选择 UI；多文件顺序观察非原子快照，真实 SSH/native GUI 未验。 |
| W12 对话可见性 | **4/4** | Store/DTO/native/集合与导航/default/active/overlay源码任务已审；最终fix有374 closest/Web，早前full npm/native为pre-fix，GUI未验。 |
| W11 迁移预览 | **3/3** | Store/DTO、native bounded files/digest及显式生成/stale/generation/catalog/Escape消费者均独立关闭；只读preview，无实际迁移，GUI/真实服务未验。 |

`Project`、执行位置、来源、运行和证据边界分别由相关原始报告约束；此表只表示已审源码，不是十二功能的端到端完成率。生产者的 FULL/scoped 源码评审、前端消费者评审、文档同步、构建、手工使用和真实科研流程是不同证据。

## 任务矩阵：历史 1–40

下表逐行采用 `delivery-task-matrix-audit-oct14.md` 核对过的完整源码终点、fix/format 祖先、精确独立 FULL/scoped review 文件名与原有测试相位。路径键：`W01`–`W07`、`W09`、`W10` 指 `.superpowers/sdd/2026-10-04-wisp-wxx-<slug>/`，slug 分别为 `composer-drafts`、`table-preview`、`remote-jobs`、`kernel-inspection`、`context-compaction`、`run-diagnostics`、`offline-example`、`publication-evidence`、`file-baselines`；`W08h` 是 `2026-10-04-wisp-w08-skill-helpers/`，`W08s` 是 `2026-10-04-wisp-w08-scenarios/`。每行 `R` 为 `task-n-report.md`，`F` 为 `task-n-review.md`，`S` 为列明的独立 scoped rereview；各目录 `progress.md` 为完成 ledger。初次 FULL 的 Needs fixes 只能与其后 `S` 一起解释为已关闭。数字是**历史最近有效相位**，并非本稿重跑、互不重叠的总数或当前 HEAD gate。

| # | Task | Verified final source SHA | Initial / intermediate source SHA and primary review paths | Historical count and phase supported by primary records |
|---:|---|---|---|---|
| 1 | W01 T1 | `ff8595b35d35c6a1a16026e2fe6e4600403ccc53` | `afb1af334f4f443d477169b3a005e97796efb6f3`; R,F,S=`task-1-rereview.md` | Final focused 19; TypeScript pass. R fix section / S. |
| 2 | W01 T2 | `6312f604abd91d6688ede4022048b0cb36feb059` | `ae439c75b02f45968c09a6a527129577c9dd7863`; R,F,S=`task-2-rereview.md` | Final focused 261; TypeScript pass. R fix section / S. |
| 3 | W01 T3 | `bb3bbc7bb6fb5057e935a3c1083cdf33ee5611c7` | `9295ab251d8fdb721251b7a17603c9efb3112f47`; R,F,S=`task-3-rereview.md` | Final focused 263, Vitest 1,138/114 files, extension 22, Web pass. R fix section / S. |
| 4 | W02 T1 | `7439276e586857ffa9cd66cd8a539e89a3f317b0` | `725814919353e51daa8a494fd89837664521d1d4`; R,F,S=`task-1-rereview.md` | Final reader 16, workspace 1,549/17 ignored, native build pass. R fix section / S. |
| 5 | W02 T2 | `69886e4de2f102c330821d9b99cba044c78af173` | Single source; R,F | Parser 12, DTO 3, workspace 1,564/17 ignored. R/F. |
| 6 | W02 T3 | `bec43f194debd02133c7c1a6f1d744dddbc1740e` | Single source; R,F | Host 11, adapter 3, reader 16. R/F; full W02 gates came in T4. |
| 7 | W02 T4 | `153a0eab50604b5f5cf57d934ce330bf4bdfbdc5` | `a8fa4a4df948ac9d3348b193ffae68bc166948a0`; R,F,S=`task-4-rereview.md` | Initial focused 167 + nearest 5; fix focused 162, final Vitest 1,171 + extension 22/Web. Rust 1,578/17 ignored and native build belong to pre-fix source. R/F/S and `progress.md:41–44`. |
| 8 | W03 T1 | `bad6feb0571b4b45950cf89a8c56b9533950b534` | `9d415c6c857df7072d0f61f889736f97e8a52665`; R,F,S=`task-1-rereview.md` | Store 6, runtime jobs 12, DTO 3 from earlier source; R fix / S. |
| 9 | W03 T2 | `6e7519d744d519ead5459c7fbd89b52f386c7900` | Single source; R,F | Service 14, remote 3 + 1 ignored live, Store 7, host 1. R/F and `progress.md:32–33`. |
| 10 | W03 T3 | `1aaafca0f0046141b04cc53492e71c37c7bde804` | Single source; R,F | Focused 33, typecheck/Web pass; error taxonomy Minor carried to T4, then closed. `progress.md:38–40`, R/F. |
| 11 | W03 T4 | `e8fe4ec00f37641e5d62e29a190de6a37e035f0f` | `0c4be6e09f8cbbb499a92a202d97870f1dfc4f3f`; R,F,S=`task-4-rereview.md` | Final workspace 1,613/17 ignored, Vitest 1,220 + extension 22, Web/native pass. R/F/S and `progress.md:45–51`. |
| 12 | W04 T1 | `a1a7467d4a1112ba1f32118c596431365ecb80f1` | Single source; R,F | Runtime 5, protocol 3, desktop DTO 51; R/F. |
| 13 | W04 T2 | `91964e34a9e8f47c74417b26161d5929a22faaf2` | `45dd6dbcf1d74bb39939eaa0728223b99a0de5f2`; R,F,S=`task-2-rereview.md` | Final local inspect 20, old backend 13; Python 4 predates fix, Unix behavior unverified. `progress.md:51–66`, R/F/S. |
| 14 | W04 T3 | `ef7da02b0caec200f781fa299db32395fdc0c50a` | `f644d0ceed3b9f570f203419126b0f53033815bd`; R,F,S=`task-3-rereview.md` | Final Python 10, local 20, adapter 5 unit + 3 integration; R 1 ignored (unexecuted). `progress.md:78–87`, R/F/S. |
| 15 | W04 T4 | `a0fbedcc4a81d04c13ded211510a4cb5547138ac` | Single source; R,F | Host 6, registry 2, Runtime 34, DTO 51; other focused filters in R. `progress.md:105–111`, R/F. |
| 16 | W04 T5 | `660984698a3cdd117f81d21f238313d161f29a72` | `db174a6d0f1fd6827f28065aaddf2c2d2e49f1a5`; R,F,S=`task-5-rereview.md` | Final UI 174 and Vitest 1,251/119 files + extension 22/Web. Workspace 1,665/18 ignored, Python 48 and native build predate UI fix. `progress.md:119–124`, R/F/S. |
| 17 | W05 T1 | `2a191c8430dbbd5b378d2d1aa583c42da75afc83` | `c125a71acae482c8701d293764f0a52ddca0853c`; R,F,S=`task-1-rereview.md` | Behavioral RED 0/2 → GREEN 2; protocol 33 unit + 10 integration and Store 6. R/F/S. |
| 18 | W05 T2 | `5559298f138ee91a00a9d7e853315127247106f8` | `5662c2159536fabd7bc26ec04064cddaa4198d45` → style `621ca99fdaee939d649765cb8871a8a31c267539` → fixes `8ffd45525a309ef3f92f58cee13f302b8a504374`, `76b3996c4de17945dd916a825bc63355ccfab723`, `277f7f83ab88a8c4b54822449e01b63358402e0a`, `5559298f138ee91a00a9d7e853315127247106f8`; R,F,S=`task-2-rereview.md`, `task-2-fix2-rereview.md`, `task-2-fix3-rereview.md`, `task-2-fix4-rereview.md` | Final core 222, protocol 43, manual 20/replay 21 subsets. Workspace 1,679/18 ignored, npm 1,251 + 22/Web are pre-fix evidence. `progress.md:37–77`, R/F/all S. |
| 19 | W05 T3 | `786e2d9283329e6ab64341f2ad8e301f398fa84e` | Single source; R,F | Final Store 306; unchanged core 223 and protocol 34+10 from prior affected gate. Do not sum repeated subsets. R/F. |
| 20 | W05 T4 | `f6d9f139bb7b0aa60e0b72a98a7e286c9a70a843` | Single source; R,F | Host 8, workspace 1,714/18 ignored, npm 1,251 + 22, Web/native pass. R/F. |
| 21 | W05 T5 | `e64f3949fa49f3c8aaaf51c0b56476c330b58009` | `1809c75123220601672084b60173ac7f899d3479`; R=`task-5-report.md` + `task-5-fix1-report.md`, F, S=`task-5-fix1-rereview.md` | Final affected UI 268, Vitest 1,265 + extension 22, Web/native pass. Workspace 1,715/18 ignored is prior unchanged Rust source; no fresh post-fix Rust run. `progress.md:121–131`, both R/F/S. |
| 22 | W06 T1 | `bb47a89db0ec64038e7264a94cfa574cb7a52b2f` | `82960785faeb275ac404abcc4441a191c57223b9` → style final; R,F | Store 3, desktop DTO 1; final source includes separate formatting commit. R/F, `progress.md:25`. |
| 23 | W06 T2 | `240abd43028d73690307ec219c5237d64471e83a` | `f51d3218255359daa29f238631035921715ff02e` → style `399129abfa13a6e089e90785aee1ec2875e0a1ab` → fix `566c1b7123595916227d6b44c5140906d97f7091` → style final; R,F,S=`task-2-fix1-rereview.md` | Final diagnostics 17, Store 3. First FULL rejected; scoped fix rereview passed at final style endpoint. `progress.md:33–35`, R/F/S. |
| 24 | W06 T3 | `d87f2252945ec8ac5bba46c79be37bf6ff4a294e` | `2a3b012180fc8f2a6643c4b6e87248f58a37a476` → style final; R,F | Diagnostics 22, old export 4 and postcommit export 5, Store 3; normal-access rerun resolved temp-save restricted failures. `progress.md:39–42`, R/F. |
| 25 | W06 T4 | `d535b5006104eeedc9c2c533b14f80249e131e8b` | Single source; R,F | Focused 163, workspace 1,741/18 ignored, Vitest 1,290 + extension 22, Web/native pass. Manual I1 remains unexecuted. `progress.md:44`, R/F. |
| 26 | W07 T1 | `26629bea4c31622d60cc49370168a9879b563c9c` | `751153847ba1cae912c0c21cdd91f1df1ef88fa6` → style final; R,F | Store 316, focused Store 7, DTO 1; style endpoint included in FULL review range. R/F. |
| 27 | W07 T2 | `b77c393fd2139a37b9cd11891a14a0eaebb3430d` | `228d1295fb33bcb64f6bbae7598b75f8a4e3e931`; R,F,S=`task-2-fix1-review.md` | Final focused 19. The earlier pinned W06 smoke HEAD is unrelated to this final W07 fix. R/F/S. |
| 28 | W07 T3 | `7818c02e5a4963df14dc431e134c839836a412a7` | `a32fe6a53ae084ea5559f13ef302c7cc488b74b7` → style final; R,F | View 9, create 19; R/F. |
| 29 | W07 T4 | `a7e05c93a38c05bec9830babf98f8910989762bc` | Single source; R,F | Final focused 157, workspace 1,777/18 ignored, Vitest 1,316 + extension 22, Web/native pass; no manual native pass. `progress.md:64–69`, R/F. |
| 30 | W08h T1 | `74e8fa4dd9dc1560f78a9757d24ec6713b089408` | Single source; R,F | Helper focused 14, discovery 23; local Python 3.10. R/F. |
| 31 | W08h T2 | `d6a71bd338324f9f8060fd1b7d486ee0196762d3` | Single source; R,F | Helper 27, snapshot 2; fixtures/no network. R/F. |
| 32 | W08h T3 | `edfb23fafddb94bdd9899734568232cccb68f4b7` | Single source; R,F | Helper 29, discovery 38; workspace 1,613/17 ignored, Vitest 1,220 + extension 22, Web. R/F. |
| 33 | W08s T1 | `a500df7f7022e24b795e5b2a1a1541f2f09e6b1e` | `a8edbb9942ad996d5c871c8ff25dd4279004815f`; R,F | Manifest 3, core 226. Five scenario executions belong to T2, not this parser/hash task. R/F. |
| 34 | W08s T2 | `9711ef6dbefe8e2cbc64ae80883498a2cddfa2c9` | Single source; R,F | Scenario 9, core 232. R/F. |
| 35 | W08s T3 | `1081b2830da3bf47baf0261d6132eae3ba5da9de` | Single source; R,F=`task-3-review.md` (complete artifact; final response capacity error) | After absent-root fix: core 238, scenario 15/report 5/redaction 1, Python 48, Vitest 1,316 + extension 22/Web. Workspace 1,791/18 ignored predates that fix. R/F and `delivery-record-delta-w08-complete.md`. |
| 36 | W09 T1 | `15d727a6ffe04d2e36c508a1004ac32f49cda910` | `9cfa558de7bc1b3608278e72c892d2432d87e115` → `537ab7154b683de2c14a932281794c093086ab23` → `ef6fb9c88d9afe37c6696d77b7998813aa31fef2` → final; R,F,S=`task-1-rereview-1.md, task-1-rereview-2.md, task-1-rereview-3.md` | Final context 13, navigation 18; DTO 1 is earlier unchanged source evidence. Three scoped rounds, all closed. `progress.md:24–33`, R/F/all S. |
| 37 | W09 T2 | `24073b47594c8fc12b60b8f5e8d7666584d7c7a6` | `e68451692e948ce50a630ca7a48468a3c1e4c22e`; R,F,S=`task-2-rereview-1.md` | Final readiness 13 after quoted-secret RED 3→GREEN 3. Host 11, Store context 14, navigation 18 are pre-fix unchanged-source runs. `progress.md:37–43`, R/F/S. |
| 38 | W09 T3 | `dcdd8c6f236a87f856e198bf8ae06157141a7e27` | `89ae5cc2146c07cb34ef82b2503004986020ac3d`; R,F,S=`task-3-rereview-1.md` | Final package 10, evidence 23; DTO 2 predates fix. RED package 8 pass/2 fail → GREEN 10. `progress.md:49–57`, R/F/S. |
| 39 | W09 T4 | `26490970515852d4a58698faddf291d23fbc5ff7` | Single source; R,F | Final focused/modal 32, Vitest 1,345/123 files + extension 22, Web/native build pass. Workspace 1,831/18 ignored and Python 48 predate final UI amendment on unchanged Rust/Python; build is no GUI/install proof. `progress.md:60–69`, R/F. |
| 40 | W10 T1 | `5085908b68dd46ba2f85ac8e638783a32d68fe50` | Single source; R,F | Final Store 9, branch 9, native DTO 1. First Store 6 log retained; intermediate 8 log overwritten by 9. Compile/OUT_DIR failures were 0 tests; no Task2–4 credit. `progress.md:27–32`, R/F. |

历史里程碑与源码分开。W01 Task 1 后 `70593468491e94e9fe383f060c74105db7ed0623` 仅确认 Task 1，`68540be673417f0864c7083630ce72c859052de9` 才关闭三个任务；没有被核实的独立 Task 2 文档提交。W02 `07920cc304befa5c28bdb3aec15d3b3b65e0c572` **联合**关闭 Task 1+2，`8b4a9fe` 是随后 host 边界澄清 BASE，不能编造成 Task 1 文档里程碑。其余历史文档索引：W02 T3 `567992263394831ef93ed658f903c5ddc96cc9d3`/T4 `edcb5591bae08293ee0eef1762c7fd6158d7b6f8`；W03 T1–4 `d19c5e06cf8660c54003e5eca9662881184837ba`,`f953b4337d257183019cacb9088b6c252f525e9a`,`0540404e1e89b0514c96823cf62633ec4d24c817`,`c573287fef5fedee8c7c3aacc4155e85a38053bf`；W04 T1–5 `c824fe01f5c53889e341b6981a74cfefce46b642`,`0e70daa59b3e624e353a9d5df7358aa312bf2dce`,`20fd8987ca66279e020c0a0fccfc70eac22a6f88`,`3e7dea30f8656e6662dc2c56bb41cf27ff6bb8f9`,`f8a79c7e4cda625aaa16800bd99f8dbc046db92c`；W05 T1–5 `6ca636560ae470082dea6dba8e555774a2a28389`,`ca52aa568cba0addd886fc70517e7406a4d856df`,`1daa8ddfa51ef18b5dad57cc5524d1a477c514cf`,`9b8523d709b8dbe38a9aa368cf565ef304e778ee`,`62e16096da4b57cfdc6410c6bcd7738fd897a236`；W06 T1–4 `38f40990a238455054c01c78c9bc2637e4a4a10e`,`df2c19b349e1c7a5816438e46526ba6932b7accd`,`85c8aead19d5e536393992f6a89cc07832238c46`,`f1218c71e0a04e7083347efdffd02b622c882afd`；W07 T1–4 `cb2a03f20109ff88678d81941a57c946a04c347d`,`e1aec8d28feee3d0959d230bd6791a76cafaab69`,`778df348d4acc8cbd796b466703ed91bb86e4ad3`,`c1f348eb9534b461bd51f9d98dcd2bce7f5c69f6`；W08h T1–3 `5761d732d039f311bbf0eae18c420bfb23499753`,`e60ddc461e27224976ba7b972e054c37d7876958`,`9a494c79f18ebff73e6d8d85d9a88dc7f543d8b0`；W08s T1–3 `6f05b71bf41f64827e5e82623b857deee14d2980`,`97265dc321dbdf1347ee0c4716695a2d8c5119c8`,`1b1909383540b146e53c82b403c835d8cd6d62dc`；W09 T1–4 `b00993fc67cf311f8d349281c8b85d9e69286a52`,`e0c9223eff58e6a38ae0f61d374637f5586951ed`,`38bfd114732225cfe729589eddf49fb72f7a3a98`,`f93878c566f366135f4af466de0a73468530a495`；W10 T1 `ff7e522d1baa2fee05b05dc627b2066c07e90b0c`。这些既有文档索引已通过只读git rev-parse --verify <known-id>^{commit}逐项无歧义展开；这证明对象身份，不重新审阅文档内容/源码门禁；`1b1909383540b146e53c82b403c835d8cd6d62dc` 尤其不是 W08s T3 源码终点。

## 任务矩阵：已关闭41–50

每行给出 BASE、源码/fix/format、**独立审阅**和文档；前一行文档 milestone 是下一行 BASE 时，仍不得把文档计作第二次源码门禁。相位数字只描述实际执行的命令、包含关系及保留日志；完整命令与 terminal exit 在各 delta/原报告。

| # / 进度 | Task / BASE | 源码终点与独立审阅 | 文档里程碑；实际相位和限制 |
| ---: | --- | --- | --- |
| 41 / SOURCE41 | W10 T2 / `ff7e522d1baa2fee05b05dc627b2066c07e90b0c` | 功能 `f07d9a3422336db3b6676a7b03b96811f2a8b363`（10 路径）→修复 `4a6eb140550ac7913e3545e004477c3e33f0e4fe`（4 路径）；`task-2-review.md` FULL **Needs fixes** I1–I3，`task-2-rereview1.md` scoped **ADDRESSED**，无新 blocker/parked。 | `631dfb51b1ce0ec3c6ced302f9723d5c73081507` 四文档。首版 focused **62/0/0**，fix 最近 capture17+inspector5+saved4+Store11=**37/0/0**；W02 handle16/table11/delete2 是旧相位，62+37 不可相加。初期 Store stub/binding/row RED 各 0/1；缺 API 编译 0 tests，SSH FK/手写 blob hash 夹具失败分列。 |
| 42 / SOURCE42 | W10 T3 / `631dfb51b1ce0ec3c6ced302f9723d5c73081507` | 功能 `783ee03fece61a5e08511b816e63965a94352f8c`（3 路径）→预算修复 `e82aed03bb31b01848b9f06e2fc3b6575b4a4032`（1 路径）；`task-3-review.md` FULL **Needs fixes** I1，`task-3-rereview1.md` scoped I1+C1 **ADDRESSED**。 | `5b4cc5a2d8516a0bfa6720a675c4a0b3b291f5cc` 四文档。初行为 RED diff0/6、metadata0/1；功能 baseline32（diff10 为子集）、DTO59旧相位；修复误用宽 filter 得14 pass/7 fail（六预算因果、一时间精度 fixture），最后 budget6 为 baseline**38/0/0** 子集；DTO59未重跑，EOF probe 与 payload allowance 分开。 |
| 43 / SOURCE43 | W10 T4 / `5b4cc5a2d8516a0bfa6720a675c4a0b3b291f5cc` | `19ac40d87f114f594c0e6b22b0320d0a5627f5f3`（11 前端路径）；独立 `task-4-review.md` FULL **Spec compliant / Approved**，0 Critical/Important，M1–M3 延后，无 scoped fix。 | `7e404c7b279ea9de9a07bb0a89bd450114eb218b` 四文档。行为 RED/夹具/沙箱分相，最终 closest **175** ⊂ npm Vitest **1,363/124 suites** + extension22，Web 2,047 modules；Rust**1,889/18 ignored**、Python48、fmt 是未变相关字节的早前运行。首个最后源码 native 有成功 stdout 但 exit **UNKNOWN**；确认无遗留构建进程后重跑 native exit **0**、NSIS 构建，未安装/启动。 |
| 44 / SOURCE44 | W12 T1 / `7e404c7b279ea9de9a07bb0a89bd450114eb218b` | 功能 `3809c2503c1c29db20a2c48fd7eefc4c954f5d4a`（7 路径）→独立格式 `8a474548bd8648d3a116ddd3f4ede02d4195b527`（1 Store 声明）→I1 修复 `e4faae67a9e6d899081e9b8dd4b47c1cdfcf9e42`（4 路径）；`task-1-review.md` FULL **Needs fixes**，`task-1-rereview1.md` scoped **ADDRESSED**。 | `1f2b1ce2b360ac6647ec3c546440e813e33be9bd` 四文档。初编译 67/8 diagnostics、0 tests；实现后 mutation assertion 1 fail 是**后置**敏感性试验，不是原 RED；原聚焦29 与修复后 Store14+core4+native19=**37** 重叠。I1 前真正行为 RED **2 fail/12 filtered**，不是12通过。预格式候选是反转确切 fmt 差分**重构**，未即时采到 RAW。 |
| 45 / SOURCE45 | W12 T2 / `1f2b1ce2b360ac6647ec3c546440e813e33be9bd` | 功能 `8f20cc5fa087c9baaa9498194b1ed25876d0f16b`（4 native 路径）→格式 `bd6a4ecedba22e21012dee108098616c31697227`（3 已拥有路径）；`task-2-review.md` FULL **Spec compliant / Approved**，无 Critical/Important/parked；无新 formal Ruling。 | `e99ba3bbe9d02913689f1b279110b8648ead5ea5` 三文档。缺 seam 编译36（含错 fixture）→校正35 diagnostics，均 0 tests；真实 fixture 路径失败 4pass/1fail 后新测5pass。composer19，navigation 默认沙箱10pass/1fail且单项再 fail、persist OS5 诊断 fail；窄权限重试1/1、全 navigation11/11。**35 unique**=5+19+11，非逐轮相加。真实即时预格式 RAW 已采；postformat只重跑5。 |
| 46 / SOURCE46 | W12 T3 / `e99ba3bbe9d02913689f1b279110b8648ead5ea5` | 功能 `54a3318963d5bd102c404b8836c87feaea97c0e8`（7 前端路径）→I1 修复 `4f788b1e034caefc7ddfc40a4427c02d66c0d70a`（2 路径）；`task-3-review.md` FULL **Needs fixes**，`task-3-rereview1.md` scoped I1 **ADDRESSED**，无新 Minor/parked/formal Ruling。 | `7a6b2fbce4addcb1c68378dcf42800f5e3518731` 三文档。最初 API5pass/2fail、dialog import 编译/收集失败；即时 Escape 第一 fixture 顺序错，校正后初版**19 unique**；fix 前8项真行为 RED，后 API/dialog24+overlay3=**27 unique**。第一次 TS9 处 fixture typing 失败后校正；最终 TS/fmt/diff exit0。无 full npm/Web/native 或 GUI。 |
| 47 / SOURCE47 | W12 T4 / `7a6b2fbce4addcb1c68378dcf42800f5e3518731` | 功能 `67257deb5d1636e7f18206bb63ed7942e8ccfda1`（11planned路径）→fix `4e5db85915b711c70d1fbc0ed2cfd27f6eabc141`（DesktopApp/test两路径）；`task-4-review.md` FULL **Needs fixes** I1–I3 → `task-4-rereview1.md` SCOPED全**ADDRESSED**，0open/无新parked。 | `6aa8a80aa0bcc875fecd5b31789944b87fdd1042` 四文档。最终fix closest **374/374 across8files**、Web/typecheck/diff0；早前full npm **1409/125files+22Node**、native0/NSIS1均 **PRE-FIX**。Rust1914/18ignored/Python48/fmt0为未变backend早前相位；原CargoUNKNOWN、后ownedABORTED、normal recovery分列。 |
| 48 / SOURCE48 | W11 T1 / `6aa8a80aa0bcc875fecd5b31789944b87fdd1042` | 语义 `72a3c1cd1b447cacf9709b01fa9bdace36b802e9`（6路径）→独立纯format `f55860a4d767580728198ad1b9c097b9ec4f76e2`（5已拥有路径）；`task-1-review.md` FULL **Spec compliant/Approved**，无Critical/Important，无fix/SCOPED。 | `7461ae5c27a478d25d9012e75c11f212328b3351` 四文档。Store **5/0/0**、nativeDTO **1/0/0/724filtered**是**格式前语义**字节；真实即时preformat RAW/exactrustfmt5证明连接最终格式字节，未重测postformat。fmt-before1/apply0/final0/diff0；无workspace/npm/Web/native。 |
| 49 / SOURCE49 | W11 T2 / `7461ae5c27a478d25d9012e75c11f212328b3351` | 语义 `cbc0dd6fb64cffd281fc3e186e3a8a492780ccb9`（3nativepaths）→纯格式 `e56fc1256e7972f946df6db1aa7fe15fdf03dd4f`；独立 `task-2-review.md` FULL **Spec compliant / Approved**，0Critical/Important，无fix/scoped/parked。 | 三docs `b90f99365527ee130c8a01306ca6e00673cd9a6a`。native15+Store5+DTO1=21unique；重复15不累加。compilerRED0、FKfixture12/1、binarymetadata causalRED1分列；semantic-before-format/exactrustfmt3proof。 |
| 50 / SOURCE50 | W11 T3 / `b90f99365527ee130c8a01306ca6e00673cd9a6a` | source `19e2e32018778fb6636b7b65bd6ec7aba99474ea`（10frontendpaths）；独立 `task-3-review.md` FULL **Spec compliant / Approved**，0Critical/Important，无fix/scoped/format/parked。 | 三docs `bcec075d791366985e9e3cb0ef1886b63361f4e0`。closest30→38→WindowsRED1/17skip→final41；fullRust1935/18ignored/Python48/npm1438+22/Web/native/diff0为PRE-REVIEW，后经实际adoption。 |

W10 Task 2 的 W02 读取上限、固定 app-data blob、敏感文件名读取前降级、UUID replay 与删除清理共锁是 host/Store 边界；不能以 diff/UI 评审代替。W10 Task 3 的三个新增原生命令以及读取失败时保守 `Unreadable`、同次请求 4 MiB payload allowance、全请求 200,000 work/256 KiB JSON/2,000 输出行界限，是其已审行为；W10 Task 4 消费者持有面板关闭后的同 ID 请求。W12 Task 1 新表/owner FK/expected-revision CAS/历史 receipt，与 Task 2 native exact getter/presentation 路由、Task 3 Shelved dialog 分相；Task 3 的 `next_offset>10000` UI 拒绝没有修掉 Task 1 Store cursor Minor。各 task 的真正当前状态以独立 source review、scoped closure 与该行 commit 为准。


## Exhaustive original-ruling coverage

Companion contains exact285151-byte SOURCE47 body SHA256251c587efec7824f3e23ad19af7556dc2511604f37761609bcca44f7bd33e67c;57noncontiguous R groups (not57productdecisions),R81/A79/U24,15N/12P/S37/28laterbody+W10oldalias. Original reason/cost variants and authorityblocks remain verbatim. N03 no distinct explicitcost; N12reportpolicy/N13testscope; A50→N08/A20→N14/A55→N15/N09supersededN10/R36falseomissioncorrection retained. Audit477references/134exactblocks; root read10242byteaudit/new1257–1558/217oldnonemptylines, not literallywhole285KB. Post47-R01–R04 fourformalrulings:kindcounts/conditionaladoption/generateddistrecovery/queuedCIorder. Other scheduling/model/chronology/adjudication entries separately classified; no newTask2formalRuling. Controller finalcoverage approval PENDING.

## 当前发现、残余与不同证明强度

`delivery-residual-inventory-42-oct14.md` 已核的 1–42 原 ID/别名与 resolved/conditional/residual 必须在最终永久记录按原 review/fix 逐项继承，不能把下列摘要当作原始 123 KB 裁决/发现材料的替代。已闭合的重要源码缺陷包括 W03 T3 可恢复 Store 错误→T4 typed Unavailable，W05 T2 四轮修复，W06 T2 遮罩循环修复，W07 T2 发布/清理身份，W09 T1 三轮修复、T2 quoted secret、T3 chooser `.zip`，W10 T2 I1–I3/T3 I1+C1，W12 T1 公共标题 I1、T3 作用域 A→B→A I1、T4异步exact-open/active-read/安全选择错误I1–I3。每个闭合只覆盖其源码范围，未给真实 GUI 或科研流程通过。W09 T3 的固定 `.zip` 修复消除确定性 sibling 覆写风险，实际 Windows chooser/cancel/磁盘行为仍未验。 W04 T3 原有正确的固定 **3 秒 inspection deadline** 后来补上暂停虚拟时间 0/2999/3000 ms 的过期边界回归证明；没有生产 deadline 修复，也没有制造行为 RED。

**仍需明确记录的残余：** W05 N01 自动压缩在意外 postarchive Store/语义失败时可能留下 archive/events；Fix4 添加实际重载/渲染/验证及错误，**没有 rollback**，与手工 T3 SQLite CAS 原子事务不同。W04 队列/脏帧检查是瞬时可见字节，不保证任意较晚帧；Linux/macOS 检查未编译/执行。W03 receipt 敏感模式可能保守拒绝旧收据、disconnect 清理 best effort、20 秒远端截止不含后续 SQLite CAS；远端未提供取消。W07 deterministic fixture `2026-10-04T00:00:00Z` 不是创建时间，Linux/macOS 示例创建显式不可用。W08 scenario Task2 字面 fixture ID 易碎、Task3 absent-root 修订后未重跑此前 workspace 1791；CI 只配置未执行。W09 T4 原件字段精确为 `original_file_status`=`not_checked`、重现为 `reproduction_status`=`not_executed`；readiness 无原文件重检、无独立重现。W09 T4 M1 缺公共来源/相关记录 ID，C3 初次 native 106 unresolved imports 经 bounded cache/最终字节 NSIS 恢复，**精确 Cargo freshness 原因未知**。W10 T2 injected reparse 拒绝不证明真实对抗性 Windows junction race 或 macOS 祖先保护，私有孤儿 blob/空目录可留；T3 4 MiB 是 payload allowance，非 EOF probe 之外全部物理 I/O 的绝对合计。W10 T4 M1 密集 JSX/次要测试可读性、M2 延迟 diff 与分页 append/dedup 具体测试不足、M3 chunk/linker/cache warning 未修；不能只因文件长度拆模块。W12 T1 M1 Store `next_offset` 可超 10000 使继续请求不可用，T3 UI 拒绝只缓解展示，未修 native producer；W12 T1 M2、T2 M1 以及较早 hard-link、linker、CRLF、Vite chunk 是输出债务，不是无警告的“全绿”。W12T3 FULL无新Minor/parked；T4 M1继承cursor/M2warnings/M3settings UNKNOWN；W11T1无Critical/Important/parked，hardlink warning Minor延后。

各 FULL 审阅的 `Cannot verify from this diff` 应按生产者/消费者矩阵处理：W01–W09 的后续源码任务能闭合相应实现责任，却不能回填先前 native/CI/live 证据；W10 T1 的 host/blob/diff/UI 分配到 T2–4 的各自 gate，T2/T3 frozenHEAD cleanarchive现有独立确定性通过，真实SSH未验；W12 T1 的 native/default/active/overlay 分配给 T2–4，T2 的前端历史 receipt requery/active/banner/Escape 分配给 T3–4，T3 host integration/default/active/cache/overlay由Task4 FULL+SCOPED闭合源码责任，人工native仍未验。W11T1 file/budget/hash/digest/public_project/fixedsafeerrors由Task2 FULL闭合，TS/UI/defaultempty/stale/generation/catalog/Escape由Task3 FULL闭合；manual/CI/live仍未闭合。结构性无 SSH/runtime/model/keyring 调用与 Store-only 仓库签名只支持**调用边界**，不是仪表化物理零网络计数。

## 新增47/48的实际相位、残余和CV

新增primary目录键：W12为 sibling `2026-10-04-wisp-w12-conversation-visibility/`，W11为 `2026-10-04-wisp-w11-migration-preview/`；所有新行review/report/proof basename均在该目录，不是全局同名文件。

W12T4用户行为：Visible用于默认导航、search/# discovery；Shelved exact-open保留title/transcript/draft/Agent-run与Restore banner，filteredlist不是existence判据，Shelf/newmessage不cancel/reconnect/自动restore。source/Usage/explicitID/collection/branch按exact owned getter，W07 marker/W01 draft adoption保持。defaultresume查无Visible时empty+Shelved入口；resume-disabled仅跳latest-used（Visiblelist仍可加载）而建blank，explicitNew仍建、requestedID优先。每project/conversation窗口内immutable pending同ID/body/expectedrevision，unknown/lostreply保留；distinct retry先重播原操作再exactstate/list requery，后续opposite必须另一次显式点击；known prewrite拒绝才释放，historicalreceipt非currentstate。generation与operation ownership拒A→B→A/late responses，实际event更新exact activeobject并authoritative full Visiblelist refresh，不prepend Shelved；每event额外full unpaginated list及现有legacy placeholder-title repair/可能读messages成本已在appendix原裁决中，非pagebound或per-tokenpolling。没有新增schema/credential/approval/runtimecancel/persistence。


| Scope | 当前处置 |
| --- | --- |
| W10T4 M1/M2/M3 | JSX/次要测试可读性；缺真实延迟diff与分页append/dedup supersession具体回归；chunk/linker/cache warnings。延后最强wholebranch，0parked required source fix。native恢复只确认无剩余build进程，非清理smoke。 |
| W12T1 M1/T2/T3/T4 | Store next_offset>10000仍OPEN，T3 outbound guard只缓解。T1M2/T2M1/T4M2 warnings保留；T4M3两settings immediateEscape失败UNKNOWN，isolated/后full PASS不抹失败或证明timing/sandbox/leak，未改settings/hook/setup/config，future trace UNEXECUTED。T4 I1–I3经fix4e5db85/SCOPED全ADDRESSED，无newparked。 |
| W12T4 CV | Store CAS/ledger/schema/Visible/catalog/ownership由Tasks1–3自己的FULL/必要SCOPED支持（publichelper fix/exactowner getter）；consumer gate不等于重审producer。collection/search窗口Escape受控前端覆盖已执行。W12 Task4当时的fix阶段未重跑final-fix full npm/native；这些fix字节后来已纳入SOURCE50 PRE-REVIEW working adoption与frozen committed-archive全套覆盖（下文给出相位/来源证明）。native手工/manual、live与实际CI仍未执行。 |
| W11T1 CV/Minor | hardlink fallback属环境copy warning，未抑制/清cache。Task2已独立验证W02所有权/全部selection与bytebudget/未知expectedfull拒绝/SSH recorded-only/digest含kind_counts+exclusions排time/selfhash/command注册/public_project/固定safeerror。Task3已独立验证kind_counts TS/UI/默认空/manualGenerate-stale/projectgeneration/metadata-onlycatalog/immediate topEscape。两task现已各自FULL关闭；人工/CI/live未验。 |

W12T4保留初RED315pass/3fail of318（含错API名fixture）、integration313/5和349/3；354、359、363、368各closest为不同历史phase，非总计。retry按钮因果RED1→GREEN、superseded getter-count RED1→GREEN3、actual delivered-event RED3fail/1pass→GREEN5。FULL I1=延迟sameactive exactOpen快照覆盖Restore，I2=nonactive B mutation使active A getter失效无replacement，I3=async选择错误进入discardedpromise无safeUI。fix真RED I1/I2/branch；I3 **1fail+1alreadyPASS**，六focused GREEN。finalclosest首373/1仅Restore断言未等getterfixture，保留立即Escape后等exactgetter，最终**374/374**/Web-typecheck/diff0；早前full1409/125files+22与native0均PRE-FIX。

W12运行三相不可混：原Cargo22568 start-only terminal **UNKNOWN**；owned78510两loggedfail/eightblocked mock probes后 **ABORTED/INCOMPLETE**，无完整总数/panic诊断；精准识别Python29308→cargo32176→toolchain21512→adapters14980后仅Ctrl-C owned session，terminal1/06:47:32UTC PIDabsence，无broadnamekill。之后同source normal-access `cargo test --locked --workspace` **1914pass/0fail/18ignored across89targets**（18zero/16doc）terminal0。Python首47pass/1WinError5 error of48，normal48/48；fmt0为未变backend早前phase。完整npm两settings失败分别1403/1 of1404、1408/1 of1409，当次`&&` Node未跑；isolated及full unchanged recovery1404+22/1409+22保留。ComposerReferences缺zh TS2741修fixture后Web0。release E0432后只对精准currentrepo core release allowlist **21files/13,703,382bytes**备份hash/无compilerowner，再package-onlyclean0/native普通retry0/NSIS1；dep-info关联旧smoke source支持stale cross-source reuse，但selected oldrustc --extern/root freshness未完整捕获/证明。无source/mtimehack/whole-target/smoke清理/appaction。fix helper名撞JSON导致pre-index KeyError:head，失败invocation留存、另名regenerate/stage/postproofPASS，非sourcefix/auto-review rejection。

W11T1为一次deferred readTX/白名单SELECT/rollback，独立COUNT与selectedexamples<=20/category，全owner conversations含Shelved/legacyArchived、science numeric header/固定kind-status/roundrobin，publication maxrevision scalar及当前合法64hex hash lowercase suffix、title800后suffix。shared public_text name800/description4096 UTF8bounds，internal Project在非serializable host context，Task2只发public_project。不读body/sciencevalueJSON/profile/keyring/Skillmemory，不调用会legacy titlewrite的presentation list。projectowned model/context/jobs与appglobalregistered MCP/Skill COUNT分开，非grants/enabled/authtransfer。tests连真DB snapshots/controlUPDATE被query_only拒后sameconnection Store read/poisonedtitleview未选category COUNT/25rows每kind+otherproject/globalregistrations/activejobledgerprofiles不变/nativeforbiddenfields-null-hashscope-zero kindcounts；结构边界不等于仪表化physical zero network。

W11初Store/DTO及后kind_counts compilerRED均**0executed**；firstStore1PASS3FAIL错messages.markdown应content→4PASS，publicationhash真RED **1FAIL/4filtered**，后4PASS1FAIL为Legacy固定句大小写fixture→最终Store5/DTO1。中间nativeDTO08:09:48–08:13:08UTC与publicationhashStore08:10:51–08:13:10UTC误launch overlap/lockwait，两terminal保留、无kill/interrupt，之后等terminal再重检。最终Store5/DTO1在**格式前语义**，即时preformatRAW真实捕获、exactrustfmt5 proof接最终format字节，非postformat重跑/fullworkspace/native通过。

## Working coverage — adopted PRE-REVIEW

POST47-A01 embeds actual adoption. Following exact independent audit text is a historical recommendation later adopted, not another run. Limits remain:no raw-write trace,candidate10 DURINGRust/full1121 AFTER RustPython, ordinary premise not freshpostreview/coldcache/forensic guarantee. Coverage retains seven sidecars/loghashes.

## Task3 working-gate provenance audit — 2026-10-15

Status: **read-only evidence audit complete; recommendation only, NOT gate adoption**. Exact source endpoint audited: `19e2e32018778fb6636b7b65bd6ec7aba99474ea`, parent/Task3 BASE `b90f99365527ee130c8a01306ca6e00673cd9a6a`. During this audit the independent Task3 FULL became Approved without fixes, and documentation-only milestone `bcec075d791366985e9e3cb0ef1886b63361f4e0` followed. Final adoption remains root/controller work after its closure and frozen-document bridge decision. This artifact does not execute a check, approve the whole branch, or replace final archive validation.

### Decision and its precise assurance level

The retained evidence **can support reuse of all seven historical Task3 working-checkout gates**, explicitly labelled **PRE-REVIEW**, under the existing conditional process Ruling and the ordinary single-writer/no-source-write chronology premise. I found **zero actual unaccounted relevant source paths or demonstrated changed gate inputs** that require a duplicate gate run merely because SOURCE50 was reached later. There is no requirement in the supplied human AGENTS to rerun valid unchanged checks solely after review. Missing a prelaunch final-owned10/full-tree hash capture does not by itself demonstrate that Rust or Python tested different inputs.

The assurance differs across phases and must stay visible:

- npm/Web/native/diff have a captured complete 1,121-file RAW map **before their launches**, equal to two later complete maps through source commit. This directly bridges their nonignored source-file bytes.
- fmt/Rust/Python are supported by the actual pre-edit BASE/owned-before/protected evidence, the independent BASE/current/retained-map partition comparison, the frontend-only Task3 diff, protected-seven before/during/after checks, and the writer's statement that no product source changed after the final filter correction. Their final whole-tree bytes were **not** captured before launch. There is **no independently retained raw tool/session event trace** in the evidence supplied to this auditor. The no-write chronology is an attributed writer assertion in the report/manifest chronology, corroborated by source ownership and the retained later bytes; it is not an independently inspected filesystem-write trace or simultaneous full-tree snapshot.

This is ordinary source/provenance auditing, not a forensic guarantee against hypothetical transient edits by every process. Do not describe the early gates as having an independently captured prelaunch final-byte manifest. If root/controller accepts the ordinary single-writer chronology premise, no actual input gap remains and I recommend **no repetition solely to erase the timing limitation**. If that premise is explicitly rejected or contradicted by actual evidence, the temporal limitation affects **fmt, workspace Rust and Python discovery only**; rerun those affected commands after closure with an actual prior source manifest. The later npm/Web/native/diff source bridges remain independently usable unless their own inputs change. This fallback does not authorize an execution by this auditor.

Final fact check from the original writer, relayed by controller/root: **no raw tool/session event extract was retained in task-3-proof or saved during implementation**. Report and manifest-chronology are retrospective notes. Actual command sidecars/logs establish commands/terminals but are not a complete source-write trace. The conversation contains the actual filter apply_patch and following calls, but no verified filesystem session path was supplied and this auditor did not independently read such an event extract. Nothing was reconstructed to fill that absence. This confirms the epistemic limit above; it is not newly discovered evidence of changed Rust/Python inputs.

### Authority, scope and method

Read first: global `CURRENT.md`; `final-gates-preparation.md`, including its conditional working-coverage Ruling and later timing correction. Read W11 primary `progress.md`, the entire `task-3-report.md`, actual Task3 FULL `task-3-review.md`, the relevant proof scripts/sidecars/manifests, protected baseline `task-2-docs-post-proof.json`, and final `task-3-docs-post-proof.json`. Also inspected the shared staging recipe and final-record brief for preservation/reporting limits. These files live under `E:/Project/OmicsOps/.superpowers/sdd/2026-10-04-wisp-improvements/` and sibling `2026-10-04-wisp-w11-migration-preview/`; proof references below are relative to the latter's `task-3-proof/` unless stated otherwise.

Independent read-only operations performed:

1. Read actual Git source endpoint diff/status and the documentation milestone stat/ancestry. The Task3 source commit changes exactly ten frontend paths, five added/five modified; no Rust, Python, dependency, lock, native configuration or build-script changes.
2. Parsed all three complete retained manifests and compared their **files dictionaries**, not JSON container hashes. Verified equal path sets and all 1,121 RAW hashes. Metadata/timestamps differ, so the JSON files themselves correctly have different hashes.
3. Derived the BASE path universe with `git ls-tree` and independently checked the exact partition: 1,104 BASE-tracked paths outside owned/protected sets + ten owned frontend paths + seven protected paths = the complete 1,121-file manifest, with **zero unaccounted paths**. BASE has 1,113 tracked paths; the complete map adds five owned-new and three protected-new paths.
4. Reconciled the controller's actual `controller-relevant-source-bridge-preclosure.json`: its reported 1,104 unchanged BASE files and 801 CRLF-normalized comparisons are consistent with the independently derived universe. That controller comparison establishes current/retained bytes against BASE, allowing RAW equality or CRLF-to-LF equivalence. I did **not** repeat its full 1,104 Git-blob computation or claim my own independent recomputation of the 801 count. Canonical equivalence is not RAW equality and says nothing by itself about an earlier execution time.
5. Independently hashed 167 current control/owned/protected files against the retained complete map: no mismatch. The bounded control set included manifests/locks/configuration/build inputs plus every retained `scripts/` and `skills/wisp-science/` path. Verified every owned candidate RAW hash matches that map; verified current owned-ten RAW and canonical source-commit blob hashes. Verified all protected-seven current RAW hashes equal pre-Task3 hashes and the external binary diff is exactly retained `before-external.diff`.
6. Read all seven actual terminal sidecars and the runner implementation; verified explicit exit 0, correct original workspace cwd, and strict end-before-next-start ordering. Parsed the actual Rust log's 90 result lines independently; inspected Python/npm/Web/native result summaries. No test/build command was executed.
7. At `2026-10-15T09:38:20.573088Z`, independently compared current nonignored path universe and hashes to the retained source-post map for the necessary documentation bridge. Same 1,121 paths; **only the three exact milestone documents differ**, and their current RAW hashes equal `task-3-docs-post-proof.json`. Owned-ten source/canonical hashes and protected hashes remain identical. The index was empty. This new comparison is a current closure observation, not a reconstructed earlier snapshot. Controller's frozen final-document audit remains distinct.

One read-only result-summary script encountered a GBK UnicodeEncodeError while printing a Web checkmark, after printing the map/sidecar/Rust checks. A second read-only parser using UTF-8 completed. No product test/build failed or reran because of this auditor-output issue. Its initially printed count of 22 zero-**pass** Rust targets includes four ignored-only targets; the corrected zero-total-test count is 18, agreeing with the retained report. This was not a discrepancy in production evidence.

### Actual timing and commands

All times below are UTC on 2026-10-15; Asia/Shanghai is UTC+8. Original cwd for every gate is `E:/Project/OmicsOps`. Each `<name>.json` retains argv, cwd, phase, start, runner/child PID and child start, end and explicit terminal exit; `<name>.log` retains combined output. `run.py` waits for the child before recording end/terminal status. Successful stdout alone was not used as a substitute for a terminal.

| Event/gate | Actual timing | Evidence/result |
| --- | --- | --- |
| Pre-edit capture | approximately 09:02:58–59 | `before.json`, exact existing-owned RAW/HEAD copies and `before-external.diff`; BASE known. New owned paths absent. Protected7 already match Task2 docs baseline. This is **pre-edit**, not final-owned10. |
| Last source correction/focused freeze | final filter correction before closest command 09:10:45.796–09:10:50.448 | `green-closest-final2.json`/log: 41 passes/four files, after actual Windows-unsafe RED1/17 skipped. Writer's no-later-product-write statement is in report and `manifest-chronology.json`. |
| `cargo fmt --all -- --check` | 09:11:37.444546–09:11:39.595097 | `fmt-final`: terminal0, genuinely empty stdout log; no formatting operation/format commit needed. |
| `cargo test --locked --workspace` | 09:11:58.062687–09:16:52.556267 | `cargo-workspace-final`: terminal0, 1,935 passed/0 failed/18 ignored/0 measured/0 filtered across 90 result targets. Sixteen zero-test doctest + two other zero-test targets; not extra passes. |
| First final-owned10 candidate | 09:13:24 | `source-candidate/manifest.json` and exact candidate/RAW/canonical/inverse/replay files. **DURING Rust**, after fmt; neither prelaunch final-owned10 nor full-tree capture. |
| `python -B -m unittest discover -s scripts -p test_*.py` | 09:17:00.182497–09:17:00.631881 | `python-final`: terminal0, 48 tests/OK. |
| First complete 1,121-file map | 09:17:25.336665 | `all-source-after-rust-before-frontend.json`: AFTER Rust/Python, BEFORE npm/Web/native. Metadata HEAD still BASE because source not yet committed. |
| `npm.cmd test` | 09:17:27.913418–09:18:12.074620 | `npm-final`: terminal0; 128 Vitest files/1,438 tests + separate Node bridge 22 pass/0 fail/cancel/skip. |
| `npm.cmd run build` | 09:18:36.278430–09:18:46.435884 | `web-final`: terminal0; tsc/Vite, 2,051 modules. |
| `npm.cmd run build:desktop` | 09:19:06.686484–09:22:56.782172 | `native-final`: terminal0; nested beforeBuild Web build, release3m11, application + one NSIS bundle, **BUILD ONLY**. The nested Web invocation is real build work, not another invented independent gate. |
| `git diff --check` | 09:23:11.014927–09:23:11.101913 | `diff-final`: terminal0, after native end/before staging. |
| Later whole-tree maps | files saved approximately 09:23:12 and 09:23:38 | `all-source-post-gates-prestage.json` and `all-source-post.json`: files dictionaries exactly equal first complete map, through source commit. |
| Source commit | actual `19e2e32018778fb6636b7b65bd6ec7aba99474ea` | Parent BASE; candidate tree `9140b1868e6332692d0583e678bc481fa0886587`; exactly ten paths, 434 insertions/4 deletions. `post-proof.json` and `controller-post-audit.json` retain exact byte/preservation/terminal evidence. |

Do not overwrite any earlier report/progress sentence with a silent reinterpretation: the explicit later correction supersedes the earlier mistaken “candidate predates gates” wording. `manifest-chronology.json` was recorded later; it describes the actual sequence and is not itself a contemporaneous prelaunch capture.

### Relevant-input coverage and limits

Rust/fmt input coverage is not inferred merely from the ten filenames: the BASE/current partition includes workspace/Cargo manifests and lock, `.cargo/config.toml`, every crate/native Rust source, migrations, compile-time JSON/SQL/Python/R fixtures and native build resources. Exact protected inputs include `crates/omicsops-agent-core/src/lib.rs`, `src-tauri/src/agent_v4.rs`, `src-tauri/src/agent_v4/go_live_acceptance_tests.rs` and new external `scientific_failure_contract_tests.rs`, so the shared-working runs include those bytes. Candidate creation during Rust rechecks protected hashes/external diff, and later proofs agree. The two intentional working-only initializers remain within protected files; they do not enter committed HEAD.

Inspected actual Rust `include_str!`/`include_bytes!` references, including acceptance JSON, scenario manifest, kernel Python/R helpers, database SQL, model/domain JSON, offline example/license, capability JSON and native remote-job Python. These are accounted in the unchanged/protected map, not omitted because they lack `.rs` suffixes. No Task3 frontend source is among those static Rust include references. `src-tauri/build.rs` delegates to tauri_build; native configuration declares `frontendDist: ../dist`, so frontend bytes do matter to the native build, whose complete-map timing is valid. The working native build actually runs its Web producer first. This is not a claim that arbitrary ignored generated state was hashed.

Python discovery's actual inputs include `scripts/`, bundled skill helper Python and the Rust worker source it parses. The inspected tests really parse worker assemblies in runtime/adapters and load the three fixed bundled Wisp helpers; those files/configuration belong to the BASE partition and bounded current check, not Task3's frontend delta. No unaccounted Python fixture/source change was found.

The first full manifest covers `git ls-files -co --exclude-standard` regular files. It does **not** capture ignored `node_modules`, `target`, `dist`, global package registries, toolchain binaries, environment variables, or every external process's writes. This is standard shared-working coverage, not cold dependency validation or forensic runtime attestation. Existing working dependencies/caches are reused. Native/Rust logs identify `E:/Project/OmicsOps` crate paths and shared debug/release targets; no fresh target or exhaustive transitive cached-artifact path audit is claimed. Preserve the separately documented prior compiler-artifact provenance risk; fresh owned archive Rust target remains mandatory.

The independently inspected source closure bridge is only three documents:

- `docs/superpowers/plans/2026-10-04-wisp-improvements.md`
- `docs/superpowers/plans/2026-10-04-wisp-w11-migration-preview.md`
- `docs/superpowers/specs/2026-10-04-wisp-science-improvement-backlog.md`

At `bcec075d791366985e9e3cb0ef1886b63361f4e0`, those are exactly 13 insertions/7 deletions; parent is `19e2e32`. Focused `rg` across code/scripts/browser-extension/skills found no non-Markdown reference to `docs/superpowers` or these document basenames (exit1 = no matches). There is no evidence that these documentation-only changes invalidate product checks. Preserve their exact candidate/inverse/replay/protected proof and controller's final bridge rather than asserting the entire 1,121 RAW map is unchanged after this milestone: three document hashes **did** change.

### Failure history, residuals and remaining work

Retain original Task3 causal RED5/18 plus separate suite-load error, unsupported Vitest matcher GREEN26/4 failure, intermediate30 then38, Windows path RED1/17 skipped, final closest41; no invented31. Original runner GBK tail-print exception occurred after child result retention, not an erased product failure. Retain hard-link fallback/copy, Vite >500kB chunk (main JS1,603.61kB/gzip486.90kB), linker stdout and Git LF-to-CRLF notices. Historical PluginsSettings/SkillDetails immediate-Escape diagnosis remains **UNKNOWN**, even though Task3 npm1438+22 passed; no cause/resolution or weakened Escape claim follows from one pass. Task3 FULL Approved has no Critical/Important, keeps warning Minor and final provenance/manual limits. It is task-scoped, not strongest wholebranch approval.

This recommendation changes no future obligation. Controller/root must still retain and finish:

- Its explicit postclosure working-evidence adoption decision, exact source endpoint/document-only bridge, protected7/external diff/index proof, original command sidecars and candid PRE-REVIEW wording; any actual subsequent relevant source/fixture/config/lock change invalidates **affected** commands. A future npm lock change requires actual clean installation before affected checks. Do not blanket-invalidate unrelated commands or silently assume a later fix is covered.
- Fresh isolated **committed-source archive** checks, actual `npm ci` under the existing wholebranch Cargo.lock ruling, fresh owned Rust target/provenance and serial required gates. Archive bytes exclude external protected working changes and both working-only initializers, which is why the working runs cannot substitute for archive checks.
- Actual W08 normal safe five-result JSON, isolated semantic fault/nonzero complete JSON, exact RAW/hash restoration and normal rerun; strongest independent wholebranch review from verified merge-base `e3b20a53b92fb06939a98557b9338ab6fc6c1672` to final HEAD, any reviewed fix wave/scoped re-review, and permanent exhaustive tracked record before authorized scratch cleanup.
- Physical GUI/browser/panel/app-control STOP. No GUI/manual/native interaction/installer launch or install/distribution is established by these builds. Real model/SSH/R/Micromamba/PBMC, macOS/production E2E and remaining native smoke stay UNEXECUTED; ignored tests are not acceptance. Preserve both smoke roots/executable/data/profile trees, all failures/warnings, settingsUNKNOWN, existing queued-panel incident and other documented residuals. No new permission, publication, push/PR/tag/release, app control or cleanup is created by this audit.

The process Ruling's cost remains the extra durable provenance/phase record and final audit, in exchange for avoiding redundant unchanged full runs. Its benefit does not excuse missing/failed/UNKNOWN terminals, actual changed inputs, fresh archive gates or review. This auditor found no actual affected-input change demanding a rerun at the audited endpoint; it does not certify a future endpoint.

### Stable fingerprints of principal retained inputs

SHA256 over the actual JSON file bytes as read; equality assertions above concern their contained maps, not these different container hashes.

| Proof | SHA256 |
| --- | --- |
| `before.json` | `d375c81f36c5efc134b416bc93546678418136922d41e2106b0057b122307c02` |
| `source-candidate/manifest.json` | `97c2845b1cc6d859a4854fddaeef0b34d531981ae3b8e2ca58e11fe719be4fb0` |
| `all-source-after-rust-before-frontend.json` | `efc97e51a2fd1e4b96049a7978e843aa580c537cf59251f8ac401bde077b264d` |
| `all-source-post-gates-prestage.json` | `42e56d8383c49253d80c5592a0fff3af274461d1c4c01591d14f1e065c188d62` |
| `all-source-post.json` | `767375e7eb2e13a02b208891c08859b737403a38cc228f55142158fe6716fef6` |
| `controller-relevant-source-bridge-preclosure.json` | `1b0bfd5104d3da994e711eea62199d79357e98532fd5e0f9705318b4fa9c5133` |

Audit boundary: no source/index/HEAD/test/build/process-control/cleanup/UI/browser/panel/open_in_codex/delegation action by this auditor. Only this owned ignored Markdown report is written. No restored/reconstructed prelaunch source snapshot was created or claimed.

## Fresh committed-source archive — actual retained results

Full checked validator report follows (SHA256c8615f54824918e1b020c8d1cc2228fea0502111981e857d736d2f8af72c908c;resultsSHA5abbcff781d26e8b28c05450a2d95e4be8052a16ecd5d28bad6924e4359df89c). Wholebranch remains pending.

## Frozen committed-source archive validation — October 15, 2026

Status: required deterministic archive gates PASS after generated-asset prerequisite recovery. The separate npm audit has unresolved dependency findings. This report does not declare whole-branch approval, delivery completion, GUI smoke, installer execution, or real-service acceptance. The controller combines this evidence with the separate strongest read-only review before any fixwave.

### Source and isolation

Frozen commit `bcec075d791366985e9e3cb0ef1886b63361f4e0`, tree `d7ea0dcd35495d3660dabc5399b74ef11e2bf85a`. `git -c core.autocrlf=false archive --format=tar` extracted to `E:\Project\OmicsOps\target\final-archive-bcec075-oct15`; all 1,118 tracked files were checked against their exact committed Git blob object IDs before gates. Archive tar SHA-256 `81aff47b27492cb8f27316169a0d5f749e853a167fe0c224e0b49c98952372c7`. Source-manifest digest `9988d23148c76adb9244eaac1d2ec98ed1311d697217b28eb77f6955c191d18b`. Before, pre-retry, post-fault restored, and after manifests retain exact file hashes; all source bytes are restored at completion. The archive excludes all external-only files/hunks and both intentionally working-only W04/W10 initializer residues by exact committed-blob construction.

The archive and compiled target `E:\Project\OmicsOps\target\final-archive-target-bcec075-oct15` were new, absent, ignored paths resolved under the repository's owned `target` namespace; allocation proof recorded 152.81 GiB free. No Git worktree, dirty checkout copy, main node_modules, shared compiled target, or smoke-source artifact was imported. Cargo metadata terminal 0 confirms all 16 workspace manifests inside the archive and the exact owned target. Debug/release dep-info is preserved with source dependency paths resolved against the recorded archive Cargo cwd, Windows extended prefixes normalized, and generated assets restricted to the fresh target. No smoke-source reference was found. Default registry/download caches were available and reused separately from the fresh compiled target; `.cargo/config.toml` uses rsproxy sparse registry. npm cache/network split was not individually traced, so neither npm ci nor Cargo is claimed wholly cold or offline.

Environment: Windows 10.0.26300, Node v24.16.0, npm11.13.0, Python3.10.0, cargo1.97.0/rustc1.97.0, Git2.53.0.windows.2. Local Node/Python differ from CI22/3.12. `environment.json` retains exact output and smoke-root presence metadata.

### Commands and terminal evidence

Every command below ran serially from `E:\Project\OmicsOps\target\final-archive-bcec075-oct15`. Each same-name `.json` sidecar in `E:\Project\OmicsOps\.superpowers\sdd\2026-10-04-wisp-improvements\final-archive-proof` records exact argv/cwd, baseline frozen source SHA/manifest hash, selected explicit environment overrides, UTC/local start/end, runner/child PID/start identity, explicit terminal exit and log SHA-256. The intentional W08 fault is **HEAD plus the sole fixture mutation**, not an exact-HEAD test; `w08-mutation.json` retains its actual mutated fixture SHA and semantic delta, and the sidecar's baseline manifest remains a provenance reference. Full stdout/stderr is in the sibling `.log`. `CARGO_TARGET_DIR=E:\Project\OmicsOps\target\final-archive-target-bcec075-oct15`, `CARGO_TERM_COLOR=never`, and `NO_COLOR=1` were explicit; W08 also sets its exact report destination. Normal Windows access was requested directly for these gates; no restricted-access failure is relabeled here. No overlapping heavy gate occurred.

| Gate | Actual command | UTC start | UTC end | Child PID | Exit / result |
|---|---|---|---|---:|---|
| `npm-ci` | `npm.cmd ci` | 2026-10-15T09:47:04.910501+00:00 | 2026-10-15T09:47:10.578144+00:00 | 34908 | 0 — PASS |
| `fmt` | `cargo fmt --all -- --check` | 2026-10-15T09:47:23.331493+00:00 | 2026-10-15T09:47:25.480415+00:00 | 15788 | 0 — PASS |
| `cargo-metadata` | `cargo metadata --locked --no-deps --format-version 1` | 2026-10-15T09:47:32.791227+00:00 | 2026-10-15T09:47:32.851133+00:00 | 14348 | 0 — PASS |
| `rust` | `cargo test --locked --workspace` | 2026-10-15T09:47:45.189939+00:00 | 2026-10-15T09:49:47.868530+00:00 | 12348 | 101 — RETAINED compile failure; 0 executed tests |
| `npm-audit` | `npm.cmd audit --json` | 2026-10-15T09:50:10.220115+00:00 | 2026-10-15T09:50:12.433935+00:00 | 31736 | 1 — FINDINGS (12 vulnerable packages); nonzero expected |
| `web` | `npm.cmd run build` | 2026-10-15T09:50:22.353797+00:00 | 2026-10-15T09:50:32.899073+00:00 | 14360 | 0 — PASS |
| `rust-after-web` | `cargo test --locked --workspace` | 2026-10-15T09:50:49.870206+00:00 | 2026-10-15T09:54:49.402082+00:00 | 36584 | 0 — PASS |
| `python` | `python -B -m unittest discover -s scripts -p test_*.py` | 2026-10-15T09:55:06.213245+00:00 | 2026-10-15T09:55:06.728336+00:00 | 8904 | 0 — PASS |
| `npm` | `npm.cmd test` | 2026-10-15T09:55:34.421816+00:00 | 2026-10-15T09:56:20.438141+00:00 | 16572 | 0 — PASS |
| `native` | `npm.cmd run build:desktop` | 2026-10-15T09:56:46.412356+00:00 | 2026-10-15T10:04:07.453236+00:00 | 30552 | 0 — PASS |
| `source-integrity` | `python -B E:/Project/OmicsOps/.superpowers/sdd/2026-10-04-wisp-improvements/final-archive-proof/validate.py integrity post-native` | 2026-10-15T10:04:18.155761+00:00 | 2026-10-15T10:04:18.347705+00:00 | 21720 | 0 — PASS |
| `w08-normal` | `cargo test -p omicsops-agent-core scenario_evaluation_suite -- --nocapture` | 2026-10-15T10:04:26.043133+00:00 | 2026-10-15T10:04:57.011251+00:00 | 31684 | 0 — PASS |
| `w08-fault` | `cargo test -p omicsops-agent-core scenario_evaluation_suite -- --nocapture` | 2026-10-15T10:05:31.626153+00:00 | 2026-10-15T10:05:48.390546+00:00 | 3468 | 101 — EXPECTED semantic failure; complete 4/1 JSON |
| `w08-restored` | `cargo test -p omicsops-agent-core scenario_evaluation_suite -- --nocapture` | 2026-10-15T10:06:07.979755+00:00 | 2026-10-15T10:06:12.151159+00:00 | 33876 | 0 — PASS |
| `final-preservation` | `python -B E:/Project/OmicsOps/.superpowers/sdd/2026-10-04-wisp-improvements/final-archive-proof/validate.py post` | 2026-10-15T10:06:28.223280+00:00 | 2026-10-15T10:06:29.040430+00:00 | 36312 | 0 — PASS |

Initial Rust terminal 101 is retained: both desktop lib and lib-test compilation failed in `tauri::generate_context!` at src-tauri/src/lib.rs423 because pristine archive `frontendDist=../dist` did not exist; zero test-result summaries/tests executed. The controller approved running the already-required Web build next to create generated dist, then one unchanged-source Rust retry. Exact source manifest before the retry matched all 1,118 committed files. Cost: one failed compilation plus prerequisite reordering and one Rust retry. No product/dependency fix occurred. The standalone Web build's exit0 satisfies that gate; native's intrinsic beforeBuild Web invocation remains part of its own command.

### Actual counts and build results

- Locked workspace Rust retry: **1,916 passed /0 failed /17 ignored**, 90 result summaries. 22 zero-executed targets include 4 ignored-only targets; 18 zero-total targets comprise 16 Doc-tests+2 other targets. The 17 ignored tests are not passes or live acceptance. All result rows and ignored names are retained in counts/results.
- Separate adopted working-checkout result remains **1,935 passed /0 failed /18 ignored**,90 summaries, explicitly PRE-REVIEW with its own provenance limits. Read-only target-level log comparison accounts for the 19-pass/1-ignored difference: agent_core238→229 passes and desktop_lib729/11ignored→719/10ignored. Both logs preserve 90 rows, 86 normalized target labels with repeated labels retained. This is a target-level explanation, not an individual missing-test inventory or a repeated working run.
- Python: **48 tests, OK**, terminal 0.
- npm: **128 Vitest files /1,438 passed tests**, then **22 Node browser-bridge tests passed /0 failed/cancelled/skipped/todo**, terminal 0. No GUI/browser was launched by this command.
- Web: TypeScript+Vite terminal 0;2,051 transformed modules are build units, not tests. Native: Tauri release and NSIS bundle build only, actual terminal recorded above. The built exe/installer was not launched, installed, or distributed.

Build artifacts:

- `E:\Project\OmicsOps\target\final-archive-target-bcec075-oct15\release\omicsops-desktop.exe` — 85479936 bytes; SHA-256 `38ce2c596353d08ac5e0e9b44992aff7edafca82fe4fda1fa874dc567bbd3acb`.
- `E:\Project\OmicsOps\target\final-archive-target-bcec075-oct15\release\bundle\nsis\OmicsOps_0.1.0_x64-setup.exe` — 18207360 bytes; SHA-256 `eea1594eac45064bee1348ed3a3ec406df5ba5cb699aa9215725da35f465d866`.

Retained warnings: npm ci deprecated whatwg-encoding and audit findings below; Vite's >500kB minified-chunk warning (JS1,603.61kB/gzip486.90kB); native Windows linker stdout generated one warning. W08 normal compilation retained an incremental-session finalization access-denied/OS5 note; its actual test still exited0, and no cache cleanup or product workaround was used. No claim of warning-free output. Main preservation Git reads emitted existing LF→CRLF notices; the working whitespace gate is separately adopted. Two ignored dep-info analyzer diagnostics (relative source paths, then Windows extended prefix) are retained in `dep-info-initial-analyzer.json` and `dep-info-second-analyzer.json`; corrected helper resolves actual recorded paths and passes. W08 normal Cargo terminal 0 was followed by a wrapper post-proof parser UnicodeDecodeError (default GBK reading UTF-8 compiler output); `w08-normal-wrapper-diagnostic.json` retains wrapper1 versus Cargo0, and explicit UTF-8 read-only proof regeneration passed without repeating the valid directed test. These were analysis-helper failures, not product tests/source changes; no product/artifact was modified to satisfy them.

### npm audit findings (unresolved)

Actual `npm ci` exited0:304 packages added/305 audited. Separate read-only `npm audit --json` exited1 with a valid finding payload. **12 vulnerable packages: 4 low/3 moderate/3 high/2 critical**. These are vulnerable-package counts including propagated `via` findings, not 12 independent advisories. Lockfile context gives 8 dev packages and 4 production low-severity math-rendering packages. Installed dependency classification is not a claim about runtime reachability or exploitability; those were not assessed. No automatic audit fix, update or lockfile mutation was performed.

| Package / installed version | Severity | Directness / lock context | Advisory or propagated via | npm fixAvailable metadata |
|---|---|---|---|---|
| `@vitest/mocker` 2.1.9 | moderate | transitive / dev | [Vitest: Path Traversal / Arbitrary File Read via @vitest/mocker Redirect Mock](https://github.com/advisories/GHSA-82fw-gwwq-j7x9), vite | `{"name": "vitest", "version": "5.0.4", "isSemVerMajor": true}` |
| `esbuild` 0.21.5 | moderate | transitive / dev | [esbuild enables any website to send any requests to the development server and read the response](https://github.com/advisories/GHSA-67mh-4wv8-2f99) | `{"name": "vitest", "version": "5.0.4", "isSemVerMajor": true}` |
| `katex` 0.16.47 | low | transitive / production dependency | [KaTeX: Existing prototype pollution can bypass trust restrictions](https://github.com/advisories/GHSA-238p-pmpm-9mq7) | `{"name": "rehype-katex", "version": "1.2.0", "isSemVerMajor": true}` |
| `micromark-extension-math` 3.1.0 | low | transitive / production dependency | katex | `{"name": "remark-math", "version": "3.0.1", "isSemVerMajor": true}` |
| `nanoid` 3.3.16 | high | transitive / dev | [nanoid: custom generators can loop indefinitely when size is zero](https://github.com/advisories/GHSA-2v37-7h3g-55p8) | `true` |
| `rehype-katex` 7.0.1 | low | direct / production dependency | katex | `{"name": "rehype-katex", "version": "1.2.0", "isSemVerMajor": true}` |
| `remark-math` 6.0.0 | low | direct / production dependency | micromark-extension-math | `{"name": "remark-math", "version": "3.0.1", "isSemVerMajor": true}` |
| `source-map-js` 1.2.1 | high | transitive / dev | [source-map-js allows event-loop denial of service through indexed source-map section offsets](https://github.com/advisories/GHSA-68fv-2mgg-jv7q) | `true` |
| `tinypool` 1.1.1 | critical | transitive / dev | [Tinypool: Prototype Pollution gadget in worker options leads to Remote Code Execution](https://github.com/advisories/GHSA-5gmw-xhrv-c9v3), [Tinypool: Prototype Pollution Gadget to RCE in run() options](https://github.com/advisories/GHSA-85c8-ppgw-ccpr) | `{"name": "vitest", "version": "5.0.4", "isSemVerMajor": true}` |
| `vite` 5.4.21 | high | transitive / dev | [Vite Vulnerable to Path Traversal in Optimized Deps `.map` Handling](https://github.com/advisories/GHSA-4w7w-66w2-5vf9), [launch-editor: NTLMv2 hash disclosure via UNC path handling on Windows](https://github.com/advisories/GHSA-v6wh-96g9-6wx3), [vite: `server.fs.deny` bypass on Windows alternate paths](https://github.com/advisories/GHSA-fx2h-pf6j-xcff), esbuild | `{"name": "vitest", "version": "5.0.4", "isSemVerMajor": true}` |
| `vite-node` 2.1.9 | moderate | transitive / dev | vite | `{"name": "vitest", "version": "5.0.4", "isSemVerMajor": true}` |
| `vitest` 2.1.9 | critical | direct / dev | @vitest/mocker, [When Vitest UI server is listening, arbitrary file can be read and executed](https://github.com/advisories/GHSA-5xrq-8626-4rwp), [Vitest: Path Traversal / Arbitrary File Read via @vitest/mocker Redirect Mock](https://github.com/advisories/GHSA-82fw-gwwq-j7x9), tinypool, vite, vite-node | `{"name": "vitest", "version": "5.0.4", "isSemVerMajor": true}` |

Raw finding JSON is `npm-audit.log` (not the terminal sidecar `npm-audit.json`); parsed full advisory metadata is `npm-audit-summary.json`, exact installed version/dev flags are `npm-audit-lock-context.json`. fixAvailable major changes/downgrade suggestions are untested npm metadata, not an endorsed remediation plan. Audit metadata reports 407 dependency entries versus npm ci's 305 audited packages; both raw totals are retained without equating their units.

### Actual W08 normal → semantic fault → exact restore → normal

All three runs invoked the real directed `cargo test -p omicsops-agent-core scenario_evaluation_suite -- --nocapture`, with output inside the **archive workspace's** `target/evaluation` namespace (separate from compiled CARGO_TARGET_DIR). Normal terminal 0:1 directed Rust test passed, five scenario results/5passes/0failures. The archive manifest RAW was captured to `w08-original-manifest.raw`, original SHA-256 `c3880ab8a7eb26c8cbbe9933197ba7ff89702ef4d7dee4a65b9fee8368964d6b`. The sole semantic mutation changed `read_evidence` expected `tool_dispatches`0→1, byte replacement plus parsed-JSON equivalence proof. Fault terminal 101: real directed Rust assertion failed, while the complete safe five-result JSON recorded 4passes/1failure, matching `read_evidence` with `assertion_failed`. This expected induced failure is not an unexplained product failure.

The finally block restored the exact original RAW/hash before any further phase; a full 1,118-file source integrity check passed. Restored normal terminal 0: five passes/0failures and byte-identical report to initial normal. Normal report SHA-256 `47c5fbba1c4e049682cfc8b5acf3969945ac1ffd6d5f34c25810ef8534c150c7`; fault report SHA-256 `091d626d9af924aa100eb3a8606d5c4867ecaab67fe4a253212cc13ed15492df`. All report JSON files parse completely, use exact allowed suite/scenario field sets and closed identity/status/failure codes, contain numeric-only counters, match the safe stdout record exactly and carry valid suite hashes. No raw prompts/arguments/results/transcripts/credentials/absolute paths/host metadata are present in that projection. Individual report proofs preserve exact artifact paths and all five numeric result rows.

### Preservation and remaining boundaries

Main before/after HEAD/tree, indexempty, git status, all 1,121 nonignored source-file RAW hashes and external binary diff are identical. Protected seven full-file hashes match the supplied latest Task3 docs baseline before/after, preserving both working-only residues in those files. Main external diff SHA-256 `dad4f92292e1e464e16f79cd4be0d0dcafa24b97c3d326b1c4a81410efe509c6`. `main-before/after.json`, raw external diff evidence, archive manifests and final terminal sidecar retain the proof. Both preserved native-smoke roots still exist and were never launched/closed/cleaned/changed by this validator; no process was killed, cache deleted or worktree created.

| Protected path | Preserved RAW SHA-256 |
|---|---|
| `src-tauri/src/agent_v4.rs` | `d8910fb6cce2173fce9420efbfc50a7c02e44d37aedd7e8692ff3f9f2eba7a76` |
| `acceptance/README.md` | `2295684ce021d013dbdde0de3c467d4864ac10de45f6158452d40791b9f86a1a` |
| `src-tauri/src/agent_v4/scientific_failure_contract_tests.rs` | `47a51bd8b7a8a771acdeecc1617ea29eadb83b2621d7f7c2b10df41a8f9a07f1` |
| `src-tauri/src/agent_v4/go_live_acceptance_tests.rs` | `82d54a1a4e14aed87864d50dd486b30911de78f1341b8063d2b92b2c8f343d2b` |
| `docs/superpowers/specs/2026-10-06-local-runtime-failure-recovery.md` | `d37971b68a83cf609406e9df36edea4ab3531f6d5e4cf17502db74d35bd48696` |
| `docs/superpowers/specs/2026-10-06-reviewer-json-regeneration.md` | `38791096aabdbd7478d1f4c25a052d9f8e15ea0bf68850cb83e3bae5352114d6` |
| `crates/omicsops-agent-core/src/lib.rs` | `e31a9f2f1dea79bf390dc28e2d651719165acdee9436dddf1fb33f7b2dcc76fc` |

GUI/browser/panel/native manual smoke remains UNEXECUTED under the Oct11 human STOP; this validator invoked no UI APIs or CLI app control. The earlier separate queued file-panel incident remains part of the global record and is not erased by this statement. Build output does not prove GUI or installer behavior. CI/macOS/real model/SSH/R/Micromamba/PBMC/GPU/SLURM acceptance was not run; ignored Rust tests and deterministic substitutes do not establish it. Strongest wholebranch review is separate and no approval is asserted here. No main product/source/index/HEAD/ledger mutation, dependency remediation, cleanup, install/distribution, push/PR/tag/release or subagent occurred. The intentional archive-only fault fixture change was restored exactly as proved above. All evidence/archive/output roots remain preserved for the permanent tracked delivery record.

## Independent controller archive proof

Read-only controller proof SHA2564cc42bc35879a4f350362e547fb98fc86cf156e602046015f5ff6c5d5ff6d8cb independently verifies current main1121/archive1118 RAW,15terminals/loghashes/serialorder,W08hash/restored equality,HEAD/index/protected7/externaldiff; no test rerun or wholebranchapproval. Public proof payload:

```json
{
  "utc": "2026-10-15T10:11:56.884197+00:00",
  "head": "bcec075d791366985e9e3cb0ef1886b63361f4e0",
  "main_current_hashes_verified": 1121,
  "terminal_records_verified": 15,
  "serial_and_log_hashes_verified": true,
  "explicit_exceptions": {
    "rust": 101,
    "npm-audit": 1,
    "w08-fault": 101
  },
  "archive_map_read": true,
  "w08_json_hashes_and_restored_equality": true,
  "protected": {
    "src-tauri/src/agent_v4.rs": "d8910fb6cce2173fce9420efbfc50a7c02e44d37aedd7e8692ff3f9f2eba7a76",
    "acceptance/README.md": "2295684ce021d013dbdde0de3c467d4864ac10de45f6158452d40791b9f86a1a",
    "src-tauri/src/agent_v4/scientific_failure_contract_tests.rs": "47a51bd8b7a8a771acdeecc1617ea29eadb83b2621d7f7c2b10df41a8f9a07f1",
    "src-tauri/src/agent_v4/go_live_acceptance_tests.rs": "82d54a1a4e14aed87864d50dd486b30911de78f1341b8063d2b92b2c8f343d2b",
    "docs/superpowers/specs/2026-10-06-local-runtime-failure-recovery.md": "d37971b68a83cf609406e9df36edea4ab3531f6d5e4cf17502db74d35bd48696",
    "docs/superpowers/specs/2026-10-06-reviewer-json-regeneration.md": "38791096aabdbd7478d1f4c25a052d9f8e15ea0bf68850cb83e3bae5352114d6",
    "crates/omicsops-agent-core/src/lib.rs": "e31a9f2f1dea79bf390dc28e2d651719165acdee9436dddf1fb33f7b2dcc76fc"
  },
  "external_diff_identical": true,
  "index_empty": true,
  "report_sha256": "c8615f54824918e1b020c8d1cc2228fea0502111981e857d736d2f8af72c908c",
  "results_sha256": "5abbcff781d26e8b28c05450a2d95e4be8052a16ecd5d28bad6924e4359df89c",
  "wholebranch_review": "PENDING",
  "archive_current_hashes_verified": 1118,
  "archive_hash_verified_utc": "2026-10-15T10:12:08.198638+00:00"
}
```

## Historical residual/CannotVerify inventory — current supersession

Exact SOURCE42 inventory retained for every original Minor/parked/CV alias. Cutoff-pending states below are historical. Current supersession:W10T4/W12T1–4/W11T1–3 source FULL/SCOPED closed;SOURCE50closed;workingadoptedPRE-REVIEW;frozenarchive/W08passed. Original live/native/CI/platform/N01/warning/maintenance boundaries remain. Every final residual disposition PENDING_FINAL_REVIEW; no sourcefinding resolution becomes physicalacceptance.

## SOURCE42 historical residual and CannotVerify inventory — 2026-10-14

Documentary provenance only. Cutoff is the 42 independently reviewed source tasks W01–W09 (including both W08 parts) and W10 Tasks 1–3; W10 Task 4 is excluded even though its source commit is present. This is no new source review, test, native smoke, or final gate. `resolved` means the cited later source/review or report correction closed the specified point. `conditional` means a scoped implementation decision or inherited producer proof exists but the stated platform/manual/final boundary still applies. `residual` means the limitation or verification gap remains at this cutoff. A source Approved verdict alone never supplies manual or clean-HEAD evidence.

Primary path key: `W01`–`W10` mean sibling `.superpowers/sdd/2026-10-04-wisp-wxx-<slug>/` directories, with slugs composer-drafts, table-preview, remote-jobs, kernel-inspection, context-compaction, run-diagnostics, offline-example, publication-evidence, file-baselines. `W08h` is `w08-skill-helpers`, `W08s` is `w08-scenarios`. `F` is `task-N-review.md`, `S` the named scoped rereview, `R` is `task-N-report.md`, and `L` is that feature's `progress.md`. Line references are to those primary documents as read at cutoff. Full historical source SHA/proof index: global `delivery-task-matrix-audit-oct14.md` rows 1–40; W10 T2–3 endpoints and review names are in W10 L:55,80 and their F/S/R. The table preserves original issue IDs where assigned; unlabeled warnings are identified by the exact task/review location. Identical warning debt is semantically one environment theme but each alias remains listed.

### Historical Minor, parked and out-of-scope entries

| Original ID / source SHA | Faithful issue and primary references | Disposition and actual closure/remaining evidence |
|---|---|---|
| W01 T1 no Minor; `ff8595b` | F:4,23 explicitly excludes app-window ownership/caller receipts and defers full frontend to T3. | resolved as task boundary: W01 T3 `bb3bbc7` F + `task-3-rereview.md`, L:36. No manual native/real model proof. |
| W01 T2 no Minor; `6312f60` | F:6,24 defers T3 receipt cleanup and frontend/build. R:62 says null-scope DesktopApp test is synthetic. | resolved as task boundary by W01 T3 review/rereview; synthetic fixture is only test scope. |
| W01 T3 no actionable Minor; `bb3bbc7` | F:24 notes pre-existing Vite chunk warning; S:5 notes global legacy planning-loading behavior outside this fix. R:240 defers desktop/real-host acceptance. | residual warning/legacy behavior, conditional acceptance boundary. Fix source `bb3bbc7` S:2–5 closes accepted-offscreen receipt defect, not legacy loading or native acceptance. |
| W02 T1 unlabeled Minor; `7439276` | F:23–25 temporary reader dead-code, inherited linker/cache/Vite warnings; R:43 says observable changes do not promise atomic hostile-writer snapshot. | temporary reader consumed by T3 `bec43f1`; other warning/no-atomic-snapshot qualifications residual. F/S `task-1-rereview.md`, W02 L:37. |
| W02 T2 M1 evidence wording; `69886e4` | F:28–30: RED had **71 total** compile errors including `OUT_DIR`, not 71 missing-interface plus infrastructure. | resolved documentation correction, W02 L:30 and R addendum; compile RED never assertion RED. |
| W02 T2 M2 temporary integration noise; `69886e4` | F:31: unused parser seam plus inherited hard-link, reader, linker, chunk warnings. | parser consumed by T3 `bec43f1`; other environment warning aliases residual, W02 L:30,37. |
| W02 T3 unlabeled Minor; `bec43f1` | F:28–30: passing Rust logs retain cache hard-link warnings. R:71 assigns UI/Escape/stalled SSH smoke to T4. | warning residual; UI source T4 `153a0ea` and S support focus fix, real SSH/native smoke unverified. |
| W02 T4 unlabeled Minor; `153a0ea` | F:27–29: Web >500k chunk, native dead-code/linker, CRLF output. S:5: unchanged PluginsSettings immediate-Escape test failed once, then 6/6 and final full passed; cause not established. | residual warnings and intermittent-test history; successful rerun does not establish root cause. Native GUI/installer/live SSH remain unexecuted (F:38; R:79). |
| W03 T1 unlabeled Minor; `bad6feb` | F:24–26: sandbox canonicalization/hard-link warnings, no new source warning. R:33 leaves full/default/live gates to T4. | residual environment warning alias; T4 broad shared-tree gates only, live acceptance unexecuted. |
| W03 T2 unlabeled Minor; `6e7519d` | F:31–33: hard-link/path warnings. R:67: known-pattern sensitive receipt rejection may reject old receipts and is not arbitrary-secret classification; receipt metadata presence is not body integrity; disconnect cleanup best effort. | residual disclosed policy/transport limits, not new source findings. `F:13–16` explains 20s deadline excludes later SQLite CAS; no remote cancellation claim. |
| W03 T3 M1 error taxonomy; `1aaafca` | F:Minor section: recoverable Store/profile read errors mapped Rejected and stopped finite retry; thrown error scheduling missing. L:39–40 required T4. | resolved at review level by T4 `e8fe4ec`, F “Required Task 3 Minor closure”, S `task-4-rereview.md`, L:52: typed Database/Io Unavailable and integrity/identity/trust reject, thrown retry/stop cases. Bounded retry can include unrepairable availability failures. |
| W03 T3 M2 build warning; `1aaafca` | F:Minor section: existing Vite >500k chunk. | residual output debt, T4 M1 alias. |
| W03 T4 M1; `e8fe4ec` | F:M1: Vite chunk, native dead-code/linker, incremental hard-link warnings. F:7 defers future W04 dual-entry coexistence and clean archive/CI/live acceptance. | warning residual; W04 T5 `6609846` F:Escape/W03 check and R focused tests cover later coexistence. Clean-HEAD/CI/live still unverified. |
| W04 T1 unlabeled Minor; `a1a7467` | F:26–28: canonicalization/hard-link warnings. F:7–8 assigns sent-frame cancellation guard to T2/3 and full public envelope validation to T4. | warning residual; integration items resolved by W04 T2 `91964e3` S and T4 `a0fbedc` F:10, not by T1 alone. |
| W04 T2 unlabeled Minor; `91964e3` | F:Minor: carried hard-link warning. R/S record 8192/16384 queued duplicate fix and Windows PeekNamedPipe/Linux/macOS poll0. | warning residual. Boundary issue resolved by S:1; **only observable queued bytes at instant checks** are covered. Arbitrarily later frames have no absolute guarantee; Linux/macOS uncompiled/unexecuted, S:7 and L:66. |
| W04 T3 M1; `ef7da02` | F:28: incremental-cache warnings. | residual environment alias, L:87. |
| W04 T3 M2; `ef7da02` | F:29: no direct paused-time assertion for fixed 3s expiry. | resolved by fix `ef7da02` and S `task-3-rereview.md`, L:87; fixed deadline unchanged, paused 0/2999/3000ms proof in R:72–77. It is no late-frame guarantee. |
| W04 T4 M1 evidence count; `a0fbedc` | F:31–33: RED logs have 15 missing API/field diagnostics plus 1 `OUT_DIR`, total16. | resolved report-only addendum R:70 and L:109. Not 16 feature-only errors or executed assertion RED. |
| W04 T4 M2 output; `a0fbedc` | F:34: cache/canonicalization/CRLF warnings. F:44–45 records one W04 initializer in external dirty test is unstaged. | residual environment debt and protected working-only compatibility residue; final clean-HEAD gate cannot import that line. |
| W04 T5 unlabeled Minor; `6609846` | F:29–31: Web chunk and Rust/native dead-code/linker/cache warnings. F:24–27 had two Important UI findings (pending generation/focus under trajectory). | warning residual; Important findings resolved by `task-5-rereview.md` after fix `6609846`, L:124. Rust/native broad results predate UI fix and do not prove native manual behavior. |
| W05 T1 no separate Minor; `2a191c8` | F:30–32: none. F:14 says historical fixture creation provenance not established by package; F:17–18 says unchanged DTO export/Store match need no edits; L:31 carries bounded completed-admission hydration to T3. | conditional historical-provenance limit. T3 `786e2d9` F:9 verifies bounded 64-hex completed hydration; no invented fixture provenance. |
| W05 T2 P1/P2 and successive scoped findings; `5559298` | F P1 actual durable request/native replay budget mismatch and P2 `auto_compact=false` scientific view mismatch; scoped `task-2-rereview.md`, `task-2-fix2-rereview.md`, `task-2-fix3-rereview.md` include auto growth/native replay/postarchive full-context defect. | resolved as implementation findings only after fourth fix `5559298`, `task-2-fix4-rereview.md`, L:77/global L:124. Earlier workspace1679/npm1251/Web predate fixes. |
| W05 T2 N01 current automatic non-atomic limit; `5559298` | Global L:118,120,124; R:163,239: predictable budget refusal writes zero, actual postwrite validation prevents inadmissible send, but unexpected postarchive Store/race/provider semantic failure can leave archive/events. | **residual current**. Fix4 added actual reload/render/validate and error, no rollback. Manual T3 SQLite atomic transaction is separate. Do not describe approved source as atomic automatic compaction. |
| W05 T3 unlabeled Minor; `786e2d9` | F:32–34: Windows canonicalization/hard-link warnings. F:10 assigns host lease/driver/admission/UI to T4/5. | warning residual; producer boundary reviewed in T4 `f6d9f13` F and T5 `e64f394` F/S, but native/real acceptance unexecuted. |
| W05 T4 no actionable Minor; `f6d9f13` | F:70 existing cache, `project_file_read` dead-code, Web chunk warnings; F:72 later UI/clean-HEAD/live gates. | warning residual; UI source T5 reviewed/fixed, clean-HEAD/live not supplied by T4. |
| W05 T5 no additional Minor; `e64f394` | F Important R1/P2 app-library unmount lost pending request ID, allowing another ID; F:96 says same-mount guard is insufficient. | resolved source by `task-5-fix1-report.md` and `task-5-fix1-rereview.md`, L:131: app-window lifetime identity/pending/receipt ledger. Lifetime is window, not restart persistence or cross-window lease. Existing warning debt and live GUI/model/SSH remain residual. |
| W06 T1 no Minor; `bb47a89` | F:37: default/frontend/native/UI/live checks assigned later. | conditional task boundary; T4 shared-tree broad tests/build and source review exist, GUI/live/clean-HEAD remain unexecuted. |
| W06 T2 no Minor; `240abd4` | F required repair: recursive keyword masking may loop/expand on overlapping markers; F:54 deferred broad/UI/live. | resolved source by `task-2-fix1-rereview.md`, final formatting endpoint `240abd4`, L:35. Warning noise is carried in later T3/T4 reports. |
| W06 T3 no actionable Minor; `d87f225` | F:24: immutable snapshot test models post-prepare interval, does not drive real native dialog; F:40 cache warnings. | residual native/manual limit and environment alias. T4 UI source reviewed; native manual I1 below remains. |
| W06 T4 I1 acceptance gap; `d535b50` | F:39–44: Windows filtering/locator/save-cancel/static HTML smoke unexecuted. Global L:76 and W06 L:44 explicitly defer to final unexecuted list. | **residual Important manual gap**, not source fix waiver or pass. `native-smoke-results-2026-10-11.md` only old HEAD `228d129` isolated harness partial pass for filters/locator/save-cancel/HTML+JSON/immediate+nested Escape; static browser failed before physical STOP. No production W06 native acceptance. |
| W06 T4 M1; `d535b50` | F:41–43: cache, unused reader members, MSVC linker, Web chunk warnings. | residual output debt, final whole-branch triage assigned; source review Approved does not close warnings. |
| W07 T1 unlabeled Minor; `26629be` | F:28–30: cache/CRLF warnings. L:31 records Win32 MoveFileExW no-replace; non-Windows returns explicit `example_platform_unavailable`. | residual environment debt and conditional platform limit: Linux/macOS example creation unavailable; no portability claim. |
| W07 T2 M1; `b77c393` | F:50–52: cache/CRLF warnings. F I1/I2 path replacement at publish/cleanup. | warning residual; I1/I2 resolved by S `task-2-fix1-review.md` after `b77c393`, L:46. T3/T4 production/routing and full gates were only later task obligations (F:5). |
| W07 T3 two unlabeled Minors; `7818c02` | F:26–29: cache warnings and RED wording 12 missing E0425 plus 1 `OUT_DIR` =13 total. F:7–8 inherits fixture/path/registration safety and defers inert UI/clean-HEAD/native. | report count resolved by coordinator scratch correction, L:53; cache residual. Producer facts from T1/T2 and T2 S, UI source T4, but native and clean-HEAD still pending. |
| W07 T4 unlabeled Minor; `a7e05c9` | F:33–35: cache/dead-code/linker/chunk/CRLF warnings. F:11 says no-credential creation/restart/changed/missing/deleted-receipt/zero-dispatch native smoke unexecuted. | warning residual; **manual residual** W07 L:67–69; overall native GREEN unchecked. Older W06 harness is unrelated. Real scientific/model/SSH remains unexecuted. |
| W08h T1 no Minor; `74e8fa4` | F:26–28: no task Minor. Test-only helper source, no live helper runtime/CI claim. | conditional scope; T3 `edfb23f` adds CI discovery configuration, but local Python3.10 run is not CI Python3.12 execution. |
| W08h T2 M1; `d6a71bd` | F:29–31: incremental hard-link warning in passing Wisp snapshot tests. | residual environment alias. |
| W08h T3 unlabeled Minor; `edfb23f` | F:26–28: hard-link, unused-field/method and Web chunk warnings. L:32 says CI/Node22/live/clean-HEAD not executed here. | residual output and execution-scope debt. CI configuration is not CI run. |
| W08s T1 unlabeled Minor; `a500df7` | F:28–30: Windows canonicalization/cache/compiler warning summaries. | residual environment alias. Runner/report belong T2/T3, reviewed later, not T1 proof. |
| W08s T2 M1 fixture literal identities; `9711ef6` | F:35–37: `fixture-server`, `receipt-3`, `call-4`, `corrected-4` hard-coded observations may fail after otherwise-valid fixture rename. Current five fixtures match. | residual maintenance brittleness; not current five-scenario failure. Whole-branch triage assigned, W08s L:32. |
| W08s T2 M2 output; `9711ef6` | F:38: canonicalization/cache warning and intermediate cache-finalization issue. | residual environment alias. |
| W08s T3 unlabeled Minor; `1081b28` | F:29–31: hard-link/dead-code/Vite chunk output. F:9–10 delegates unchanged manifest/runner and fresh full/clean-HEAD/native/CI/live gates. | warning residual; producer obligations reviewed in T1/T2; post-amendment core238/scenario15 do not make pre-amendment workspace1791 a fresh full gate (L:42). |
| W09 T1 unlabeled Minor; `15d727a` | F:24–26: hard-link/cache warnings. F:4 assigns DTO consumer/classifier/export/UI to T2–4. | warning residual; original three Important plus LegacyArtifact regression closed only after three scoped rereviews (`task-1-rereview-{1,2,3}.md`, L:33); later source tasks supply respective boundaries, not T1. |
| W09 T2 no new product Minor; `24073b4` | F:27–29: existing canonicalization/hard-link warnings. F:5 defers Task1 DTO and Task3/4 ZIP/UI/final/live. | warning residual; I1 escaped quoted secret scan fixed and independently rereviewed `task-2-rereview-1.md`, L:43. T1 and T3/4 source reviews cover inherited producer/consumer. |
| W09 T3 M1; `dcdd8c6` | F:45–47: hard-link/CRLF warnings. F:8: zero-external-call claim is structural call-path/capability inspection, not instrumented numeric measurement. | warning and measurement limitation residual; Store-only projection/read invariance support scoped claim, no invented counter. |
| W09 T3 C1 chooser extension; `dcdd8c6` | F:6: extensionless chooser path rewritten to sibling `.zip`, native overwrite assurance unverified; `task-3-fix1-findings.md:20`, L:51 established deterministic overwrite target gap. | deterministic code gap **resolved** by `dcdd8c6`, S `task-3-rereview-1.md`: require returned `.zip`, reject other/extensionless, exporter regression preserves sibling/Store. Actual Windows chooser/cancel/disk/manual remains unverified. |
| W09 T4 M1; `2649097` | F:33–35: findings render severity/code/message but omit public source and related record ID; multiple changed sources indistinguishable. | residual usability Minor; no required-label violation; two-source display test suggested, L:67. |
| W09 T4 M2; `2649097` | F:36: existing cache/linker/CRLF/metadata/chunk warnings. | residual output debt. |
| W09 T4 C3; `2649097` | F:13,48: initial native release 106 unresolved imports, bounded cache probe then final-byte NSIS build pass; exact Cargo stale-artifact cause unknown. Local Node24.16/Python3.10 differ CI22/3.12. | residual cause/CI equivalence uncertainty; successful recovery build is recorded, no inferred freshness root cause or CI run. |
| W10 T1 W10-T1-M1; `5085908` | F:28–30: sandbox canonicalization/hard-link warnings, no final source lint. F:9–10 lists later host/blob/diff/UI and final gates. | warning residual; host/blob T2 `f07d9a3`+`4a6eb14`, diff T3 `e82aed0` reviewed; Task4 UI/full gates outside SOURCE42. |
| W10 T2 M1; `4a6eb14` | F:43–45: hard-link warnings. F:5–6 clean committed HEAD/full gates, live/adversarial Windows junction and macOS ancestry unverified. | warning and platform/manual/final residual. I1–I3 closed by S `task-2-rereview1.md` after fix, L:55; one external detached-job test initializer intentionally working-only, so final clean-HEAD must exclude it (R:100,144). |
| W10 T3 M1; `e82aed0` | F:29–31: hard-link warnings. F:8–10 clean HEAD/full, native/live acceptance later. | warning/manual/final residual. I1 attempted-read budget plus C1 coverage closed by S `task-3-rereview1.md`, L:80; failed payload allowance and separate EOF probes remain distinct, not one measurement. |

No other task in this SOURCE42 set records a currently parked required source fix: where FULL review requested changes, the cited scoped rereview cleared it. “No parked” in ledgers refers to source work only and does not erase the residuals above.

### CannotVerify and deferred verification map

This map retains original task aliases even when several refer to one producer or one final gate. It is intentionally separate from the Minor table because `Cannot verify from this diff` does not itself mean a product defect.

| Original review/ledger scope | Disposition at SOURCE42, with authority |
|---|---|
| W01 T1 F:4 / T2 F:6 | Later app-window receipt routing and frontend validation were verified at W01 T3 `bb3bbc7` F/S and full frontend/Web logs in R. Real desktop/model/SSH not run. |
| W02 T1–3 F/R later consumer/UI/full checks | T3 host `bec43f1` and T4 UI `153a0ea` source reviews resolve planned producer/consumer responsibilities. T4 post-fix native build was not rerun on UI-final bytes (S:11); native/manual/live and final branch gates remain. |
| W03 T3 F backend authority / runtime entry and T4 F future W04 coexistence | Native Store/receipt/profile contract from W03 T1–2 reviews; T4 `e8fe4ec` resolves typed error contract and runtime entry. W04 T5 `6609846` F tests dual-entry Escape/stack. W03 T4 clean archive, CI, GUI/live remained outside its diff. |
| W04 T1 F:7–8 / T2 F:1 / T3 F:29 | Cancellation/partial-frame integration and full public envelope closed by W04 T2 S/T3 S/T4 F. Timed M2 direct regression closed by T3 S. Limits remain instantaneous queue observation, Linux/macOS unexecuted, real R/SSH/container/model/manual native unexecuted. |
| W04 T5 F host dependency and final gates | Host/worker facts inherited from independently reviewed T1–4; T5 only consumer/stack source. Its UI fix S closes generation/focus issues. Clean committed HEAD, native GUI/live and CI remain unverified. |
| W05 T1 F fixture provenance / T2 F atomicity/host / T3 F:10 host | T3 `786e2d9` verifies bounded Completed hydration and atomic manual Store; T4 `f6d9f13` verifies lease/admission; T5 `e64f394` S closes app-window UI identity. Historical legacy fixture creation provenance remains unknown. Automatic path remains N01 non-atomic. Full clean-HEAD and real/native acceptance still absent. |
| W06 T1–3 F/R producer and UI/native | T1 bounded chain/DTO, T2 projection (after `240abd4` fix), T3 export were separately reviewed; T4 `d535b50` review checks consumer. T4 I1 native filtering/locator/save-cancel/static HTML persists. Old-HEAD isolated harness is partial only; static browser failed, no GUI restart after STOP. |
| W07 T2 F:5 / T3 F:7–8 / T4 F:11–13 | T1 fixture/Store, T2 `b77c393` identity-safe publication, T3 `7818c02` host commands and T4 `a7e05c9` UI/routing source gates establish producer-consumer chain. W07 manual create/restart/receipt/zero-dispatch checklist **UNEXECUTED**; Linux/macOS creation unavailable by explicit policy; final clean-HEAD and real acceptance remain. |
| W08h T1–3 scope / W08s T1 F later runner / T2 F:13–15 / T3 F:9–10 | Helpers CI discovery is configured by W08h T3, not executed CI. Scenario Task1 parser/hash, Task2 runner, Task3 report writer/fault and current empty external inventory are independently reviewed; T3 post-amendment subset is not a fresh full workspace run. SQLite/live MCP/SSH/UI/science behavior and clean HEAD/CI remain separate. |
| W09 T1 F:4 / T2 F:5 / T3 F:7 | T1 DTO/science bytes; T2 `24073b4` Store/classifier/public projection; T3 `dcdd8c6` bounded ZIP/export; T4 `2649097` UI consumer reviewed. T3 C1 is split: deterministic destination rewrite gap fixed, actual native chooser behavior unexecuted. Structural zero-external-call evidence is not instrumented numeric proof. |
| W09 T4 C1 | Inherited Store/capability/closure/report/ZIP safety comes from closed T1–3 FULL/scoped reviews and retained focused gates; T4 F:11, W09 L:66. This is source-producer resolution, not a fresh native re-audit or GUI pass. |
| W09 T4 C2 | **Residual** actual Windows cancel, path with spaces, overwrite, disk failure, source mutation during chooser, ZIP opening/checksum and visual/Escape steps; F:12, R:86–98, global `remaining-native-smoke-checklist.md`. The DTO fields `original_file_status` and `reproduction_status` use `OriginalFileStatusV1::NotChecked` (`not_checked`) and `ReproductionStatusV1::NotExecuted` (`not_executed`); these remain scientific limits: original files were not rechecked for readiness and independent reproduction was not run (W09 L:10, T4 context:8). Real model/SSH/R/Micromamba/PBMC also UNEXECUTED. |
| W09 T4 C3 | **Residual** unexplained initial native release cache failure and local-vs-CI version gap, F:13,48; final exact build pass is narrower. |
| W10 T1 F:9–10 | Host sensitivity/W02, blob quota/staging/cleanup and binding resolved only through T2 `4a6eb14` FULL+S; list/get/diff/current hash/budget through T3 `e82aed0` FULL+S. UI/Escape/full W10 gate belongs active excluded Task4. Real/manual/clean HEAD pending. The conditional relation-only future delete invariant remains W10 L:23; existing conversation delete unlinks now. |
| W10 T2 F:5–6 | Clean committed HEAD/full tests pending; working-only external initializer explicitly excluded. Injected reparse refusal/Win32 handle proof is not actual adversarial junction-race or macOS ancestry acceptance; real SSH/model/science/native not run. T3/T4 task split is explicit. |
| W10 T3 F:8–10 | I1/C1 source/coverage addressed by `task-3-rereview1.md`; latest6 is a subset of38, payload allowance reserved before failed read and EOF probes are separate. Clean HEAD/full/native and real scientific acceptance remain pending; T4 excluded here. |

Global cutoff qualifications: physical Escape on Oct 11 stopped GUI/browser/app control. W06 native isolated `228d129` results cannot be applied to later source or W07/W09/W10. `CURRENT.md` and `final-delivery-record-brief.md` also document one **queued file-panel tool incident** in a separate read-only supplement; actual display unverified and it supplies no smoke evidence. All 18 ignored/live Rust tests at later task phases are unexecuted; mock results and package builds do not establish production end-to-end. The final clean committed-HEAD archive, final serial full gates (including the actual safe W08 JSON and nonzero-fault invocation), whole-branch review, and SOURCE50 closure are pending at this cutoff. Seven external protected paths, the W04 go-live initializer and distinct W10 detached-job test initializer require their recorded byte/commit distinction; neither working-only line may be counted as committed clean-HEAD evidence.

## Concrete native/manual steps — UNEXECUTED under humanSTOP

W07 credential-free syntheticcreate→restart→changed/missing/deletedreceipt/zerodispatch remains UNEXECUTED. W06 old228d129 isolatedfilter/locator/cancelHTMLJSON/immediatenestedEscape partialonly;staticbrowserfailedbeforeSTOP. Preservebothsmoke roots/source/exe/data/profile. Separateopen_in_codexattempt returnedQUEUED/displayunverified, disclosed/stopped; no blanket noUIhistory. Following checklist is documentary, not authorization to run it.

## W09/W10/W11/W12 Windows native smoke checklist

Status: extraction from approved plans/spec only. The actions below are required manual smoke and are **UNEXECUTED**. This checklist reports no implementation or test result.

### Standing UI stop

The Oct 11 physical Escape stop applies to every remaining native/browser action. Do not operate GUI/browser until the human explicitly resumes. Existing W06 record: [.superpowers/sdd/2026-10-04-wisp-improvements/native-smoke-results-2026-10-11.md](native-smoke-results-2026-10-11.md). That report is W06 evidence only; do not treat it as W07 or W09–W12 evidence. Source: `.superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:12`; W06 report section “Physical stop and remaining acceptance”.

### Deterministic/source checks (specified, not reported as run here)

- W09 UI/API assertions: chosen saved revision remains the request identity when an unsaved draft exists; stale report is discarded after revision selection changes; child overlay Escape closes only the top layer and leaves parent open. Native export assertions require one post-dialog reread of the exact revision/context, reject deleted/changed revision, return no result on chooser cancel, leave Store untouched on cancel/write failure, and verify the fixed four archive entries/checksums. Sources: `docs/superpowers/plans/2026-10-04-wisp-w09-publication-evidence.md:27-31,105-128,150-160`; `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:179-187`.
- W10 prescribed UI assertions include explicit file selection (no implicit directory expansion), TextSnapshot/reference-only/unknown display, source-missing snapshot visibility, root-change explanation, project/generation stale-response discard, immediate Escape from正文/diff child with parent retained, and a save receipt that does not clear another project’s selection. Sources: `docs/superpowers/plans/2026-10-04-wisp-w10-file-baselines.md:154-165`; spec: `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:370-386`.
- W11 prescribed assertions: default empty categories/files; no SSH refresh or session open; only explicit “Generate preview”; selection change marks prior preview stale and requires regeneration; old-project response discarded; immediate Escape closes only dialog. Sources: `docs/superpowers/plans/2026-10-04-wisp-w11-migration-preview.md:120-133`; spec: `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:511-515`.
- W12 prescribed assertions: opening a shelved row does not restore it; restore uses that row’s current revision; close does not mutate; Escape immediately, without internal focus, closes only the collection dialog; direct source/search/usage/branch references open by exact ID even when absent from Visible list; visibility change refreshes search/# catalog; old scoped responses do not overwrite new project state. Running session message/group/run/status/draft stay unchanged and no cancel/reconnect occurs. Sources: `docs/superpowers/plans/2026-10-04-wisp-w12-conversation-visibility.md:121-133,150-168`; spec: `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:646-656`.

### Manual Windows native smoke — all UNEXECUTED

**W09 publication evidence** — With a synthetic project and saved small citation, inspect an older saved revision; modify and delete its source; export and check that the saved snapshot remains, current availability and omissions are reported, and archive hashes/entries verify. Use a Windows destination path containing spaces. Exercise save-dialog cancel and a disk write failure. Verify the UI continues to identify “saved revision N” while a draft is unsaved. Sources: `docs/superpowers/plans/2026-10-04-wisp-w09-publication-evidence.md:160-161`; spec `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:181,187`.

**W10 file baselines** — With a synthetic small script/project, capture an explicitly selected file using a path with spaces; edit it after saving and inspect the diff; inspect missing-source and changed-root results; delete the associated branch and confirm this only unlinks the association; restart the app and inspect the stored baseline and diff. The plan also calls for root change and omission/reference cases. Sources: `docs/superpowers/plans/2026-10-04-wisp-w10-file-baselines.md:165-166`; spec `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:370,382,386`.

**W11 migration preview** — From a project card, open preview directly; inspect empty and selected states, known and unknown hashes, category switching, stale preview after selection changes, explicit regeneration, and dialog close. The preview is a read-only checklist; plan says no real sample/SSH or whole-database export. Sources: `docs/superpowers/plans/2026-10-04-wisp-w11-migration-preview.md:132-133`; spec `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:511,515`.

**W12 conversation visibility** — Seed synthetic Idle and mock Running conversations; shelve both, close and reopen the app, inspect default navigation and search/#, open one through the shelved collection and another through an old publication/source reference, then restore. Confirm running state, messages, groups, and references remain. Opening the shelved row itself must not restore it; restoration is a separate action. Confirm direct source opening works for exact ID outside the Visible list, and nested/immediate Escape closes just the collection top layer. Sources: `docs/superpowers/plans/2026-10-04-wisp-w12-conversation-visibility.md:132-133,154-168`; spec `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:648,654-656`.

### External acceptance

The approved scopes do not require real model or SSH acceptance for W09–W12. W09 explicitly says real reproduction/SSH is not part of the prescribed smoke; W10/W11 use synthetic files/metadata and say real SSH/samples are not used; W12 says real runtime/SSH is not an acceptance prerequisite. Any separate real scientific reproduction, real backend job, SSH, or model acceptance remains UNEXECUTED and must not be inferred from mocks, source assertions, or a successful build. Sources: W09 plan `docs/superpowers/plans/2026-10-04-wisp-w09-publication-evidence.md:161`; W10 plan `docs/superpowers/plans/2026-10-04-wisp-w10-file-baselines.md:166`; W11 plan `docs/superpowers/plans/2026-10-04-wisp-w11-migration-preview.md:133`; W12 plan `docs/superpowers/plans/2026-10-04-wisp-w12-conversation-visibility.md:168`; spec `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:187,515,656`.

### Documented ambiguity

W11 is explicitly a read-only, in-memory migration *preview* with no export/import or actual migration; “migration smoke” should therefore be read as preview interaction only. The plans do not prescribe one unified dataset setup or exact UI labels for all four smokes beyond the quoted saved-revision and generate-preview wording. W09’s post-dialog revision/source recheck is an export-path expectation; no equivalent save-dialog action is specified for W10–W12. Sources: W11 spec `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:407,511`; W09 spec `docs/superpowers/specs/2026-10-04-wisp-evidence-workspace-design.md:179`; W09 plan `docs/superpowers/plans/2026-10-04-wisp-w09-publication-evidence.md:105-128`.

## Closed49/50 evidence inputs — historical text with current states above

Root-authored whole deltas retain observedcompiler/fixture/behavior/proof phases. Their then-pending SOURCE50/adoption/archive states are specifically superseded by actual current evidence above, not current pending gates.

## W11 Task2 closed delivery input — SOURCE49

Root-owned ignored preparation, not permanent tracked record, rerun or independent source gate. Root WHOLE-read actual task-2-report.md and task-2-review.md, then actual docs-postproof. Excludes ACTIVE Task3/all50/finalgates.

### Identity and review

BASE7461ae5c27a478d25d9012e75c11f212328b3351. Semantic cbc0dd6fb64cffd281fc3e186e3a8a492780ccb9 exact3nativepaths, tree81a8a513375e7c4aa58cc1b0deb0420f4dd2794b. Pureformat e56fc1256e7972f946df6db1aa7fe15fdf03dd4f exact3/tree dd742850c70bfb05ca850c7afcc581c9fbd3710b. Independent FULL task-2-review.md on BASE..e56fc12 Speccompliant/qualityApproved, noCriticalImportant/no fix/scoped/parked required. Hardlink cachefallback warning Minor retained, no source diagnostic or permission to suppress/clean smoke roots. Task1 native CannotVerify closed by this task's own producer review, Task3consumer/finalgates remain.

Three-document milestone b90f99365527ee130c8a01306ca6e00673cd9a6a (master/W11plans/backlogspec), candidate8e850636e4076098a0b0520df034f40f346f5771. Controller verified candidate/inverse/replay/protected7/externaldiff/indexempty; root actualdocs-postproof ownedRAW3/protected7/externaldifftrue/indexemptytrue read. Source49 independently closed. No source50/finalgates/nativeGUI claim.

### User behavior and actual boundaries

Native command workspace_preview_project_migration delegates Store-only preview_project_migration and generates UTC RFC3339 millisZ observation. Exact3paths new src-tauri/src/project_migration_preview.rs, module/commandregisterlib.rs, existingdto_contract_tests.rs. No schema/DTO/dependency/lock/Store/W02/W10producer change. Serializes only public_project, not internalProject. Allpublicfields inclcategoryflags/fivekindcounts/scienceheader/rebind/exclusions/Shelvedintegernote enter digest, onlyobserved_at+selfhash excluded. Nonatomic DB-read thenfileobservation fingerprint, NOT approval/export/restoretoken.

Max32 explicitinputs/backend128bytes/path1024bytes, normalizewithactualsharedpath+W02Windowsreserved checks; canonical64hexexpectedhash normalized, exactbackend/pathsortdedup, conflictingexpectationsrefuse, everybackendvalidatedBEFOREfirstsourceaccess. Onlylocal orcanonicalssh:UUID equalinternalprojectboundconnection+nonemptyremoteRoot. Existingvalidate_backend_binding useslist_connections/profile_json, soproducerinsteadchecksinternalProjectdirectly and existingrecordedreferenceEXISTS; danglinglegacyconnectionunknown. This is routinewithinno-profilecontract/3paths, NOTnewformalRuling.

Sensitivefilename/publicprojection-changingpath preflightbeforeopen, sharedpureinspect_small_workspace_text reused; otherunsupportedfilescanretainlocalmetadata/SSHreferencewithoutpayload. W02ONLYretainedhandleopen/metadata/read_complete; noalternateprobe/scanner/reopen. W02Unavailable conservativeUnreadable, notMissing. Openedsize reservedBEFOREattempt against512KiBitem/4MiBrequestpayload; EXACTallowance passedread_complete/noErrrefund. Oversize/overbudgetmetadataonly/nohash; EOF/Full plusmetadata/read/bodylengthagreement requiredFullFile. SourceChangedwholefail. SeparateonebyteEOFprobe notpayload, no absoluteallphysicalIO<=4MiBclaim. Knownselectedbytesmetadata mayexceedreadbudget; unknownsizeNone/count distinct.

SSHstrictlyreviewedtypedboundedV4recordedreferenceonly; malformed/ambiguous/legacy/unmatchedunknown, trustedRecordedFile notcurrentFullFile; noSSHstat/download/profileparse. Expectedfullalwaysactualcompleteverifiedlocal+equality elseentirerequestrefuse, budgetLimitExceeded/otherimpossibleormismatchSourceChanged. MatchingrecordedSSHhashinsufficient. FixedNotFoundmissingprojectsentinel/otherStoreStorageReadFailed, noSQL/path/underlyingtext leakage. NoStore/filewrite, export/import/destination/baseline, runtime/model/kernel/SSH/MCP/keyringports/calls.

### Actual phase evidence, nonadditive

Initialred-native exit101 missingfunction compilerRED0executed. Firstgreen-native-1 exit101 12PASS1FAIL/725filtered: danglingfixtureSQLiteFKsetup BEFOREpreview. CorrectedtestseedlegacyFKoff/restored BEFOREproducer, no productionbypass. Connectedred-unsupported exit101 1FAIL/739filtered actualbinarymetadata sizeNonevsSome7; fixmetadataonlywhilepreservingsensitiveprecheck. Green-native-2 15PASS/725filtered; Store5PASS; DTO1PASS/739filtered; finalnative15PASS/725filtered terminal0at2026-10-15T08:45:34.011456+00:00 includesadditionalorder/aggregateexpectedhash/lengthhash/nilidentity/actualpoisonedSQLsafeerror assertions, notclaimeveryassertioncausalRED. Strictserialheavycommands/eachterminalawaited, nooverlap/kills. Native15+Store5+DTO1 are21 focusedtests, repeatednative15notadditional15.

Connectedtests actualW02temphandles/literalhello-newlineSHA/oversizepanicpayloadreader/9x512KiB/eightactualreads4MiBthenfailedattemptscenario/remaininghalfallowance/postopengrowthSourceChangedwithallowance6, actualmissingUnavailableUnreadable, narrowunsafeopener, laterinvalidownerbeforefirstopen; expectedmatch/mismatch/impossibleSSH/oversize+aggregate. TypedvalidV4+malformedhistory/danglingunknown/noncanonicalbackend/invalidprivateprofileJSONnotdeserialized; actualSSHbranchpanicopenreadproveslocalboundarynevercalled, NOTSSHacceptance. Realquery_onlycontrolUPDATEfails thenactualproducerread/total_changes/sourcebytes/dirinventoryunchanged. Store-onlycapabilitycomposition/sourcegraphstructuralnoexternalcall, NOTinstrumentedrealservicezero-networkcounter.

fmt-before1 restrictedowned3; trueimmediatepreformatRAWcaptured, cargo fmt --all0/fmt-final0/diff-final0. TestsSEMANTIC-BEFOREfmt; rustfmt edition2024 emitstdout skip_children exact3output proof connectsfinalbytes, no postformatrerunclaim. Warningshardlinkfallbackcopy/CRLF; initialunusedtestimportremoved, finalno sourcewarning; no cachecleanup/mtime/sourceworkaround.

### Proof, decisions and pending

Controller independent actualancestry/twocandidatetrees/RAW3canonical/preformat==semanticHEAD/exactrustfmt3/before-after-inverse-replay/exactallowlist/protected7/externaldiff/indexemptyPASS. Before/post/manifests/scratchcopies/stageddiffseachphase, newfileinverseabsence; no wholefilestage/reset/stash/externalhunkremoval. Both working-only W04go_live/W10externaldetachedjob initializers preserveduncommitted; cleanHEADmustexcludeexternalonlytestdeps. NormalapprovedGit escalation, noautomaticapprovalrejection. No newformalRulinginTask2, profilevalidatorchoice appliesexistingno-profilecontract; priorTask1kind_countsRulingcost remainscurrentdigestconsumerfulfilled, UIpending.

Task3TS/UIkindcounts/defaultempty/categories-files/manualgenerate-stale/projectselectiongeneration/metadataonlycatalog/immediatewindowtopEscape/FULL required. All50freshworking+committedarchive actualnpmci/freshRusttarget/strongestwholebranch e3b20a5..finalHEAD/permanentembeddedoriginalRulingsbeforecleanup PENDING. PhysicalGUISTOP/preservedsmokeroots intact, noGUI/browser/panel/appcontrol; manual/native/liveSSH/model/R/Micromamba/PBMC/macOS/CI22Python3.12 UNEXECUTED, tests/build/ignorednotproductionE2E. Rootwrotemerelythisignoredprose, no source/tests/index/HEADaction.

## W11 Task3 closed delivery input — SOURCE50, final phases pending

Root-owned ignored preparation only. Root WHOLE-read actual Task3 report and independent FULL plus primary W11 progress. No root source/index/HEAD/test/build/UI action. All12 firstscope/50 SOURCE independently closed; fresharchive/W08/finalwholebranch/permanentdelivery gates remain separate PENDING. No claim finalproject/E2E complete.

### Identity and gate

BASEb90f99365527ee130c8a01306ca6e00673cd9a6a; source19e2e32018778fb6636b7b65bd6ec7aba99474ea exact10frontendpaths434insert4delete/tree9140b1868e6332692d0583e678bc481fa0886587. No formatter/sourcefmt commit needed (fmtcheck0). Fresh single gpt6-astra high FULL task-3-review.md Speccompliant/Approved,0CriticalImportant/no fix/scoped/parked. StrongreviewriskactualDesktopApp independentprojectlifetime/asyncgenerationfinally/catalogrecurrence/windowEscape, freshstrongcontext/reasoningcost asorchestrationchoice NOTnewformalproductRuling/duplicate/capacityfallback. ProducerCannotVerify supportedreviewedTask1StoreDTO+Task2native; thisTask3 closes theiractualTS/UI/defaultempty/manualgenerate/stale/catalog/async/Escapeconsumerobligations. Manual/finalcoverage/archive/wholebranchCVremain. WarningMinor cachehardlink/Vitechunks/linkerretained; historicalsettingsEscapeUNKNOWNnotresolvedbythisgate.

Three-doc milestone bcec075d791366985e9e3cb0ef1886b63361f4e0 exact13insert7delete/master+W11plans/backlogspec; tree d7ea0dcd35495d3660dabc5399b74ef11e2bf85a. Controller actualcandidate/inverse/replay/staged/ownedRAW3/protected7/externaldiff/indexempty pre/postPASS. CombinedmanualStep4UNCHECKED. SOURCE50 closure doesnotcountdocsmilestoneassecondsourcegate.

### User-visible scope

SeparateProjectLibrarycallback/button+DesktopApp independentpreviewproject openswithoutprojectopen/latest/resume/create/modelprobe. Dialogdefaultcategories/filesempty/noautomaticprevieworcatalog; explicitGenerate invokesactualwrapper workspace_preview_project_migration exactly{request}, outsidehostrejects. TSreusesW09selection/hashshapes/mirrorspureDTOkind_counts/nullablefields/scope/states/no destinationcredentialsapproval. ReportusesONLYnativepublicproject, notinternalWorkspaceProject name/description/roots/connection. Showspublicidentity/template/date/observation/sciencerevision/selectedflags/totals/fivezero-inclusivekindcounts/examplesmax20/filesstate-reason-knownunknownsizehash-fullorrecorded/rebind/exclusions/fingerprint. GlobalMCP/Skillwordingverbatim/noprojectgrantclaim; fingerprintnotapproval/exportguarantee/DBthenfileobservationsNONatomic. No ExportImport/destination/bodydownload/recovery/migrationsuccessstate.

Selectionchangesinvalidatepreviewgeneration/markoldreportstale, explicitregenerate;32cap/removalacrosssavedpages. Local actuallistLocalComposerFiles metadata/existinghost2000entries-depth8 bound, no newcrawler/payloadread. Savedexplicitartifactpage20/offset/limit+<=20serialdetailcalls/nopagechase/noremoterefresh. Exactsourceeveryfield/project/artifact/available/reference_only/historicalcanonicalsshUUIDmatchingcurrentprojectbinding+nonemptyremoteRoot/exactWindows-safepath. Legacy/missingbackend/foreign/unsafe/incompatible/unavailabledisabledfixedreason/nocurrentbindinginference. GenericLocal/SavedSSHlabels/norawserveridentity/roots/detailtext/titles/metadatamapsrendered. ExistingdetailDTO DOEScontainstoredrecordtext; consumerusesonlyidentity/availability/filemetadata, notclaimwholeDTOmetadata-only.

Lifetimekeyincludesproject/bindingroots/connection; unmountcloseinvalidateslifetime/generation. Previewsuccess/error/finally use lifetime+monotonicgeneration+selectionrevision+selectiondigest, soA-B-Acannotreviveoldwork/clearnewbusy. Catalogsuccess/error/finally andawaiteddetailuselifetime+syncpendingref; same-lifetimeselectiondoesnotneedinvalidateunrelatedreadonlycatalog. ActualuseWindowEscapeLayer/z360aboveactualCreatez40; immediatewindowEscapenofocusmovementclosestoponlyandpreservesrealparent, pendingclose/reopenoldresponseinvalidated.

### Exact tests, nonadditive chronology

redclosestexit1 5FAIL18PASS=23executed plusseparatemissingdialogload0executed; missingwrapper/card/Desktopbuttoncausal, notclaimeveryfutureassertionRED. Firstgreen26PASS4FAIL unsupportedVitest2 toHaveBeenCalledExactlyOnceWith, replacedseparatecount/exactpayloadassertions. Green30/4files->selfreview38/4files->WindowsunsafeRED1FAIL17skip(3disabledvs7expected/offered4unsafeinternalpaths)->conservativeUIpathfilter->final41/4files0fail0skip:dialog18/card19/Desktop2/API2. NO31result. GBKtailUnicodeprinterrorAFTERchildexit1+endmetadataretained, runnerUTF8printfixonly/no rerunjusttohidepresentation. Testsactualcompositiononlynativeboundariesmocked, notmock-onlyUI/disconnectedcounters. Deferredselection+projectrecurrence/newpending/late success-error-finally/catalogpage/detailrebinding/close-reopen;safeerror/32cap/boundedpages/preservedselection/foreignresponse/defaultempty/publiconly/hashscopes/fivekindcounts covered.

AllTask3fullgatesSTRICTserialterminal0/PID/start/end/logsidecars, no producteditduringsequence perwriterhistoryassertion. fmt09:11:37-39Z emptylog; Rust09:11:58-09:16:52Z1935PASS0FAIL18ignored0filtered/90resulttargets,16zerodoctest+2otherzero (desktopmain/session_runtime), ignorednotPASS; Python09:17:00Z48PASS; npm09:17:27-09:18:12Z1438Vitest/128files+22Node0fail0skip/cancel; Web09:18:36-46Ztsc+Vite2051; native09:19:06-09:22:56Zrelease3m11+1NSIS BUILDONLY; diff09:23:11Z0. Workingtreeexternalprotected7+twoinitializers present NOTcleanarchive. HistoricalsettingsfailureNOTrecurthisnpmrun/no weakening, oldcauseUNKNOWN. Rusthardlinkfallbackcopy/Vite>500kB(main1603.61kBgzip486.90)/linkerstdout/CRLFwarningsretained/nosuppression; no npmact/stderrwarning. NoTask3npmlock/dependencychanges, noTask3npmcirequired; separatefinalarchiveactualnpmcirequiredwholebranchCargolockchange.

### Source proof and timestamp correction

BeforeeditactualBASE/emptyindex/RAWHEAD10/protected7/externaldiff captured; owned10canonicalequalHEADbefore. Candidate09:13:24ZDURINGRust (NOTpregate) afterfinalfilteredit/focusedfreeze09:10:45/50Z. NOcapturedfinalowned10manifestbeforefmtRust; full1121filemap09:17:25ZAFTERRustPythonBEFOREnpmWebnative. Latermapsall1121exactunchangedthroughcommit. EarlierintermediatepregatecandidatewordingexplicitlySUPERSEDED, no reconstructed/relabeledsnapshot. Writer-tool no-write chronology assertionhasno independentlyretainedtraceunlessactualartifactlaterprovided; recordthatlimit. Controlleractualcurrent/retainedmap/relevantBASEbytes+protectedpartition audit distinctfromthatassertion; finalworkingadoptionnotTask3sourceverdict.

Postproof actualancestry/tree/allowlist10/RAWcanonical/inverse-replay/newfileinverseabsence/protected7/externaldiff/indexempty/all1121map/strictsidecarorderingPASS. Candidatecacheinfoprecisestage/notwholefilestage/reset/stash/delete; normalapprovedGit/noauto-reviewrejection. W04go_live andW10externaldetachedjob TWOdistinctworking-onlycompatibilityinitializers remainuncommitted; cleancommittedHEADmustexcludeboth/externalonlydeps. Docssyncseparateproofclass.

### One later PROCESS refinement: original decision/reason/cost (not product scope)

PrimaryW11progress formalRuling: AFTERall50independentclosures, terminalPASS Task3fullruns mayservefinalWORKINGcoverage ONLYifexacttested-source/candidate/canonical/protected7/externaldiffauditbridgesclosureNOrelevantchanges. AnyFULL/fix/source/fixturechangeinvalidatesaffectedcommandsuntilfinalbytererun; failureUNKNOWNcannotadopt. Requiredfmt/lockedRust/Python/npm/Web/native/diff unchanged. Originalreason actualhumanAGENTSrequiresclosestthencompletechecksBEFOREDELIVERY, notfreshpostreview; developerforbidsunnecessaryvalidunchangedrepeats, avoidredundantfullsuiteforSOURCE50labelwithoutreducingcoverage. Originalcost finalrecordexplicitPRE-REVIEWexecutiontiming+fullsource/provenanceaudit/nofreshpostclosureclaim. Cleancommittedarchive/freshownedRusttarget/actualnpmci/fresharchivegates/actualW08faultnonzero+RAWrestore+normal/strongestwholebranchallMANDATORY. ThisrootinputdoesNOTadoptanygate; actualpostclosureaudit/adjudicationmustappendseparately. Separateparallelimmutablearchive+readonlywholebranchschedulingrecordedglobalphase, notnewproductRuling; finalresultonlyafterboth, nofixwaveduringarchive.

PhysicalGUISTOP/2smokeroots/exe/data/profilespreserved; noGUI/CUA/browser/panel/open_in_codex/appcontrol/processkill/cachecleanup/sourcehack/worktree/chat/goal/push/PR/tag/release/install/distribution. WindowsGUI/manual/realSSH/model/R/Micromamba/PBMC/macOS/CI22Python3.12UNEXECUTED/testsmocksbuildignorednotproductionE2E. Earlierqueuedfilepanelincidentelsewheremustremainfinalrecord, noblanketteamneverUIclaim. FinalpermanentMAIN+EMBEDDED285KBoriginalreasoncostappendix+W11/finaldecisions/gates/proofs/residualsBEFOREownedscratchcleanup stillpending.

## 历史最终修复前门禁与残余快照

|Item|Actualstate|requiredclosure|
|---|---|---|
|Strongestwholebranch|FROZEN_REVIEW_ACCEPTED_WITH_FIXES / OPEN atbcec075|Original18192B+completion19614B preserved verbatim below;root WHOLE-read accepted3Important/8Minor/security disposition for fix dispatch. Original complete-package and fresh targeted-read provenance distinct; actual fix/scoped/affected gates PENDING.|
|Combinedfixwave|IMPLEMENTATION_REPORTED / PENDING_SCOPED_REVIEW_AND_PROVENANCE|Source54196dcee83218b6a808eae91ba7108259879b63+purefmta993efaaaa8e1157b57d28d03cb6b9268b3e4149;18owned paths. Actual report and21terminal metadata appended below; independent finding/gate adjudication pending.|
|Scoped/affectedgates|PENDING_SCOPED_REVIEW / PENDING_AFFECTED_VALIDATION_ADJUDICATION|Writer records actual final-byte gates; root independent scoped and provenance readers ACTIVE. Old working/archive phases do not substitute for fix-byte validation or establish closure.|
|Permanentrecords|PENDING_CONTROLLER_COVERAGE / PENDING_PERMANENT_AUTHORIZATION|Controllerverifycontent/task/gate/ruling/protectedmappingthenactualtrackedauthoring.|
|Native/CI/live|UNEXECUTED|PhysicalSTOP;actualCI22Python3.12,macOS/Linuxspecificunverified,realAgentV4/SSH/model/R/Micromamba/PBMC/GPU/SLURM;tests/builds/ignorednotproductionE2E.|
|Cleanup/publication|Notauthorizedhere|No cleanup/push/PR/tag/release/install/distribution;NSISbuildnotlaunchedinstaller.|

Current residuals:N01 unexpectedpostarchiveStore/semanticfailuremayleavearchive/events(no rollback);W12nativecursor>10000(UIguardonly);settingsimmediateEscapeUNKNOWNdespitelaterPASS;W10delayed-diff/pageappend/dedupspecifictestdebt/JSXreadability;W09publicsource/relatedrecorddisplayMinor;W08literalfixtureidentitybrittleness;hardlink/Vite/linker/incremental/CRLFwarnings;historicalfreshnesscause/platform/manual limits. Finalreviewmayrequireboundedfixorresidualruling;assemblydoesnotpreadjudicate.


## October19 documentary correction and interrupted review chronology — historical first-correction snapshot

The original strongest gpt-6-astra ultra reviewer reported reading all55,473 diff lines /3,093,496 bytes /173 changed paths at frozen bcec075, saved a 18192-byte report with 3 Important /8 Minor groups, and was interrupted before its security/residual/closure tail. This is the original reader's complete-package evidence, not the fresh completion reader's personal reading. The fresh gpt-6-astra ultra completion reader must report its actual scoped reads; its saved completion artifact, root whole-read acceptance, final security disposition, unified fixes, scoped review and affected final-byte validation remain PENDING at this correction snapshot.

Original review: `.superpowers/sdd/2026-10-04-wisp-improvements/final-wholebranch-review.md`, SHA256 `1e00b7e0650689cfbbcfb87be21f0fd35dbbc8dcc3d808312ee032c8ec5164d9`. Original stated verdict is preserved; the final combined review/security completion is not certified. Root primary originals for this chronology and decisions are embedded verbatim in the companion as POST47-R05/O04/R06/R07, with exact paragraph hashes in coverage.

Root disposition from progress.md:306: fix WB-I1–I3 and current-facing WB-M1, narrow WB-M5's title to its actual late-text/mismatched-diff assertions, and retain missing deferred-diff/pagination coverage as test debt. WB-M2/M3/M4/M6/M7/M8 stay OPEN (producer cursor cap; publication affected-source identity; compressed JSX; safe-ID duplication; warnings/performance; current-message prefetch/aggregate resource policy). This disposition does not eliminate these findings. Dated observed12-package npm audit remains unresolved, reachability unassessed; no dependency mutation or runtime safety claim.

All129 verified original per-task report/review task/phase/path/SHA256 pairs now appear in the permanent-ready coverage Markdown section **Exact primary report/review fingerprints**. Their original repository-relative identities survive eventual ignored-JSON removal; this index is evidence preparation, not tracked publication. All seven protected external dirty/untracked paths remain outside this correction writer's write allowlist. Physical GUI/browser STOP remains in force; no app or panel was opened.


## Preserved frozen wholebranch original and completion reports — With fixes / OPEN

Frozen review original+completion are now saved and root WHOLE-read accepted for fix dispatch: **With fixes / OPEN at bcec075**, 3 Important /8 Minor groups. Security disposition is recorded and accepted while the dated12-package npm audit remains unresolved; acceptance is not a dependency remedy or runtime-safety claim. The original strongest reader owns the reported complete55,473-line/173-path package pass; the fresh strongest completion reader owns only its explicitly listed targeted reads. Sole productwriter /root/final_fixwave_oct19 is ACTIVE under the frozen review brief. Actual final fix report/commits, scoped verdict, affected final-byte gates, controller coverage and permanent tracking remain PENDING.

Both source artifacts follow verbatim as UTF-8 bytes. These are two final-review inputs, distinct from129 historical per-task report/review fingerprints. Their verdict is historical at frozen bcec075 and does not judge the active writer's later source.

### Original strongest wholebranch report — exact original bytes

Original repository-relative path `.superpowers/sdd/2026-10-04-wisp-improvements/final-wholebranch-review.md`; 18192 bytes; SHA256 `1e00b7e0650689cfbbcfb87be21f0fd35dbbc8dcc3d808312ee032c8ec5164d9`.

<!-- BEGIN EXACT final-wholebranch-review.md -->
# Independent whole-branch review — frozen bcec075

Reviewed range: **e3b20a53b92fb06939a98557b9338ab6fc6c1672..bcec075d791366985e9e3cb0ef1886b63361f4e0**, OmicsOps all12/50. This is the requested independent integration/specification/quality review after all50 individual task gates closed, not a repeat of those task reviews or approval of subsequent fixes.

**Verdict: With fixes; not ready at this HEAD.** Three Important findings remain: clean-build gate order, native save destinations that can overwrite a different unconfirmed sibling, and Runtime/kernel Tab interception beneath global Search. Eight Minor groups and the explicit acceptance/security/provenance residuals below also require controller disposition. No Critical source finding was established. Deterministic archive passes do not close these findings or unexecuted native/real-service acceptance.

All source line references refer to **bcec075**, including externally dirty paths. Relative paths are rooted at `E:/Project/OmicsOps`.

## Scope and actual coverage

I read CURRENT.md first, the whole review brief, AGENTS.md and the requested requesting-code-review/code-reviewer.md template. The supplied frozen package `review-e3b20a5..bcec075.diff` was read in bounded contiguous passes through **all55,473 lines /3,093,496 bytes /173 changed paths**, including all changed tests/documentation, all13 subplans and all six binding designs. The package has47,108 additions/984 deletions. No diff was regenerated. Every truncated span was explicitly recovered; per-area coverage is in `final-wholebranch-review-notes.md`. Earlier approximate path/EOF descriptions are superseded by actual complete coverage.

The read covered Store migrations/contracts, Agent compaction and frozen authority, runtime lifecycle, shared Windows reader/hash scopes, native registration, DTO/API/TS contracts, all changed React integration/hooks/tests/styles, fixtures, Python helpers, CI and docs. Targeted frozen git-show/grep and selected BASE context checked unchanged surrounding code, including all three native chooser suffix sites, Tauri build configuration, message persistence and global Search. This is not a claim to have read every unchanged repository file.

Evidence reconciliation included the whole final-gates-preparation, working-gate-provenance-audit, working-gate-adoption JSON, SOURCE42 residual inventory, reviewed48 delivery draft, W11 Task2 root delta, W11 Task3 report/review, task-matrix audit, rulings-appendix audit, settings-Escape diagnostic and remaining-native-smoke checklist. Later W10 Task4, W12 Tasks1–4 and W11 review/rereview records and relevant report passages were inspected for residual disposition. The large rulings appendix used authorized audited navigation plus relevant original reasons/costs; it was not claimed wholly read. Binding plans and relevant W11 primary kind-count/process rulings were inspected directly.

The independent archive validator finished during this review. I read its whole final-gates-report and npm audit summary/lock context. These are external validation evidence, not reviewer-run tests. I ran no tests, GUI/browser/panel/app controls or subagents, and made no source/index/HEAD/shared-ledger mutations, cleanup, worktrees, installs, distribution, PRs or releases. Only the owned ignored review artifacts were written. Protected external changes and both smoke roots were preserved.

## Strengths

- All12 first scopes have connected Store/native/UI behavior; centralized DTO contracts, persistent visibility, request receipts and baseline manifests reuse existing project/conversation/run concepts. W11 closes the prior producer/consumer obligations rather than leaving a static preview.
- Manual compaction has atomic preparation/CAS, immutable retry, full-request admission and persisted replay validation. Authority, checkpoint/reference identity and rejected-request no-dispatch behavior are treated explicitly. Automatic postarchive partial effects remain separately disclosed below.
- W02 retained-handle reading is reused by W07/W10/W11. Prefix/full/recorded/unknown hashes, sensitive content, failed-read budget reservation and reference-only remote behavior remain distinct. Metadata/hash does not become execution or approval evidence.
- Diagnostics, publication evidence, offline examples and migration preview have narrow capabilities and public projections. Opening them does not itself grant model/SSH/kernel execution authority. Publication packaging labels original files/reproduction as unchecked/not executed.
- Most async UI paths validate scope/generation/ownership, preserve unknown-operation request identity and use the window Escape stack. W12 exact-open/action-generation fixes address real cross-scope risks. WB-I3 is a remaining focus-ownership seam despite these strengths.
- Tests include fault, retry, stale-response, redaction, CAS, bounds and no-effect behavior. Fresh committed-source validation excludes external-only tests/initializers, preserving its distinction from adopted working-tree evidence.

## Important findings — open at bcec075

### WB-I1 — Clean workspace Rust tests need generated Web assets, but CI/default instructions put Web last

**Files:** `.github/workflows/ci.yml:33` and `:35`; `AGENTS.md:113` and `:118`; `src-tauri/src/lib.rs:423`; Tauri build configuration and `src-tauri/build.rs`.

**Trigger/effect:** a pristine checkout after npm ci follows committed CI: Python, core-only V4 scenario, workspace Rust, frontend tests, then Web. The workspace desktop crate's `tauri::generate_context!` embeds `../dist`. Bare Cargo does not run the Tauri CLI beforeBuildCommand. Package scripts have no install/postinstall/prepare build; build.rs only invokes tauri_build::build; ordinary Tauri dependencies/features do not supply dist. Actual pristine archive Rust exited101 at lib.rs:423 for missing dist with zero executed tests. The documented clean order therefore cannot reliably reach its required gates. BASE has the same order: this is inherited, but still a current delivery/CI contract defect.

**Minimal fix:** run the required standalone Web build before workspace Rust; preserve npm ci, Python, V4 scenario and always-upload report artifact, Rust and frontend tests. Align current AGENTS/plan/testing instructions and the explicit old W08 order contract. Preserve the failed compile/recovery chronology. No product workaround or dependency upgrade is needed.

**Verification:** inspect the committed workflow and check the clean sequence with absent dist. The successful reordered local retry establishes that prerequisite in its environment, not CI Node22/Python3.12 PASS. The controller's queued ruling is not closure at this SHA.

### WB-I2 — Three native save entries can overwrite an extension-added sibling the user did not choose

**Files:** new `src-tauri/src/run_diagnostics.rs:532–539` (rewrite:536); inherited `src-tauri/src/conversation_export.rs:96–100` (rewrite:98); inherited publication Markdown `src-tauri/src/workspace_navigation.rs:881–902` (rewrite:885). Shared export writer: `conversation_export.rs:61–74`.

**Trigger/effect:** a chooser returns extensionless `report` while report.json/html/png/md already exists. Code calls set_extension after the chooser, then persists to that changed path. Case-insensitive suffix validation runs after rewriting; NamedTempFile::persist can replace an existing target (the writer test establishes replacement). Confirmation for the exact returned name does not authorize its changed sibling. Dialog auto-extension on some configurations cannot justify accepting this explicit branch. W06 adds the diagnostics route; the two existing routes share the same boundary. W09 ZIP already avoids this class by requiring the exact selected suffix.

**Minimal fix:** preserve the exact returned path at all three entries, accept expected suffix case-insensitively, and reject missing/wrong suffix with a safe fixed error before writing. Cancellation stays no-write. If suffixing is retained instead, the final changed target needs actual overwrite confirmation. No broad export framework is necessary.

**Verification:** test actual production seams for uppercase valid suffix, extensionless chosen path with existing sibling (both untouched), wrong extension, cancellation and no target rewrite. Serialization/helper-only tests do not prove chooser authorization. This is Important because a normal save can replace a different existing file; no destructive GUI experiment was run.

### WB-I3 — Runtime and kernel inspection trap Tab beneath global Search

**Files:** `src/features/workspace/RuntimeDialog.tsx:61–68`, `:84`, `:104–107`; `WorkspaceShell.tsx:1395`; `src/DesktopApp.tsx:94–111`; `WorkspaceSearchDialog.tsx:93–117` and `:178–193`; runtime-dialog.css:1 and workspace-search.css:1 in the workspace feature directory.

**Trigger/effect:** open Runtime or its kernel-inspection child, Ctrl/Cmd+K opens Search, then Tab/Shift+Tab from the focused search field. DesktopApp only blocks Search for a navigation-decision prompt; chromeDismissRequest reaches ApplicationMenu, not Runtime. Search appears above Runtime/kernel (z1100 versus30/130) and focuses its input. Shell disables Runtime focus only for remote jobs; Runtime suspends its parent for its child; the child calls useDialogFocus(true). The lower window listener therefore preventDefaults/stops every Tab and focuses its own control even when activeElement is in Search. Search has no Tab interception. Keyboard focus moves behind the visible modal and can activate a covered Runtime control. This is a concrete integration regression, distinct from historical unexplained Settings Escape failures.

**Minimal fix:** focus ownership must follow the top visible overlay, including global Search; suspend both Runtime and kernel traps beneath it and restore on close. Preserve Runtime's deliberate ownership above the lower trajectory and remote-jobs ordering. DOM sibling order or a search-only local workaround does not establish ownership generally; avoid unrelated dialog rewrites.

**Verification:** Desktop/Shell integration opens Runtime then Search using the shortcut, verifies Tab/Shift+Tab stay in Search, and one Escape closes Search while Runtime remains and regains focus. Cover kernel child and retain Runtime-over-trajectory tests. No GUI acceptance was attempted.

## Minor findings — explicit disposition required

### WB-M1 — Current-facing W08 documentation says implemented scenario work is pending

`acceptance/README.md:24–26` and `:676`, `docs/bundled-science.md:145–146`, `scripts/README.md:64–65` still describe pending manifest/runner/report or incomplete all-W08. Source, CI configuration and actual archive normal/fault/restore evidence now exist. Update current guidance to distinguish delivered deterministic work from unexecuted real-service/native acceptance and link scenario docs; preserve dated historical failure/implementation records. This is separate from WB-I1 order.

### WB-M2 — Visibility catalog emits a cursor it rejects next time

`crates/omicsops-store/src/workspace_conversation_visibility.rs:59` rejects offset>10000, while :112 emits offset+limit if more rows exist. Near the boundary a caller following that cursor gets invalid_input. Current UI preserves the returned page and hides Next with “Pagination limit reached. Narrow search”; it does not drop the page or crash. Emit usable continuations while retaining truthful bounded-result indication, and cover producer/consumer boundary behavior. W12 Task1 M1 remains open; removing the UI explanation silently is not a complete fix.

### WB-M3 — Publication findings hide affected-source identity

`src/features/workspace/WorkspacePublicationPage.tsx:280` renders severity/code/message but omits public source and related_record_id. Two identical SourceChanged findings cannot be tied to references. Display already public source/record identity and test two otherwise identical findings. W09 Task4 M1 remains a usability defect, not evidence corruption.

### WB-M4 — Baseline panel JSX is unnecessarily compressed

`src/features/workspace/WorkspaceFileBaselinesPanel.tsx:174`, `:180`, `:190`: large single-line branches obscure selection/unknown-operation/error controls. Expand readable blocks without behavior changes or module splitting solely for length. This is W10 Task4 M1, localized maintenance work.

### WB-M5 — Baseline tests overstate late-diff/pagination coverage

`src/features/workspace/WorkspaceFileBaselinesPanel.test.tsx:88–94` defers text but resolves diff immediately with a wrong manifest despite its “late text/diff” title. No equivalent deferred diff after close/scope change or pagination append/dedupe/supersession case is present. Production guards look intentional; no failing runtime race was established. Add focused cases or narrow coverage claims. This is W10 Task4 M2, not a claim the full suite failed.

### WB-M6 — Safe scenario report IDs duplicate literal fixture names

`crates/omicsops-agent-core/src/tests/scenario_evaluation.rs:26–47` repeats five IDs and suite identity. They match today and archive proof passes; a legitimate fixture rename/addition can fail artifact emission for maintenance reasons. Preserve closed safe identities, centralize validated contract identities or assert consistency with a clear error. This is W08 scenarios Task2 M1; it is not permission to emit arbitrary fixture strings or a current missing-report finding.

### WB-M7 — Build/environment warnings remain

Fresh archive evidence retains deprecated whatwg-encoding, Web chunk>500kB (largest JS about1603.61kB/gzip486.90kB), linker stdout warning and Windows OS5 incremental-session warning during a successful W08 command. Historical tasks also reported hardlink/incremental warnings. Exit0 remains exit0, but results are not warning-free and retries do not establish causes. Preserve exact warnings; profile frontend cost and investigate reproducible environment issues without deleting protected roots/caches. Do not claim every historical warning recurred. Security findings below are separate.

### WB-M8 — Publication current-message reads have no pre-fetch byte budget

`crates/omicsops-store/src/workspace_journey.rs:139–156` fetches/serializes the whole current Message without a length check/request debit; Tool :165–187 checks256KiB only after fetch. `workspace_navigation.rs:294–333` can retain one such value per saved reference. Saved snapshots are bounded, but a valid short excerpt can cite a much larger full message: `lib.rs:6064–6087` has no message-body cap and native `workspace_navigation.rs:125–158` validates full-body hash before selecting the excerpt. Multiple small references therefore need not imply bounded current-source materialization. No memory-pressure test or ordinary-size failure was demonstrated; this is a resource-bound gap, not an asserted crash/exploit.

Add pre-fetch lengths and an explicit aggregate policy for current non-scientific projections while preserving full-body identity. Over-budget means uninspected/unavailable with inspection-limit indication, not changed/missing/verified or a partial hash called full. Document/test the cost to long existing source messages. Science-only limits cannot be advertised as limits on every source kind; saved snapshot limits alone do not close this gap.

## Validation evidence and boundaries

Working seven gates were explicitly **ADOPTED PRE-REVIEW** by the controller. That is a process decision with limits: no independent raw write trace spans early fmt/Rust/Python; candidate10 occurred during Rust; the1121-file map was after Rust/Python and before frontend work. This is not forensic or fresh postreview evidence.

Independent archive used exact bcec075/tree d7ea0dcd35495d3660dabc5399b74ef11e2bf85a, all1118 tracked files checked against Git blobs, new archive/compiled roots, all16 workspace manifests and debug/release dep-info within those roots, no smoke-source references. External-only hunks and two working-only initializers were excluded. Registry/download caches were reused; npm cache/network split was untraced, so it is neither wholly cold nor offline.

| Gate | Actual evidence | Limit |
|---|---|---|
| npm ci |exit0;304 added/305 audited|Committed lock clean install; audit unresolved|
| fmt / metadata |exit0 /exit0|No reviewer source mutation|
| First Rust before Web |exit101;missing dist;0 executed|Retained failure supporting WB-I1|
| Standalone Web |exit0;2051 transformed modules|Generated prerequisite; modules are not tests|
| Unchanged-source locked Rust retry |exit0;1916 passed/0 failed/17 ignored;90 summaries|22 zero-executed targets; ignored/empty targets not acceptance|
| Python |48 tests OK;exit0|Deterministic helpers|
| npm test |128 Vitest files/1438 pass +22 Node bridge pass;exit0|No native GUI acceptance|
| Native |release/NSIS build exit0|No app/installer launch/install/distribution|
| W08 normal |exit0;1 directed Rust test/5 scenario PASS|Actual report proof|
| W08 intentional fault |exit101;complete4-pass/1-fail JSON|HEAD plus sole deliberate fixture mutation|
| W08 restore/normal |all1118 bytes restored;exit0;normal report identical|Failure chronology retained|

Environment: Windows10.0.26300, Node24.16/npm11.13, Python3.10, Cargo/Rust1.97. CI Node22/Python3.12 was not run. Native's intrinsic Web invocation belongs to that build, not an extra independent test gate. Working1935/18ignored versus archive1916/17ignored is explained at target level (agent_core238→229; desktop729/11ignored→719/10ignored), not an individual missing-test inventory.

Historical compiler/OUT_DIR errors, faulty fixtures, semantic failures, unknown terminals, overlapping checks and corrected count claims remain facts, never additional passing tests. Archive analysis path/prefix fixes and a W08 output-wrapper encoding failure after Cargo0 are retained in its report; regenerating UTF-8 evidence is not rerunning tests. Fresh archive gates close committed-source deterministic validation, not historical trace gaps, GUI smoke or this review.

<!-- END EXACT final-wholebranch-review.md -->

### Fresh strongest scoped completion report — exact original bytes

Original repository-relative path `.superpowers/sdd/2026-10-04-wisp-improvements/final-wholebranch-review-completion-oct19.md`; 19614 bytes; SHA256 `1593551a447f1e89fe9afed57f905bf5ed2e7859df6a810e1d1cb4f01c2280ae`.

<!-- BEGIN EXACT final-wholebranch-review-completion-oct19.md -->
# Frozen whole-branch review completion — 2026-10-19

**Verdict: With fixes; OPEN / not ready to merge at frozen `bcec075d791366985e9e3cb0ef1886b63361f4e0`.** The original three Important findings and eight Minor groups remain the findings of the frozen review. This completion accepts their recorded technical disposition and supplies the interrupted security, residual, acceptance and closure boundaries. It does not approve any later fix or certify another full package review.

## Handoff and provenance

- Reviewed branch range: `e3b20a53b92fb06939a98557b9338ab6fc6c1672..bcec075d791366985e9e3cb0ef1886b63361f4e0`; frozen tree `d7ea0dcd35495d3660dabc5399b74ef11e2bf85a`.
- Preserved original: `final-wholebranch-review.md`, **18,192 bytes**, SHA-256 **`1e00b7e0650689cfbbcfb87be21f0fd35dbbc8dcc3d808312ee032c8ec5164d9`**. It is unchanged. It ends after validation evidence; this separate completion does not replace or silently edit it.
- The original `gpt-6-astra` ultra reviewer recorded complete contiguous reading of the supplied **55,473-line / 3,093,496-byte / 173-path** package, including changed tests, documentation, 13 subplans and six binding designs. The preserved notes contain earlier incremental checkpoints, including 48,900; the final original report expressly supersedes those coverage checkpoints. The controller's handoff confirms completion of package reading before interruption during final sections.
- I am the fresh scoped completion reader `/root/final_review_completion_oct19`, not that original reader. I did **not** read all package lines, regenerate its diff, repeat a complete review, or independently recertify its full coverage. Original findings and complete-package coverage remain attributed to the preserved reviewer; targeted source verification below is my own.
- This artifact is ignored and owned by this completion task. No source, index, HEAD, shared ledger, dependency, external dirty path or smoke root was modified. No tests/builds/install/audit reruns, network research, cleanup, subagents, GUI/browser/panel/open_in_codex actions, or CLI app launch/close/control occurred.

## Actual completion reads

Read `CURRENT.md` first; its long initial output was truncated in the middle, and lines 46–110 were reread to recover that span. Read the complete original review, brief, notes, final-gates report, audit summary and audit lock context. The following hashes identify those inputs as sampled during this completion (CURRENT is a live coordination input and may subsequently receive controller additions):

| Input under this directory | Bytes | SHA-256 |
|---|---:|---|
| `CURRENT.md` | 63,881 | `65f89c5917bc63a63ba6af5514801bfc1e11ba16cbbf7b78543dae4880480fde` |
| `final-wholebranch-review-brief.md` | 8,016 | `b8016da01e7274625d3ebeac539096ad51b50d1e0210bb59ec6356e17afd4698` |
| `final-wholebranch-review-notes.md` | 4,873 | `b839c58b3097e75a9f646761f5ed531104d2ec406edee8b2dc1bdf48af281a35` |
| `final-gates-report.md` | 18,463 | `c8615f54824918e1b020c8d1cc2228fea0502111981e857d736d2f8af72c908c` |
| `final-archive-proof/npm-audit-summary.json` | 11,625 | `5a3d6627af6feb5565c722d394b7762120473d6fb861282bb0d1440ceb8b0017` |
| `final-archive-proof/npm-audit-lock-context.json` | 3,898 | `37cdc12d4026b18b732b501fc88004bb0606538b020fd1c46f6e879f58c402d5` |

Additional reads used explicit frozen `git show bcec075...:path`, including for protected `acceptance/README.md`; working-only changes were not used as branch source. These were bounded inspection of CI/AGENTS ordering, complete package scripts and build configuration, the three native save paths and shared persistence test, Runtime focus hook/child/parent, Desktop Search shortcut and menu routing, Shell Runtime props, Search focus/Escape/key handlers and overlay CSS, M1 current-facing text, M5 test body, and dependency configuration. A large combined source output was truncated; all I3 anchors used below were reread in bounded slices or were fully visible before truncation. No whole-file or whole-repository reading claim is made for those larger source files.

Read-only Git object checks compared package manifest/lock blobs across BASE and frozen HEAD; selected lock entries were parsed to distinguish root Vite from nested audit nodes. The first PowerShell JSON parse rejected the lockfile's empty-string root property; the corrected `ConvertFrom-Json -AsHashtable` read succeeded. Null entries from the failed parse were discarded, not evidence. This was a reader diagnostic, not a product test failure.

## Important findings: independently checked, all OPEN at frozen HEAD

| Finding | Frozen source/evidence checked | Disposition and required closure |
|---|---|---|
| **WB-I1 — clean-build ordering** | `.github/workflows/ci.yml:19–35` puts workspace Rust before Web; `AGENTS.md:113–120` repeats that order. `src-tauri/src/lib.rs:423` calls `generate_context!`; `src-tauri/tauri.conf.json:6–8` sets `frontendDist=../dist` and Tauri-only beforeBuild; `build.rs:1–3` only calls `tauri_build::build`. `package.json` has no install/postinstall/prepare Web build. Preserved pristine archive Rust exited 101 with missing dist and zero executed tests, then unchanged-source Web/Rust retry passed. | Confirmed inherited current CI/delivery contract defect. Root's queued unified fix should move the single required standalone Web build before workspace Rust, preserving npm ci, Python, V4 scenario/report artifact, Rust and frontend tests; align current instructions/order contract. Local recovery is not CI22/Python3.12 execution. Ruling or planned fix is not closure. |
| **WB-I2 — chosen save destination rewritten** | `run_diagnostics.rs:532–539` rewrites at536; `conversation_export.rs:96–100` at98; `workspace_navigation.rs:881–902` at885. Shared writer `conversation_export.rs:61–75` validates only the already-changed suffix and persists there. Existing test `:161–172` writes an existing suffixed file and asserts replacement, confirming intended replacement semantics without any experiment here. | Confirmed: extensionless chooser result can redirect to an unconfirmed existing sibling. Root's exact-returned-path repair is appropriate for all three seams: case-insensitive expected suffix, safe error for missing/wrong suffix, cancellation no-write, no suffix addition or target rewrite. Production-seam temp-directory regressions must cover these cases and sibling preservation. No destructive native experiment was performed. |
| **WB-I3 — lower Runtime/kernel traps Search Tab** | `RuntimeDialog.tsx:55–77` installs a window Tab trap that prevents default/stops propagation and moves focus even when current focus is outside that dialog; child `:84` uses true; parent `:104–107` suspends only for child/its focusActive prop. `WorkspaceShell.tsx:1395` sets focusActive only from remoteJobsOpen. `DesktopApp.tsx:94–111` opens Search by shortcut, with only navigation-decision exclusion; `:1393–1394` routes chromeDismissRequest to ApplicationMenu. `WorkspaceSearchDialog.tsx:93–117,178–193` focuses input/restores old focus, registers Escape, and handles arrows/Enter, without a Tab owner. Runtime/Search CSS line1 gives z30/1100; kernel inline z130 is atRuntime:86. | Confirmed integration focus-ownership defect distinct from Settings UNKNOWN. Unified fix must suspend both lower Runtime and kernel traps while global Search is visually above them, restore ownership on close, retain Runtime over trajectory and remote-jobs ordering, and verify shortcut → Tab/Shift+Tab → one Escape in Desktop/Shell integration for both parent and child. No GUI validation occurred. |

No Critical source finding was established by the original review or this scoped verification. That statement does not dismiss the separate critical dependency audit labels below.

## Eight Minor groups and controller disposition

All eight remain recorded at the frozen SHA; future fixes must carry their own evidence. Except M1/M5 anchors explicitly checked here, these entries retain the original review's source findings rather than claiming a second independent source pass.

| Finding | Current disposition |
|---|---|
| **WB-M1** stale W08 current-facing guidance | Include in the unified fix. Confirmed `acceptance/README.md:24–26,676`, `docs/bundled-science.md:145–146`, `scripts/README.md:64–65` still say manifest/runner/report or W08 pending. Correct delivered deterministic scope while retaining dated history and unexecuted real/native limits. Distinct from I1 order. |
| **WB-M2** unusable visibility continuation beyond 10,000 | Explicitly deferred/open native cursor Minor. UI retains current page and exposes a pagination-limit explanation; that mitigates UX but does not repair the producer's rejected next cursor. |
| **WB-M3** publication finding source identity omitted | Explicitly deferred/open usability issue. Public source/related record identity should eventually disambiguate otherwise equal findings; no evidence corruption is asserted. |
| **WB-M4** compressed baseline JSX | Explicitly deferred/open localized readability debt. No behavior defect or broad module split is required by this finding. |
| **WB-M5** overstated baseline race coverage | Include only a truthful title/claim correction in unified fix. At test:88–94 text is deferred, while diff immediately resolves with the wrong manifest. Narrowing the title is a valid claim repair; **missing deferred-diff-after-close/scope-change and pagination append/dedupe/supersession tests remain explicit deferred debt**. Title correction alone must not be called complete race/pagination coverage. |
| **WB-M6** duplicated safe scenario fixture identities | Explicitly deferred/open maintenance debt. Existing names match and report proof passed. Preserve the closed safe identity boundary; no arbitrary fixture strings in public reports. |
| **WB-M7** build/environment warnings | Explicitly retained/deferred: deprecated whatwg-encoding, large Web chunk, linker warning, and observed Windows OS5 incremental-session note. Exit0 stands; no warning-free or retry-proves-cause claim. Historical warnings need not all have recurred. Separate from security audit. |
| **WB-M8** publication current-source materialization budget | Explicitly deferred/open resource-bound gap. Full Message fetch and Tool post-fetch cap are not a pre-fetch/aggregate byte policy. No demonstrated crash/exploit or memory-pressure test. A future limit must preserve full-body identity and report uninspected/unavailable, not changed/missing/verified or a partial hash as full. |

The intended single fixwave is **I1–I3 + M1 + M5 title/coverage-claim correction**. M2/M3/M4/M6/M7/M8 and the remaining M5 test debt are explicitly deferred. I found no concrete evidence in this completion requiring a different scope. This is disposition at frozen HEAD, not implementation or closure.

## Security disposition: dated unresolved npm audit

The preserved **2026-10-15** archive `npm audit --json` exited1 and reported **12 vulnerable packages: 4 low, 3 moderate, 3 high, 2 critical**. This is a dated observed result, **not 12 independent advisories**, not a fresh October19 audit, and not proof of runtime exploitability. Propagated `via` entries and duplicated advisory references contribute to package classifications. Audit metadata's 407 dependency entries and npm ci's 305 audited packages have different reported units and are not equated.

The preserved lock context classifies eight affected package names as dev: `@vitest/mocker`, `esbuild`, `nanoid`, `source-map-js`, `tinypool`, nested `vite`, `vite-node`, `vitest`. Four production math-rendering package names are all low severity: `katex`, `micromark-extension-math`, `rehype-katex`, `remark-math`. Multiple installed paths for one package name do not create additional package/advisory counts.

Applicability checked against committed configuration:

- `package.json:11` runs `vitest run` then Node bridge tests; `test:watch` is `vitest`. `vitest.config.ts:3–13` configures jsdom/setup/exclusions, without an explicit UI/API server setting. These inspected commands/configuration do not demonstrate the audit's listening Vitest UI-server precondition. They also do not disprove alternate invocations or every transitive exploit path. Critical Vitest/Tinypool labels remain unresolved development-tooling security debt, not a demonstrated desktop remote-code-execution vulnerability or a finding dismissed because it is dev-only.
- The audit's Vite nodes are nested under `vitest` and `vite-node`, both **5.4.21**. The frozen lock's root `node_modules/vite` is **6.4.3**. Do not silently apply the nested5.4.21 finding to the root development-server version. `vite.config.ts` fixes port1420/strictPort without an explicit host; `tauri.dev.conf.json` uses `http://localhost:1420`. This is configuration evidence, not a network exposure test or assurance against all dev-server issues.
- Production math packages do have a real render consumer: `conversationShare.ts:4,6,37` imports remark-math/rehype-katex and renders with MathML and `trust:false`. The preserved KaTeX advisory explicitly concerns **existing prototype pollution** bypassing trust restrictions. `trust:false` therefore cannot dismiss that conditional advisory; this completion neither establishes an attacker-controlled prototype-pollution path nor proves absence of one.
- Release Tauri config embeds `../dist` with the recorded CSP rather than configuring the development URL. This supports keeping development-tool-server and shipped UI discussions separate. It is not a full dependency reachability, bundle-content, CSP-bypass or exploitability audit.

Git confirmed both manifest and npm lock are **inherited unchanged** across BASE..frozenHEAD: package-lock blob `b18fe9092dca5aaab42df1149aa6167c18565579` at both ends; package.json blob `7b9d5138ec72539b5cc7e66e7165f491b401db2f` at both ends. Inheritance does not resolve the security debt. The report's `fixAvailable` Vitest5 major upgrade and math-package old-major/downgrade suggestions are untested npm metadata, **not an endorsed remediation**. No blind `npm audit fix --force`, major upgrade, downgrade or lockfile change is proposed as part of this source fixwave. Appropriate follow-up is a separate compatibility/reachability-informed dependency triage with explicit tests and clean-install validation for any eventual lock change. No network audit was rerun here.

## Residual and acceptance dispositions

- **N01 remains CURRENT:** unexpected Store/semantic failure after automatic compaction archives can leave archive/events or other partial effects. W05 Fix4 validates persisted postwrite context but did not add rollback. Manual compaction's atomic preparation/CAS/replay guarantees must not be generalized to automatic compaction or erase this residual.
- **Settings immediate-Escape failures remain UNKNOWN.** The preserved diagnostic, summarized in CURRENT/original review, found no event/register/callback trace or deterministic cause. Later passing full frontend runs do not prove harmlessness, fix, timing, sandbox or leak causation. I did not reread that diagnostic's full source or run its proposed experiment; the original reviewer did. No settings/hook/config changes are justified from this uncertainty alone, and the immediate one-key/top-layer rule remains binding.
- **W12 native cursor Minor remains open** as M2 despite the consumer guard. Preserved per-task CannotVerify/producer obligations resolved by later reviewed tasks do not imply native UI or real-service acceptance.
- **GUI physical STOP since October11 remains in force.** Old W06 smoke was partial on an older isolated HEAD; later W07/W09–W12/W11 and combined final native acceptance are UNEXECUTED. One separate earlier file-panel tool call returned **QUEUED**, with actual display unverified; do not say this continuation history contained no UI-tool invocation. This completion itself used none, and did not undo/resume or control the app. Both smoke roots and external dirty7/two working-only compatibility residues remain protected.
- **Real model/SSH/R/Micromamba/PBMC/GPU/SLURM and native GUI/installer acceptance remain unexecuted** for this final delivery. Deterministic mocks, ignored tests, successful release/NSIS creation and old partial smoke do not establish these outcomes. No macOS/Linux-native guarantee or actual CI pass is inferred.
- **Local versus CI:** archive evidence is Windows10.0.26300, Node24.16/npm11.13, Python3.10, Cargo/Rust1.97; committed CI specifies Node22/Python3.12. Their equivalence was not tested here. CI-order repair does not retroactively create CI execution evidence.

## Validation evidence disposition and closure requirements

The archived validator's report is external evidence, not completion-reader execution. It records actual npm ci, fmt/metadata, retained first Rust101/missingdist with zero tests, Web prerequisite, unchanged-source Rust retry **1916 pass/0 fail/17 ignored**, Python48, frontend **1438 Vitest +22 Node bridge**, and native release/NSIS build terminal0. W08 normal5/5, intentional fault4/1 with nonzero/complete JSON, and exact restore/normal proof remain distinct; the fault was HEAD plus one deliberately mutated fixture. No result count is inflated with build modules, ignored/empty targets, reruns or helper diagnostics.

Archive proof checked all1118 tracked Git blobs, fresh archive/compiled roots, workspace manifests/dep-info and source restoration; registry/download cache reuse and untraced npm cache/network split mean neither wholly cold nor offline. This completion read the report rather than rehashing every archive file or rereading every log. Protected external-only changes were excluded from that committed archive.

The controller's seven adopted working gates remain **ADOPTED PRE-REVIEW**, with the recorded no-source-write premise, missing independent early raw event trace, candidate10 during Rust and1121-file manifest after Rust/Python. Working1935/18ignored versus archive1916/17ignored has preserved target-level explanation, not an individually reconstructed missing-test inventory. These are validly distinguished evidence classes, not forensic or fresh postreview guarantees. Subsequent relevant fixes invalidate affected-byte coverage until the required final checks are recorded; existing passes do not close I1–I3.

**Requirements verdict: With fixes. Ready to merge at frozen SHA: No.** Complete-package strengths and all50 independent task closures are preserved from the original reviewer, but the three integration/contract defects remain open. Save this original+completion handoff before the single authorized fixwave; then use one scoped rereview of the actual fix package and affected checks, retain every deferred/security/acceptance boundary, and incorporate the permanent tracked delivery record. This report does not claim that a queued ruling, future commit, title edit, or subsequent fix has already passed review.

**Declined to judge:** runtime exploitability or absence of dependency attacks; unexecuted real services/scientific validity; native GUI/installer/macOS/CI behavior; causal origin of unexplained settings/environment warnings; and technical approval of source after frozen bcec075. Reasons are limited personal scope, preserved conditional evidence, missing runtime traces, or explicit unexecuted acceptance. No unresolved factual uncertainty blocks handing the saved review to the authorized fix writer; the open findings and residuals themselves remain recorded.

<!-- END EXACT final-wholebranch-review-completion-oct19.md -->

## Root saved-review acceptance and dispatch observations

POST47-O05; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:308; SHA256 `44e15ad6b619c41f8993fad7cc5dbdd91dcb0a8b36fe06766d8deb50861ac48a`. Exact original root observation (not a new ruling):

> 2026-10-19 final frozen review accepted for fix dispatch: root WHOLE-read final-wholebranch-review-completion-oct19.md (19614bytes SHA2561593551a447f1e89fe9afed57f905bf5ed2e7859df6a810e1d1cb4f01c2280ae) after original18192-byte review (SHA1e00b7e0650689cfbbcfb87be21f0fd35dbbc8dcc3d808312ee032c8ec5164d9). ThreeImportant independently sourcechecked; eightMinors/security/N01/settingsUNKNOWN/nativeSTOP/queuedpanel/CI/provenance limits explicit. Original strongest reader's complete55473-line/173path coverage remains original attribution; fresh strongest completion reader actualtargetedreads only. Verdict With fixes/OPEN atbcec075; no sourceclosure claimed. No factualblocker. Root dispatched sole productwriter /root/final_fixwave_oct19 (gpt-6.1-sol high standardintegration) with final-fixwave-brief-oct19.md; sourceBASEbcec075 frozen, finalfixreport/scopedreview/affectedchecks pending. No secondfixer or GUI.

POST47-O06; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:310; SHA256 `899be3282c4114ad4d379bfbe9a0e16d1b4006ec9e833569ae3397a4bb22aa9d`. Exact original root observation (not a new ruling):

> Root WHOLE-read correction-report-oct19.md (6541bytes SHA256a7170bf6b5f1624295ce904e98a1bcadeb8f7e674a0c3475b5a9c5649b2022ab): two corrections applied,129historical report/review hashpairs embeddedMD/0mismatch, appended4original paragraphs preserveallreasoncost andexactSOURCE47appendix285151bytes. Newcandidatesmain130283B SHA819da0f901c8186450f3550fcccad679625c6e2466215a9defa6fc14eb099fb0/rulings309427B SHAd00f41e8576ac68ad0cc032d9a8dc7e60e7c99b2008c91eb23981fb62c6d8c4d/coverageMD88032B SHA997085dae0c6b8460acf20085f383508625380f9a36a0f630ead374defbfde68. This snapshot is preparation, not permanentcertification; actualreviewcompletion/fix/scoped/affectedvalidation/securitymustappend beforetracking. Recordcorrectionworker DONE, ownsno productfiles.

Final-fix evidence placeholders: **PENDING_FINAL_FIX_REPORT / PENDING_FINAL_SOURCE_FIX_FORMAT_DOCS_IDS / PENDING_SCOPED_REVIEW / PENDING_AFFECTED_VALIDATION / PENDING_CONTROLLER_COVERAGE / PENDING_PERMANENT_AUTHORIZATION**. No source commit, test result, scoped approval, final deliverability or cleanup authorization is supplied by this documentary append.


## Unified final-wave implementation report — exact RAW; independent closure PENDING

Unified final-wave writer reports DONE_WITH_CONCERNS at source54196dcee83218b6a808eae91ba7108259879b63 (18owned paths), separate pure-format3Rust commit a993efaaaa8e1157b57d28d03cb6b9268b3e4149, tree24002241e730bbe44df2b9fc2faf0e79d48a1a9f. Root WHOLE-read the report and dispatched independent scoped review plus provenance audit; both remain ACTIVE/PENDING at this snapshot. Implementation and affected gate outcomes are reported evidence, not accepted finding closure, late-allowlist/timing adjudication or permanent certification.

Late documentation RAW bridge: tested acceptance/README.md SHA256 ce42beb47a8f84e2b82ef4aaefdaa7c173598676aee4b0cc21753fe392f8df9c was normalized during the documentation helper edit. After gates/commits it was repaired to14b230d496ca02efa8ef739b7ead9e4b38a553dc1a385a127d004848e079ee67. Saved1121-file manifests differ only on that path; all817 equal/unowned lines and original external18 RAW (SHA256 bc0c7ca2cce83a40512374edb48e5018d3a923f9f5dfba0c3191d305c4836fbe) were restored by the approved bridge, canonical owned/HEAD/product inputs unchanged and other6 protected RAW preserved. Tests ran BEFORE this RAW repair; no whole-RAW-manifest-unchanged or postrepair-test claim. Root final provenance adjudication is pending.

npm outcomes remain distinct: initial full exit1,1439pass/1fail with Node bridge NOT REACHED; unchanged exact focused diagnostic exit0,1pass/27skipped; ONE authorized full retry exit0,1440Vitest+22Node PASS. The original publication child-dialog wait failed BEFORE Escape, and cause remains UNKNOWN; no timing/leak/access/harmlessness/fix conclusion, and it is separate from historical Settings UNKNOWN. No third blind retry or timeout/assertion weakening is claimed.

Late owned-path allowlist was saved after edits; actual pre-edit complete manifest/RAW7 and authorized brief/proposal/specs are distinct evidence. Closest UI/native-export sidecars share21:44:15 to-second end/start and lack proved subsecond nonoverlap; delivered UI terminal had not arrived before the next launch. Final gates retain their actual terminal chronology. No root final adjudication is invented for these limits. Prior archive npmci/Python48/W08 normal-fault-restore are retained on recorded unchanged committed inputs, not new October19 execution or a new final-commit archive. Native build reused current target/cache and is not a cold build, GUI/install/live/CI acceptance.

Fix-report byte identity: original RAW24144bytes/SHA25600cdbc46b207e81f1209f788f338bb87e0f94f4d53fbdabe4c2f23e6b3a12ba3 contains107 ordinary CRLF pairs and0 CRCRLF. Canonical CRLF-to-LF projection24037bytes/SHA256e57623ff1d9108b5969ec24281b6568a5ec471a86a0441838ccdcb201b471bb5 is a different byte representation. Earlier writer/root size24037 was the LF canonical count labeled as RAW; those primary paragraphs remain verbatim as history. This correction worker initially inferred CRCRLF from rendered output, then directly counted bytes and corrected that inference. The exact RAW report below is preserved; any later tracked Git normalization requires explicit projection/span verification and must not claim the same RAW span without checking.

<!-- BEGIN LOSSLESS LF PROJECTION final-fixwave-report-oct19.md -->

This tracked span is the explicit 24037-byte LF projection of original24144-byte CRLF RAW; SHA256 e57623ff1d9108b5969ec24281b6568a5ec471a86a0441838ccdcb201b471bb5. LF-to-CRLF reconstructs the original RAW/hash exactly.

# Unified final fixwave — October 19, 2026

Status: **DONE_WITH_CONCERNS**, implementation and required affected deterministic checks complete; independent scoped review and permanent delivery record remain root-owned. This writer does not certify merge readiness. BASE `bcec075d791366985e9e3cb0ef1886b63361f4e0`, branch `codex/wisp-science-improvements`.

Source commit: **`54196dcee83218b6a808eae91ba7108259879b63`**. Separate formatting-only commit: **`a993efaaaa8e1157b57d28d03cb6b9268b3e4149`**. Final tree: `24002241e730bbe44df2b9fc2faf0e79d48a1a9f`. Source candidate tree: `8cdd230bfd7126f03943414a20b27bca14821a31`. Implementation and canonical source remain identical to final checked bytes. After gates/commits, acceptance-only RAW terminators were repaired under a separately approved exact documentation-byte bridge; no Git canonical delta or gate rerun. Acceptance retains its external18 RAW and the other protected external paths/residues remain unstaged.

## Implemented requirements and boundaries

| Finding | Implemented change and verification |
|---|---|
| WB-I1 | Moved the ONE existing CI standalone Web step before workspace Rust, retaining every npm ci/Python/V4 scenario/always-upload report/Rust/npm test step. AGENTS/current testing guidance and explicit W08 helper-plan order agree. `ci-order-proof.json` proves exact single existing-step move and line preservation; it is structural verification, not a new product test or actual CI execution. Final standalone Web0 precedes full locked Rust0. |
| WB-I2 | All three actual chooser-to-writer paths preserve the exact returned path and reject absent/wrong expected suffix before writing, case-insensitively. Diagnostics retains fixed `diagnostics_save_failed`, conversation retains fixed suffix error, publication retains fixed Markdown error. No `set_extension` remains in those entries. Conversation chooser/writer logic was extracted unchanged first to a real production seam consumed by the native command, allowing fake chooser inputs without native GUI. New temp-directory regressions verify uppercase valid suffix + returned exact path/bytes, extensionless selected existing file and suffixed sibling both untouched, wrong suffix/no created target, cancellation/no added file and no target rewrite. Existing serialization/atomic persistence tests pass. No new export framework. |
| WB-I3 | Explicit Desktop `searchOpen` → Shell `globalSearchOpen` suspends both Runtime and kernel Tab eligibility while Search is visible; remote-jobs suspension remains, Runtime remains above lower trajectory. Search has scoped bounded Tab handling inside its actual surface, leaving higher navigation decisions outside its event path. Existing focus restoration returns the exact previous Runtime/child control. Trajectory Escape registration is suspended while Runtime is visible rather than inferring priority from DOM/mount order. Actual Desktop + real Shell parameterized parent/kernel regressions open frozen Runtime from trajectory, shortcut-open Search, test lower window traps suspended, forward/backward wrap within Search, window Escape closing only Search, exact focus return and restored Runtime/child Tab ownership. Existing trajectory/remote-job regressions are retained in closest354/full1440. |
| WB-M1 | Current-facing acceptance, bundled-science and scripts guidance links delivered deterministic scenario docs and distinguishes manifest/runner/report delivery from unexecuted real/native acceptance. Dated historical failure/implementation records remain. |
| WB-M5 | Only narrowed the baseline test title to deferred text and mismatched diff responses. Missing deferred-diff after close/scope change and pagination append/dedupe/supersession coverage remains OPEN test debt. |

Existing runtime, diagnostics and evidence-workspace design specs were updated at root-approved exact paths because these are behavior/boundary changes. No DTO/schema/migration/dependency/lock change or external-only API dependency was introduced. The source allowlist is the18 paths in `owned-paths.json`/`candidate-proof.json`.

## Root decisions: original reasons and costs preserved

- **I1 inherited order repair:** pristine committed archive Rust101 executed zero tests because `generate_context!` needs generated dist; bare Cargo does not invoke Tauri beforeBuild. BASE inherited this order. Cost is making the existing Web toolchain/build a prerequisite plus ordering/document changes. Original archive compile101 → Web0 → unchanged-source Rustretry0 remains history; local Node24/Python3.10 does not establish CI22/3.12.
- **I2 exact chooser boundary:** appending a suffix can overwrite a sibling whose destination/overwrite was not chosen. Cost is requiring an explicit correct suffix for extensionless host returns, changing two inherited entries as well as diagnostics, plus connected regressions. Cancellation and legitimate explicit targets remain. No GUI reconfirmation, broad legacy cleanup or export framework.
- **I3 explicit ownership:** Desktop owns global Search; Shell previously suspended only for remoteJobs and kernel was always eligible, while Runtime/trajectory registration order conflicted with visual priority. Cost is one App/Shell prop, parent/child eligibility and localized Escape correction with actual composition regressions. Incorrect ownership can displace/trap focus; Search's DOM-scoped handler does not consume a higher decision's events. No general modal framework, GUI acceptance, or Settings UNKNOWN cause/fix is claimed. Root approved this source-backed proposal before implementation.
- **Minor disposition:** M2/M3/M4/M6/M7/M8 and remaining M5 coverage do not establish another load-bearing first-scope failure in this wave; M8 needs deliberate resource policy/long-existing-message compatibility, with no demonstrated crash. Cost is retaining listed usability, maintenance, test, warning/performance and resource debt for deliberate follow-up, including possible excessive current-source materialization. No claim those findings were eliminated.

## Actual validation

All commands ran from `E:/Project/OmicsOps`, using existing installed dependencies and default current-repository `target` debug/release outputs. This is WORKING-CHECKOUT tested implementation coverage, not a new pristine archive or cold/offline build; the acceptance-only RAW terminator bridge below is explicit. Registry/download/compiler caches were available. No smoke-source artifact was offered as proof of final coverage; final commands have complete prelaunch source manifests.

`native-artifact-provenance.json` resolves483 top-level release dep-info paths under this repository, including changed native3 and no smoke/archive source references. Acceptance README is not a declared native input. Release exeSHA2569f6fa38ac0381f60af1cdc5be937f28d1537c0287d9276c0ba05d6de5947de31; NSISSHA256e6c9f4ecb65a275bfd4b06df0ad991d4dd055a1760e491e20ace0064963c296c. This binds actual cwd/source manifests/log/dep-info/output paths; existing target caches were reused and this is not a new cold target, complete --extern dependency or binary-content reconstruction audit. Declared directories are represented as directory metadata, not recursively rehashed contents.

| Check | Actual outcome |
|---|---|
| I2 behavioral RED normal access |3 failed/0 passed, child101: each extensionless selection returned success at the unchosen suffixed sibling. |
| I3 behavioral RED corrected completed-run fixture |2 failed/0 passed: lower Runtime/kernel window trap wrongly prevented Search Tab. |
| New chooser GREEN |3 passed/0 failed; overlaps export filter/full Rust, never summed as extra unique coverage. |
| Native export closest |11 passed/0 failed including original serialization/atomic/cancel/error checks; overlaps chooser/full Rust. |
| UI closest |5 files/354 passed/0 failed, including2 new actual composition cases and retained trajectory/remoteJobs. |
| Formatting |Initial check1 solely owned3 formatting; immediate preformat RAW18 captured; apply0 changed only owned3; finalcheck0. Actual edition2024 rustfmt on scratch preformat candidates exactly reproduces final3 bytes. |
| Final standalone Web |exit0,2051 modules; largest JS1604.28kB/gzip487.12kB; modules are not tests. |
| Final locked workspace Rust |exit0,1938 passed/0 failed/18 ignored across90 summaries;22 zero-executed targets. Desktop732/11 ignored. No ignored/empty target or rerun is counted as live acceptance. |
| First full npm |exit1,128 files:127 pass/1 fail;1440 Vitest tests:1439 pass/1 fail. Node bridge NOT REACHED because command uses &&. |
| Bounded diagnostic |Unchanged exact publication `escape_top_layer` case1 pass/27 skipped. This does not diagnose/fix the original failure. |
| One authorized unchanged full npm retry |exit0,128 files/1440 Vitest pass +22 Node bridge pass. Separate from1439/1, not additive and not proof failure was harmless/fixed. |
| Final native build |exit0, release plus NSIS build only. Intrinsic beforeBuild Web invocation belongs to native build, not a second standalone Web gate. No launch/install/distribution. |
| Final whitespace |`git diff --check` exit0 before subsequent canonical-identical acceptance RAW repair; staged candidate checks also0. |

Full command/PID/time/exit table follows. Every `NAME.json` points to its exact source manifest and sibling complete log with SHA256; `index.json` binds them all, with current-source comparison and parsed Rust counts. Earlier fmt/Web/Rust/closest records use second-resolution times/child PID only; later npm/native records use fractional times and runner+child PID. No retroactive timestamps/process identities were invented.

| Evidence label | Actual argv | Child PID | Start → end | Child exit |
|---|---|---:|---|---:|
| i2-red | `cargo test --locked -p omicsops-desktop --lib chooser_keeps_exact -- --nocapture` | 33184 | 2026-10-19T21:36:40+0800 → 2026-10-19T21:39:04+0800 | 101 |
| i3-red | `npx.cmd vitest run src/DesktopApp.test.tsx -t global Search owns focus` | 11484 | 2026-10-19T21:39:59+0800 → 2026-10-19T21:40:05+0800 | 1 |
| i3-red-corrected | `npx.cmd vitest run src/DesktopApp.test.tsx -t global Search owns focus` | 4292 | 2026-10-19T21:40:16+0800 → 2026-10-19T21:40:21+0800 | 1 |
| i3-red-valid-fixture | `npx.cmd vitest run src/DesktopApp.test.tsx -t global Search owns focus` | 37884 | 2026-10-19T21:40:29+0800 → 2026-10-19T21:40:35+0800 | 1 |
| i2-red-normal | `cargo test --locked -p omicsops-desktop --lib chooser_keeps_exact -- --nocapture` | 32012 | 2026-10-19T21:40:50+0800 → 2026-10-19T21:41:06+0800 | 101 |
| i3-red-completed-run | `npx.cmd vitest run src/DesktopApp.test.tsx -t global Search owns focus` | 34020 | 2026-10-19T21:41:13+0800 → 2026-10-19T21:41:18+0800 | 1 |
| i3-green | `npx.cmd vitest run src/DesktopApp.test.tsx -t global Search owns focus` | 35844 | 2026-10-19T21:41:52+0800 → 2026-10-19T21:41:56+0800 | 1 |
| i3-green-complete | `npx.cmd vitest run src/DesktopApp.test.tsx -t global Search owns focus` | 34900 | 2026-10-19T21:42:08+0800 → 2026-10-19T21:42:13+0800 | 0 |
| i2-green | `cargo test --locked -p omicsops-desktop --lib chooser_keeps_exact -- --nocapture` | 36288 | 2026-10-19T21:42:22+0800 → 2026-10-19T21:43:01+0800 | 0 |
| ui-closest | `npx.cmd vitest run src/DesktopApp.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/RuntimeDialog.test.tsx src/features/workspace/WorkspaceSearchDialog.test.tsx src/features/workspace/WorkspaceFileBaselinesPanel.test.tsx` | 37600 | 2026-10-19T21:43:39+0800 → 2026-10-19T21:44:15+0800 | 0 |
| native-export-closest | `cargo test --locked -p omicsops-desktop --lib export -- --nocapture` | 36376 | 2026-10-19T21:44:15+0800 → 2026-10-19T21:44:17+0800 | 0 |
| fmt-before | `cargo fmt --all -- --check` | 36996 | 2026-10-19T21:44:32+0800 → 2026-10-19T21:44:35+0800 | 1 |
| fmt-apply | `cargo fmt --all` | 35688 | 2026-10-19T21:45:11+0800 → 2026-10-19T21:45:13+0800 | 0 |
| fmt-final | `cargo fmt --all -- --check` | 5552 | 2026-10-19T21:45:27+0800 → 2026-10-19T21:45:29+0800 | 0 |
| web-final | `npm.cmd run build` | 14412 | 2026-10-19T21:45:32+0800 → 2026-10-19T21:45:42+0800 | 0 |
| rust-final | `cargo test --locked --workspace` | 5576 | 2026-10-19T21:46:14+0800 → 2026-10-19T21:51:31+0800 | 0 |
| npm-final | `npm.cmd test` | 35876 | 2026-10-19T21:51:53.322538+08:00 → 2026-10-19T21:52:37.865308+08:00 | 1 |
| npm-failure-focused | `npx.cmd vitest run src/features/workspace/WorkspacePublicationEvidence.test.tsx -t escape_top_layer` | 31944 | 2026-10-19T21:53:28.795721+08:00 → 2026-10-19T21:53:31.028062+08:00 | 0 |
| npm-final-retry | `npm.cmd test` | 36552 | 2026-10-19T21:54:09.311221+08:00 → 2026-10-19T21:54:51.688258+08:00 | 0 |
| native-final | `npm.cmd run build:desktop` | 30756 | 2026-10-19T21:56:15.832345+08:00 → 2026-10-19T22:01:03.061141+08:00 | 0 |
| diff-final | `git diff --check` | 36328 | 2026-10-19T22:01:32.782716+08:00 → 2026-10-19T22:01:33.077889+08:00 | 0 |

## Unexpected failure, diagnostic and process limits

The first full npm failure is **WorkspacePublicationEvidence.test.tsx:146**, `escape_top_layer: immediate window Escape closes the real report while the real parent remains open`: it failed **BEFORE any Escape**, waiting for `Saved revision evidence` after clicking Check. An early parent message incorrectly called this a late-report test; this report corrects that title without changing the original log. PublicationPage/test, ResearchPagePrimitives, shared BrowserSettings Escape hook, setup and Vitest configuration are byte-unchanged from pre-task. The isolated case does not compose Desktop/Search/Runtime. Source shows synchronous evidenceOpen and an invalidation effect, but no retained boundary trace proves which branch occurred originally. Root authorized exactly one unchanged focused normal-access diagnostic and one full normal-access retry. Later1pass/full1440+22pass does not prove timing, load, leak, harmlessness, root cause, or a fix. **Cause remains UNKNOWN**, separately from earlier Settings UNKNOWN. No source/config/test/timeout change or weakened immediate-Escape assertion was made; no blind third full run.

Other retained noncausal histories: initial sandbox native RED had OS5 persistence errors, then unchanged ordinary Windows access produced genuine3 causal failures; wrapper printed terminal metadata then encountered a GBK console encoding error (child101 preserved separately from wrapper1). Initial UI fixtures used textbox instead of searchbox and a nonterminal frozen-run event that left the composer disabled; these are fixture failures, not product RED. One quoted Python edit failed syntax without mutation. Initial focus-edit script encountered sandbox Err22, so a partial GREEN attempt remained2failed until approved edits were applied under ordinary access. Scratch external-delta comparison initially included shifted hunk coordinates; corrected zero-context payload comparison accompanies independent exact inverse/replay proof. Initial scratch rustfmt replay used2021 and failed; corrected actual workspace2024 replay passes with no source/index mutation from failed helper. The first index assembly had a missing Path alias NameError; that ignored helper was corrected before final index/report generation, with no source/index or gate change. These helpers/errors are not counted as product tests or erased.

The bounded native dep-info helper initially tried reading a declared capabilities directory as a file (Err13); corrected directory representation retains that limit. This was a proof-helper diagnostic, not a native build/test failure or source change.

At the closest UI→native-export boundary, both sidecars have21:44:15 to-second end/start and the UI terminal had not yet been delivered when the next command launched. There is **no proved subsecond nonoverlap**; do not call that boundary an instrumented serial history. No affected source changed at this boundary and both outcomes are retained. Final gates awaited actual delivered terminal before next heavy command, with the explicit timing limits above. No broad process kill or cleanup occurred; read-only known-process inspection did not control other apps.

## Exact preservation and staging

- Actual pre-edit `before.json`/`before-manifest.json`/RAW7/acceptanceHEAD/external-before.diff captured BASE, branch, empty index and all1121 tracked/protected paths. All7 RAW hashes matched October15. The saved allowlist file itself was written **after** edits; authorization came from the brief, proposal and root-approved specs. Root was told this chronology; do not claim a saved pre-edit allowlist. Cost is reliance on the actual complete pre-edit manifest/RAW7 and exact final diff/candidate audit.
- Final changed source set is exactly18 owned paths. Protected6 RAW byte hashes remain equal; acceptance intentionally changes RAW from `2295684ce021d013dbdde0de3c467d4864ac10de45f6158452d40791b9f86a1a` to final repaired `14b230d496ca02efa8ef739b7ead9e4b38a553dc1a385a127d004848e079ee67`. Intermediate tested normalized RAW `ce42beb47a8f84e2b82ef4aaefdaa7c173598676aee4b0cc21753fe392f8df9c` is preserved separately, not relabeled as final RAW. Its canonical owned candidateSHA256 `3d2e0077a7285da0dd1b1e9d75badb5346c49f194cb5e44e80f7b2225ca1dbf7` excludes original18 external additions. `git merge-file` candidate and exact inverse/replay succeed; zero-context external payload is exactly preserved18 additions. No whole-file acceptance add, stash/reset/clean, or external content change.
- Source index used exact owned-only candidate blobs via hash-object/update-index, not dirty acceptance working bytes. Source candidates use the captured exact preformat Rust3; formatting-only commit stages finalRust3 separately, with rustfmt2024 transformation proof. Cached allowlist/check and final candidate/tree/HEAD canonical equality are retained in stage-entries/source-staged.diff/format-staged.diff/post.json. Index empty postcommit.
- Both intentionally working-only initializer residues remain in byte-unchanged protected Rust files. Original external untracked files and external Rust hunks remain; acceptance external18 remains the only difference for that shared file after commit. Tests of working source include those external tests/residues, while the old committed archive excluded them; no equivalence claim or uncommitted API dependency.

**Repaired RAW-preservation defect, not erased:** late self-audit found the documentation edit helper had normalized acceptance's646 original CRLFs toLF. Canonical/external payload proof was valid but did not prove original RAW terminators. Before mutation root whole-read/independently recomputed the ignored repair candidate and authorized its exact application: all817 equal/unowned lines copy captured original RAW verbatim, including all18 external added lines181–198 (RAWsha256bc0c7ca2cce83a40512374edb48e5018d3a923f9f5dfba0c3191d305c4836fbe); only7 approved owned diff operations differ. Replacements use their explicit original terminators, giving647CRLF due one added owned line. CandidateSHA25614b230d496ca02efa8ef739b7ead9e4b38a553dc1a385a127d004848e079ee67 exactly matches final working RAW. Canonical bytes remain identical to tested after and HEAD+external18; other6 protected RAW/indexempty/seven external status paths rechecked. `acceptance-raw-repair-proof.json` and `raw-repair-applied.json` retain preproof and actual application. Original tested manifest/post/index/report are preserved; new postrepair manifest differs **only acceptance README RAW terminators**. Cost is this restoration, additional root audit and explicit source-byte/provenance bridge. No Git canonical/product code change or extra commit, no rerun of valid checks solely for this noninput document-byte repair. Do not claim the whole final RAW manifest stayed unchanged after repair; index gate entries expose the sole documentation difference and approved bridge.

## Retained old evidence and open debt

`index.json` contains exact committed-input equality mappings against bcec075 for npm-ci dependency inputs, Python helper/worker/bridge/Skills inputs and W08 scenario/runtime dependency inputs. The Oct15 npm ci304added/305audited, Python48 and actual W08 normal5/fault4+1 with nonzero/restore-normal are **retained old archive evidence on unchanged relevant committed inputs**, not new October19 execution or a new final-commit archive. Actual lock/manifests were unchanged; no repeat npmci solely for this wave. Python was not rerun. Final workspace Rust separately executes the ordinary scenario suite without claiming a new emitted report or fault injection. Protected external test-only agent-core changes remain working-only and are not silently added to the old archive proof. Original pristine Rust101/zero tests and Web0/Rustretry0 are retained.

Open explicit Minor debt: **M2** producer continuation >10000 needs truthful cap signaling (UI mitigation remains); **M3** publication finding source/related-record identity usability; **M4** compressed baseline JSX; remaining **M5** actual deferred-diff/pagination tests; **M6** duplicated safe fixture identities; **M7** warning/performance investigation; **M8** prefetch/aggregate current-source inspection policy with full-body identity and long-message compatibility. No demonstrated M8 crash was asserted.

Dated Oct15 npm audit remains unresolved:12 vulnerable packages (4low/3moderate/3high/2critical), not12 independent advisories and not fresh audit/runtime exploitability proof. Dependencies/locks unchanged; no blind upgrade/downgrade/remediation or runtime safety claim. Final runs observed Web>500kB chunk warning, npm update notice and Rust linker stdout warning; do not claim warning-free or that all historical OS5/hardlink/incremental warnings recurred. Earlier whatwg-encoding deprecation/audit findings remain historical.

N01 automatic-compaction partial-effect/rollback limitation remains CURRENT. Historical Settings immediate-Escape failures remain UNKNOWN. Original strongest reader's55473-line/173-path whole-package coverage remains attributed to that reviewer; this writer read original18192B+completion19614B distinct reports and does not claim a new whole-branch review. Root will conduct the scoped fix review after this report.

Future manual smoke, **UNEXECUTED under STOP**: in a disposable native project open Runtime from trajectory, then global Search by Ctrl/Cmd+K; cycle Tab/Shift+Tab at both ends, press one window Escape, verify Search alone closes and original Runtime focus returns; repeat with kernel metadata and with remote-jobs ordering. In a disposable directory for each diagnostics JSON/HTML, conversation HTML/PNG and publication Markdown export, choose an uppercase valid suffix, cancel, and choose an extensionless target with synthetic preexisting target+suffixed sibling; verify fixed error and both synthetic files unchanged. Check actual clean Windows CI22/Python3.12 sequence separately. These are pending manual/CI procedures, not completed acceptance or authorization to resume UI.

GUI physical STOP sinceOct11 remains; this writer invoked no GUI/browser/open_in_codex/panel/app/CLI app launch/close/control. Historical queued-panel incident remains separate history. Both target/wisp-native-smoke roots preserved; no installer launch/install/distribution, real model/SSH/R/Micromamba/PBMC/GPU/SLURM, native GUI/macOS/Linux-native or actual CI22/Python3.12 acceptance. Local observed tools: Windows, Node24.16.0/npm11.13.0/Python3.10.0/Cargo+Rust1.97/Git2.53. No subagent/worktree/chat/goal/PR/push/tag/release, no broad cache cleanup or dependency mutation.

Machine-readable primary index: `final-fixwave-proof/index.json`, SHA256 **`4f1a394be1e8432e6017da87ca34af920b3f2b87f0ebdc39e5b07776a5dd45b0`**. It points to all exact logs, sidecars, source/format candidates and pre/post hashes; preserved files remain available for root's independent audit and permanent record.

<!-- END LOSSLESS LF PROJECTION final-fixwave-report-oct19.md -->

Final-wave machine index `.superpowers/sdd/2026-10-04-wisp-improvements/final-fixwave-proof/index.json`:230961bytes/SHA2564f1a394be1e8432e6017da87ca34af920b3f2b87f0ebdc39e5b07776a5dd45b0. Coverage contains all21 actual terminal argv/source-manifest/time/exit/sidecar/log fingerprints; raw logs are not embedded. Historical129 task fingerprints and2 frozen final-review inputs remain distinct from this new implementation report/index.

POST47-R08; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:312; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:164; exact original primary paragraph:

> Ruling: implement WB-I3 with explicit global Search ownership from DesktopApp through WorkspaceShell to both Runtime and kernel focus eligibility; preserve Runtime above the lower trajectory and suspension for remote jobs. Search owns bounded Tab navigation inside its actual surface and restores its previous control on close; its trap must not steal focus from an actual higher navigation-decision/child while guarded navigation is pending. Suspend the lower trajectory Escape registration while Runtime is visible; retain Runtime, child and Search layers in their actual visual order. — Why: source confirms global Search state is owned in DesktopApp, Shell currently only suspends Runtime for remoteJobs, kernel is always eligible, and mount-order Runtime/trajectory registration conflicts with visual ordering. Explicit ownership addresses the confirmed composition without inferring visual priority from DOM/mount order or adding a general modal framework. — Cost: a Search-open prop crossing the existing App/Shell boundary, updated parent/child focus eligibility and a localized Escape-order correction; actual DesktopApp+realShell parent/kernel shortcut/Tab/ShiftTab/immediateEscape/focus-return regressions plus existing trajectory/remotejobs coverage are required. Mistaken ownership can trap or displace keyboard focus, so higher-child behavior must remain intact. No GUI acceptance or existing SettingsUNKNOWN cause/fix is claimed. Root approved the sole writer's source-backed proposal before implementation; I1/I2 continued independently.

POST47-O07; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:314; no exact CURRENT alias present at this snapshot; exact original primary paragraph:

> 2026-10-19 finalwave related-doc scope acknowledgement: root approved existing runtime-design, diagnostics-example-design and evidence-workspace-design current behavior/boundary updates for the approved I3 ownership and exact chooser destination rules. This is AGENTS-required synchronization within the wave, no new abstraction/scope/ruling. Writer reports actual normal-access I2causalRED0pass3fail separate from initial sandboxOS5, and corrected-fixture I3RED0pass2fail→minimalfixGREEN2pass. Fixture/searchbox/completed-run and shellquotingdiagnostics retained, no finalsource/fullgate/commit/scopedclosure asserted yet.

POST47-R09; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:316; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:166; exact original primary paragraph:

> Ruling: after preserving final npm exit1 (1439pass/1fail,128files127pass/1fail, Node bridge not reached), permit ONE unchanged-source normal-access full npm retry with a distinct execution label following actual focusednormal0 (1pass/27skipped) and unchanged relevant-source proof. If that full retry fails, stop repeating and gather specific boundary trace/evidence before any fix or further run. — Why: the exact WorkspacePublicationEvidence escape_top_layer case fails at test146 waiting for the child dialog BEFORE any Escape; root/writer inspected the actual component/test, and relevant publication/test/primitives/sharedEscape/setup/Vitest bytes are unchanged. The normal focused pass supplies new bounded evidence but does not explain the full-run failure. — Cost: the original unexpected failure remains causeUNKNOWN and is not a claimed fix, timing/leak/sandbox conclusion or Settingsfailure. A successful later full run would establish its own actual final-byte outcome, not erase the failure; changed access is an execution condition, not proof of cause. No test/config/product mutation, weakening or timeout increase is authorized by this ruling.

POST47-O08; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:318; no exact CURRENT alias present at this snapshot; exact original primary paragraph:

> 2026-10-19 finalwave late preservation diagnosis (before DONE): writer found shared acceptanceREADME RAW had original646CRLF but documentation helper normalized working file toLF. Canonical candidate/inverse/replay/external18 body proof had passed, but this didNOT establish unchanged original RAW outside owned edits. Root acknowledged necessary ignored-candidate RAW repair using exact before.raw equal/unowned spans and original external18 terminators; candidate proof must be read before actualfilemutation. No canonicalGit/source implementation change, test/CI/GUI action or revisedPASS asserted. Preserve original testedmanifest and later explicitdoc-byte bridge, not wholemanifestunchanged claim after repair. Allother6protected remainuntouched. Pending candidateactualhash/invariants and rootadjudication.

POST47-R10; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:320; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:168; exact original primary paragraph:

> Ruling: restore only shared acceptanceREADME working RAW from the exact ignored repair candidate14b230d496ca02efa8ef739b7ead9e4b38a553dc1a385a127d004848e079ee67 after root independently verified it; retain original testednormalizedRAWce42beb47a8f84e2b82ef4aaefdaa7c173598676aee4b0cc21753fe392f8df9c and capture an explicit tested-to-repaired doc-byte bridge. No Git canonical/source change or repeated test/build is required solely for this non-runtime-input newline repair. — Why: late self-audit showed the documentation helper normalized all original sharedfileCRLF toLF, so earlier canonical/inverse/replay proof did not establish original unowned RAW preservation. Root WHOLE-read3925-byte repairproof and independently recomputed all817equal/unowned lines RAWexact, originalexternal18 RAWexact (SHAbc0c7ca2cce83a40512374edb48e5018d3a923f9f5dfba0c3191d305c4836fbe), canonicalcandidate equaltestedRAW and currentHEADdiff only18insertions/0removals; workingfile was stilltestedbefore mutation. New ownedline endings are explicitly anchored, not guessed. — Cost: the earlier normalization remains a disclosed repaired preservation defect, and the final wholeworkingmanifest differs from testedmanifest in this document's RAW only. Claims must distinguish canonical source match, originalunownedRAW restoration and actual laterpostrepair evidence; never say all7wholeRAW stayedunchanged or that tests ran after RAWrepair. Allother6protected remainuntouched and bothsmokeroots preserved.

POST47-O09; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:322; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:170; exact original primary paragraph:

> 2026-10-19 ONE finalfixwave writerDONE_WITH_CONCERNS: source54196dcee83218b6a808eae91ba7108259879b63 (18paths), separatepurefmt a993efaaaa8e1157b57d28d03cb6b9268b3e4149 (3Rust), finaltree24002241e730bbe44df2b9fc2faf0e79d48a1a9f. RootWHOLE-read final-fixwave-report-oct19.md24037B SHA00cdbc46b207e81f1209f788f338bb87e0f94f4d53fbdabe4c2f23e6b3a12ba3; machineindexSHA4f1a394be1e8432e6017da87ca34af920b3f2b87f0ebdc39e5b07776a5dd45b0. Root independentlyactualGitHEAD/tree/parents/owned18/status/indexempty checked; Rust90summaries1938pass0fail18ignored/loghashmatches, frontendretry128files1440+22Nodepass/terminal0/loghashmatches, native0/loghashmatches. Allgates terminal; no runningheavy/writerwritesplanned. Rawrepairactual14b230... aftertestsexplicitdocbridge, testednormalizedce42... andoriginalunowned/external18RAW preserved; originalfailure/helper/timing/lateallowlist/canonical-vsRAW limitations retained. Rootread-only PowerShell helpers had corrected HEAD^{tree} quoting and foreach-pipeline parse diagnostics; no source/index/testmutation, actualtreeverifiedwithliteralquotedrevision.

POST47-O10; authority .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:324; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:172; exact original primary paragraph:

> Finalscopedreview /root/final_scoped_review_oct19 gpt-6-astra high ACTIVE: review-bcec075..a993efa.diff97035B plus verbatim final-scoped-findings-oct19.md6549B and actualreport, scope3I/M1/M5claim+fixbreakageonly. Separate /root/final_provenance_audit_oct19 gpt-6.1-sol high ACTIVE checksactual21terminals/source/staging/format/retainedinputs/docRAWbridge. /root/record_corrections_oct19 gpt-6.1-sol high ACTIVE ignoredcandidateappend only. Rootcoordinatesno productfix. No finalfindingclosure/permanentwrite/cleanup yet; physicalGUISTOP unchanged.

PENDING_SCOPED_REVIEW / PENDING_FINAL_PROVENANCE_ADJUDICATION / PENDING_AFFECTED_VALIDATION_ADJUDICATION / PENDING_PERMANENT_AUTHORIZATION. No closure or cleanup authorization is inferred.


## Saved unified-wave scoped review and provenance audit — NB-I1 OPEN

Saved scoped review at frozen a993efaaaa8e1157b57d28d03cb6b9268b3e4149: WB-I1/WB-I2/originalWB-I3/WB-M1 and M5 title claim ADDRESSED, but new NB-I1 Important remains OPEN: Search Tab ownership traps focus below the higher unsaved-navigation decision. Root WHOLE-read and independently source-confirmed the boundary, and authorized ONE bounded same-implementer frontend refinement plus targeted independent verification under an explicit exception to the default final-wave cap. No50-task restart or broad review repeat. The bounded proposal is now root-approved by the appended primary Ruling; its actual source commit/checks/targeted verdict remain PENDING at this snapshot; permanent certification is not ready.

Saved independent provenance audit accepts the frozen a993efa source/evidence package WITH LIMITS after1300 static checks/0failed; root WHOLE-read accepts the late saved-allowlist basis (actual pre-edit1121/RAW7+approved scope+18candidate/inverse/replay), without calling the allowlist file predeclared. Closest UI→native subsecond nonoverlap remains unproved; later full gate coverage and recorded precision remain separate. Acceptance RAW bridge817+external18 and other6 preservation is accepted at that frozen scope. This is not behavior closure or approval of subsequent refinement bytes; relevant later changes require their own validation bridge.

Dated Oct15 execution evidence is retained via actual committed-source.tar1118entries/SHA25681aff47b27492cb8f27316169a0d5f749e853a167fe0c224e0b49c98952372c7, before/postfault-restored manifests,5npmci/Python/W08 sidecars+logs,3W08 proof snapshots/stdout and fixture restoration. Audit verifies262 projected inputs (npm2/Python227/W0833). Original target/final-archive-bcec075-oct15 execution directory and3 emitted target/evaluation reports are currently absent; the audit lists265 absent source/output paths. They cannot currently be reread there; no current-existence, actor/time/cause, recreated execution or cleanup-performer claim. Root/controller/writer report no performed/authorized cleanup in this continuation; attribution does not establish what removed those paths. Preserved dated execution and unchanged-input projections remain the evidence class.

Scoped reviewer ran one in-memory Vite diagnostic; its setup failed before reaching the NB-I1 Tab assertions because the asynchronous result was not awaited. It is NOT NB-I1 causal RED or a full rerun; no corrected probe or complete saved raw log is claimed. NB-I1 rests on saved source reasoning and root confirmation. Existing UNKNOWN npm publication wait failure, SettingsUNKNOWN, deferred Minors/security/native/live/CI limits remain explicit.

Both reports below are preserved verbatim as actual source bytes. They do not approve future refinement or permanent delivery.

### Exact saved final-fixwave-scoped-review-oct19.md

Original path `.superpowers/sdd/2026-10-04-wisp-improvements/final-fixwave-scoped-review-oct19.md`;14884bytes;SHA256`81268118cb14f1a361f5afa2ba5dc67b0d346575d77a08fc419150fc8b020d25`.

<!-- BEGIN EXACT final-fixwave-scoped-review-oct19.md RAW -->
**WB-I1 — Clean workspace Rust tests need generated Web assets** — **ADDRESSED**. `.github/workflows/ci.yml:33` moves the single existing standalone Web build immediately before workspace Rust (`:34`); npm ci, Python, V4 scenario, always-upload scenario artifact, workspace Rust and npm test remain. The package shows a single step move, no duplicate gate or dependency change. `AGENTS.md:113`, `scripts/README.md:48`, and `docs/superpowers/plans/2026-10-04-wisp-w08-skill-helpers.md:99` agree with the prerequisite. Acceptance's current command lists are reordered. The report preserves pristine Rust101/zero executed tests followed by Web0/Rust retry0 as inherited-order history and does not equate local execution with CI Node22/Python3.12 acceptance.

**WB-I2 — Native save entries rewrite the selected target and can overwrite an unchosen sibling** — **ADDRESSED**. `src-tauri/src/conversation_export.rs:82` is a real production chooser-to-writer seam called by the command at `:102`; it forwards the returned path unchanged to `write_export_bytes`, whose case-insensitive extension validation at `:64` precedes temporary-file creation and persistence. `src-tauri/src/run_diagnostics.rs:532` forwards the exact path to the same writer and maps rejection to the fixed diagnostics error; the actual dialog calls this seam at `:550`. `src-tauri/src/workspace_navigation.rs:881` checks the exact returned `.md` suffix before writing; the native command calls it at `:939`. No suffix addition remains in these three routes. Connected temporary-directory cases at `conversation_export.rs:127`, `run_diagnostics.rs:1424`, and `workspace_navigation.rs:1452` cover each supported format's uppercase suffix, exact successful return/bytes, existing extensionless target plus suffixed sibling both untouched, wrong suffix, cancellation, and final directory contents. Existing serialization and atomic replacement behavior are retained. This is production-seam coverage, not GUI chooser acceptance.

**WB-I3 — Runtime and kernel inspection trap Tab beneath global Search** — **ADDRESSED for the frozen finding**, with a separate **new Important breakage NB-I1 below**. `src/DesktopApp.tsx:1667` sends actual `searchOpen` through `WorkspaceShell.globalSearchOpen`; `src/features/workspace/WorkspaceShell.tsx:1396` suspends Runtime focus for Search and remote jobs. `RuntimeDialog.tsx:84` now gives the child the same eligibility, while `:107` still suspends the parent for its kernel child. The original lower window traps therefore cannot steal Tab/Shift+Tab from Search. Search's existing cleanup at `WorkspaceSearchDialog.tsx:97` restores the prior connected control. `WorkspaceShell.tsx:562` suspends trajectory Escape registration while Runtime is visible, matching visual priority rather than later DOM placement. The two actual Desktop/real-Shell cases at `src/DesktopApp.test.tsx:914` open Runtime from trajectory, optionally its kernel child, shortcut-open Search, exercise both Tab directions and boundaries, dispatch window Escape, verify Search alone closes and the exact prior control regains focus, then verify restored Runtime/child ownership. Existing trajectory and remote-job Escape cases remain at `WorkspaceShell.test.tsx:217`, `:271`, and `:289`. The new Search trap does not, however, establish highest-visible ownership when Search itself opens a higher navigation decision.

**WB-M1 — Current-facing W08 guidance says delivered deterministic work is pending** — **ADDRESSED**. `acceptance/README.md:24` and current working line `:695` (canonical committed line `:677`, excluding the preserved external18) distinguish delivered deterministic scenarios from real/native acceptance and link scenario guidance. `docs/bundled-science.md:145` and `scripts/README.md:66` do likewise. Dated helper-plan historical records retain their original execution state and chronology. The fixes do not claim real model/SSH/R/native or CI acceptance.

**WB-M5 — Baseline title overstates deferred-diff coverage** — **ADDRESSED as the explicitly scoped claim correction**. `src/features/workspace/WorkspaceFileBaselinesPanel.test.tsx:88` now says deferred text and mismatched diff responses, accurately matching its deferred text promise and immediately resolved wrong-manifest diff. **Remaining M5 coverage debt stays OPEN:** no added deferred-diff-after-close/scope-change or pagination append/dedupe/supersession cases. Renaming the test does not supply that coverage or close the underlying test debt.

## New Breakage in the Fix Diff

**NB-I1 — Important — Search's new Tab loop traps keyboard focus beneath the higher unsaved-manuscript decision.** New code at `src/features/workspace/WorkspaceSearchDialog.tsx:196`–`:205` handles every Tab bubbling from Search, calls `preventDefault`, and explicitly focuses another Search control without checking whether a higher navigation decision is visible. The DOM-scoped handler avoids intercepting an event that already originates in the higher dialog, but the actual higher dialog does not move focus there.

**NB-I1 trigger/effect —** Edit a manuscript without saving; open Search with Ctrl/Cmd+K; wait for a session result, select it using the focused search input and press Enter. `WorkspaceSearchDialog.tsx:154` starts the guarded action while leaving Search mounted. The actual navigation decision is `WorkspacePublicationPage.tsx:291`, rendered through `ResearchPagePrimitives.tsx:54`–`:59`; that component registers Escape but has no focus transfer or Tab management. Its backdrop is above Search (`workspace-research-pages.css:25`, z1200; `workspace-search.css:1`, z1100). Search's input stays enabled because `:236` uses `busy`, while the in-flight action uses `pendingKey`/`actionPending` and disables the result buttons at `:273` instead; Desktop supplies no `busy` prop at `src/DesktopApp.tsx:1462`. Thus focus can remain on the covered Search input. The newly introduced Tab/Shift+Tab handler cycles among Search controls beneath the decision, so the keyboard user cannot Tab to Save/Discard/Keep editing and can activate a covered Search close control. Escape cancellation still works; this does not remove the new keyboard trap.

**NB-I1 required correction —** Transfer keyboard ownership to the actual higher decision when it opens and suspend the lower Search trap for that interval, with focus restored appropriately when the decision closes. A localized explicit ownership/focus solution is sufficient; no general modal redesign is required. Add a connected Desktop/real-Shell regression using dirty manuscript → loaded Search result → keyboard Enter → decision, then Tab and Shift+Tab inside the higher decision and immediate Escape preserving Search/restoring its usable focus. Keep the existing Runtime/kernel/Search/trajectory/remote-jobs regressions. `src/DesktopApp.test.tsx:1027` currently uses clicks to open the result and tests only Escape/retry; it does not cover this keyboard transition. The runtime design's higher-decision statement and implementation report must be reconciled with the corrected behavior rather than relying on DOM event scoping alone.

**New-breakage remainder —** No other new Critical/Important/Minor defect identified in the 18-path fix package. NB-I1 is introduced by the new Search Tab handler and is in this fix diff; the preexisting lack of decision autofocus is context for its trigger, not a second whole-branch review finding.

## Checks and Evidence

**Scope check —** Read the supplied brief, frozen five-item finding text, complete implementer report and all 1,079 lines of the supplied full fixed package, recovering output-truncated sections with bounded reads. No diff regeneration or Git state mutation. FIX_BASE `bcec075d791366985e9e3cb0ef1886b63361f4e0`; HEAD `a993efaaaa8e1157b57d28d03cb6b9268b3e4149`; source `54196dcee83218b6a808eae91ba7108259879b63` plus separate three-Rust-file formatting commit. Package RAW 97,035 bytes, SHA256 `5499346b426353b0270061e699f8e5ec4f2dd4af6ac8660a35ac255ce77fcd7b`. Report actual RAW 24,144 bytes, SHA256 `00cdbc46b207e81f1209f788f338bb87e0f94f4d53fbdabe4c2f23e6b3a12ba3`; the earlier 24,037 number was its LF-canonical size, not RAW. Frozen finding file RAW 6,549 bytes, SHA256 `97fa92922528e89bd285e4c2cd02099fbd1e73e1f92fd1fc12c674f4f6cd4e06`.

**Covering-output check —** Inspected existing `final-fixwave-proof/i2-red-normal.log` (three actual extensionless-selected/suffixed-sibling failures), `i2-green.log` (3 pass), `i3-red-completed-run.log` (2 lower-window-trap assertion failures at actual test line941), `i3-green-complete.log` (2 pass/154 skipped), `ui-closest.log` (five files/354 pass, including real Shell/Runtime/Search/baseline files), and `native-export-closest.log` (11 pass). Compared their claimed behaviors with the changed production paths and test assertions. Counts overlap and are not additive. The report names covering final Web/Rust/frontend/native checks; independent source/terminal/hash auditing is owned by the separate provenance reviewer, not duplicated here. This review did not repeat those passed suites.

**Original npm failure check —** Inspected the actual `npm-final.log` failure name and location and `WorkspacePublicationEvidence.test.tsx:141`–`:150`: line146 waits for the report dialog before line147 dispatches any Escape. It is 1439 pass/1 fail, Node bridge not reached. The later full retry's 1440 plus22 pass is separate evidence, not a diagnosis or fix; cause remains UNKNOWN. The failing isolated component case does not compose Desktop/Search/Runtime, and no edit in this package changes that case or its shared Escape implementation. No attribution to load/timing/leak/sandbox or to historical Settings failures is made.

**One limited reviewer diagnostic —** Ran a single in-memory Vite transform of the existing dirty-manuscript Desktop test, using the real product components and mocked APIs already in that test. `node --input-type=module` consumed the PowerShell literal here-string below from stdin; `startVitest` selected exactly one existing case, used inline jsdom/setup configuration, disabled config-file loading and Vitest cache, and changed no test/product/config source file on disk. Execution tool chunk `eb319b`, exit1, Vitest start22:14:59, duration4.24s, 1 failed/155 skipped. The inserted probe did **not** reach its Tab assertions: it dispatched Enter before awaiting the asynchronously loaded matching result and could not find the navigation decision. This is a reviewer fixture/setup failure, **zero product-causal failure evidence**. The stdout DOM was tool-output-truncated; no separate complete raw log file was written. Source reasoning establishes NB-I1; root independently confirmed the source boundary and instructed no corrected probe. Do not relabel this diagnostic as NB-I1 RED or as a full suite rerun.

```javascript
import { startVitest } from 'vitest/node';
const marker = '    fireEvent.click(await screen.findByRole("button", { name: "打开 Guarded session" }));\n    expect(await screen.findByRole("dialog", { name: "未保存的稿件" })).toBeInTheDocument();';
const probe = `    const focusedSearch = screen.getByRole("searchbox", { name: "搜索工作区内容" });
    fireEvent.change(focusedSearch, { target: { value: "Guarded session" } });
    focusedSearch.focus();
    fireEvent.keyDown(focusedSearch, { key: "Enter" });
    const decision = await screen.findByRole("dialog", { name: "未保存的稿件" });
    const searchSurface = screen.getByRole("dialog", { name: "搜索工作区" });
    const tabProbe = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    console.log("SCOPED_BEFORE_TAB", JSON.stringify({ searchFocused: searchSurface.contains(document.activeElement), decisionFocused: decision.contains(document.activeElement), focusedRole: document.activeElement?.getAttribute("role") }));
    act(() => { document.activeElement.dispatchEvent(tabProbe); });
    console.log("SCOPED_AFTER_TAB", JSON.stringify({ lowerSearchPrevented: tabProbe.defaultPrevented, searchFocused: searchSurface.contains(document.activeElement), decisionFocused: decision.contains(document.activeElement) }));
    expect(tabProbe.defaultPrevented, "Lower Search must not intercept Tab beneath the visible navigation decision").toBe(false);`;
let injected = false;
const ctx = await startVitest('test', ['src/DesktopApp.test.tsx'], { config: false, watch: false, cache: false, testNamePattern: 'keeps a dirty manuscript when cancelling search navigation and permits retry', reporters: ['verbose'] }, { configFile: false, test: { environment: 'jsdom', setupFiles: ['./src/test/setup.ts'], cache: false }, plugins: [{ name: 'scoped-readonly-navigation-tab-probe', enforce: 'pre', transform(code, id) { if (id.replaceAll('\\', '/').endsWith('/src/DesktopApp.test.tsx')) { if (!code.includes(marker)) throw new Error('Exact insertion marker missing'); injected = true; return { code: code.replace(marker, probe), map: null }; } } }] });
console.log('SCOPED_PROBE_INJECTED', injected);
await ctx?.close();
```

**Review execution limit —** Other operations were bounded source/document/log reads and file metadata hashes; two rg attempts used Windows-invalid shell glob operands and were corrected to directory plus `-g` searches or actual paths. No product edit, Git command, index/HEAD/branch change, dependency action, app/GUI/browser control, install, external service or network operation. Only this ignored report was intentionally written. The inline diagnostic is a test process, not a native GUI acceptance run. The separately approved post-gate acceptance RAW newline bridge is documentary provenance; this review does not claim tests ran after it or that all seven protected whole-file RAW values stayed unchanged.

## Out-of-Scope Observations

**Out-of-scope new observations —** None beyond the existing contracts needed to trace NB-I1. Existing M2/M3/M4/M6/M7/M8, remaining M5 test debt, dated npm audit package findings, N01 partial-effect/rollback limitation, historical Settings UNKNOWN, and manual/native/real-service STOP remain as already dispositioned; none is silently closed or newly re-reviewed here. No new whole-branch/security/evidence audit is claimed.

## Verdict

**Fix round: Findings remain open — NB-I1 (Important).** I1, I2, the original I3 Runtime/kernel-under-Search defect, M1, and the M5 title correction are addressed. The newly introduced Search-under-navigation-decision keyboard trap prevents an all-clear for this round. Remaining M5 coverage debt is separately open and was intentionally outside the claim-only fix. Required next work is the bounded higher-decision ownership correction and its connected regression; this verdict does not authorize broader product work or resume native GUI acceptance.

<!-- END EXACT final-fixwave-scoped-review-oct19.md RAW -->

### Exact saved final-fixwave-provenance-audit-oct19.md

Original path `.superpowers/sdd/2026-10-04-wisp-improvements/final-fixwave-provenance-audit-oct19.md`;13706bytes;SHA256`4ebc573c6b7a7dd4cd644a43e42c39564d7ba471d99f12afa9ec6ae888daac74`.

<!-- BEGIN EXACT final-fixwave-provenance-audit-oct19.md RAW -->
# Final fixwave provenance audit — 2026-10-19

**ACCEPT for the frozen `a993efaaaa8e1157b57d28d03cb6b9268b3e4149` evidence/source-byte scope, with the limits below.** This is not a behavior review, merge-readiness decision, complete-product claim, new execution of the recorded gates, or approval of subsequent source changes. Root owns the separate scoped code review and retained-evidence adjudication.

## Frozen inputs and method

I read the available CURRENT anchor first (the first root-relative lookup failed; the actual anchor is under this SDD directory), then the final brief, safe shared staging recipe, and the final implementation report. CURRENT's large tool display was truncated; I do not claim a literal whole-anchor read. The final report was read as a whole. The complete index was parsed statically, followed by bounded actual log/source reads and exhaustive checks of the listed hash/path projections. This is not a claim that a human-style literal rendering of the complete index was read.

| Frozen artifact | Actual RAW bytes | SHA256 |
|---|---:|---|
| `final-fixwave-proof/index.json` |230961|`4f1a394be1e8432e6017da87ca34af920b3f2b87f0ebdc39e5b07776a5dd45b0`|
| `final-fixwave-report-oct19.md` |24144|`00cdbc46b207e81f1209f788f338bb87e0f94f4d53fbdabe4c2f23e6b3a12ba3`|

The report is 24037 bytes after CRLF→LF normalization. The earlier 24037-byte description was a canonical length, not its RAW length. The 120820-byte index is the preserved pre-RAW-repair snapshot, not this final index. No input artifact was repaired by this auditor.

Independent audit code and evidence are `final-fixwave-provenance-audit-oct19.py`, `final-fixwave-provenance-audit-supplement-oct19.py`, and `final-fixwave-provenance-audit-proof-oct19.json`. The proof contains **1300 explicit checks, zero failed**, terminal summaries, exact projected-path checks and the absent old-archive path inventory. Proof SHA256: `bb96f706ef97bb007eea1bfcbc26bcc5a6ca1e4fc230c2ad89e8af18666660e8`.

Only these ignored audit artifacts were written. No test/build, compiler/cache mutation, source/index/HEAD/ledger change, GUI/browser/app control, cleanup, subagent or external communication was performed. Pure `rustfmt --edition 2024 --emit stdout` on saved preformat stdin and `git merge-file -p` were used as read-only transformation checks; their output was compared in memory, not applied to product files.

## Source, commits, staging and protected data

Actual Git confirms source `54196dcee83218b6a808eae91ba7108259879b63` has parent BASE `bcec075d791366985e9e3cb0ef1886b63361f4e0`; final formatting commit `a993efaaaa8e1157b57d28d03cb6b9268b3e4149` has that source parent. Final tree is `24002241e730bbe44df2b9fc2faf0e79d48a1a9f`. Source changes exactly the 18 owned paths; formatting changes exactly the three named Rust files; total BASE→HEAD scope remains 18. The index is empty and the seven preexisting external status paths remain.

I hashed all 1121 paths in the final postrepair manifest against actual working bytes. Before→final changes are exactly the owned18. The original before RAW7 snapshots match the declared hashes; protected6 still match actual working bytes. All18 source candidate files match their source-commit Git blobs, recorded SHA256 and blob IDs. The other17 owned working files match final HEAD canonically; acceptance has precisely its retained external18 insertions and no removals. Independent stdout-only three-way merge reproduces the acceptance candidate, inverse and replay exactly from saved input bytes. The saved source/format candidates and actual Git objects establish the claimed ownership separation. A saved allowlist was created after editing; it is not a pre-edit authorization snapshot. The complete pre-edit manifest/RAW7 and approved brief remain the basis of that chronology.

The source commit's preformat Rust3 match their saved hashes; final Git blobs, saved format replay and current files match the after hashes. Independently running actual rustfmt edition2024 over each preformat stdin reproduces all final bytes exactly. The initial edition2021 helper failure remains historical, not a failed product gate. Closest UI/native source manifests differ from the tested final source only by these purefmt3; full final gate manifests equal the tested1121 manifest exactly.

Both working-only initializer residues remain in the exact unchanged protected Rust files. Working tests include external tests/residues that the old committed archive excludes; these are different source scopes, and no all-working-content-is-committed claim is accepted. Preservation of the smoke roots is a reported writer boundary, not independently reconstructed filesystem history in this audit.

## Actual RAW repair bridge

Original acceptance RAW `2295684ce021d013dbdde0de3c467d4864ac10de45f6158452d40791b9f86a1a` contained646 CRLFs. The retained tested normalized RAW `ce42beb47a8f84e2b82ef4aaefdaa7c173598676aee4b0cc21753fe392f8df9c` contained0. Final actual RAW equals the approved candidate `14b230d496ca02efa8ef739b7ead9e4b38a553dc1a385a127d004848e079ee67`, with647 CRLFs. Its normalized bytes equal the tested RAW exactly.

I independently recomputed the semantic line alignment: all817 equal/unowned lines preserve original RAW bytes exactly. The original external18-line payload is RAW-exact, SHA256 `bc0c7ca2cce83a40512374edb48e5018d3a923f9f5dfba0c3191d305c4836fbe`. Actual HEAD→working canonical acceptance delta is only18 insertions/zero removals. The embedded preproof/application records equal the actual JSON sidecars. The postrepair1121 manifest differs from tested1121 only at this acceptance RAW entry; other6 protected RAW and product inputs remain unchanged.

This is an actual repaired preservation defect, not continuous whole-file RAW preservation. Tests ran before this non-runtime-document terminator restoration. `post.working_manifest_unchanged` is explicitly a pre-repair postcommit snapshot; the final index's phase label and RAW bridge supersede any unqualified reading. Do not claim all7 whole RAW stayed unchanged or gates ran after the repair.

## Terminal/log facts and phase distinctions

All21 actual sidecars, complete logs and source-manifest hashes match the index; every sidecar field present matches its embedded record, including recorded cwd/env/start/end/child PID/exit. Rust summary counts were independently parsed. Bounded actual log reads confirm the failure locations and non-Rust summaries.

| Evidence phase | Actual result |
|---|---|
| I2 ordinary-access causal RED |0pass/3fail, exit101; extensionless selections return the unchosen suffixed sibling|
| I2 GREEN / native-export closest |3pass /11pass, exit0; overlapping scopes, not additive unique coverage|
| I3 completed-run causal RED → complete GREEN |2fail →2pass, with154 skipped in each targeted filter|
| UI closest |5files/354pass, exit0|
| fmt |initial check1; apply0; finalcheck0; apply/final empty logs hash correctly|
| standalone Web |exit0,2051 modules,1604.28kB largest JS; these are build metrics|
| final locked workspace Rust |90 summaries:1938pass/0fail/18ignored;22 zero-executed targets; exit0|
| first full npm |128files:127pass/1fail;1439pass/1fail; exit1; Node bridge not reached|
| focused diagnostic |exact publication case1pass/27skipped, exit0|
| one full npm retry |128files/1440pass plus22 Node tests/22pass/0fail; exit0|
| native build / whitespace |release+NSIS build exit0; diff-check exit0|

The first npm failure is publication test line146 waiting for `Saved revision evidence`; Escape is line147 and had not executed. The focused pass and one full retry do not explain or fix it. Relevant source/setup/config bytes did not change between the recorded runs. Cause remains UNKNOWN; this is distinct from Settings UNKNOWN and from I3. The initial I2 sandbox OS5 errors are distinct from ordinary-access causal3RED. Initial I3 wrong-role/nonterminal-run fixture failures and partially applied focus changes are distinct from completed-run causal2RED. The log/sidecar evidence does not justify conflating those phases.

Earlier fmt/Web/Rust/closest records have child PID and second-resolution timestamps; later npm/native/diff have fractional timestamps plus runner PID. UI closest end and native-export start share21:44:15; UI terminal delivery had not preceded the next launch according to the retained report. There is no independently proved subsecond nonoverlap at that boundary. Final gates have separate ordered starts/ends and explicit terminal records, while terminal-delivery order remains the writer/root session attribution. No retrospective runner identities or precision were invented. A successful stdout alone was never substituted for a terminal exit.

Reported helper diagnostics remain distinguished: rustfmt2021 failure corrected with2024; native dep-info declared-directory Err13 corrected to directory metadata; ignored index assembler NameError and GBK wrapper error are not product-test outcomes. My own supplementary reader initially assumed an archive file still existed, then used a wrong top-level key for the nested postfault manifest; neither failure mutated inputs. The corrected read binds the actual retained tar and `hashes` map. A canonical comparison was also corrected to normalize both sides because three retained Skills Git blobs themselves contain CRLF; this exposed no source difference.

## Native artifact scope

The actual `target/release/omicsops-desktop.d` SHA256 is `12e2517b7913f5cf1051a00287b8a0cc8794d9db9c5b46b58849a3ea8d0c31e4`. Independently parsed normalized dep-info resolves exactly483 declared current-repository paths, equal to the provenance JSON path set. Changed native3 are present; no smoke/archive path reference appears; acceptance README is absent. All declared files match recorded size/hash; declared capabilities directory is checked as a directory, with children deliberately not recursively rehashed.

Actual release executable SHA256 `9f6fa38ac0381f60af1cdc5be937f28d1537c0287d9276c0ba05d6de5947de31` and NSIS SHA256 `e6c9f4ecb65a275bfd4b06df0ad991d4dd055a1760e491e20ace0064963c296c` match. This is bounded cwd/source-manifest/top-level dep-info/output provenance with existing caches. It is not a cold-target build, full `--extern` dependency reconstruction, binary-content proof or launch/install/manual acceptance.

## Retained October15 evidence and newly observed absence

All262 indexed relevant path mappings were independently checked against current HEAD and BASE Git blobs and current working RAW/before hashes: npm-ci2, Python227, W0833. Relevant committed inputs are unchanged. Complete Python projection includes all discovered test files, imported catalog helper, both literal Rust workers, their include_str Python/R helpers, remote bridge and bundled Skills/helper tree. The only canonically different projected working input is the explicitly protected external `omicsops-agent-core/src/lib.rs`; it is unchanged from before and is not silently equated to the old archive. Other RAW-vs-commit differences are CRLF representation, including CRLF present in three committed Skills blobs; normalizing both sides confirms no Python/npm semantic change.

The following retained evidence exists and was actually read: `committed-source.tar` (1118 files, SHA256 `81aff47b27492cb8f27316169a0d5f749e853a167fe0c224e0b49c98952372c7`), archive before/postfault-restored manifests, five exact npm-ci/Python/W08 terminal sidecars and logs, three W08 report-proof JSON snapshots, original fixture RAW and mutation/restoration records, and old Rust first-failure/retry sidecars/logs. All262 projected old inputs match the actual tar bytes and archive before/postfault hashes. Sidecars/logs verify npmci304added/305audited; Python48/OK; normal5/0; fault4/1 exit101; restored5/0 exit0. Each preserved W08 stdout JSON equals its report-proof snapshot. Original fixture RAW matches tar/BASE/restoration hash. Old archive Rust remains initial101/zeroexecuted then1916pass/17ignored, distinct from final working1938/18.

**New observation:** `target/final-archive-bcec075-oct15` is currently absent. Consequently its262 projected source files and original emitted `target/evaluation/final-normal.json`, `final-fault.json`, `final-restored.json` cannot be re-read at those paths. The exact265-path absence list is in my proof. The preserved report-proof snapshots/recorded hashes/stdout are dated evidence, not currently existing original emitted output files. I did not recreate or rerun them and infer no actor, time or cause of absence. Root reports no authorized/performed cleanup by root/controller/writer in this continuation; that attribution does not establish what removed the directory. Reuse is therefore supported by preserved dated execution plus tar/manifests/snapshots, with this availability limitation explicitly carried forward, not by current existence of the old execution directory.

## Verdict limits

The frozen report/index accurately expose source scope, recorded terminals, actual final RAW bridge, distinct failures and bounded native provenance. The metadata-length correction and newly observed absent archive/output paths must accompany permanent delivery claims. No additional source-byte discrepancy was found. **Accept this frozen provenance package within that scope.** Any subsequent I3 or other source refinement invalidates affected claims until its own delta and relevant checks are bridged. This audit does not close behavior findings, unresolved npm/Settings causes, native GUI STOP, real model/SSH/R/Micromamba/PBMC or actual CI Node22/Python3.12 acceptance, security/minor debt, or the permanent delivery record.

<!-- END EXACT final-fixwave-provenance-audit-oct19.md RAW -->

## Later root observations and original exception/retained-evidence Rulings

POST47-O11; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:326; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:174; exact original paragraph:

> 2026-10-19 rootWHOLE-read final-fixwave-scoped-review-oct19.md14884B SHA81268118cb14f1a361f5afa2ba5dc67b0d346575d77a08fc419150fc8b020d25. I1/I2/originalI3/M1/M5claimcorrected ADDRESSED; NB-I1Important remainsOPEN: actualhighernavigationDecision hasnoautofocus, keyboardSearchEnterleavesfocusSearchinput, newTabhandlertrapsbelowz1200decision. RootindependentlycheckedactualResearchDialog54–59, Search196–205, Desktopdirtyguard1027. Revieweroneinmemorydiagnostic failedfixtureBEFOREprobe (resultnotawaited), notproductRED; no correctedprobe, no sourcemutation. Rootacceptsrealwithin-diffloadbearingkeyboardgap; notparked orcalledfixed.

POST47-R11; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:328; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:176; exact original paragraph:

> Ruling: authorize ONE bounded additional frontend refinement for NB-I1, despite the SDD default single-final-fixwave/scoped-review cap, using the same sole implementer and a targeted independent verification of the new delta. Do not reopen50tasks or redo broadwholebranchreview. — Why: savedscopedreview proves the unifiedwave introduced a real higher-decision keyboardtrap in the approved ownership boundary; human instruction is to continue and complete development, and higher-level autonomy/verification requirements require correcting an actionable load-bearing defect instead of presenting unfinished behavior as complete. Every path forward is not a guess: actual navigationDecision must own focus/Tab and restoreSearch onclose. — Cost: explicit exception to the default cap, one extra focused implementation/review context and changed-input validation/native packaging time; preserve the original WithFixes/OPEN verdict and all earlier validresults rather than rewriting them. Scope is highernavigationDecision/Search ownership and connected keyboardregression, no generalmodalrewrite or unrelatedMinor implementation, no GUIresume.

POST47-O12; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:330; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:178; exact original paragraph:

> 2026-10-19 rootWHOLE-read final-fixwave-provenance-audit-oct19.md SHA4ebc573c6b7a7dd4cd644a43e42c39564d7ba471d99f12afa9ec6ae888daac74;1300independentstaticchecks0failed, frozena993 source/18candidate/3fmt/21terminals/1121manifests/RAW817+external18/native483 boundedprovenance ACCEPT_WITH_LIMITS. Rootaccepts late savedallowlist proof via actualbeforecomplete1121/RAW7+approvedbrief/proposal/specs+actual18Gitcandidate/inverse/replay, notpredeclaredfileclaim; closest UI→native lackofprovedsubsecondnonoverlap remains, laterfullgateactualcoverage distinct. ActualRAWreport24144B CRLF107/CRCRLF0 SHA00cdbc46... unchanged; earlier24037 was LFcanonical length, corrected metadata notreportmutation.

POST47-R12; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:332; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:180; exact original paragraph:

> Ruling: retain dated Oct15 npmci/Python48/W08normal-fault-restored evidence on unchanged relevant inputs using preserved actual committed-source.tar/manifests/sidecars/logs/report-proof snapshots, explicitly recording the nowabsent original executiondirectory/emittedpaths. — Why: independentaudit verifies all262projected inputs againstactualtar1118entries (SHA81aff47b27492cb8f27316169a0d5f749e853a167fe0c224e0b49c98952372c7), actualdatedterminals/stdout/reportproof equivalence andfixtureRAWrestoration. Currentabsence does not alter those preserved execution facts or unchanged-source mappings. — Cost: original target/final-archive-bcec075-oct15 and3emittedreports cannotcurrentlyberead;265absencepaths are preserved inauditproof, and no actor/time/cause isknown. No claimcurrentdirectory/originaloutputs exist, no reconstructedoldexecution, no rerun solelytoeraseavailabilitylimit. Root/controller/writer report no performed/authorizedcleanupinthiscontinuation; that attribution is notproofwhoremovedit. Future source changes stillneedaffected-input bridge.

POST47-R13; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:334; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:182; exact original paragraph:

> Ruling: correct NB-I1 with opt-in navigationDecision autofocus, bounded currentlyenabled-control Tab/ShiftTab and connected-focus restoration in existing ResearchDialog, plus explicit suspension of Search's lower Tab handler using the alreadyexisting data-navigation-decision marker. Restrict edits to ResearchPagePrimitives, WorkspaceSearchDialog, connected DesktopApp test and runtime-design spec; ordinary ResearchDialogs keep prior behavior. — Why: source and savedscopedreview establish that a keyboardguarded navigation opens the higher decision without transferringfocus, so DOM eventscoping alone cannot establish ownership. The existing navigationDecision prop/marker is the concrete minimal boundary, requiring no modalframework or DOM/mount-order inference. — Cost: opt-in sharedcomponent focus behavior changes and one extra Search guard; incorrect restoration can steal focus, so connectedloaded-result→Enter→decisionfocus→bothTabdirections→immediateEscape→exactSearchfocus tests plus existingRuntime/kernel/trajectory/remotejobs regression coverage are required. Previousfailedreviewerprobe remainsfixture-only; newwriter suppliesactualcausalRED/GREEN.

POST47-R14; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:336; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:184; exact original paragraph:

> Ruling: rerun finaldefaultchecks for the boundedfrontend refinement, including workspaceRust after the new standaloneWeb build, fullnpm andnativebuild; retain Python48/npmci/W08 dated actualevidence only with unchanged relevant-input bridge. — Why: updated UI/Web assets change generated Tauri embedding inputs, so an actualnew-composition compile/test/packaging sequence supports finalsource coverage without relabeling1938oldRust oroldnativebytes as newHEAD execution. — Cost: another changed-input check/nativepackaging cycle and superseded shared buildartifactpaths; preserve old release.d RAW/hash and call oldbinary/installerhashes dated, notcurrentmatches. No newRustlogic/dependency or originalarchive recreation is implied; allgates awaitexplicitterminals, no GUI/install/realservice/CI acceptance.

POST47-R15; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:338; exact alias .superpowers/sdd/2026-10-04-wisp-improvements/CURRENT.md:186; exact original paragraph:

> Ruling: permanent tracked Markdown uses UTF8LF with explicit lossless newline projection for the frozen finalfixwave report only where required; preserve originalRAWsize/hash plus projectedsize/hash and reconstruction proof instead of claiming a CRLFsource occupies an identicalRAWspan after Gittext normalization. — Why: directbyteinspection shows originalreport24144B/SHA00cdbc46b207e81f1209f788f338bb87e0f94f4d53fbdabe4c2f23e6b3a12ba3 has107ordinaryCRLF and0loneCR/CRCRLF; CRLF→LF yields24037B/SHAe57623ff1d9108b5969ec24281b6568a5ec471a86a0441838ccdcb201b471bb5. Earlier24037 metadata mislabeledcanonicalasRAW, and a rendered-outputCRCRLF inference waswrong. UniformLF avoids silently changing span identities atnormalGitstaging. — Cost: dualrepresentations and exactLF→CRLF reconstruction/hashcheck for that report, plus recalculatedactualtrackedspans/maps; originalsourceisnotmutated, no words/reasons/costs lost, and SOURCE47appendix285151bytes alreadyLF remainsbyteexact. Historicalsourceparagraphs retainoriginalmistakenlength asattributedhistory withcurrentmetadata correction, notsilentrewrite. No attributes/framework/binary/base64 artifact or product/source change.

NB-I1 remains OPEN. PENDING_NB_I1_REFINEMENT_COMMIT / PENDING_NB_I1_TARGETED_REVIEW / PENDING_REFINEMENT_AFFECTED_CHECKS / PENDING_PERMANENT_AUTHORIZATION. No cleanup or GUI resume.


## Later primary refinement/diagnostic coverage — final evidence not yet supplied

NB-I1 remains OPEN. Root accepted the independently audited frozen record baseline (2123static checks/0failed; no baseline documentary discrepancy), not final certification. The same source writer is active on the bounded four-path refinement; final refinement report/index/source commit/targeted review have not been supplied as final evidence. New partial gate outcomes below are attributed actual root observations; no final closure or complete successful validation is inferred.

New refinement full npm exit1 is1440pass/1fail of1441,127of128filespass; Node bridge NOT REACHED. The fixed-safe-message revision_changed export case lacks its expected alert; causeUNKNOWN. This is separate from original unifiedwave1439/1 pre-Escape child wait and historical SettingsUNKNOWN. Root reports new keyboard RED1fail/156skipped→GREEN1pass/156skipped,closest7files403pass,newfmt/Web0/newRust1938/18, but no final index/report is adopted here. A bounded unchanged failing-case boundary trace is authorized; instrumentation pass would not establish the cause or erase the original failure. Further original primary paragraphs below retain their actual timing and control any later authorization.

### Exact independent record baseline audit — historical frozen candidate snapshot

Original `.superpowers/sdd/2026-10-04-wisp-improvements/permanent-record-baseline-audit-oct19.md`;11057bytes;SHA256`02c4c4b29c6730469f86107fdc1b9e96dd026965529586482f5519b634e4b8f7`. Its READY FOR FINAL DELTA AUDIT verdict is explicitly not final certification.

<!-- BEGIN EXACT permanent-record-baseline-audit-oct19.md RAW -->
# Independent permanent-record baseline audit — October19

Result: **READY FOR FINAL DELTA AUDIT, with no new material documentary discrepancy observed in the frozen candidate snapshot. This is not final certification.** NB-I1 Important remains OPEN; bounded refinement source commit, affected gates, targeted independent review and root adjudication remain pending. No tracked permanent record exists at the three prepared destinations at this audit observation. Those future files must receive the final delta and a fresh documentary audit before staging/commit or cleanup.

This auditor is separate from the candidate assembler and product writers. The only writes were this ignored report, its owned Python helper and ignored proof JSON. No Git command or mutation, source/shared-ledger edit, product test/build, GUI/browser/panel/app-control, cleanup, subagent or process-control operation occurred.

## Audited byte snapshot

All candidate/report/proof paths below are beneath `E:/Project/OmicsOps/.superpowers/sdd/2026-10-04-wisp-improvements/`. Candidate bytes were independently rechecked before and after the static audit; all match the root-dispatched baseline.

|Candidate|Bytes|SHA256|
|---|---:|---|
|final-record-candidate-main.md|250592|e6451089aaf04575e909d51d678523a4ec863ccd6096c2258c14e8a606d3e6bf|
|final-record-candidate-rulings.md|338150|eae33c4fb8526604ae29609951875b65697da69b7220b3141eb4d04aa741c0de|
|final-record-candidate-coverage.md|144799|6270af73ac177c16ad5cead7f49eacf1a0d1c298d6b98dfb04d9c742c5cd492a|
|final-record-candidate-coverage.json|641263|4759d1cf014d3991aeef7ac9e9939553bf1dd070f76085faba0453ad6b4cd10a|

## Actual method and limits

Read CURRENT first at its actual ignored-workspace location, then the permanent-author brief, final-delivery brief, original October15 consistency audit and the correction/review/fix/scoped-provenance append reports. Read current status, correction, residual, authority and final-phase evidence passages. Processed all four candidates completely as UTF-8 bytes/text/JSON. Repeated byte/hash/line-map comparisons against actual retained authorities and evidence files. This is **complete-file static processing plus selective literal reading**, not a claim to have personally read every character of the old285151-byte appendix, rediscovered every historical decision, inspected every source implementation, or performed a new behavior review.

Original477 cited-reference/134 exact Authority-block verification remains attributed to the prior appendix audit. Original138 Git commit identities/90 ancestry checks and full50 task-chain Git validation remain attributed to the October15 auditor; no Git was rerun here. This audit independently recounted the recorded138 identities/90 edges and checked every recorded task SHA appears in main. Original strongest review's55473-line/173-path coverage remains that reviewer's work; the fresh completion reader's actual scoped reading remains separate.

The owned helper performed2123 checks with0 failures. One initial helper count incorrectly matched the phrase `no formal ruling`; it was corrected to count classifications beginning `formal ` and the complete audit was rerun successfully. This was an audit-helper diagnostic, not a candidate defect or product test failure.

## Independently repeated documentary checks

|Evidence class|Actual result|
|---|---|
|Original inputs|All15 recorded source-input byte lengths/SHA256 values match actual retained files.|
|SOURCE47 appendix|Exactly one contiguous occurrence in rulings, bytes748..285899 exclusive,285151B/SHA256251c587efec7824f3e23ad19af7556dc2511604f37761609bcca44f7bd33e67c; zeroCR.|
|Original decision/cost text|All217 nonempty original C35 body lines9–313 remain literal; missing0.|
|Navigation maps|All184R/A/U exact rows and actual candidate line positions match: R81/A79/U24. All94 body/later headings,235 heading positions,49 later formal/observation rows and4W08 alias rows also match.|
|Established semantic classifications|Unchanged exact appendix preserves57 noncontiguous historical R groups, A34aliases/29observations/15retained/1scope-resolved, N15/P12/later labels, reason/cost variants, primary-reason details and aliases. Selectively checked N01 current residual, N03 no distinct explicit cost, N09→N10 supersession and N14/N15 distinctions; no new product-decision count inferred.|
|SOURCE50 tasks|50 unique SOURCE_CLOSED records; all endpoint/source/fix/format/docs identities occur in main. Recorded138 identity/90 edge recount agrees with prior Git audit; ancestry not independently rerun.|
|Historical report durability|All129 distinct actual historical report/review hashes match. Each exact task/phase-role/path/SHA tuple remains in coverage Markdown; this is separate from later final-phase reports.|
|Post47 originals|All33 primary paragraphs match their actual cited source lines and paragraph SHA256; every body occurs in rulings and every recorded CURRENT alias matches. Exactly15 begin formal classifications. Actual global progress ends at338 at this observation, matching the candidate cutoff.|
|Residuals|All68 original residual rows remain literal in main; later final-review dispositions are separately present.|
|Final report bodies|All5 source-report hashes/sizes match; each occurs exactly once in main at its recorded actual byte span. See table below.|
|Terminals|All7 working+15 archive+21 finalwave sidecar hashes match; parsed actual sidecar contents equal the recorded objects. All43 corresponding log hashes match. All21 finalwave source-manifest hashes match. No command was repeated.|
|Frozen provenance proof|Actual13706B provenance report and241327B proof hashes match. Parsed proof has1300 checks/0failed. Source/staging/native/protected findings remain that independent auditor's scoped work, not a fresh source audit here.|
|Dated retained archive|Rehashed actual committed-source.tar to81aff47b27492cb8f27316169a0d5f749e853a167fe0c224e0b49c98952372c7; counted1118 file entries; independently matched all262 projected committed inputs (npm2/Python227/W0833) against tar bytes. Five retained relevant sidecars/logs are included in the43 checks above; all3 W08 proof snapshots were reopened/parsed. Their original stdout/report-equivalence/restoration analysis remains attributed to frozen provenance audit.|
|Availability identities|All265 recorded absence identities equal the frozen independent proof and remain in Markdown. Current execution-directory/output existence was not reprobed here. No actor, removal time, cause or current-original-output existence is inferred.|
|Report newline projection|Original fix report is24144B,107ordinaryCRLF,0CRCRLF and0loneCR. Its24037B LF projection hashes toe57623ff1d9108b5969ec24281b6568a5ec471a86a0441838ccdcb201b471bb5; exact LF→CRLF reconstruction reproduces original RAW bytes.|

|Five exact main report spans, end exclusive|Bytes|SHA256|Span|
|---|---:|---|---|
|final-wholebranch-review.md|18192|1e00b7e0650689cfbbcfb87be21f0fd35dbbc8dcc3d808312ee032c8ec5164d9|132575..150767|
|final-wholebranch-review-completion-oct19.md|19614|1593551a447f1e89fe9afed57f905bf5ed2e7859df6a810e1d1cb4f01c2280ae|151170..170784|
|final-fixwave-report-oct19.md|24144|00cdbc46b207e81f1209f788f338bb87e0f94f4d53fbdabe4c2f23e6b3a12ba3|176688..200832|
|final-fixwave-scoped-review-oct19.md|14884|81268118cb14f1a361f5afa2ba5dc67b0d346575d77a08fc419150fc8b020d25|213325..228209|
|final-fixwave-provenance-audit-oct19.md|13706|4ebc573c6b7a7dd4cd644a43e42c39564d7ba471d99f12afa9ec6ae888daac74|228581..242287|

The four other final reports contain zeroCR and are already LF. The ignored candidate still embeds the original fix report RAW; this audit does not claim those RAW span bytes will survive tracked Git text normalization.

## Evidence distinctions retained

Current candidate correctly leaves NB-I1 OPEN and separates original bcec075 WithFixes/OPEN review, frozen a993 source/provenance acceptance WITH LIMITS, and pending refinement. Historical W12Task4 no-fullnpm/native-at-fix-phase wording is qualified by the later SOURCE50 adopted-working/frozen-archive evidence. Working1935/18, archive1916/17 and finalwave1938/18 are separate phases. Failed pristine Rust101/missingdist, intentional W08fault101, npm audit1, ignored tests and build-only native results remain distinct from final successes and live acceptance.

Original full npm1439/1+NodeNOTREACHED, exact focused1/27skipped and sole authorized retry1440+22PASS remain distinct. Publication's failure occurred while waiting for the child before Escape, causeUNKNOWN; SettingsUNKNOWN is separate. Reviewer fixture diagnostic is not NB-I1 causal RED. Late saved-allowlist timing and lack of proved subsecond closestUI→native nonoverlap remain explicit. No cache/access/timing cause is invented.

Acceptance tested normalizedce42... versus repaired14b230... is explicit: tests preceded repair;817 unowned lines/external18 RAW restoration and other6 protected preservation are attributed to the accepted independent proof. No all7wholeRAW-unchanged claim or postrepair-test claim is introduced. Original archive executiondir and3 outputs are reported absent at the frozen audit observation; retained tar/manifests/logs/snapshots support dated execution without claiming current originals exist.

Open WB-M2/M3/M4/M6/M7/M8, remaining M5 deferred-diff/pagination tests, N01 postarchive partial effects, dated12 vulnerable packages, Windows/macOS/CI boundaries, localNode24/Python3.10 versus unexecutedCI22/3.12, physicalGUISTOP, oldpartialW06 smoke and precise attempted/QUEUED panel incident remain. Native builds are not GUI/installer acceptance. Real model/SSH/R/Micromamba/PBMC/GPU/SLURM and later manual flows remain unexecuted.

## Pending final delta

No new assembly correction is requested for this frozen snapshot. Required future append is not a historical omission: add actual bounded NB-I1 source/gates/targetedreview/root outcomes and any later original primary paragraphs with reasons/costs. Root must supply the actual three tracked-ready documents after closure. Their current header, final evidence, companion links, dates, source phases, report spans and mapping positions must reflect those actual files.

Final audit must verify all historical bodies still survive, new report/terminal fingerprints and final status agree, SOURCE47 remains exact once, fix-report LF projection/dual metadata/reconstruction remain explicit, every actual tracked span is recalculated, and Git candidate/blob filters preserve the claimed representation. This baseline supplies no staging/commit/cleanup authorization. No product checks need repetition solely for this static record audit.

Proof: `permanent-record-baseline-audit-proof-oct19.json`,284554B,SHA256e49ed57289ade67ffef2a366635ca20395f3726b1d6314e9261635612a88d657. The proof records actual checks and candidate fingerprints; it contains no private raw logs or protected source bodies. Auditor is ready to remain idle until root supplies the final tracked delta.

<!-- END EXACT permanent-record-baseline-audit-oct19.md RAW -->

### Exact later primary paragraphs, with original reason/cost

POST47-O13; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:340; no exact CURRENT alias at this snapshot; exact original primary paragraph:

> 2026-10-19 root WHOLE-read permanent-record-baseline-audit-oct19.md11057B SHA02c4c4b29c6730469f86107fdc1b9e96dd026965529586482f5519b634e4b8f7. Independent2123 static checks0fail on frozen candidates; no material documentary discrepancy. Static complete-file processing plus selective literal reading, not every-character oldappendix/source or repeatedGit ancestry. Original source/audit attribution preserved. Final certification still requires actual NB-I1 closure/new evidence/tracked-file delta audit.

POST47-O14; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:342; no exact CURRENT alias at this snapshot; exact original primary paragraph:

> 2026-10-19 bounded refinement meaningful keyboard RED1fail/156skipped -> GREEN1pass/156skipped; closest7files403pass; newfmt/Web0 and newworkspaceRust1938pass0fail18ignored delivered. Source frozen fourpaths. New fullnpm exit1:1440pass1fail/1441,127of128filespass, NodeNOTREACHED. WorkspacePublicationEvidence fixed-safe-message revision_changed case missingalert afterExportclick; logSHA4d163e1a4e6d21870be3efd5cfca4f160118fdb1c1025a2081717548b1a51443. This is distinct from originalwave pre-Escape childwait and historicalSettings failures; causeUNKNOWN. New source no Desktop/Search composition in this directly-rendered ordinary navigationDecision=false case; that boundary is not a proved timing/cause diagnosis. Native not yetstarted, no blind fullretry/sourceweakening.

POST47-R16; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:344; no exact CURRENT alias at this snapshot; exact original primary paragraph:

> Ruling: authorize ONE bounded exact unchanged failing safe-error case with in-memory Vite boundary instrumentation of eligibility/selection invalidation/catch/current flags, safe booleans/identity only, disk source/test/config RAW preserved before/after. No payload or true service call; preserve rawstdout/terminal including helper failures. Root reviews the actual result before authorizing any fullnpm retry or native. — Why: fullfailure lacks the expected evidenceDialog/alert, while the new focus path is opt-infalse here; actual trace can distinguish an early eligibility/no-call boundary from a later invalidated completion without guessing. — Cost: one extra diagnostic execution/context and perturbation limit; a pass under instrumentation will not establish the cause of the original failure or erase it. No timeout/assertion weakening or retry-until-green.

No final refinement report/index is synthesized. PENDING_NB_I1_REFINEMENT_COMMIT / PENDING_NB_I1_TARGETED_REVIEW / PENDING_REFINEMENT_AFFECTED_CHECKS / PENDING_PERMANENT_AUTHORIZATION.


## Final bounded refinement, targeted closure and provenance acceptance

Root accepts final first-scope source integration at c9a5338d70ad14207a7a58eb7d37793498e9b840/treeaf173b7086f34cd4d5c29cb8ccf03fda0abe495e: original50task FULL/necessarySCOPED, strongest frozen original+completion, threeImportant fixes, NB-I1 targeted ADDRESSED/no newactionable5deltafinding and bounded refinement provenancePASS3653/0. Acceptance carries all residuals/UNKNOWN/manual/security limits. This is not fresh clean-latestHEAD/CI/GUI/live acceptance. Four engineering documents are authorized now; independent final document audit/staging/commit are NOT yet completed. Primary cutoff is globalprogress378.

Refinement causal keyboard RED1/156skipped→GREEN1/156skipped; original4closest403. Two normalnpm1440/1failures (revision_changed then raw), NodeNOTREACHED, stayUNKNOWN; earlier1439/1pre-Escape and SettingsUNKNOWN separate. Tracehelper collectionfailure=zero producttests; corrected1/27skipped perturbedtrace and normalconfig fullVitest1441diagnostic (Node not invoked) do not diagnose/replace failures. Fifthfixture awaits async setup/eligibility, no product fix or weakened assertion/timeout. Final5closest first5files226 plus omitted2files177 disjoint403, wrongargvretained. Final normalnpm1441+22 and native0 are actualfinal5 outcomes, not additive with earlier runs.

Retained fmt/Web/Rust1938/18 executions PRE-DATE fifthfixture; manifest6a174208f6cb4eed772b419b2677dd7ccfb9438bfc9f5f5c0248b3d713a3fe81→42cedda78ee8288916a4682b6ef805902ffa3b3912de4c4d2f2902507bc72d10 differs only in thattest,1120otherRAW unchanged. tsconfig includes tests, so native beforeBuild tsc/Vite validates final5 typecheck; recorded3dist identities equal before/after, preserving datedRust coverage without a postfifth rerun claim. Protected7 unchanged in THISrefinement; old acceptanceRAWrepair remains earlier history. Olda993 native outputs superseded at sharedpaths; priorrelease.d RAW preserved. Python/npmci/W08 are dated retained262-input evidence, not newrun/latestHEADarchive. First3sidecars capture4envkeys, later15capture7; no retroactive/fullOSenv claim. All18 newcommandrecords include stage/commit observations, not18tests.

New reports below are LF/zeroCR and byte-exact. Original finalfixwave RAW24144B/CRLF107 remains separately exact in this ignored candidate; eventual tracked LF projection preserves dual metadata/reconstruction.

### Exact final-i3-refinement-report-oct19.md

Original `.superpowers/sdd/2026-10-04-wisp-improvements/final-i3-refinement-report-oct19.md`;20445bytes;SHA256`2706da4402c52f0e4e1aaa0dc881b0c300492e328ac743cc45409196c13da93f`;zeroCR.

<!-- BEGIN EXACT final-i3-refinement-report-oct19.md RAW -->
# Bounded NB-I1 ownership refinement — October19

**DONE_WITH_CONCERNS.** Source commit **`c9a5338d70ad14207a7a58eb7d37793498e9b840`**, tree **`af173b7086f34cd4d5c29cb8ccf03fda0abe495e`**, parent **`a993efaaaa8e1157b57d28d03cb6b9268b3e4149`**. Implementation/checks complete; root's new targeted independent review and permanent delivery record remain pending. No claim of merge readiness or a new whole-branch review. This is the authorized bounded follow-up to the original scoped report's NB-I1, not a restart of50 closed tasks.

## Requirement results

| Exact owned path | Result |
|---|---|
| `src/features/workspace/ResearchPagePrimitives.tsx` | Only `navigationDecision=true` captures prior focused control, autofocuses Close in layout effect, and attaches scoped Tab handling. Controls are recomputed on every key from enabled/visible controls; pending Save/Discard are skipped. Unmount/cancellation restores prior focus only if still connected and focus still belongs to decision/body. No focus takeover when focus has moved elsewhere. Existing window Escape stack remains sole Escape owner. Ordinary ResearchDialogs retain their previous focus/keyboard behavior. |
| `src/features/workspace/WorkspaceSearchDialog.tsx` | Lower Search Tab handling explicitly returns while actual `[data-navigation-decision="true"]` exists, using the established Desktop shortcut boundary. Higher decision autofocus is implemented too; this is not a Search-only workaround or a general modal framework. |
| `src/DesktopApp.test.tsx` | Real Desktop/real Shell dirty manuscript → CtrlK → query guarded session → await exact loaded/enabled result → Enter on focused search input. Confirms actual decision is mounted, then measures missing/present autofocus and lower Search's effect on top focus. Verifies both-end Tab/ShiftTab wrapping, actual in-flight Save disables Save/Discard and Tab skips them, window Escape closes decision alone/restores exact Search input/preserves dirty draft, failed save settles without navigation, Search Tab works again, next Escape closes only Search. Old mouse/retry, Runtime/kernel/Search/trajectory/remote-jobs cases remain. |
| `docs/superpowers/specs/2026-10-04-wisp-runtime-design.md` | Explicit autofocus, upper Tab ownership, lower marker suspension, conditional connected focus restoration and opt-in boundary replace the earlier insufficient DOM-scoping assumption. |
| `src/features/workspace/WorkspacePublicationEvidence.test.tsx` | Additional root-authorized fixture contract refinement: resolve Study DOM query before async act; await click/load settlement in async act; retain Check lookup and explicitly assert Check and Export enabled. All export events, safe-error assertions and parameter cases remain. This is not a diagnosed fix for the two historical failed full runs. |

No other product path, Rust source, DTO/schema/migration, dependency/lock or unknown-save/navigation callback semantics changed. No formatting-only commit was needed: required fmt check passed without mutation. Exact5 staged explicitly; canonical Git/source mappings and exact RAW proof are in the index/staged map.

## Original reason/cost and scope exception

The original I1/I2/Runtime-kernel-under-Search fixes and M1/M5 claim correction were reviewed as addressed at a993 (semantic54196dcee83218b6a808eae91ba7108259879b63; separate purefmt a993efaaaa8e1157b57d28d03cb6b9268b3e4149). That scoped review found new Important **NB-I1**: keyboard Enter on a loaded Search result opens the higher unsaved-manuscript decision but its old ResearchDialog never moves focus; new Search Tab handling can trap keyboard below it. DOM scoping alone cannot confer ownership when focus remains in the lower surface. Root source-checked and accepted this finding, then approved one bounded extra frontend correction despite the SDD default single-final-wave cap because the human instruction requires complete working ownership. Cost is an extra focused implementation/review context, explicit opt-in/marker coordination and full affected validation/native packaging. No50-task restart, general modal rewrite, broad rereview or deferred-Minor scope.

Original frozen report/index/scoped/provenance artifacts remain byte-identical. Their WithFixes/OPEN verdict, old test results, normalization-repair provenance defect, late allowlist chronology and closest subsecond nonoverlap caveat remain historical. This refinement captured its original four-path allowlist BEFORE edits, then captured the authorized fifth path's RAW and updated exact5 allowlist BEFORE that edit. Historical four-path manifest6a174208f6cb4eed772b419b2677dd7ccfb9438bfc9f5f5c0248b3d713a3fe81 remains preserved; final five-path manifest differs only in that test helper. All7 protected whole-file RAW stayed unchanged throughout; these are new phase facts, not retroactive corrections to the earlier phase.

## Actual RED/GREEN and full checks

- **Behavioral RED:** new connected case reached the actual decision after awaiting the loaded/enabled result;1 failed/156 skipped. Literal observed result was initialTopFocus=false, lowerSearchConsumedTab=true, topFocusAfterLowerTab=false. This proves missing upper transfer and lower trap interference. No compiler/fixture failure. The earlier reviewer's in-memory attempt failed before reaching its probe because it did not await results; that is still zero causal product RED.
- **GREEN:**1 passed/156 skipped after minimal product fix; the same case additionally passes busy-disabled controls and cancel/restoration/pending-save settlement behavior. No test timeout/assertion weakening.
- **Closest original4 phase:**7 files/**403 passed/0 failed**, Desktop, Shell, Runtime, Search, PublicationEvidence, WorkspaceResearchPages and ResearchOverlays. **Final5 covering closest:**5 files/226 pass then2 files/177 pass, same seven real suites/403 pass in two disjoint serial invocations. Initial invocation mistakenly named two nonexistent filters; its actual five-file result is retained, then only the two omitted actual paths were run. No passed suite repeated. Counts overlap full npm and targeted case; never additive.
- **Retained fmt:**exit0 on four-path bytes; empty log recorded. Fifth change is frontend test only, Rust formatting inputs unchanged; not a rerun after fifth edit.
- **Retained standalone Web:**exit0,2051 transformed modules; JS1605.28kB/gzip487.36kB, existing>500kB warning. PRE-fifth-fixture: `tsconfig.app.json` includes src tests, so not all Web typecheck inputs are unchanged. Required native beforeBuild tsc/Vite validates final5 test bytes and current production assets. Modules are not tests.
- **Retained locked workspace Rust:**exit0, **1938 passed/0 failed/18 ignored**, 90 summaries/22 zero-executed targets. This new run followed the four-path product/Web changes, before fifth fixture edit. Final5 changes only one test helper; all1120 other tracked/protected RAW remain identical, with actual before/after native dist content bridge. Not relabeled as an execution after fifth edit. Ignored/zero targets are not real-service acceptance.
- **First final npm:**exit1,127 passed/1 failed Vitest files, **1440 passed/1 failed**. `WorkspacePublicationEvidence.test.tsx:120`, exact `revision_changed` safe-message case, failed waiting for role alert after export click; no evidence dialog in failure DOM. Node bridge **NOT REACHED**. Cause remains **UNKNOWN**; this differs from the original wave's before-Escape case at line146 and from historical Settings UNKNOWN. Source/test/config/assertion/timeout bytes stayed unchanged through the bounded diagnostics and unchanged retry; later fifth fixture refinement is separate below.
- **Bounded diagnosis:**root authorized one exact case with in-memory Vite boolean-only traces. First helper failed exact LF marker matching against293 CRLF lines; collection failed, zero product tests. One authorized helper-only correction normalized in memory and retained exact unique boundary assertions. Corrected diagnostic **1 passed/27 skipped**: enabled click, eligible selection, current ticket catch, failure rendered. Disk1121 RAW remained equal frozen manifest. `config:false`/logging and focus on one case differ from full-suite conditions; this does **not** identify cause or prove the full failure fixed/harmless. Reader/helper diagnostics also included a nonexistent Page filename (corrected to actual PublicationPage), ignored-index patch verification failure without application, a Python one-liner quoting SyntaxError, and a default-GBK helper read failure (corrected to explicit UTF8). These caused no source/index/test mutation and are not product-test outcomes.
- **One authorized unchanged normal-config npm retry:**exit1,127 passed/1 failed files/**1440 passed/1 failed**; this time safe-message parameter `raw` at same alert assertion. Node **NOT REACHED** again. Stopped further unmodified retries.
- **One normal-config diagnostic full Vitest run:**root authorized in-memory unique-boundary tracing with actual config/workers, all128 files/**1441 passed**; Node not invoked. All9 cases had enabled buttons, eligible entries and current catches in this perturbed run; no failed path reproduced and no cause finding. Summary preserves per-case ordering; not substituted for either failed normal npm gate.
- **Authorized fifth fixture refinement and final normal npm:**independent source review rejected detail-before-finally-only loading explanation because loading excludes the editor subtree; RTL synchronous fireEvent/DOM wait did not explicitly own async loading/effect settlement. Root allowed strengthening fixture setup, not changing production or weakening assertions. Final one normal npm invocation on new final5 frozen bytes:exit0,128 files/**1441 pass +22 Node bridge pass**. This is changed-test-input validation, not another unchanged retry. Original wave npm1439/1, both new refinement npm1440/1 failures and Settings failure remain **UNKNOWN**, including possible passive-effect window; later pass does not diagnose them or prove them harmless.
- **Final native:**exit0, release/NSIS build only; intrinsic beforeBuild Web belongs to native command, not another independent standalone gate. No launch/install/distribution.
- **Final whitespace:**exit0 plus staged whitespace check0.

Every command below ran from `E:/Project/OmicsOps`, with recorded explicit env, prelaunch1121-source manifest/hash, fractional start/end, actual runner/child PIDs, terminal exit and complete log SHA256. Heavy gates were run one at a time and the preceding actual terminal was delivered before the next began. Original4 source manifest and later final5 manifest remain separate; no bytes changed after the final5 freeze. Existing default target caches/dependencies were used; not a new cold target, clean archive or offline/CI run.

| Evidence | argv | Runner/child PID | Start → end | Exit |
|---|---|---|---|---:|
| keyboard-red | `npx.cmd vitest run src/DesktopApp.test.tsx -t keyboard navigation decision owns focus` | 1440/21400 | 2026-10-19T22:24:34.035605+08:00 → 2026-10-19T22:24:37.341986+08:00 | 1 |
| keyboard-green | `npx.cmd vitest run src/DesktopApp.test.tsx -t keyboard navigation decision owns focus` | 35156/17604 | 2026-10-19T22:25:22.148787+08:00 → 2026-10-19T22:25:25.476375+08:00 | 0 |
| ui-closest | `npx.cmd vitest run src/DesktopApp.test.tsx src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/RuntimeDialog.test.tsx src/features/workspace/WorkspaceSearchDialog.test.tsx src/features/workspace/WorkspacePublicationEvidence.test.tsx src/features/workspace/WorkspaceResearchPages.test.tsx src/features/workspace/ResearchOverlays.test.tsx` | 14820/28084 | 2026-10-19T22:26:02.913528+08:00 → 2026-10-19T22:27:15.182659+08:00 | 0 |
| fmt-final | `cargo fmt --all -- --check` | 38052/35572 | 2026-10-19T22:28:02.102537+08:00 → 2026-10-19T22:28:04.267044+08:00 | 0 |
| web-final | `npm.cmd run build` | 37560/11660 | 2026-10-19T22:28:17.273360+08:00 → 2026-10-19T22:28:36.279698+08:00 | 0 |
| rust-final | `cargo test --locked --workspace` | 5320/15616 | 2026-10-19T22:28:46.860359+08:00 → 2026-10-19T22:32:01.210247+08:00 | 0 |
| npm-final | `npm.cmd test` | 33328/15188 | 2026-10-19T22:32:51.415228+08:00 → 2026-10-19T22:34:11.908831+08:00 | 1 |
| publication-boundary-trace | `node .superpowers/sdd/2026-10-04-wisp-improvements/final-i3-refinement-proof/trace-publication.mjs` | 32012/29596 | 2026-10-19T22:37:07.588862+08:00 → 2026-10-19T22:37:08.999647+08:00 | 1 |
| publication-boundary-trace-corrected | `node .superpowers/sdd/2026-10-04-wisp-improvements/final-i3-refinement-proof/trace-publication.mjs` | 31600/35836 | 2026-10-19T22:38:59.003379+08:00 → 2026-10-19T22:39:00.782966+08:00 | 0 |
| npm-final-retry | `npm.cmd test` | 19180/41760 | 2026-10-19T22:39:39.295296+08:00 → 2026-10-19T22:40:51.920787+08:00 | 1 |
| publication-normal-full-trace | `node .superpowers/sdd/2026-10-04-wisp-improvements/final-i3-refinement-proof/trace-publication-normal.mjs` | 36940/33852 | 2026-10-19T22:42:36.345965+08:00 → 2026-10-19T22:44:05.690916+08:00 | 0 |
| ui-closest-fixture | `npx.cmd vitest run src/DesktopApp.test.tsx src/components/workspace/WorkspaceShell.test.tsx src/features/runtime/RuntimeDialog.test.tsx src/features/workspace/WorkspaceSearchDialog.test.tsx src/features/workspace/WorkspacePublicationEvidence.test.tsx src/features/workspace/WorkspaceResearchPages.test.tsx src/features/workspace/ResearchOverlays.test.tsx` | 27956/1008 | 2026-10-19T22:46:44.896839+08:00 → 2026-10-19T22:47:59.902409+08:00 | 0 |
| ui-closest-fixture-omitted | `npx.cmd vitest run src/features/workspace/WorkspaceShell.test.tsx src/features/workspace/RuntimeDialog.test.tsx` | 20032/20220 | 2026-10-19T22:48:07.083668+08:00 → 2026-10-19T22:48:43.115855+08:00 | 0 |
| npm-final-fixture | `npm.cmd test` | 39832/35420 | 2026-10-19T22:49:04.527206+08:00 → 2026-10-19T22:50:18.861353+08:00 | 0 |
| native-final | `npm.cmd run build:desktop` | 40240/40232 | 2026-10-19T22:50:34.416383+08:00 → 2026-10-19T22:57:10.107922+08:00 | 0 |
| diff-final | `git diff --check` | 36192/38024 | 2026-10-19T22:57:20.588632+08:00 → 2026-10-19T22:57:20.691132+08:00 | 0 |
| stage-final | `python .superpowers/sdd/2026-10-04-wisp-improvements/final-i3-refinement-proof/verify.py stage` | 37740/38692 | 2026-10-19T22:57:35.712918+08:00 → 2026-10-19T22:57:36.843465+08:00 | 0 |
| commit-final | `git commit -m fix(workspace): give navigation decisions keyboard ownership` | 36248/17048 | 2026-10-19T22:57:44.377672+08:00 → 2026-10-19T22:57:44.466379+08:00 | 0 |

## Preservation, source and artifact bridge

Actual pre-edit before.json/before-manifest.json/owned-before4/protected7 RAW/external-before.diff capture HEAD a993, branch, empty index and original allowlist4. fifth-before.json/raw/manifest capture the added helper path and updated5 allowlist before edit. Final difference from original manifest is exactly those5; all7 protected RAW match, including acceptance **14b230d496ca02efa8ef739b7ead9e4b38a553dc1a385a127d004848e079ee67**, external18 and both working-only initializers. External Git delta is byte-identical; postcommit status is exactly original7 external paths, index empty. All owned5 canonical bytes equal committed HEAD and final5 tested RAW manifest is unchanged. No acceptance write/normalization, shared-file stage, stash/reset/clean, source/cache/mtime trick or external-only API dependency.

Before new Web/native commands, saved original top-level release `.d` RAW as `prior-release-dep-info.raw`, SHA256 **12e2517b7913f5cf1051a00287b8a0cc8794d9db9c5b46b58849a3ea8d0c31e4**.

Current release dep-infoSHA256 **3e4d47e9e8568e7e6bf35fef2dba46434f39447f511d935c7d5e204afcf12434** resolves483 declared paths under the current repository, with no smoke/archive source references. Declared files are hashed; declared directory contents are not recursively rehashed. Current output sizes/hashes:

- `target/release/omicsops-desktop.exe`: 85518336 bytes; SHA256 `231518e8e3346293b53eebf30aa9b8495dc4717fca91e110b936e36bfc1d881e`.
- `target/release/bundle/nsis/OmicsOps_0.1.0_x64-setup.exe`: 18096194 bytes; SHA256 `eac2439abe20d16b82fe41b2a6ff88b49f3b997083d8a21ee490302eadefda5c`.

The earlier a993 release/exe/installer/generated-asset hashes are dated verified artifacts, **superseded at those shared paths**, not claimed to match current outputs. Prior `.d` RAW and all original source/log/index evidence remain preserved; no binary installer backup or distribution. Native proof is bounded cwd/source-manifest/top-level-dep-info/output identity with existing caches, not complete --extern reconstruction/binary-content verification or runtime acceptance.

Python48, actual Oct15 npmci304added/305audited, and W08 normal5/0 → intentional fault4/1(nonzero) → restored5/0 remain **retained dated evidence**, not new helper/install/report-emission runs. All262 prior indexed relevant input projections (npm2/Python227/W0833) remain unchanged in this refinement; owned5 are disjoint and index records each actual before/final RAW hash with prior audited committed mapping. The preserved committed-source.tar hash81aff47b27492cb8f27316169a0d5f749e853a167fe0c224e0b49c98952372c7 and old immutable provenance audit/manifests/sidecars/logs/report-proof snapshots support reuse. The old execution directory and its three original emitted report paths remain absent in the actual availability checks recorded by this index. No reconstruction or actor/time/cause inference; root/controller/writer report no performed/authorized cleanup, which is not proof who removed them. Ordinary W08 suite executes within new Rust with report env unset; no new emitted-report/fault/restore claim.

## Residuals, acceptance and handoff

Independent static publication-boundary strategy note: `final-i3-refinement-proof/publication-boundary-independent-review-oct19.md`,5768 RAW bytes/SHA256 e5d6f0c9dd2e94a80223298212b59eb9252d550a5ffee1ed716789b0a8650272, root WHOLE-read and writer WHOLE-read. It rejects the unsupported detail-while-loading-only explanation, distinguishes no-entry, selection invalidation and wrong-command/alive hypotheses, and supports explicit async fixture settlement. It does not establish historical cause, user reachability or a production fix. The new fixture is only the bounded approved suggestion; no deferred-detail variant, forced schedule or extra product test was silently added.

Original M2/M3/M4/M6/M7/M8 and remaining M5 deferred-diff/pagination test debt remain OPEN. M8 requires deliberate prefetch/aggregate policy and long-source compatibility, with no demonstrated crash. N01 automatic-compaction partial-effect/rollback limitation remains current. Dated npm audit12 vulnerable packages (4low/3moderate/3high/2critical), not12 independent advisories, remains unresolved; no dependency mutation/audit rerun/runtime safety claim. Observed Web large-chunk and Rust/native linker warnings are retained; no warning-free or historical-warning-cause claim.

All tests are deterministic/jsdom/mock Tauri or existing deterministic Rust. GUI physical STOP remains: no GUI/browser/panel/open_in_codex/app/CLI app launch/close/control, installer execution, real model/SSH/R/Micromamba/PBMC/GPU/SLURM or actual CI Node22/Python3.12 acceptance. Local previous environment is Windows/Node24/npm11/Python3.10/Rust1.97; no native macOS/Linux guarantee. Smoke roots untouched, no subagent/chat/worktree/goal/PR/push/tag/release/broad cleanup. Native builds do not establish installation, keyboard GUI behavior or scientific validity.

Pending manual smoke, UNEXECUTED under STOP: in a disposable native manuscript edit a draft, shortcut Search, wait for a loaded session result and keyboard Enter; verify decision Close focus and Tab/ShiftTab wrap, busy Save/Discard exclusion, one immediate Escape preserving Search/draft and exact input focus, then Search Tab and next Escape. Repeat original Runtime/kernel/Search/trajectory/remotejobs ordering. These are future procedures, not GUI authorization or completed acceptance.

Root owns targeted independent delta verification and final disposition. Machine index: `final-i3-refinement-proof/index.json` SHA256 **2d301a7489a467204e2c5fe5872762c6f57bd8711547212ff4916f9eba5f653d**. Original immutable artifacts and all new evidence remain available, with stated old execution-path availability limits.

<!-- END EXACT final-i3-refinement-report-oct19.md RAW -->

### Exact final-i3-refinement-proof/publication-boundary-independent-review-oct19.md

Original `.superpowers/sdd/2026-10-04-wisp-improvements/final-i3-refinement-proof/publication-boundary-independent-review-oct19.md`;5768bytes;SHA256`e5d6f0c9dd2e94a80223298212b59eb9252d550a5ffee1ed716789b0a8650272`;zeroCR.

<!-- BEGIN EXACT final-i3-refinement-proof/publication-boundary-independent-review-oct19.md RAW -->
# Independent publication-boundary review — 2026-10-19

Scope: read-only source/config/dependency inspection and this ignored note only. No source edits, tests, builds, UI actions, Git mutation, or subagents. `.gitignore` excludes `/.superpowers/`. Line references below refer to inspected working files. No production fix is proposed and no failure cause is confirmed.

## Actual contracts

- `WorkspacePublicationEvidence.test.tsx:30–32` renders the publication page directly; `open()` performs a synchronous `fireEvent.click` after finding Study, then awaits existence of Check saved revision 2. It does not explicitly await an asynchronous React transition or assert enabled state. Safe-error cases install a one-shot rejected host response and click Export at lines 116–120.
- `WorkspacePublicationPage.tsx:105–121`: `accept` invalidates evidence and sets detail/revision; `open` then clears loading in finally without another await. Crucially, evidence buttons at 253–254 are inside `editing && !loading` at 233–257. Therefore button existence cannot be explained by a render where detail is accepted but loading remains true. A disabled click would require another actual blocker from 103, not that ordering alone.
- `WorkspacePublicationPage.tsx:87–100`: selection key is assigned to a ref during render. The selection passive effect invalidates evidence, closes its dialog, and clears failure; cleanup also increments generation. At 183–201 a valid action acquires a new generation ticket, opens the dialog, calls the host, and publishes failure only if alive, ticket, and selection key still match. An invalidation after action entry could suppress/erase its result, but its occurrence in a failing run is not established.
- Evidence renders an ordinary `ResearchDialog` at page line 260. `ResearchPagePrimitives.tsx:54–65` defaults `navigationDecision=false` and its new layout effect immediately returns in that mode. Tab handling is also opt-in at 77. Safe-error setup neither opens a leave decision nor mounts Desktop/Search. There is no demonstrated direct focus-fix action on this path.
- Installed RTL `dist/pure.js:83–109` wraps fireEvent synchronously in act; async query wrapper temporarily disables the act environment and drains a timer turn. Installed DOM `dist/query-helpers.js:84–92` implements find queries as getter-in-waitFor. This is an existence contract, not explicit ownership of the whole asynchronous open/effect transition. React's installed development implementation supports asynchronous act queue draining (`react.development.js:901–913`).
- `vitest.config.ts` uses jsdom plus `src/test/setup.ts`; setup registers cleanup after each test. No custom async wrapper or strict-mode configuration appears there. `package.json` chains Vitest before Node extension tests with `&&`.

## Evidence limits and minimal hypotheses

Inspected original logs show the two missing-alert failures (`revision_changed`, then `raw`) and 1440 passed / 1 failed. Their final DOM cannot establish click-time eligibility, host-call ordering, or generation history. The parent reports successful isolated and full-config instrumented runs; those runs likewise do not reveal the failed path. No verdict is made about unseen trace contents.

H1: the action never starts (disabled listener or handler guard). Installed React suppresses button click handlers when disabled (`react-dom-client.development.js:3274–3305`). H1 requires click-time blocker evidence. H2: action starts but a subsequently flushed selection effect invalidates it. H2 requires action ticket followed by effect cleanup/setup and a rejected current check or later clearing of failure. Neither follows merely from a missing alert. Also preserve a third branch: wrong command consumes the one-shot rejection, or unmount/alive changes. The API wrapper simply returns invoke (`workspace-navigation-api.ts:18–20`); command sequence must identify consumption.

## Bounded next step

For this safe-error fixture, a minimal defensible refinement is to resolve the Study element first, wrap its click in `await act(async () => { fireEvent.click(study); })`, then retain the Check query and assert both evidence buttons enabled. Keep export click, exact host request, and safe-error assertions intact. The immediate mocked detail promise can settle under async act. Do not await a new DOM query inside that same act callback: the awaited query can depend on a commit flushed after the callback returns. This improves the fixture's stated ready-to-export precondition; it is not a diagnosis or a production-race fix.

A deterministic deferred-detail variant can assert no evidence button while unresolved, resolve detail inside awaited async act, then immediately export once settlement completes. Use a command-specific export rejection and assert exactly one export request before inspecting its safe alert. That validates settled-state behavior without timer sleeps or retries. A pass only supports this fixture contract.

To distinguish H1/H2 on an actual failure, a bounded trace needs monotonic order and instance/test IDs for click DOM enabled/connected state, handler eligibility, command consumption, ticket acquisition, selection-effect cleanup/setup, catch/current operands, and dialog/failure commit. H1 is no handler/host call plus an identified blocker; H2 is an eligible call followed by same-selection delayed invalidation. A forced pre-passive synthetic click can demonstrate a possible schedule, but cannot alone prove real user reachability or historical causation. An enabled-only wait is insufficient to exclude H2; a successful instrumented run is insufficient to select either hypothesis. No additional blind full run is recommended as diagnosis.

<!-- END EXACT final-i3-refinement-proof/publication-boundary-independent-review-oct19.md RAW -->

### Exact final-i3-targeted-review-oct19.md

Original `.superpowers/sdd/2026-10-04-wisp-improvements/final-i3-targeted-review-oct19.md`;8030bytes;SHA256`fff42c7e3d345f66171e6a097f19f0cd8ecf2bdd9a54f1c249bad7b2c92c2703`;zeroCR.

<!-- BEGIN EXACT final-i3-targeted-review-oct19.md RAW -->
# Final NB-I1 targeted review — 2026-10-19

**NB-I1 ADDRESSED. No new actionable Critical, Important or Minor finding identified in this five-path delta.** This verdict applies to actual commit `c9a5338d70ad14207a7a58eb7d37793498e9b840`, tree `af173b7086f34cd4d5c29cb8ccf03fda0abe495e`, parent/base `a993efaaaa8e1157b57d28d03cb6b9268b3e4149`. It is a targeted closure/new-breakage review, not whole-branch approval, a rereview of M2–M8, or a claim that historical unexplained failures are fixed.

## Reviewed package and identity

Read the refinement brief, the original scoped report's verbatim NB-I1, the full preliminary 239-line diff, changed actual files and immediate focus/navigation/save/Escape context. After the source writer completed, read the complete final implementer report and the committed package header; verified that the committed package's complete diff suffix exactly equals the already-read preliminary diff. Independently queried commit/tree/parent and the exact five changed paths. All five current RAW hashes still equal the preliminary readings, and each current file's Git canonical blob equals the actual committed blob. Thus this verdict concerns the committed package, not an assumed future commit.

- Preliminary diff: `final-i3-precommit-code-review.diff`, 20,463 RAW bytes, 239 lines, SHA256 `41d4f3389d2219f0c0dc909e1b1bb29e845639eaed51040ea919083fb56e41a7`.
- Actual package: `review-a993efa..c9a5338.diff`, 21,055 RAW bytes, 253 lines, SHA256 `892128f3b809b23c4792000af114604d7cb0cdfe7fc2097f987d62342a3e9886`.
- Whole-read final report: `final-i3-refinement-report-oct19.md`, 20,445 RAW bytes, 87 lines, SHA256 `2706da4402c52f0e4e1aaa0dc881b0c300492e328ac743cc45409196c13da93f`.
- Machine index: `final-i3-refinement-proof/index.json`, SHA256 `2d301a7489a467204e2c5fe5872762c6f57bd8711547212ff4916f9eba5f653d`. Read relevant ownership, residual, failure-status and terminal-summary projections; no claim of a separate whole-index/provenance audit. Its final source-manifest SHA256 is `42cedda78ee8288916a4682b6ef805902ffa3b3912de4c4d2f2902507bc72d10`.

| Reviewed path | RAW bytes | Unchanged RAW SHA256 | Actual committed canonical blob |
|---|---:|---|---|
| `src/features/workspace/ResearchPagePrimitives.tsx` | 10566 | `885a908b0d202d066b3cae6d9a44ef9751ee2297e7d82f2c6321145aeef637bb` | `edcd9bb9692e2755d981314c5a1564a167a54fdf` |
| `src/features/workspace/WorkspaceSearchDialog.tsx` | 13877 | `e744a13c8180692c72e236c1b2dd851833746e2e917efd12aa6d7318bde0eb12` | `829dabc125557e96376e65690d3bb55f09824f88` |
| `src/DesktopApp.test.tsx` | 232141 | `78d9bc194897fa75e92b0de453464754b2508cfad72f93697c165e10625eab5e` | `c73f8966fc6bd1370b3cee06ae9f61eaaaec8879` |
| `docs/superpowers/specs/2026-10-04-wisp-runtime-design.md` | 33139 | `8ae0082f6e88e75d7c8b6d8d74febbc4c5064460346cd8491b5a657a615a0137` | `dd77f1b8b5eb765a8ed3d24319364f02caa09275` |
| `src/features/workspace/WorkspacePublicationEvidence.test.tsx` | 15724 | `b4f9d588e0cf17ee240a47e61a5db9fd978462147effb17c2892d78de4d9589f` | `53a4d9cb08266b22a1ee29bc5a7212a74db14f14` |

## NB-I1 closure and preservation

`ResearchPagePrimitives.tsx:54–81` confines the new layout effect and Tab handler to `navigationDecision=true`. The mounted decision records prior focus and focuses Close. Each Tab/Shift+Tab recomputes its controls and wraps within the decision; the actual Save/Discard buttons use native disabled attributes, so the pending-save transition removes them from that traversal. Cleanup checks the previous element's `isConnected` and restores only while focus remains in the decision or body. It does not reclaim focus deliberately moved elsewhere. Ordinary ResearchDialogs retain their prior Escape registration and receive neither autofocus nor this Tab handler. This is the sole current opt-in consumer: the actual unsaved-manuscript decision at `WorkspacePublicationPage.tsx:291`.

`WorkspaceSearchDialog.tsx:196–205` explicitly suspends its lower Tab loop while `[data-navigation-decision="true"]` exists. The matching Desktop shortcut boundary is already present. The higher decision's autofocus establishes ownership, so suspension is not being used as a substitute for focus transfer. The new handler processes Tab only; the existing window Escape stack remains the Escape owner. No save, cancel, unknown-write, navigation callback, DTO, persistence or credential behavior changed.

The connected regression at `DesktopApp.test.tsx:1027–1081` uses real Desktop/Shell composition and a dirty manuscript. It waits for the exact enabled Search result, asserts Search input focus, presses Enter and awaits the actual higher dialog before measuring autofocus. It preserves the pre-manual-focus observation in `initialTopFocus`; the following explicit `close.focus()` therefore does not mask an autofocus failure. A stray lower-input Tab must leave top focus untouched and unconsumed. The test covers reverse and forward boundary wrapping, an actual pending Save with disabled Save/Discard skipped in both directions, immediate window Escape without a focus-entry prerequisite, preservation of Search and dirty text, restoration of the exact previous input, rejected-save settlement without navigation, usable Search Tab, and the next window Escape closing Search. Existing mouse retry and Runtime/kernel/Search/trajectory/remote-job tests are retained. The runtime design now describes these explicit ownership boundaries rather than relying on DOM scoping alone.

The fifth-path helper at `WorkspacePublicationEvidence.test.tsx:33–38` awaits async act for the Study click and asserts both evidence buttons enabled. Its event and safe-error assertions, parameter cases, export requests, cancellation and stale-response tests remain unchanged in the actual diff. The independently reviewed note is 5,768 RAW bytes, SHA256 `e5d6f0c9dd2e94a80223298212b59eb9252d550a5ffee1ed716789b0a8650272`. This change strengthens the fixture's settled-ready contract; it neither demonstrates a production race nor diagnoses the historical missing-alert failures.

## Evidence and limits

Read the actual keyboard RED and GREEN logs. RED reached the causal ownership assertion and observed `initialTopFocus=false`, `lowerSearchConsumedTab=true`, `topFocusAfterLowerTab=false`: one failed/156 skipped. GREEN is one passed/156 skipped. The prior incomplete review fixture is not counted as causal RED.

The final report and indexed terminal summaries distinguish final-five-path closest coverage (5 files/226 passed plus the two omitted real paths/177 passed), final normal npm (128 files/1441 passed plus 22 Node tests), and native exit 0 on final manifest `42cedda…72d10`. The new workspace Rust run records 1938 passed/18 ignored on the earlier four-path manifest; it is not mislabeled as run after the fifth fixture edit. Native beforeBuild validates final frontend typecheck/build inputs. These are supplied execution evidence reviewed for consistency, not suites rerun by this reviewer and not native GUI acceptance.

Both normal full-suite refinement failures (1440 passed/1 failed, `revision_changed` then `raw`) remain **UNKNOWN**. Their Node phase was not reached. The in-memory normal-config diagnostic's 1441 pass is explicitly perturbed evidence, not a replacement normal gate or a cause finding. The later changed-fixture normal pass does not establish that the old failures are harmless or resolved. The report preserves the earlier wave's npm failure, Settings UNKNOWN, M2/M3/M4/M6/M7/M8 and remaining M5 debt, N01, dependency/security observations and provenance limitations. This bounded review does not close those residuals.

No new tests/builds, source edits, Git mutations, UI actions, service calls, subagents or launches were performed by this reviewer. Only this ignored review report was written. Real Windows keyboard smoke, installer execution and model/SSH/R/CI acceptance remain unexecuted under the stated STOP. No release or whole-branch merge-readiness claim follows from NB-I1 closure.

<!-- END EXACT final-i3-targeted-review-oct19.md RAW -->

### Exact final-i3-delta-provenance-audit-oct19.md

Original `.superpowers/sdd/2026-10-04-wisp-improvements/final-i3-delta-provenance-audit-oct19.md`;10292bytes;SHA256`a88369c1061ea19a6d706704d63a4ddd816648ec23a3e9ff49914c4d7ec844e9`;zeroCR.

<!-- BEGIN EXACT final-i3-delta-provenance-audit-oct19.md RAW -->
# Independent bounded I3 refinement provenance audit — October19

**PASS with explicit evidence limits below.** Audited actual committed HEAD `c9a5338d70ad14207a7a58eb7d37793498e9b840`, parent `a993efaaaa8e1157b57d28d03cb6b9268b3e4149`, tree `af173b7086f34cd4d5c29cb8ccf03fda0abe495e`. The independent readonly helper completed **3653 assertions, zero failures**, 2026-10-19T23:01:54.628668+08:00–23:01:56.357120+08:00. This verdict covers the new five-path source/evidence delta. It is not another broad behavioral review, old archive audit, test/build execution, installation or scientific/runtime acceptance.

Read CURRENT.md top, the bounded brief, final implementer report WHOLE, new index, actual sidecars/logs/source snapshots and relevant helper/bridge files. Initial preliminary reading occurred during native build; no final verdict was issued until root supplied frozen actual commit/report/index/postproof. No source/test edits, heavy command, Git mutation, UI/browser/panel/app control, cleanup, subagent or ledger write occurred. Only this audit's own ignored helper/report/proof were written.

## Frozen artifacts and source delta

- Implementer report `final-i3-refinement-report-oct19.md`: **20445 RAW bytes**, SHA256 `2706da4402c52f0e4e1aaa0dc881b0c300492e328ac743cc45409196c13da93f`. UTF8 without BOM; **87 bare LF, zero CRLF, zero bare CR**. LF-canonical bytes/hash equal RAW bytes/hash, so later LF embedding must not invent a normalization difference.
- New machine index `final-i3-refinement-proof/index.json`: **287363 RAW bytes**, SHA256 `2d301a7489a467204e2c5fe5872762c6f57bd8711547212ff4916f9eba5f653d`.
- Final1121-source RAW manifest SHA256 `42cedda78ee8288916a4682b6ef805902ffa3b3912de4c4d2f2902507bc72d10`; preserved historical four-path freeze `6a174208f6cb4eed772b419b2677dd7ccfb9438bfc9f5f5c0248b3d713a3fe81`.

Actual BASE→HEAD changes are exactly ResearchPagePrimitives.tsx, WorkspaceSearchDialog.tsx, DesktopApp.test.tsx, WorkspacePublicationEvidence.test.tsx and runtime-design spec. All1121 current RAW files match the final manifest. Original before→four freeze differs only in the original owned4; four→five differs only in the authorized publication fixture. The original owned4 snapshots and fifth-before RAW each match BASE canonical bytes. Fifth-before manifest equals the preserved four freeze, with the recorded exact5 allowlist. No retrospective fifth-path allowlist substitution is needed.

Checked staged.diff byte-for-byte against actual BASE→HEAD diff; independently replayed every exact-context unified hunk in memory from BASE to HEAD and inversely from HEAD to BASE for all5. Candidate-tree, postproof tree and actual HEAD tree match. Every staged-map Git blob/canonical SHA and current RAW SHA matches actual committed/source bytes. The index's owned source map equals these independently recomputed mappings.

All7 protected whole-file RAW still equal both before hashes and saved RAW snapshots. Acceptance stays `14b230d496ca02efa8ef739b7ead9e4b38a553dc1a385a127d004848e079ee67`; both working-only Rust initializer residues remain covered by their whole-file RAW and byte-identical external diff. Current status exactly equals original7 external status, with empty index. Whole working external diff equals external-before.diff. No whole-file external staging or acceptance normalization occurred in this phase.

The4 immutable historical artifacts retain captured bytes/SHA: original report, index, scoped review, and provenance audit. The old provenance audit remains **13706B/SHA256 `4ebc573c6b7a7dd4cd644a43e42c39564d7ba471d99f12afa9ec6ae888daac74`**, its prior1300-check audit remains historical. This audit did not repeat the old archive/129-task/18-path/three-fmt audit. Only262 indexed prior relevant-input projections were compared against this phase's before/final hashes; each is unchanged and disjoint from owned5. This preserves the bounded Python/npmci/W08 adoption bridge without fabricating a new execution or reconstructing absent original execution directories.

## New execution evidence

All18 new terminal sidecars independently match indexed sidecar SHA, complete log SHA,1121-source snapshot SHA/path set, cwd, actual recorded runner/child PIDs, argv, fractional six-digit start/end boundaries, terminal outcome, parsed frontend summaries and Rust counts. Source differences in the index equal independently computed current-versus-recorded snapshots. Final closest5+2, final normal npm, native, whitespace, staging and commit snapshots equal final5. Earlier GREEN/closest/fmt/Web/Rust, failed npm and diagnostics differ only in the fifth fixture; causal keyboard RED additionally predates the product/spec correction.

All17 adjacent recorded child intervals are strictly serial. Smallest gap **7.181259s**, final closest5→omitted2. This is stronger fractional sidecar nonoverlap evidence than whole-second chronology. It does not independently reconstruct an OS descendant-process trace or delivery of prior terminal messages; root/writer attest actual delivered-terminal ordering.

**Environment qualification:** initial keyboard RED/GREEN and original ui-closest record four selected fields: CARGO_TARGET_DIR, TEMP, TMP, NODE_OPTIONS. Later15 record those plus OMICSOPS_EVAL_REPORT_PATH, CARGO_TERM_COLOR, NO_COLOR. These are recorded subsets, not a full inherited environment or retroactive capture of the missing early fields. First helper execution used an overly strict seven-field expectation for all18 and stopped at keyboard-green; correcting this audit-only guard to the actual preserved subset produced the PASS above. No original evidence/source was altered. Readonly git diff emitted existing LF→CRLF advisories; RAW verification still passed and no files were normalized.

Independently parsed actual outcomes:

- Causal keyboard **RED1/156 skipped**, then **GREEN1/156 skipped**; RED log includes upper-focus/lower-trap probes. Prior review fixture failure remains distinct from product RED.
- Original closest **7 files/403 pass**; final5 closest **5 files/226 pass** followed by actual omitted **2 files/177 pass**. Wrong nonexistent filters remain in the first argv; its real5-file result is not relabeled7. No additive duplication with full suite.
- Earlier fmt0, standalone Web0 and locked workspace Rust **1938 pass/0 fail/18 ignored**,90 summaries,22 zero-executed targets remain PRE-fifth executions.
- Two normal full npm runs each **1440 pass/1 fail**, respectively revision_changed and raw alert assertions. Node bridge absent/not reached. Cause remains **UNKNOWN**.
- First trace helper: collection failure/**zero product tests**, LF marker versus CRLF source mismatch. Corrected transformed trace **1 pass/27 skipped**. Actual normal-config transformed full diagnostic **128 files/1441 pass**, Node not invoked. Nine case summaries show enabled pre-click, eligible entry and current catch in this perturbed run only. This does not diagnose historical failures or replace either failed gate.
- Final changed-fixture normal npm: **128 files/1441 pass +22 Node pass**, exit0; complete log SHA `e899868abedf137ba12c77bed777807a26cca492f39f36b21ceb721279fa0d1e`.
- Final native, whitespace, staged whitespace and source commit terminal0. No test/build rerun was performed by this auditor.

Independent static boundary note remains **5768B/SHA256 `e5d6f0c9dd2e94a80223298212b59eb9252d550a5ffee1ed716789b0a8650272`**. Its static fixture-settlement strategy is not a reproduced diagnosis or product-race fix. Report and index preserve UNKNOWN historical failures, setup-versus-production distinction and normal-versus-instrumented evidence.

## Final native and retained embedded inputs

Final native log/sidecar terminal0: runner40240/child40232, **22:50:34.416383→22:57:10.107922+08:00**, log SHA256 `9dc5ed49296e519c9e6736f9642aac3a9e809daf687dc2bc4d47bc52ed9020b0`. Log records intrinsic beforeBuild `tsc -b && vite build`,2051 modules and final release/NSIS output. Native source snapshot equals frozen final5. Thus this build's compiler includes final test bytes; standalone Web is not mislabeled as post-fixture.

Independently enumerated current dist and compared its3 files to actual before-native content snapshot and after-native bridge: index.html, index-B8N6inVv.css and index-zXV-VRsG.js are byte-identical across that bridge. Earlier standalone Web log declares those content-addressed production assets.1120 other tracked/protected RAW files, including Rust/build resources/config, are unchanged; retained Rust execution remains dated rather than after the fifth edit. No fresh Rust execution or cold-cache claim is made.

Old release.d RAW remains preserved under SHA256 `12e2517b7913f5cf1051a00287b8a0cc8794d9db9c5b46b58849a3ea8d0c31e4`. Current release.d SHA256 `3e4d47e9e8568e7e6bf35fef2dba46434f39447f511d935c7d5e204afcf12434`; independently resolved its exact483 declared paths, all under the current repo, no smoke/archive references. Each declared file length/hash and directory existence matches current native proof; declared directories are not recursively hashed.

Current outputs independently match:

- Desktop exe85518336B/SHA256 `231518e8e3346293b53eebf30aa9b8495dc4717fca91e110b936e36bfc1d881e`.
- NSIS18096194B/SHA256 `eac2439abe20d16b82fe41b2a6ff88b49f3b997083d8a21ee490302eadefda5c`.

Earlier hashes at these shared paths are superseded dated artifacts. Bounded cached cwd/source/dep-info/output identity is not complete --extern dependency reconstruction, binary-content proof, clean archive execution, installation or GUI acceptance. All physical STOP/manual/real-model/SSH/R/Micromamba/PBMC/CI limitations and deferred product/security residuals remain. This audit makes no broader behavior verdict.

## Audit artifacts

Own helper `final-i3-delta-provenance-audit-oct19.py`:17057B/SHA256 `3df8241320d71a92b2e93556a183a9128d0a9982125823f474fa3ebe705d3246`.

Own machine proof `final-i3-delta-provenance-audit-proof-oct19.json`:280701B/SHA256 `2c08a37a8d7a408738c57a8f178fa29b7608be107309a4effafb4a48a492c6bf`. It records all3653 assertion labels, actual18 terminal facts,17 gaps, native/dist maps and report newline classification. Root owns tracked final record and final disposition; these artifacts do not authorize broader work.

<!-- END EXACT final-i3-delta-provenance-audit-oct19.md RAW -->

### Later exact primary paragraphs through378

POST47-O15; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:346; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 root reviewed firsttrace helper failure cef024...: Vite exactLFmarker fails before collection, productInjectedfalse/testInjectedtrue, zero producttests. Authorizedonehelper-only correction: actualoriginalproducthas293CRLF; normalizeonlyinmemory and assert each unique marker. Correctedtrace exit0,1pass27skipped/logSHA3a2c06a463e8cf0b799dfa0969b49b31dbf44bc913dcc4c5513f9120d1243e9b. RootWHOLE-readlog/afterproof: enabledbeforeclick, hasSavedSelectiontrue/evidenceBlockedfalse/busyfalse/gen5; selectioneffect invalidation precededclick; catchcurrenttrue/ticket6 and evidenceOpen/failuretrue. This trace doesNOTreproduce/explain originalfullfailure; configfalse/logging perturbation explicit. All1121RAW matchfrozen6a174208f6cb4eed772b419b2677dd7ccfb9438bfc9f5f5c0248b3d713a3fe81,protected7same,indexempty. No disk source/test/config mutation.

POST47-R17; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:348; no exactCURRENTalias at snapshot; exact original paragraph:

> Ruling: authorize exactly ONE unchanged normal-config fullnpm retry after the bounded safe-error trace; native follows only deliveredPASS, otherwise stop for specific diagnosis. — Why: actual bounded trace reaches enabled export/current safe-error completion successfully, while original failure remains nonreproduced and doesnotcompose Search/Desktop/newopt-infocus; fullconfiguredsuite supplies actual finalcoverage without speculative source edits. — Cost: additional fullsuite execution and retained original1440pass1fail/NodeNOTREACHED, helperzero-tests and perturbedtrace limits. A laterPASS will not prove original cause/fix/harmlessness; UNKNOWN remains distinct from prior PublicationEvidencepre-Escape and Settings failures. No assertions/timeouts/config weakening or blind repeated retries.

POST47-O16; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:350; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 sole authorized normal fullnpmretry FAILED exit1,1440pass1fail/1441,127of128filespass,NodeNOTREACHED; logSHA9ba4d658ca0277e56ff798c36f0c89bfcebefb774fe8728c1f709d311cac093f,22:39:39.295296->22:40:51.920787+08,runner19180/child41760. Same safe-error family120 but parameter raw, not firstrevision_changed. No alert/evidenceDialog in finalDOM. Frozen1121RAW6a174... unchanged, no native/stage/newsource. Originalfailure causeUNKNOWN. Actual testopen waitsCheckbuttonexistence, notenabled; productionevidenceBlocked includesloading and passive selectioneffect invalidates selection; these are distinct candidateboundaries, not diagnosis.

POST47-R18; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:352; no exactCURRENTalias at snapshot; exact original paragraph:

> Ruling: stop blind fullnpm retries and authorize ONE diagnostic fullVitest with actualnormalconfig/settings/workers plus only in-memory uniquelyscoped nine-safe-error-family pre-clickdisabled/entry/selectioninvalidation/currentcatch trace; no disk source/test/config mutation, nopayloads or livecalls. Inparallel a fresh independent gpt-6-astra high read-only narrow boundaryreview evaluates deterministic causalstrategy; no sourcewriterconflict. — Why: two samefamily failures on distinctparameters make a single isolatedperturbedPASS insufficient, and trace underactualfullconfiguration can distinguish noevent/disabledclick from laterinvalidatedcompletion before guessing or weakeningtests. — Cost: one further diagnostic fullsuite/context and instrumentationperturbation limitation; evenPASSdoesnotdiagnose previousfailures or constitute finalunchangednormalgate. Native waits. Preserve allfailedterminals separately; no thirdblindretry/assertiontimeoutweakening.

POST47-O17; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:354; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 normalconfig instrumentedfullVitest diagnostic exit0,128files1441pass; NodeNOTINVOKED. Writertracesall9safe-errorcases disabledfalse/savedSelectiontrue/evidenceBlockedfalse/loadingfalse/catchcurrentticket6. LogSHA9daf00c47a5670c515c8da527052bec5db58bec936847372d56778ef5abaaa21,22:42:36.345965->22:44:05.690916+08,runner36940/child33852. Root inspected trace-summary's actual firstcaseordering; all9fulltraceanalysis attributedwriter. Disk1121/protected7/indexsame; diagnostictransformation isnot finalnormalgate anddoesnotexplain earlierfailures. Independentboundaryreader corrected detail-before-finallydisabledhypothesis: buttonsrenderonlyediting&&!loading and noawaitaccept->finally, so loadingalone cannotexplain found-but-disabled button. Otherblockers/lateeffectremainunproved.

POST47-R19; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:356; no exactCURRENTalias at snapshot; exact original paragraph:

> Ruling: expand boundedrefinement ownedallowlist by exactly WorkspacePublicationEvidence.test.tsx for stronger async setup, soleexistingwriter, capture fifthRAW/updatedallowlist BEFOREedit. Resolve Studybutton outside awaitedact; awaitact(async()=>fireEvent.click(study)); retain findCheck plus explicit Check/Exportenabled assertions; preserve exportevents/all9safe-errorassertions and timeouts. — Why: freshindependentreviewer traced installedRTL eventWrapper to synchronousact and findByRole/waitForasyncWrapper to a query/timerdrain rather than explicit ownership of the asynchronous manuscript-load/effect settlement. Awaitedact around immediate mockedload guarantees setup settlement underReacttestcontract; buttonenabled checks enforce actioneligibility. This is fixturecontract strengthening, NOT confirmedoriginalcause/productfix or proofabsence of realpassive-effectwindow. — Cost: oneadditionaltestpath/sourceinputmanifestphase, coveringclosest rerun and ONE finalnormalnpm on changedtestbytes; retain twooriginal1440/1failures plus diagnosticlimits and UNKNOWNcauses. No productbehavior/config/dependency/assertiontimeoutweakening. Actual fmt/Web/Rust remainvalid datedexecutions through exactunchangedproduct/buildinputbridge, notrelabelled newlyrerun; native stillmustexecute finalcomposition andwaitterminal. If finalnormalfails stopfordiagnosis, no blindretry.

POST47-O18; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:358; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 root WHOLE-read independent publication-boundary review in final-i3-refinement-proof/publication-boundary-independent-review-oct19.md. ActualRTL syncact/querycontract and loading-subtree constraints support stronger asyncfixtureprecondition only. Missingalert finalDOM cannotchoose H1disabled/guard, H2lateinvalidation, or alternativeone-shotresponseconsumption/unmount. No failuretrace causeconfirmed, no productracefixclaimed; successfulinstrumentedfullrun isnot normalgate or diagnosis. Reader performed only static source/config/installeddependency inspection, no tests/Git/UI/mutation. Finalnormalrun after explicitfixturechange validatesnewtestbytes, not an extrablindunchangeddiagnosticretry.

POST47-O19; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:360; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 writer captured fifthtestRAW/updatedowned5 BEFOREedit and changedonly helper asauthorized. Corrected validationbridge: tsconfig.app.json includes allsrc, including .test.tsx, so retained standaloneWeb0 PREDATES fifthfixtureedit and cannotclaim allWebinputsunchanged. Alreadyrequired native beforeBuild tsc/Vite will checkfinal5bytes and provide finalWebvalidation; no extra standaloneWebrun solelylabel. Rustlogic and productionentryinputs unchanged byfixture; actual distcontent/nativeinputbridge required before retaining earlierRust asdatedvalidcoverage, never relabel executionafterfifth. This qualifies previousRuling's shorthand unchangedproduct/buildinput bridge without changing scope or droppingchecks.

POST47-R20; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:362; no exactCURRENTalias at snapshot; exact original paragraph:

> Ruling: final permanent-document author scope includes the three prepared tracked reports plus narrowly updating docs/superpowers/plans/2026-10-04-wisp-improvements.md after actual finalsourcegates/reviewclosure. Only currentW05/W11/W12 statuscells, existingstandaloneWeb-beforeRust commandordering preservingallchecks, and appended actualclosure/reportlinks maychange; datedtaskhistory remains. — Why: rootread currentmastertable still describes cleanHEAD/finalbranch pending and earlierfixsubset/pre-fixfull phases; authoritativecurrentstatus mustlinkactualfinalevidence, and knownpristineRustgeneratedWebprerequisite mustalsobe reflected in this finalcommandblock. — Cost: fourthdocumentationpath/candidateaudit, exactbeforeRAW/diff and finalindependentstaticreview; no newsourcebehavior or runtimeinput, so no extra producttestcycle solelyengineeringrecord. Preparedbrief updated, trackedwritesstillNOTauthorized pendingactualNB-I1closure; no merge/push/release/install/cleanup implied.

POST47-O20; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:364; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 final5 closest actualfirst5suites226pass plus omitted2suites177pass (disjoint403); originalincorrectfilterarguments retained, no passedrepeat/false7filesfirstcommandclaim. Finalnormalnpm afterstrongerfixture terminal0,128files1441pass+22Node,22:49:04.527206->22:50:18.861353+08,runner39832/child35420,logSHAe899868abedf137ba12c77bed777807a26cca492f39f36b21ceb721279fa0d1e,source42cedda78ee8288916a4682b6ef805902ffa3b3912de4c4d2f2902507bc72d10. Earlierboth1440/1failures UNKNOWN remain; no confirmedfix/harmlessnessclaim.

POST47-O21; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:366; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 independenttargetedreview /root/final_i3_targeted_review_oct19 gpt-6-astra high read actualrootgenerated5pathprecommitdiff239lines20463B/SHA41d4f3389d2219f0c0dc909e1b1bb29e845639eaed51040ea919083fb56e41a7 duringnative. No actionablefindingpreliminarily; FINALverdictrequires actualcommitpackage/report/finalbyteequality, notprecommitapproval. Separate /root/final_i3_delta_provenance_oct19 gpt-6.1-sol high readbeforeproof and waitsactualsource/report/index/post forboundednewdeltaonly; noold1300/auditsrepeat. Both no source/tests/Gitmutation/UI.

POST47-O22; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:368; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 finalnative deliveredterminal0; rootindependentlyread actualnative-final.json/logtail:22:50:34.416383->22:57:10.107922+08,runner40240/child40232,source42cedda...,logSHA9dc5ed49296e519c9e6736f9642aac3a9e809daf687dc2bc4d47bc52ed9020b0. Actualrelease5m30s/NSIS1 BUILDONLY, no launch/install/distribution. FinalbeforeBuild tsc/Vite0/2051modules; writerreports exact3distcontentbefore/afterequal (auditpending), currentnative.d483paths nosmoke/archive. Nativeprocessviewlimitedempty andnormalaccessactiveCPU observations are scoped, notfalse no-process/causeproof, no processcontrol. Finaldiff0; actualstagednamesexactowned5observed byroot, commit/report/postproofpending. Sourcefrozen; nofurthergatesneededunlessactualchanges/failure.

POST47-O23; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:370; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 sourcewriterDONE_WITH_CONCERNS, exact5 commitc9a5338d70ad14207a7a58eb7d37793498e9b840 treeaf173b7086f34cd4d5c29cb8ccf03fda0abe495e parenta993efaaaa8e1157b57d28d03cb6b9268b3e4149. RootWHOLE-read final-i3-refinement-report-oct19.md20445B/SHA2706da4402c52f0e4e1aaa0dc881b0c300492e328ac743cc45409196c13da93f; frozenindex287363B/SHA2d301a7489a467204e2c5fe5872762c6f57bd8711547212ff4916f9eba5f653d. IndependentlyactualHEAD/tree/parent/committednames5/remainingexternal7/indexempty verified. Rootgenerated actualreviewpackage253lines21055B/SHA892128f3b809b23c4792000af114604d7cb0cdfe7fc2097f987d62342a3e9886, senttoindependenttargetedreviewer withactualfinalreport forfinalbyteequality/verdict. Separateboundedprovenanceauditactive; ignoredrecordappendactive; trackedwritehold. Allheavychecks terminal, nofuturesourceedits/runsplanned. Newreportincludes all18commandrecords,actual3dist bridge/current483nativepaths/outputhashes/unknownfailures/helperdiagnostics/pre-fifthtypecheckcaveat, pendingindependentaudits. Root'sfirstreadonlyGetItem/GetFileHashmixedformatresult wasunhelpful; explicitJSONconfirmed independentnote5768B/SHAe5d6f0c9dd2e94a80223298212b59eb9252d550a5ffee1ed716789b0a8650272, no source/indexmutation. HistoricaldefaultGBK/quoting/marker/helpers and wrongclosestfilter diagnostics preserved ashelperphases, notproducttestoutcomes.

POST47-O24; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:372; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 root WHOLE-read/accepts final-i3-targeted-review-oct19.md8030B/SHAfff42c7e3d345f66171e6a097f19f0cd8ecf2bdd9a54f1c249bad7b2c92c2703. NB-I1ADDRESSED atactualc9a5338d70ad14207a7a58eb7d37793498e9b840, no newactionableCritical/Important/Minor in5pathdelta. Independentreader readcompleteprelim239lines andverifiedactualcommittedpackage completesuffixEXACTsame, all5RAWunchanged/canonicalblobsactualcommit; finalreportwhole-read and RED/GREENactualcoveringlogsread. This is targetedclosure/newbreakageverdict withhonestpreliminaryduringnative->actualcommitbridge, notwholebranchM2-M8rereview or approvalassumedfuturebytes. Rootacceptscapexceptionboundedfixclosed; earlieroriginalWithFixes/OPEN/scopedNBOPENfrozenreports remainhistorical and earlier1440/1failuresUNKNOWN unchanged. CodeonlycompletefirstscopesnotGUI/realserviceacceptance. Auditorremainingboundedsource/evidenceproofpending; trackedauthorhold.

POST47-O25; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:374; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 independentprovenanceprelimsource/candidate/inverse/replay/protected7/immutable4pass; preciseenvqualification: firstkeyboardRED/GREEN/originalui-closest3sidecars captured4fields(CARGO_TARGET_DIR,TEMP,TMP,NODE_OPTIONS), later15captured7. Implementerreport recordedexplicit-env meansactualrecordedsubset, notalllaterelectedfieldsretroactivelycaptured. Rootacceptsdocumentedlimit, no report/sourceedit or retestneeded, finalauditwillstateexactly. Actualcommandoutcomesnotchanged; finalcertificationwaitssavedaudit.

POST47-O26; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:376; no exactCURRENTalias at snapshot; exact original paragraph:

> 2026-10-19 root WHOLE-read/accepts final-i3-delta-provenance-audit-oct19.md10292B/SHAa88369c1061ea19a6d706704d63a4ddd816648ec23a3e9ff49914c4d7ec844e9, proof280701B/SHA2c08a37a8d7a408738c57a8f178fa29b7608be107309a4effafb4a48a492c6bf. Independent3653assertions0fail verifiesactualc9a5338exact5/1121/candidate/inverse/replay/protected7/immutable4/18logs-sidecars-manifests/counts/17fractionalserialgaps/dist3bridge/current483nativepaths/outputhashes. Rootacceptsboundedsource/evidencePASSwithenvsubset4vs7/cachednative/runnerchildnotfullOSdescendants/deliveryattestationlimits; originalRust90targets1938/18retainedPREfifththroughactualassetbridge, nativefinaltypechecks5. Newreportactual20445B/LF87/noBOM/noCRLF orbareCR, so RAW=LFprojection. Audithelperfirstoverstrict7envguard failedzeroauditcompletion thencorrectedtoactualsubset; evidenceunchanged, notproducttestfailure.

POST47-R21; .superpowers/sdd/2026-10-04-wisp-improvements/progress.md:378; no exactCURRENTalias at snapshot; exact original paragraph:

> Ruling: final first-delivery source integration is accepted on original50task FULL/necessarySCOPED chain, strongest frozenwholebranchoriginal+completion, initial3Importantfixes and actualc9a5338NB-I1targetedclosure plus boundedprovenancePASS. Authorize sole record_corrections_oct19 to finish ignored evidenceappend then create/update ONLYfourtrackedengineeringdocuments in permanent-record-author-brief-oct19.md; no stage/commit until rootactualdocumentauditapproval. — Why: finalnormalnpm1441+22/native0, retainednewRust1938/18throughactualunchangedassetbridge and earlierunchangedPython/npmci/W08 proofs satisfy deterministicchangedinputchecks; savedindependentverdictcloseslastload-bearingfinding withno newactionable5deltaissue. Finalpermanentcontentmustdurablypreserve alloriginalreason/cost/aliases, failures/UNKNOWNs, sourcephaseandreaderattributions ratherthanleaveevidenceonlyignored. — Cost: fourdocs/largeexacthistoricalappendix+reportspans+maps, onefinalindependentstaticdocumentaudit and exactpathcommit; no extra runtimechecks solelydocumentation, no newwholebranchapprovalascribed toscopedreader. Originalfrozenarchive bcec dated1916/17 and workingnew1938/18 remainseparate, no cleanlatestHEAD/freshCIclaim. RemainingWB-M2/M3/M4/M6/M7/M8+M5deferreddiff/pagination, N01, npm12vulnerablepackages, allUNKNOWNfailures and native/live/CIlimits remainexplicit. No merge/push/release/install/GUIresume/cleanup or user-choiceofintegrationoption is inferred; currentlocalbranch/workspace/evidence preserved.

