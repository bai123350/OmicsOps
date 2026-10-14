# Wisp 借鉴：证据与工作区首范围设计（W09–W12）

日期：2026-10-04。状态：用户已确认设计，实施计划待审阅；本文件只定义行为与边界，没有实现产品代码，也不是实施计划。

本文件承接 [Wisp Science 改进清单](2026-10-04-wisp-science-improvement-backlog.md) 中已经获准设计的四项首范围。W09 包含只读准备度报告和 reference-only 证据包；W10 包含显式选中小文件的基线保存与差异；W11 只包含可选择迁移清单预览；W12 包含持久、可逆的会话收起与恢复，以及导航、搜索和 `#` 的发现过滤。四章可独立交付。真实模型、Python/R、SSH、SLURM 和复现执行均不属于这些首范围。

## 共同约束与已有边界

所有新跨 UI 数据结构定义在 `crates/omicsops-dto`，由 `src-tauri/src/dto.rs` 重新导出；TypeScript 边界由 `src/types.ts` 或功能类型文件承接，并在 `src-tauri/src/dto_contract_tests.rs` 校验实际序列化。DTO 不依赖 Tauri、Store、异步运行时或 Win32。下文 Rust 签名是拟议接口，不表示代码已经存在。请求使用 `#[serde(deny_unknown_fields)]`，枚举使用 `snake_case`；UUID 使用 `Uuid`，字节数和 revision 使用 `u64`，时间使用 RFC 3339 UTC 字符串。

项目拥有关系始终由宿主的 Store 查询验证。前端提供的 backend、相对路径、run、conversation、publication 或 source ID 都不是权限凭据。读报告、生成清单、收起会话和写入宿主自己的基线存储不调用 kernel、SSH、MCP、模型或凭据读取。需要凭据的后续迁移与执行另行设计；本次没有可以移植的批准计划、连接授权、恢复 token 或运行句柄。

来源发现与既有来源解析分别处理。收起只能影响发现；保存的稿件引用、精确 Session 引用、分支、历史 run 和已有导航仍按原项目身份解析。原 `ConversationStatus::Archived` 和 `conversation_branches_v4.state` 不表示本设计的可见性。

公开投影遵循现有 `public_text`/`public_value` 脱敏，并加上用途明确的字段白名单。不能把通用字符串脱敏当作“任意文本绝无秘密”的证明。导出或持久文本检查发现私钥 PEM、口令/API key/认证 token 赋值、认证 URI 或已知敏感字段时，拒绝输出原文；检查或脱敏改变正文时，W10 降为仅引用。错误只返回安全原因码，不回显命中的原文。禁止为了扫描而读取 keyring；Credential Manager/keyring 中的值从不进入 DTO、SQLite、日志、基线、导出包或 Git。

### W02 提供的有界本地文件接缝

W10/W11 依赖 W02 的 `src-tauri/src/project_file_read.rs`，并不另建路径或 Win32 实现。建议冻结以下宿主内部契约；它不成为 DTO：

```rust
impl ValidatedProjectFile {
    pub fn open(project: &Project, relative_path: &str)
        -> Result<Self, FileReadError>;
    pub fn metadata(&self) -> PublicFileMetadata;
    pub fn read_prefix(&mut self, max_bytes: usize)
        -> Result<BoundedFileRead, FileReadError>;
    pub fn read_complete(&mut self, max_bytes: usize)
        -> Result<BoundedFileRead, FileReadError>;
}
// PublicFileMetadata { size_bytes: u64, modified_at: Option<String> }
// BoundedFileRead { bytes: Vec<u8>, size_bytes: u64, read_bytes: u64,
//                  eof: bool, sha256: String, hash_scope: Full | Prefix }
```

接缝使用同一个打开的 File handle 读取并核对前后元数据，拒绝文件/祖先 symlink、所有 Windows reparse point、设备路径、ADS、保留名、内部目录和项目根逃逸；Windows 用 handle final path 与 file identity 校验最终对象。打开后的大小越界或身份变化返回确定错误，不把短读当完整读。前缀哈希明确是 prefix；只有 `read_complete` 成功且 `eof=true` 才给出 full。这些核对不能保证敌意并发改写下的原子文件快照，也不能把多文件顺序读取称为一致时刻快照。

现有 `composer_files.rs:303` 的规范化与 `:384` 的本地解析提供项目路径语义；`composer_attachments.rs:648` 只检查 symlink，不能据此声称覆盖 Windows junction。已有 reparse 检查位于 `general_settings.rs:55`，有界打开模式位于 `integration_packages.rs:1374`。W02 接缝消除这里的必要重复，保留纯 core 与 Win32 的边界。

共享选择 DTO 定义在拟议 `crates/omicsops-dto/src/workspace_files.rs`：

```rust
pub struct ProjectFileSelectionV1 {
    pub backend_id: String,
    pub relative_path: String,
    pub expected_full_sha256: Option<String>,
}
pub enum WorkspaceContentModeV1 { TextSnapshot, ReferenceOnly }
pub enum FileReferenceHashScopeV1 { FullFile, RecordedFile }
pub struct FileReferenceHashV1 {
    pub sha256: String,
    pub scope: FileReferenceHashScopeV1,
}
```

选择是 project-scoped；命令顶层必须含 `project_id`。backend ID 最长 128 字节，相对路径最长 1,024 字节；规范化后排序去重，最多 32 个，不递归展开目录。`expected_full_sha256` 若非空须为 64 位十六进制；它只比较本次实际完整读取的文件，不用 recorded hash 冒充当前文件。SSH 身份仍校验为项目绑定，但这两个首范围不建立 SSH 连接、不 stat、不下载远端文件。

## W09：保存版本的准备度报告与 reference-only 证据包

### 用户目标、推荐方案与取舍

研究者在稿件的一个已保存 revision 上看到：引用是否仍能解析、科学证据是否已失效、provenance 是否完整、哪些种子/版本/样本信息缺失、原始文件位置是否真正核验。随后可以保存一个只包含稿件、公开来源快照与清单的离线包，以便审阅引用和遗漏。

推荐直接复用 immutable `PublicationRevision` 与 `WorkspaceSourceSnapshot`。不新增冻结/发表状态，不生成新的稿件 revision，不修改科研证据，也不要求先修复所有缺口才能导出。包记录缺口；“已导出”只说明包写入成功。`EvidenceRecordV4.valid`、`ProvenanceManifestV4.complete` 和来源当前可用性是独立观察，任何一项都不证明独立复现。

选择保存版本而不接受前端传来的 markdown/references，可以防止未保存编辑或替换来源混入包。准备度查询和导出各自产生观察，二者的 `observed_at` 和科学状态水位可能不同；UI 明示导出时重新检查。保留现有 Markdown 单文件导出，新增“检查准备度”和“导出引用证据包”入口。

### 精确命令与 DTO

拟议命令放在 `src-tauri/src/workspace_publication_evidence.rs`：

```rust
pub async fn workspace_check_publication_readiness(
    state: State<'_, AppState>, request: PublicationSelectionV1,
) -> Result<PublicationReadinessReportV1, PublicationEvidenceErrorV1>;

pub async fn workspace_export_publication_reference_package(
    app: AppHandle, state: State<'_, AppState>, request: PublicationSelectionV1,
) -> Result<Option<PublicationPackageExportResultV1>, PublicationEvidenceErrorV1>;
```

拟议 `crates/omicsops-dto/src/publication_evidence.rs` 定义：

```rust
pub struct PublicationSelectionV1 {
    pub project_id: Uuid,
    pub publication_id: Uuid,
    pub revision: u64,
    pub expected_revision_sha256: String,
}
pub enum PublicationRevisionHashScopeV1 {
    StoredRevisionContent, PublicLegacyMarkdown,
}
pub enum ReadinessFindingSeverityV1 { Attention, Unverified, Info }
pub enum ReadinessFindingCodeV1 {
    SourceChanged, SourceMissing, SourceUnavailable, InvalidEvidence,
    InvalidArtifact, StaleAnalysis, IncompleteProvenance, MissingRandomSeed,
    MissingSoftwareVersion, MissingDatabaseVersion, MissingSample, InvalidInput,
    ProvenanceNotRecorded, LineageNotRecorded, OriginalFileNotChecked,
    HistoricalLocationUnverified, LegacyRevision, InspectionLimit,
}
pub struct ReadinessFindingV1 {
    pub code: ReadinessFindingCodeV1,
    pub severity: ReadinessFindingSeverityV1,
    pub source: Option<WorkspaceSourceRef>,
    pub related_record_id: Option<Uuid>,
    pub message: String,
}
pub struct PublicationReadinessReportV1 {
    pub schema_version: u8, // 1
    pub selection: PublicationSelectionV1,
    pub revision_hash_scope: PublicationRevisionHashScopeV1,
    pub observed_at: String,
    pub scientific_revision: Option<u64>,
    pub scientific_state_sha256: Option<String>,
    pub references_checked: u32,
    pub related_records_checked: u32,
    pub attention_count: u32,
    pub unverified_count: u32,
    pub findings: Vec<ReadinessFindingV1>,
    pub truncated: bool,
    pub reproduction_status: ReproductionStatusV1,
}
pub enum ReproductionStatusV1 { NotExecuted }
pub struct PublicationPackageExportResultV1 {
    pub path: String, // host save dialog result; not written into the package
    pub package_sha256: String,
    pub size_bytes: u64,
    pub manifest_sha256: String,
    pub selection: PublicationSelectionV1,
}
pub enum PublicationEvidenceErrorCodeV1 {
    InvalidSelection, NotFound, RevisionChanged, LimitExceeded,
    UnsafePublicContent, StorageReadFailed, ExportWriteFailed,
}
pub struct PublicationEvidenceErrorV1 {
    pub code: PublicationEvidenceErrorCodeV1,
    pub message: String,
}
```

不提供 `ready: bool`、复现通过标志或执行授权。`expected_revision_sha256` 必须匹配现有宿主 `workspace_get_publication` 返回的该版本 hash：structured revision 是存储的序列化内容 hash；legacy revision 经 `refresh_publication` 后是公开 markdown hash。响应通过 `revision_hash_scope` 表明差别，不能把它们都称为“原始文件校验和”。错误类型是 serde 数据，不含底层 SQL、绝对服务器目录或命中的秘密。

宿主内部拟议 Store 接缝：

```rust
pub async fn workspace_publication_read_context(
    &self, project_id: Uuid, publication_id: Uuid, revision: u64,
) -> Result<PublicationReadContext, StoreError>;
```

`PublicationReadContext` 为 Store 内部结构，包含选定 revision、当前来源记录投影、关联科学记录、科学状态 header 的 revision/hash，以及截断信息；不成为 UI DTO。在同一个只读事务内完成这些 DB 读取。使用现有科学记录分表的 bounded 查询，不反序列化整个 `scientific_states_v4.value_json`，不调用带旧标题修复副作用的 `list_conversations`。复用 `workspace_source_event` 的 project/run/sequence 所有权查询；source call ID 本身不建立关联。

### 检查语义与有界限制

只检查该 revision 的至多 32 个保存引用及其明示关联。关联闭包上限 64 个科学记录、深度 4；用 `(kind,id)` 去重，按稳定键顺序读取。关联只沿 `EvidenceSourceV4::Artifact.artifact_id`、`ArtifactRecordV4.producer_analysis_id`、`AnalysisRecordV4.input_dataset_ids`、以及 provenance 的 `analysis_id`/`run_id`；不扫描全项目推测关联。文献引用只核对已保存的来源描述，首范围不联网核对 DOI 或撤稿。

每个科学 JSON 记录读取前检查长度，最多 256 KiB；相关记录合计最多 4 MiB。来源快照与稿件沿用保存时已有大小限制；输出报告最多 512 KiB、128 条 finding，每条公开消息最多 512 字节。超过闭包/记录预算产生 `InspectionLimit` 和 `truncated=true`，保留已检查结果，不把未检查记录视为正常。无法安全读取选定 revision、所属项目不存在或整个报告超过硬上限则失败。

当前来源 changed/missing/unavailable 不覆盖保存的快照正文。Evidence invalid、Artifact invalid、Analysis stale、provenance 的 `complete=false` 和已有 issue 明确分别列出。对应记录不存在时报告未核验；不把“不存在失败记录”当成成功。种子/软件/数据库/样本检查沿用已有记录要求与 `ProvenanceIssueKindV4`，不把每个随机种子为空的非分析引用一律判错。

原始文件不读取、不重新 hash，SSH 不连接。保存的 `file_sha256` 只标为 recorded file hash，来源正文 hash 只标为 saved snapshot text hash。artifact 的历史 backend/root 未核验、legacy backend 未记录和 `file_availability=not_checked` 必须保留为 Unverified。当前项目重新绑定 SSH 不能改变历史 artifact 的位置含义。导出包中的位置使用安全相对路径和 `<local_project_root>`/`<historical_ssh_root>` 符号根，原 backend 只作为不带主机/用户名/秘密的身份引用；无法安全表达则省略并记录原因。

### 包格式、持久化与失败语义

格式为一个 `.zip`，只有四个固定根级 entry；不按来源 ID 或路径生成 zip entry 名，不包含 HTML、可执行脚本、原始样本、数据库、环境目录或原始产物字节：

| entry | 内容与 hash 范围 |
| --- | --- |
| `publication.md` | 所选保存版本的公开标题、正文与引用注记；UTF-8 LF；包内此文件字节的 hash |
| `manifest.json` | 下述 schema v1；记录引用、位置、遗漏与明确的 hash scope |
| `readiness.json` | 本次导出时的 `PublicationReadinessReportV1` |
| `checksums.sha256` | 按文件名排序的前三项完整文件 SHA-256；不自包含自身 hash |

`PublicationReferencePackageManifestV1` 的精确字段为 `schema_version:u8=1`、`mode:WorkspaceContentModeV1=ReferenceOnly`、`selection:PublicationSelectionV1`、`revision_hash_scope:PublicationRevisionHashScopeV1`、`exported_at:String`、`reproduction_status:ReproductionStatusV1=NotExecuted`、`references:Vec<PublicationPackageReferenceV1>`、`omissions:Vec<PublicationPackageOmissionV1>`。reference 字段为 `source:WorkspaceSourceRef`、`title:String`、`text:String`、`saved_snapshot_sha256:String`、`exported_text_sha256:String`、`current_availability:SourceAvailability`、`recorded_file_sha256:Option<String>`、`relative_path:Option<String>`、`location_root:Option<PackageLocationRootV1>`、`original_file_status:OriginalFileStatusV1`。`PackageLocationRootV1={LocalProjectRoot,HistoricalSshRoot,Unknown}`；`OriginalFileStatusV1` 首范围只有 `NotChecked`。省略项字段为 `source:Option<WorkspaceSourceRef>`、`code:String`、`message:String`，code 从宿主固定原因白名单生成。

manifest 不包含任意原始 metadata map、完整 runtime/process identity、SSH 地址/用户/指纹/私钥引用、模型 endpoint、环境变量、请求批准对象、外部工具授权或恢复句柄。`saved_snapshot_sha256` 表示保存快照文本，`exported_text_sha256` 表示再次公开投影后的 UTF-8 text；若二者不同，添加公开脱敏遗漏说明，不宣称原文完整。JSON 对象使用稳定字段顺序，数组按稳定 source key 排序。zip entry 顺序和时间戳固定，压缩方式固定；内容相同的包字节稳定，但重新观察产生的时间/状态可以改变包 hash。manifest hash 是最终 manifest 文件字节 hash；整个 zip hash 单独由回执返回。

科学记录和Run的保存快照正文按种类做包专用字段白名单投影；无法解析的历史正文使用固定安全省略文本与`saved_snapshot_text_omitted`原因，保留原saved snapshot hash并按实际导出文本重算hash。这个保守边界可能省略旧的纯文本科学摘要，但不会修改保存版本或用当前记录替换。Message/Tool普通文字说明仍沿用公开和敏感内容检查。

总生成未压缩字节最多16 MiB。严格顺序为：预检选定revision存在/所属项目/公开hash及输入预算；打开宿主保存对话框；确认目的地后重新读取所选revision及完整只读context；以这次context一次生成report、manifest、publication、checksums和zip，并计算所有最终字节hash；最后atomic persist。预检不保留可复用的旧package或旧report，不能混用对话框前的正文与之后的准备度。对话框取消返回 `Ok(None)`，不写Store。目的地不是请求字段。宿主对话框返回的路径必须已经具有大小写不敏感的`.zip`扩展名；无扩展名或其他扩展名返回安全的ExportWriteFailed，不改写对话框所选路径，避免覆盖未经该次选择确认的同名`.zip`文件。如果系统选择器返回无扩展名路径，用户须显式输入`.zip`文件名。重新读取时revision被删除或hash不符即停止，不能导出head替代所选历史版本。写入目的地同目录临时文件、flush/sync后原子persist；既有目的地的覆盖由宿主保存对话框确认，不在后台覆盖。写失败返回 `ExportWriteFailed`，不创建“已导出”状态或更改稿件。无DB migration。

UI 报告绑定 `(project,publication,revision,expected_sha256)`，切换或离开时用 generation 丢弃旧响应。未保存稿件编辑存在时两个入口明确显示“检查/导出保存版本 N”，不会隐式保存，沿用现有离开 guard。报告里的“来源可用”只指 Store 记录当前可解析；“原始文件未检查”和“未执行独立复现”始终可见。包只是本机保存，没有投稿、云发布或远程传输。

### 验收、文件地图与依赖

自动化覆盖：保存历史版本与 head 分离；源 changed/deleted 时保存快照仍可导出；跨项目 source/run/record 被拒；失效 evidence/产物、stale analysis、缺种子/软件/数据库/样本均产生对应 finding；缺失关联和预算截断不能显示检查通过；legacy hash scope 与 structured 区分；再次脱敏后的包 hash 正确；私钥/认证/token/密码样例不出包、不进错误；固定 zip 名拒绝路径注入；每项 checksum 校验正确；取消及写入失败无 Store 写入；真实 Store 前后状态/SQLite写入计数不变，并核对实际 repository-only 委派无 kernel/SSH/凭据/模型能力端口；当前具体 AppState 没有可注入计数接口，故该证据是结构性无调用边界，而不是插桩零调用计数，也不使用与产品路径无连接的 mock 计数。UI 覆盖未保存稿件、切版本旧回包、立即 Escape 且一次只关最顶层。

手工 smoke 使用合成项目和已保存小型引用：检查一个旧 revision，修改/删除其来源，再导包核对保留快照、当前 availability 和遗漏；Windows 路径含空格、保存取消、磁盘写入失败。无需真实模型/SSH。测试通过仍不表示独立科研复现已执行。

| 现有/拟议文件 | 责任 |
| --- | --- |
| `src-tauri/src/workspace_navigation.rs:273`, `:488`, `:506`, `:876` | 现有 source 解析、availability、legacy 公开 hash、Markdown 导出；抽出必要公开投影接缝，不扩大重构 |
| `crates/omicsops-store/src/workspace_navigation.rs:720` | 现有 revision 读取；新增单 revision 只读 context 读取，避免加载所有历史 |
| `crates/omicsops-store/src/workspace_journey.rs:12`, `:32`, `:63` | 复用所有权和受限 record 投影 |
| `crates/omicsops-science/src/lib.rs:58`, `:111`, `:127`, `:142` | 复用已记录的 lineage、有效性和 provenance issue；不修改科学状态 |
| `crates/omicsops-dto/src/publication_evidence.rs`（新增） | 请求、报告、错误、包 manifest 数据契约 |
| `src-tauri/src/workspace_publication_evidence.rs`（新增） | 报告编排、安全包生成、宿主导出 |
| `src/features/workspace/WorkspacePublicationPage.tsx:61`, `:131`, `:156` | 保存版本选择、报告与包入口、原有语义提示 |
| `src/workspace-navigation-api.ts`, `src/workspace-navigation-types.ts` | typed 命令包装和前端契约 |
| `src-tauri/src/lib.rs`, `src-tauri/src/dto_contract_tests.rs` | 注册及 serde 契约验证 |

W09 不依赖 W10/W11/W12；W12 交付后仍能解析已收起会话的历史引用。

## W10：显式小文件基线与有界差异

### 用户目标、推荐方案与取舍

研究者在一次探索中显式选择若干脚本或小型文本产物，保存当时内容及身份，稍后与当前本地文件比较。基线可关联一个现有探索分支，但不创建分支、不切换 Git、不写项目文件、不重跑计算。首范围没有“恢复到文件”命令；基线正文是后续恢复设计可使用的材料，当前只提供查看和 diff。

推荐独立 typed baseline 表与内容寻址的宿主文件存储。legacy `workspace_snapshots` 保留原样，不解释旧 `manifest_json` 为新 schema。只有安全 UTF-8 小文本捕获正文；binary、Notebook/Office 压缩容器、大文件、SSH 文件和敏感文本保留明确 omission/reference。相对路径的任意目录没有隐式发现或递归选择，科学数据集不会因关联分析而自动加入。

### 精确命令与 DTO

拟议命令放在 `src-tauri/src/workspace_file_baselines.rs`：

```rust
pub async fn workspace_create_file_baseline(
    app: AppHandle, state: State<'_, AppState>, request: CreateFileBaselineRequestV1,
) -> Result<FileBaselineV1, FileBaselineErrorV1>;
pub async fn workspace_list_file_baselines(
    state: State<'_, AppState>, request: ListFileBaselinesRequestV1,
) -> Result<FileBaselinePageV1, FileBaselineErrorV1>;
pub async fn workspace_get_file_baseline(
    state: State<'_, AppState>, project_id: Uuid, baseline_id: Uuid,
) -> Result<FileBaselineV1, FileBaselineErrorV1>;
pub async fn workspace_diff_file_baseline(
    app: AppHandle, state: State<'_, AppState>, request: DiffFileBaselineRequestV1,
) -> Result<FileBaselineDiffV1, FileBaselineErrorV1>;
pub async fn workspace_read_baseline_text(
    app: AppHandle, state: State<'_, AppState>, request: ReadBaselineTextRequestV1,
) -> Result<BaselineTextSnapshotV1, FileBaselineErrorV1>;
```

拟议 `crates/omicsops-dto/src/workspace_files.rs` 在共享选择类型之外定义：

```rust
pub struct CreateFileBaselineRequestV1 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub branch_conversation_id: Option<Uuid>,
    pub title: String,
    pub files: Vec<ProjectFileSelectionV1>,
}
pub struct ListFileBaselinesRequestV1 {
    pub project_id: Uuid,
    pub branch_conversation_id: Option<Uuid>,
    pub offset: u32,
    pub limit: u32,
}
pub struct DiffFileBaselineRequestV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub expected_manifest_sha256: String,
}
pub struct ReadBaselineTextRequestV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub expected_manifest_sha256: String,
    pub entry_index: u32,
}
pub struct BaselineTextSnapshotV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub entry_index: u32,
    pub relative_path: String,
    pub full_sha256: String,
    pub text: String,
}
pub enum BaselineOmissionCodeV1 {
    RemoteReference, TooLarge, UnsupportedContent, SensitiveContent,
    Missing, Unreadable, UnsafeFile, AggregateBudget,
}
pub struct FileBaselineEntryV1 {
    pub backend_id: String,
    pub relative_path: Option<String>,
    pub content_mode: WorkspaceContentModeV1,
    pub size_bytes: Option<u64>,
    pub hash: Option<FileReferenceHashV1>,
    pub omission: Option<BaselineOmissionCodeV1>,
    pub captured_at: String,
}
pub struct FileBaselineV1 {
    pub schema_version: u8, // 1
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch_conversation_id: Option<Uuid>,
    pub title: String,
    pub entries: Vec<FileBaselineEntryV1>,
    pub manifest_sha256: String,
    pub captured_text_bytes: u64,
    pub created_at: String,
}
pub struct FileBaselinePageV1 {
    pub items: Vec<FileBaselineV1>,
    pub next_offset: Option<u32>,
}
pub enum BaselineDiffStateV1 {
    Unchanged, Changed, Missing, Unreadable, UnsafeFile,
    ReferenceOnly, BindingChanged, CurrentTooLarge, SensitiveCurrentContent,
}
pub struct BaselineTextHunkV1 {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<BaselineDiffLineV1>,
}
pub enum BaselineDiffLineKindV1 { Context, Added, Removed }
pub struct BaselineDiffLineV1 {
    pub kind: BaselineDiffLineKindV1,
    pub text: String,
}
pub struct FileBaselineDiffEntryV1 {
    pub baseline: FileBaselineEntryV1,
    pub state: BaselineDiffStateV1,
    pub current_size_bytes: Option<u64>,
    pub current_hash: Option<FileReferenceHashV1>,
    pub line_endings_changed: Option<bool>,
    pub final_newline_changed: Option<bool>,
    pub hunks: Vec<BaselineTextHunkV1>,
    pub hunks_truncated: bool,
}
pub struct FileBaselineDiffV1 {
    pub project_id: Uuid,
    pub baseline_id: Uuid,
    pub manifest_sha256: String,
    pub observed_at: String,
    pub entries: Vec<FileBaselineDiffEntryV1>,
}
pub enum FileBaselineErrorCodeV1 {
    InvalidSelection, NotFound, OwnershipMismatch, RequestConflict,
    SourceChanged, ManifestChanged, LimitExceeded, StorageFailed,
}
pub struct FileBaselineErrorV1 {
    pub code: FileBaselineErrorCodeV1,
    pub message: String,
}
```

基线DTO不暴露内部blob路径、原始绝对项目根或file identity。`TextSnapshot` entry必须具有安全 `relative_path=Some(...)`、`hash.scope=FullFile` 和非空blob hash；敏感路径只能为reference-only且公开/内部manifest的relative_path都为None。manifest内部额外保存 `local_root_binding_sha256:String` 和每项 `blob_sha256:Option<String>`；公开manifest hash对固定schema的内部manifest UTF-8字节计算，包含此根绑定指纹但不能据指纹恢复根。UI只使用回执中的hash，不自行重算或构造内部manifest。

Store 内部接口为：

```rust
pub async fn workspace_find_file_baseline_request(
    &self, request_id: Uuid, request_sha256: &str,
) -> Result<Option<FileBaselineV1>, StoreError>;
pub async fn workspace_save_file_baseline(
    &self, record: SaveFileBaselineRecord,
) -> Result<FileBaselineV1, StoreError>;
pub async fn workspace_list_file_baselines(
    &self, request: &ListFileBaselinesRequestV1,
) -> Result<FileBaselinePageV1, StoreError>;
pub async fn workspace_get_file_baseline(
    &self, project_id: Uuid, baseline_id: Uuid,
) -> Result<Option<StoredFileBaseline>, StoreError>;
```

`SaveFileBaselineRecord` 的字段为 `request_id:Uuid`、`request_sha256:String`、`baseline:FileBaselineV1`、`internal_manifest_json:String`；`StoredFileBaseline` 的字段为 `baseline:FileBaselineV1`、`local_root_binding_sha256:String`、`blob_sha256_by_entry:Vec<Option<String>>`，向量与entries一一对应。这些类型仅在Store/宿主边界使用，不能反序列化前端请求为它们。宿主核验 immutable manifest 与blob hash，不接受前端提供的正文或blob路径。查找请求、写入和quota判定复用workspace request ledger的operation/digest/result语义；`workspace_save_file_baseline`事务内再检查replay和project/branch所有权。

实施后的Store内部固定schema为`FileBaselineManifestV1`（`schema_version=1`），字段是id、project_id、原始branch_conversation_id、title、entries、captured_text_bytes、created_at与local_root_binding_sha256。每个`FileBaselineManifestEntryV1`显式嵌套`entry: FileBaselineEntryV1`和`blob_sha256`，拒绝未知字段，不使用flatten或复制另一套entry DTO。manifest不包含自身hash，按所存UTF-8原始字节计算；当前nullable分支关联由row投影，unlink不改写原manifest。这个新增内部schema成为后续宿主生产者契约，未来演进需要兼容处理。请求ledger保存project/baseline identity并按当前row返回回执，保留原manifest/hash；host给出的规范request digest原样保存。

### 捕获与 diff 的预算及失败语义

标题 1–80 字符、最多 320 字节。每次 1–32 个明确选择；仅 `.py`、`.r`、`.rmd`、`.md`、`.txt`、`.json`、`.csv`、`.tsv` 的 UTF-8 文本允许正文捕获，扩展名大小写不敏感。扩展名不是内容可信证明；仍需无 NUL、敏感内容检查和公开投影一致。`.env`、认证/私钥/凭据文件名、`.omicsops` 内部目录及其等价 Windows 名称禁止正文捕获。安全相对路径也必须脱敏不改变；路径含秘密时省略公开路径，给出 `SensitiveContent`，内部 manifest 不保存该原文。Notebook/HTML/Office 的解析和渲染不进入此首范围。

单文件完整读取上限 512 KiB，捕获正文合计 4 MiB；排序后的下一项若使总额超标，记录 `AggregateBudget` reference-only。捕获前即过大的文件只读取 metadata，不计算新的完整 hash。SSH 文件仅使用项目绑定引用和已有可信记录的 size/hash，未知为 `None`，标注 `RecordedFile`；不调用 SSH/MCP。实际只读Store helper在同一事务核验当前绑定，以V4 artifact/producer analysis的项目、路径、backend与row/body canonical UUID相符为依据。每份JSON最多64 KiB，JSON函数由有界有效CASE保护，最多取3对（384 KiB），仅至多2份无歧义候选可提供已知size/hash。先按实际ArtifactRecordV4/AnalysisRecordV4解码并核验schema4、artifact valid、producer succeeded及row/body一致；必填size非u64或缺失使整条未知，合法size与无效hash则保留size、仅将hash设为None。过多、畸形、冲突或无legacy provenance时保守未知，代价是不能局部挽救schema不完整的历史记录。代价是省略部分历史metadata，查询仍可能筛选项目既有rows，不进行远端核验。敏感文本即使已经暂时读取，也不写入 blob、SQLite、日志或模型上下文；若安全完整读取后可以保留 hash，它仍只标示已读字节，不表示内容被保存。

局部文件打不开、missing、unsafe、unsupported 等可预期文件级结果保留为 reference-only entry；错误消息不回显未验证路径。既有W02的`FileReadError::Unavailable`不能区分不存在、权限不足和其他不可用，捕获映射为`Unreadable`，diff及W11消费者也保留该不确定性；不新增探测或改写W02框架。`Missing`保留给未来能明确证明不存在的生产者，代价是缺失文件提示较不具体。格式非法、backend/分支归属错误、请求 ID 冲突或本次实际完整 hash 不等于 `expected_full_sha256` 则整次失败，不提交基线。expected hash 无法在预算内核验也拒绝整个请求，提示重新选择或取消此断言，不以 recorded hash 匹配。创建前先查相同请求的 committed receipt，重试不会重新捕获已变化的文件；并发提交在事务中再次判 replay。未知回执错误重试原请求，不生成新 ID。

读取保存正文仅接受所选baseline的entry index和manifest hash，不接受blob path；校验同项目、TextSnapshot、宿主固定存储路径、完整blob长度/hash和UTF-8，最多512 KiB，再次执行公开/敏感检查。reference-only项返回InvalidSelection；坏blob或公开检查不一致返回StorageFailed，不显示未经校验正文。正文查看不依赖当前项目根或当前源文件还存在。

diff校验project/baseline/manifest hash和当前local root binding。根变更返回每项 `BindingChanged`，不自动把相同相对路径当原文件。只对 `TextSnapshot` 项重新执行相同 `read_complete`；reference-only项返回 `ReferenceOnly`，不升级为读取。当前文件太大、敏感、unsafe或missing都不返回正文hunk。相同完整hash为Unchanged；不同hash为Changed。差异按UTF-8文本行计算，通过 `line_endings_changed`/`final_newline_changed` 给出CRLF/最终换行差异注记；未成功比较的项两字段均为None，不执行代码。hunk输出合计最多256 KiB/2,000行；算法输入最多每侧10,000行，比较工作量最多200,000单元，超限仅给出Changed/hash与 `hunks_truncated=true`，不运行无界二次复杂度算法。200,000比较/工作单元按整个请求累计，不逐文件重置；每个可比较文件先计2*(旧行数+新行数)遍历/等同比较预算，再为LCS预留旧行数*新行数，normalized-equal也计线性预算。该保守策略使后续文件可能更早仅返回Changed/hash及hunks_truncated。256 KiB限制覆盖所有entry的hunk数组JSON（含转义、逗号及header），2,000输出行也按全请求累计。捕获与diff均在尝试读取前按已打开metadata size预留payload额度，并将同一精确额度传给W02；失败不返还额度，单项512 KiB、全请求4 MiB约束覆盖失败尝试的payload allowance，代价是失败后后续文件可能更早被省略。W02原有一字节EOF probe保持独立，不声称含probe的全部底层I/O不超过4 MiB；SourceChanged仍整次拒绝，capture不提交基线。余量不足时V1无aggregate-budget状态，保守返回Unreadable且无当前hash/hunks、换行字段None，代价是预算耗尽提示较不具体。首版hunk为有界全文件/前缀视图，不保证最小上下文窗口；截断明确标示。diff不写Store或项目文件。

### 存储、迁移、UI 与恢复边界

新增 `exploration_baselines_v4`，字段为 `id TEXT PRIMARY KEY`、`project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE`、`branch_conversation_id TEXT NULL REFERENCES conversation_records(frame_id) ON DELETE SET NULL`、`request_id TEXT NOT NULL UNIQUE`、`request_sha256 TEXT NOT NULL`、`manifest_sha256 TEXT NOT NULL`、`manifest_json TEXT NOT NULL`、`captured_text_bytes INTEGER NOT NULL CHECK >=0`、`created_at INTEGER NOT NULL`。索引 `(project_id,created_at DESC,id)` 与 `(project_id,branch_conversation_id,created_at DESC,id)`。Store提交时校验branch是同项目现有分支，并在 `validate_before_commit` 加所有权验证。删除branch conversation时FK只清空关联，基线仍在；archive/merge只改变已有分支状态，不删除基线或源文件。若单独移除branch relation而保留conversation，移除该relation的Store事务须同时unlink基线关联。当前实际生产删除入口为`Store::delete_conversation`，无单独relation删除API；本次不为该条件新增操作，未来增加此类入口时必须落实同事务unlink。

blob 位于 `app_data_dir/workspace-file-baselines/<project_id>/sha256/<前两位>/<sha256>`，名字仅由宿主 hash 构造，每项正文 UTF-8 原字节不做换行转换。SQLite 只保存 manifest/身份/hash，不保存文件正文。每项目最多 64 个基线、正文计费总额 256 MiB（按每个基线逻辑内容计费，不能利用去重绕过），达到上限返回 `LimitExceeded`，不自动删除旧基线。持久blob的实际磁盘硬上限也为256 MiB，每次创建最多额外4 MiB staging；先清理确认未引用的旧blob，再在persist前核验物理预算，不允许失败重试累积突破上限。

先写同目录临时 blob、sync、校验 hash 并 persist，再提交引用它们的 DB 事务。同目录指最终hash前缀目录；固定`baseline-<16 ASCII alphanumeric>.tmp`计入独立4 MiB staging，最终hash文件计入256 MiB持久预算。扫描最多256前缀、4096文件、32临时文件；未知名字/不安全祖先拒绝处理，Windows保留目录句柄，清理不递归删除。其代价是额外分类/上限及保守拒绝，可能读取校验最多项目物理预算内的既有blob。已存在 hash blob 必须校验长度/hash；损坏时停止。事务失败不能产生引用缺失 blob 的已提交基线；未提交 staging 可删除，persist 后的无引用 blob只由宿主扫描 ledger/manifest 确认未引用后清理。每项目创建使用宿主 project-scoped mutex，清理与创建互斥；清理只覆盖此固定宿主目录且同样拒绝 reparse，绝不遍历/删除项目根。项目删除的 DB cascade 后由专用宿主清理移除其 blob，不把路径从数据库拼接到 shell。实际`delete_project`保留原活动运行/内核检查，同一project mutex覆盖DB删除与清理；DB已提交后清理失败必须明确报告项目已删除、基线存储清理失败，不能称回滚或暗中重试。代价是可能保留私有孤儿blob/空目录；不扩展为删除项目源文件。无基线删除或正文恢复命令进入首范围。

初始化只增加 `CREATE TABLE/INDEX IF NOT EXISTS` 和验证，不重写 legacy `workspace_snapshots`、不更改 schema version 解释、不挪动已有项目根。重复打开/迁移无重复 rows；旧库没有新表时列表为空。

UI 在现有文件侧栏提供多选后的“保存探索基线”；选择器从 `WorkspaceShell` 已绑定的backend与 `RemoteFileTree` 显式文件选择构造请求。当前会话属于现有分支时可由 `ConversationBranchBanner` 带明确branch ID打开同一选择器，不假设存在独立分支页。展示逐项TextSnapshot/reference-only和遗漏原因。保存成功显示捕获时间与“多文件依次读取”。基线列表、选择与diff为独立只读面板；没有执行、同步、恢复到原路径按钮。异步结果绑定project/baseline/generation；对话框进入窗口Escape栈，关闭不撤销已提交基线。WorkspaceShell按project/backend在窗口寿命内保留pending原请求与已知回执，通过可选prop交给面板；关闭不销毁未知request identity，不保留隐藏dialog/Escape层。代价是暂存原选择/请求metadata，不承诺跨重启持久化或跨窗口lease。返回原scope可显式重试同ID同正文，旧scope结果只能结清原record，不能覆盖新项目选择或后来打开的正文/diff。仅native创建返回SourceChanged或LimitExceeded时提供显式编辑/新请求入口且不自动派发，因为这两码发生在新基线提交前；其余码与本地错误响应仍保留原ID。InvalidSelection也可能来自已有receipt的manifest解析，不能把所有typed error视为未提交；代价是歧义错误下恢复保守。允许新请求不声称没有私有orphan blob。

### 验收、文件地图与依赖

自动化覆盖：明确选择与排序去重、正文/整体/项目预算、known/unknown recorded hash、敏感正文降引用且落盘扫描无秘密、相同请求重试不二次捕获、同 ID 不同请求冲突、文件修改导致 expected hash拒绝、并发身份/长度变化、根变更、UTF-8/CRLF/尾换行、missing/大文件/SSH/binary、diff 算法和输出上限、坏 blob、blob写失败/Store失败无已提交坏基线、重复迁移、legacy表不被解释、跨项目分支拒绝、删除分支仅 unlink、删除项目清理不触及源文件、立即 Escape、旧响应丢弃。Windows 临时目录覆盖 ADS、设备、保留名、路径大小写、空格、ancestor junction 与 reparse；无需真实 SSH。

| 现有/拟议文件 | 责任 |
| --- | --- |
| `src-tauri/src/project_file_read.rs`（W02） | 统一有界读取与 Windows handle 验证 |
| `crates/omicsops-core/src/sync.rs:19` | 保留已有显式选择语义；不把 sync 选择当完整文件安全检查 |
| `crates/omicsops-store/migrations/init.sql:99`, `:679` | 现有分支/legacy snapshot；新增 typed 表与索引，旧 rows 原样保留 |
| `crates/omicsops-store/src/workspace_file_baselines.rs`（新增） | 基线 ownership、quota、request replay、immutable metadata |
| `crates/omicsops-dto/src/workspace_files.rs`（新增） | 选择、基线、diff、错误 DTO |
| `src-tauri/src/workspace_file_baselines.rs`（新增） | 捕获、blob生命周期、有界 diff |
| `src/features/workspace/WorkspaceShell.tsx:537`, `:569`, `:1205` | 现有文件侧栏与backend身份，接入选择/基线面板 |
| `src/features/workspace/RemoteFileTree.tsx:34`, `:90`, `src/features/workspace/ConversationBranchBanner.tsx` | 显式文件选择及可选分支关联入口 |
| `src/features/workspace/WorkspaceFileBaselinesPanel.tsx`（新增） | 列表、状态、差异与 Escape 接入 |
| `src/workspace-navigation-api.ts`, `src/workspace-navigation-types.ts`, `src-tauri/src/dto_contract_tests.rs` | UI边界与验证 |

依赖 W02 的共享文件接缝；不依赖 W09/W11/W12。现有探索分支是可选关联，基线本身可以独立创建。

## W11：可选择迁移的清单预览

### 用户目标、推荐方案与取舍

研究者能先看到一个项目未来可迁移的安全 metadata、显式选中文件、大小/hash/遗漏，以及必须重新配置的能力。首范围只生成内存中的只读清单；没有包导出、复制、导入新项目、全 SQLite dump、实际重绑定、远端连接或凭据移动。

推荐从 project-scoped Store 白名单投影和 W02 文件接缝生成清单。preview digest 是观察内容的指纹，不是批准令牌或承诺未来导出相同内容。metadata 类别可选，文件必须逐项选择。以计数与有界摘要代替完整 transcript/事件/科学 JSON，可先交付真实可用的选择界面而不提前定义完整迁移协议。

### 精确命令与 DTO

```rust
pub async fn workspace_preview_project_migration(
    state: State<'_, AppState>, request: ProjectMigrationPreviewRequestV1,
) -> Result<ProjectMigrationPreviewV1, MigrationPreviewErrorV1>;
```

拟议 `crates/omicsops-dto/src/project_migration.rs`：

```rust
pub enum MigrationMetadataCategoryV1 {
    Conversations, Publications, ScientificMetadata,
}
pub struct ProjectMigrationPreviewRequestV1 {
    pub project_id: Uuid,
    pub categories: Vec<MigrationMetadataCategoryV1>,
    pub files: Vec<ProjectFileSelectionV1>,
}
pub struct MigrationProjectMetadataV1 {
    pub source_project_id: Uuid,
    pub name: String,
    pub description: String,
    pub template: String,
    pub created_at: String,
}
pub struct MigrationCategorySummaryV1 {
    pub category: MigrationMetadataCategoryV1,
    pub selected: bool,
    pub record_count: u64,
    pub examples: Vec<MigrationRecordSummaryV1>,
    pub examples_truncated: bool,
}
pub struct MigrationRecordSummaryV1 {
    pub source_id: String,
    pub kind: String, // host allowlist; not an arbitrary table name
    pub label: String,
    pub status: String,
    pub revision: Option<u64>,
}
pub enum MigrationFileStateV1 {
    SmallTextCandidate, ReferenceOnly, Missing, UnsafeFile,
    SensitiveContent, UnsupportedContent, Unreadable,
}
pub struct MigrationFilePreviewV1 {
    pub backend_id: String,
    pub relative_path: Option<String>,
    pub size_bytes: Option<u64>,
    pub hash: Option<FileReferenceHashV1>,
    pub state: MigrationFileStateV1,
    pub reason: String,
}
pub enum MigrationRebindKindV1 {
    ProjectPaths, SshConnection, ModelProfile, KernelEnvironment,
    McpServer, SkillGrant, RuntimeJobs,
}
pub struct MigrationRebindRequirementV1 {
    pub kind: MigrationRebindKindV1,
    pub count: u64,
    pub message: String,
}
pub struct ProjectMigrationPreviewV1 {
    pub schema_version: u8, // 1
    pub project: MigrationProjectMetadataV1,
    pub observed_at: String,
    pub scientific_revision: Option<u64>,
    pub categories: Vec<MigrationCategorySummaryV1>,
    pub files: Vec<MigrationFilePreviewV1>,
    pub selected_known_file_bytes: u64,
    pub unknown_size_file_count: u32,
    pub rebind: Vec<MigrationRebindRequirementV1>,
    pub exclusions: Vec<String>, // fixed safe reason labels
    pub preview_sha256: String,
}
pub enum MigrationPreviewErrorCodeV1 {
    InvalidSelection, NotFound, OwnershipMismatch,
    SourceChanged, LimitExceeded, StorageReadFailed,
}
pub struct MigrationPreviewErrorV1 {
    pub code: MigrationPreviewErrorCodeV1,
    pub message: String,
}
```

类别数组去重，空数组合法，只预览项目基本 metadata 与显式文件。Project 基本 metadata 始终包含；description 最多 4 KiB，name 最多 800 字节；公开投影改变或过长时使用安全截断并在 exclusions 说明。template/status/kind 用宿主固定值，不返回任意 metadata map。每类别 examples 最多 20 条；只在选中类别取摘要，不取正文。ScientificMetadata 的计数和例子按 dataset/analysis/artifact/evidence/provenance 分 kind 表达；publication 例子只给当前 revision 的标题/状态/hash摘要，conversation 例子只给标题/状态，不含消息或模型配置。

Store 新增 `workspace_project_migration_metadata(&self,project_id:Uuid,categories:&[MigrationMetadataCategoryV1])->Result<MigrationMetadataReadContext,StoreError>`，在一个只读事务内取项目、类别 COUNT/有界摘要、science header 与能力绑定计数。只访问显式列和批准的表，不枚举 SQLite schema，不调用 `list_conversations` 的标题修复路径，不读取 credentials/profile密文。是否包含已收起会话由 Conversations 类别统一包括并给出 `shelved` 计数注记；收起不是删除或秘密标记，选择 Conversations 时不能静默漏掉历史。

### 有界文件策略、失败语义与授权

文件最多 32 个，总小文本读取预算 4 MiB、每项 512 KiB。只对 W10 同一安全文本候选执行 `read_complete`，得到本次 full hash后立即丢弃正文；相同安全/脱敏判断决定 SmallTextCandidate 或 SensitiveContent。预览没有正文输出，也没有 blob/SQLite 写入。超过单项或整体预算的普通文件只 metadata；使用已有可信 saved artifact hash 时显式标 RecordedFile，没有 hash 时为 None。目录、unsupported binary、SSH引用保持 reference-only，不加载真实大样本或偷偷完整 hash。远端 size/hash 仅来自已保存记录；unknown 明确显示，不视为零。

`expected_full_sha256` 与 W10 同义：若提供，必须实际完整核验；预算不足或不匹配整次 SourceChanged/LimitExceeded，无“预览成功”。无 expected hash 的个别缺失/unsafe/unreadable 文件保留文件级状态。规范化非法、backend 不属于该 project 等身份错误整次拒绝。DB 状态在事务内一致；随后的本地文件读取有逐项时间，不能声称 DB+文件是原子项目快照。

`preview_sha256` 对 schema/version、公开 project 投影、选择类别与摘要、选择文件状态/hash、rebind/exclusions 的稳定序列化计算；不包含 observed_at 或该 hash 自身。因此同内容同指纹；它不替代下一次预览或未来实际导出时重新核验。

rebind 固定说明：新项目需要重新选择 local/remote roots；SSH 密钥、主机信任与连接配置需在目标环境重新设置；模型凭据与 endpoint 不移植；kernel environment 只作为需求重新探测，不能把“发现解释器”当依赖可用；MCP/Skills 重新安装并授权；运行、审批、连接/会话/后台 job 恢复身份不导入可执行状态。运行中 job 在原项目继续，预览不取消、不重连、不重新派发。计数来自安全绑定记录，不能暴露 keyring定位符、密码、私钥、服务器用户名/地址、环境秘密或被冻结批准计划。

exclusions 至少包含固定说明：原始大样本与未选文件不包含；凭据及授权不移植；缓存/日志/应用 DB 不包含；运行与恢复身份不移植；本次仅预览、实际导出/导入未执行。没有目的地参数、写回回执或迁移成功状态。无迁移表、schema变更或 legacy数据写入。

UI 从现有 `ProjectLibrary` 项目卡片的独立“迁移清单预览”按钮进入，由 `DesktopApp` 持有选中预览项目身份；不必先打开或恢复该项目会话。对话框提供类别开关、显式本地文件选择、已保存SSH引用选择、逐项状态、known bytes与unknown count、重绑定清单。默认类别均不选、文件为空；项目metadata固定显示。用户点击“生成预览”触发一次查询，选择变更标记旧结果过期，必须重新生成；没有Export/Import按钮。文件选择仅使用本地metadata catalog和已保存remote引用，不复用会触发SSH刷新的文件按钮。结果绑定project/选择digest/generation；切项目旧回包丢弃。对话框进入窗口Escape栈；关闭不产生任何迁移或运行动作。

### 验收、文件地图与依赖

自动化覆盖：空选择、类别去重、project所有权、COUNT与有界例子、类别未选不加载正文、legacy/shelved计数不漏、Windows unsafe paths、文本完整hash/大文件unknown hash、overall预算、模拟SSH零调用、profile/keyring零读取、私钥与凭据样例无输出、固定排除/rebind项目、expected hash冲突、preview digest稳定且不作为token、存储写入零次、kernel/job动作零次、选择或project变化丢弃旧响应、立即Escape。smoke用合成metadata和小脚本，在Windows检查已知/未知大小、仅引用、类别切换和重新生成；不使用真实样本、真实SSH或全库导出。

| 现有/拟议文件 | 责任 |
| --- | --- |
| `crates/omicsops-core/src/workspace.rs:26` | project metadata 来源；root/connection 不直接公开 |
| `crates/omicsops-store/src/lib.rs:361`, `:4300` | project与科学状态既有边界；预览新增header/分表投影，不读取完整state JSON |
| `crates/omicsops-store/src/project_migration.rs`（新增） | 只读COUNT/摘要/安全绑定计数 |
| `crates/omicsops-dto/src/project_migration.rs`（新增） | 纯预览契约 |
| `src-tauri/src/project_migration_preview.rs`（新增） | metadata编排、W02文件读取、安全清单 |
| `src-tauri/src/project_file_read.rs`（W02） | 本地文件identity/预算接缝 |
| `src/features/projects/ProjectLibrary.tsx:18`, `:114`, `src/DesktopApp.tsx:1250` | 项目卡片预览入口与宿主项目身份 |
| `src/features/projects/ProjectMigrationPreviewDialog.tsx`（新增） | 显式选择与只读预览 UI |
| `src/tauri-api.ts`, `src/types.ts`, `src-tauri/src/dto_contract_tests.rs` | typed调用、边界验证 |

依赖 W02 及共享 `ProjectFileSelectionV1`；可共享 W10 的纯安全文本判定函数，不依赖其基线表/blob或创建命令。实际 export/import 是独立后续，不是本文件批准实施的内容。

## W12：独立可见性、会话收起与恢复

### 用户目标、推荐方案与取舍

研究者可以把暂不需要的会话从默认导航、全局搜索会话候选与 `#` Session 候选中收起，并在独立“已收起会话”入口查询、打开和恢复。历史消息、run、分组、分支、稿件引用和精确Session引用保留原身份。

推荐用独立 visibility table，缺省 Visible，不改变 core `Conversation` 的 status/model字段，也不以 Archived 状态实现收起。运行中的会话也可收起，因为这是本机可见性偏好；其原 run继续，job不会取消，active对话仍可查看/发送并显示“已收起”横幅。新消息不会自动恢复可见性，恢复是显式操作。

默认导航只显示 Visible；收起当前打开会话后保持当前 transcript，不把它重新插入默认列表。切项目/下次默认进入项目时，latest-used只在Visible中选；没有可见候选则显示项目空状态，保留“已收起会话”入口。通过保存来源、搜索独立收起集合或精确导航打开Shelved不会自动恢复。

### 精确命令与 DTO

保留 `list_conversations(state,project_id)->Result<Vec<Conversation>,String>` 和 `latest_used_conversation` 的旧响应形状，宿主展示查询改为 Visible。保留 Store `conversations_for_project` 的 ALL 语义，避免已有引用校验依赖列表而失效。新增以下命令：

```rust
pub async fn workspace_list_conversation_visibility(
    state: State<'_, AppState>, request: ConversationVisibilityListRequestV1,
) -> Result<ConversationVisibilityPageV1, ConversationVisibilityErrorV1>;
pub async fn workspace_set_conversation_visibility(
    state: State<'_, AppState>, request: SetConversationVisibilityRequestV1,
) -> Result<ConversationVisibilityReceiptV1, ConversationVisibilityErrorV1>;
pub async fn workspace_get_conversation_visibility(
    state: State<'_, AppState>, project_id: Uuid, conversation_id: Uuid,
) -> Result<ConversationVisibilityRecordV1, ConversationVisibilityErrorV1>;
pub async fn workspace_get_conversation(
    state: State<'_, AppState>, project_id: Uuid, conversation_id: Uuid,
) -> Result<Conversation, String>;
```

拟议 `crates/omicsops-dto/src/conversation_visibility.rs`：

```rust
pub enum ConversationVisibilityV1 { Visible, Shelved }
pub enum ConversationVisibilityFilterV1 { Visible, Shelved, All }
pub struct ConversationVisibilityListRequestV1 {
    pub project_id: Uuid,
    pub visibility: ConversationVisibilityFilterV1,
    pub query: String,
    pub offset: u32,
    pub limit: u32,
}
pub struct ConversationVisibilityRecordV1 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub visibility: ConversationVisibilityV1,
    pub revision: u64,
    pub changed_at: Option<String>,
}
pub struct ConversationVisibilityListItemV1 {
    pub state: ConversationVisibilityRecordV1,
    pub title: String,
    pub conversation_status: String,
    pub conversation_created_at: String,
    pub conversation_updated_at: String,
}
pub struct ConversationVisibilityPageV1 {
    pub items: Vec<ConversationVisibilityListItemV1>,
    pub next_offset: Option<u32>,
}
pub struct SetConversationVisibilityRequestV1 {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub expected_revision: u64,
    pub visibility: ConversationVisibilityV1,
}
pub struct ConversationVisibilityReceiptV1 {
    pub request_id: Uuid,
    pub state: ConversationVisibilityRecordV1,
    pub changed: bool,
}
pub enum ConversationVisibilityErrorCodeV1 {
    InvalidInput, NotFound, OwnershipMismatch, RevisionConflict,
    RequestConflict, StorageFailed,
}
pub struct ConversationVisibilityErrorV1 {
    pub code: ConversationVisibilityErrorCodeV1,
    pub message: String,
}
```

list item 是特定用途投影，不复制model_profile_id或core对象；`conversation_status` 由 core enum序列化的现有值产生，TypeScript使用现有 `WorkspaceConversation["status"]` 校验，不另起状态枚举。直接打开返回现有 `Conversation`；它不进行模型/运行恢复，也不改变visibility。page `query` 最多200字符/1KiB，只搜公开标题；limit=1..100、offset<=10,000，默认UI50条。按 conversation updated_at DESC、id稳定排序，SQL先project/visibility/query过滤再分页；不拉一页后在React剔除。offset分页不承诺并发收起下的稳定快照；操作后重置到第一页并刷新。

Store 新增 `workspace_conversations_for_presentation(&self,project_id:Uuid)->Result<Vec<Conversation>,StoreError>`（Visible，与旧展示形状一致）、`workspace_latest_visible_conversation(&self,project_id:Uuid)->Result<Option<Conversation>,StoreError>`、`workspace_list_conversation_visibility(&self,request:&ConversationVisibilityListRequestV1)->Result<ConversationVisibilityPageV1,StoreError>`、`workspace_set_conversation_visibility(&self,request:&SetConversationVisibilityRequestV1)->Result<ConversationVisibilityReceiptV1,StoreError>`，以及 `workspace_get_conversation_visibility(&self,project_id:Uuid,conversation_id:Uuid)->Result<ConversationVisibilityRecordV1,StoreError>`。后者先验证同项目会话存在，再读取可见性；缺行返回 Visible/revision0，异项目或已删除会话拒绝，不能把查不到行都视为默认可见。它供精确来源打开后的横幅和恢复操作读取当前 revision，不能靠标题或分页集合猜测状态。精确打开复用现有 `workspace_source_conversation(project,id)`，不通过Visible列表判断对象是否存在。

### 幂等迁移、请求与失败语义

新增 unique index `idx_conversation_records_frame_project(frame_id,project_id)` 供复合FK使用；frame_id已有主键，因此不改变旧行唯一性。新增 `conversation_visibility_v4`：

```sql
CREATE TABLE IF NOT EXISTS conversation_visibility_v4 (
  conversation_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  visibility TEXT NOT NULL CHECK (visibility IN ('visible','shelved')),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  changed_at INTEGER NOT NULL,
  FOREIGN KEY (conversation_id,project_id)
    REFERENCES conversation_records(frame_id,project_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_conversation_visibility_project
  ON conversation_visibility_v4(project_id,visibility,conversation_id);
```

无行表示 Visible/revision0/changed_at=None；第一次实际变化插入revision1，恢复后保留Visible row与递增revision，不能删除行而丢失并发版本。既有会话不回填、不改title/status/updated_at、不迁移Archived或branch state。`CREATE ... IF NOT EXISTS`、父index先于新表、pre-commit完整性验证都在现有初始化事务内；重复迁移无新行。legacy库缺新表即可升级，外键/归属不合法的既存新表停止迁移而不静默修复。

设置在 `BEGIN IMMEDIATE` 事务中：先验证同一 request ID的operation与request digest；相同请求返回原receipt，不重复应用；同ID不同正文为RequestConflict。随后验证project/conversation归属与expected revision。陈旧expected值为RevisionConflict；当前值已等于目标时changed=false，不更改visibility revision/changed_at/conversation updated_at，但仍记录request receipt。实际变化只改visibility表，并在同事务写现有 `workspace_request_ledger` 的hash与result identifiers，不写raw请求或对话正文。

不同请求并发靠expected_revision决定先后；未知提交/丢失回执只能重试原request，不发新的相反操作。receipt是该次提交的历史结果；前端拿到replay receipt后再读取当前state/list，防止旧“收起成功”回包覆盖随后已经完成的恢复。project或conversation已删除时精确打开NotFound；删除仍沿现有独立流程，visibility FK cascade只清掉偏好。收起失败不从UI提前永久移除，会话状态/运行不变；UI显示可重试原请求或刷新冲突。

### 导航、搜索、`#` 与UI联动

`composer_references.rs` 只把catalog中的Session发现查询换成Visible，并在候选上限/分页前过滤；Artifact/Skill的独立可发现性保持原有规则。全局搜索通过该catalog构建Session候选，因此默认同样Visible；visibility mutation提交后使项目catalog/search缓存generation失效并刷新。仅在客户端当前页过滤不满足此边界。

`ensure_conversation` 和精确 `resolve_composer_references` 保留ALL或改为同项目direct getter；不能把其查询替换为Visible。已有Session chip、保存稿件来源和旧消息引用可解析Shelved。精确解析不是发现入口，不因运行/状态变化自动恢复。默认 `latest_used_conversation` 的展示命令用新的Visible查询，仍按真实user message时间判最近，收起不伪造message时间。

`DesktopApp.tsx` 所有通过 `listConversations(...).find(...)` 打开已知ID的路径改用 `workspace_get_conversation`：项目初始指定source、global search打开、usage/run来源、branch/merge/share等精确来源跳转。只改列表而漏掉这些入口会导致已有隐式引用“找不到”，因此属于同一首交付的必要范围。source resolver的所有权仍由Store确认，不能因为已知ID跳过project检查。

默认navigation每行提供“收起”，独立“已收起会话”入口打开paged集合，有标题查询、打开和恢复按钮。集合为空仍有清晰空状态。打开已收起会话在transcript显示横幅及“恢复显示”；打开不恢复，点击恢复才提交。集合及行操作遵循window Escape栈；立即Escape无需内部焦点，一次只关顶层。进行中的visibility写不锁整应用；关闭面板不撤销已提交操作，项目切换后旧回执不改变新项目UI。

分组成员关系保留；默认列表的分组只展示visible成员，收起集合可保留原分组label但不修改membership。现有branch archived/merged、ConversationStatus Archived和visibility可同时存在，分别展示；不把三者合并为一种归档状态。收起不成为安全隐藏，W11选择Conversations类别仍计数全部会话。

### 验收、文件地图与依赖

自动化覆盖：新旧库默认Visible与重复迁移、Archived/branch状态不变、跨project compositeFK和命令拒绝、重复request/no-op/冲突/replay旧回执、过滤先于分页且第一页不因隐藏行变空、latestVisible不选shelved、ALL内部查询保留、catalog/#/global默认无shelved Session、cache失效、直接引用/稿件旧来源仍解析、source/usage/branch精确打开不需Visible列表、运行中收起不取消/重跑/恢复kernel、active页面保留且新消息不自动unshelf、恢复出现原历史顺序、group关系不变、删除cascade无历史副作用、立即Escape及旧project回包隔离。使用temp SQLite与mock Tauri；不依赖真实模型/SSH。

Windows smoke：收起一个Idle和一个mock Running会话，关闭重开应用，检查默认导航与搜索/#，从独立集合/旧稿件来源打开后恢复；检查运行状态未变化、消息/分组/引用仍在。真实后台作业测试不作为自动验收前提。

| 现有/拟议文件 | 责任 |
| --- | --- |
| `crates/omicsops-core/src/workspace.rs:66` | status保持原义，不加入Shelved状态 |
| `crates/omicsops-store/src/lib.rs:1016`, `:1033`, `:4721` | 内部ALL查询保留；展示latest拆出Visible；初始化幂等接入 |
| `crates/omicsops-store/src/workspace_conversation_visibility.rs`（新增） | 可见性查询/CAS/ledger事务/归属验证 |
| `crates/omicsops-store/migrations/init.sql:71`, `:495` | 复合FK父index、新visibility表；复用request ledger |
| `crates/omicsops-store/src/workspace_journey.rs:12` | 同项目direct getter，精确打开隐藏来源 |
| `crates/omicsops-dto/src/conversation_visibility.rs`（新增） | requests/page/receipt/error纯DTO |
| `src-tauri/src/workspace_commands.rs:266`, `:305` | 既有展示命令改Visible，新增direct/list/set路由 |
| `src-tauri/src/composer_references.rs:85`, `:155`, `:325` | catalog发现过滤；精确解析保留ALL |
| `src/features/workspace/WorkspaceNavigation.tsx:104`, `:109` | 行操作、独立收起集合入口 |
| `src/features/workspace/ShelvedConversationsDialog.tsx`（新增） | paged查询/打开/恢复与Escape |
| `src/DesktopApp.tsx:375`, `:943`, `:985`, `:1030` | direct来源打开、Visible默认进入、receipt/cache联动；审核其他list.find入口 |
| `src/use-workspace-search.ts`, `src/workspace-search.ts` | Session发现缓存失效，默认过滤通过catalog完成 |
| `src/tauri-api.ts`, `src/workspace-navigation-api.ts`, `src/workspace-navigation-types.ts`, `src-tauri/src/dto_contract_tests.rs` | typed边界与序列化测试 |

W12独立于W09/W10/W11；与W01草稿共享project/conversation身份但不改变草稿保存语义。W09精确来源解析和W11所有会话计数是应保留的交互不变量。

## 评审结论所需信息与执行界限

本设计从当前源码确认了保存revision、公开来源快照、科学有效性/provenance、项目绑定、request ledger和对话查询接缝。新模块、新DTO、读context、新表、blob存储、UI入口和测试均是提案，没有实现或执行。已读取的上游参考事实保留在积压清单中；本设计没有重新联网读取上游，也不将上游允许scripts与same-origin的HTML做法带入产品。

首范围无需再选择真实主机、样本、模型或导入目的地。每章的预算和恢复边界已获用户确认，逐项计划见[实施总计划](../plans/2026-10-04-wisp-improvements.md)，待计划审阅及选择执行方式。实施后的确定性验证依照AGENTS.md运行最近测试及默认完整检查；此文档阶段未运行产品/运行时验收，没有独立复现已执行的主张。
