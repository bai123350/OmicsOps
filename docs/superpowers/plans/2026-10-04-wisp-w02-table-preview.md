# W02 Bounded Table Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在项目文件树直接查看本地或 SSH 的 CSV/TSV 有界前缀，准确展示读取范围、来源和截断。

**Architecture:** 共享宿主文件 handle 检查负责本地读取；独立纯逻辑记录解析器与可注入 source 分离；公开 DTO 只含脱敏、有界结果，UI 只读。

**Tech Stack:** Rust、已有 windows-sys/sha2/tempfile、现有 SFTP、Tauri 2、React 19/TypeScript/Vitest；无新增解析器或前端依赖。

**Spec:** [W02 设计](../specs/2026-10-04-wisp-composer-files-jobs-design.md#w02有界-csvtsv-表格预览)、[共同约束](../specs/2026-10-04-wisp-improvements-design.md)。用户已确认设计及8515da8实施计划。Task1共享读取、Task2 parser/DTO已实现并独立审阅通过；Task3 source/command与Task4界面待实施。

## Global Constraints

- UTF-8/BOM；由扩展名选择逗号或 tab，首行是表头。支持双引号、转义、字段内换行和 LF/CRLF，不猜编码/locale。
- 原始前缀最多256 KiB，额外1 byte只探 EOF、不计 hash；最多200数据记录、64列、公开单元格4 KiB UTF-8。列数超限拒绝，单元格只截断显示。
- hash 覆盖原始读取 bytes，包括 BOM/未展示记录/半字符；只有确认 EOF 且读取稳定才是 Full。read/parsed bytes 与 displayed rows 分开记录。
- 公共文本先调用现有 `public_text` 再做 UTF-8 显示截断；原始 bytes 不进 UI、日志、模型上下文或 Store。预览不生成 Quote，不改变完整文本引用的 hash 契约。
- `ValidatedProjectFile` 为 W02/W07/W10/W11 唯一项目普通文件读取接缝；只将确需复用的现有规范化函数改为 pub(crate)，不拆分其他模块。
- Windows 检查 ancestor/target reparse、打开后最终路径与身份；拒绝 UNC/设备/ADS/保留名/内部目录/遍历。读取前后变化拒绝，不承诺敌意并发原子快照。
- SSH 全路径20秒包含连接、信任、根与文件检查、metadata和读取。后台取消本地等待不表示远端 job 取消；预览不创建同步任务。
- 自动测试使用 tempdir、注入 metadata/transport 和 mock Tauri；所有弹层进入 window Escape 栈，晚响应按 source tuple/generation 丢弃。

## Review Focus

1. quoted newline/半 UTF-8/半记录不能制造错误行或吞掉中间非法编码（Task 2 `table_preview_record_boundaries`）。
2. budget+1 探测 byte 和未知稳定性不能被标 Full；raw hash 与 parsed bytes 分离（Tasks 1–3 `table_preview_hash_scope`）。
3. Windows junction 的 is_symlink=false 仍必须拒绝，打开 handle final path 逃逸亦拒绝（Task 1 `project_file_read_reparse_and_identity`）。
4. connect/根解析已消耗等待预算时仍受总20秒约束；切 backend 的旧响应不能覆盖 UI（Tasks 3–4 `table_preview_total_timeout`、`table_preview_scope_change`）。
5. 空/header-only 与 HTML/公式单元格显示为文本；立即 Escape 只关最顶层（Tasks 2/4 `table_preview_empty_and_text`、`table_preview_escape`）。

---

### Task 1: 同一已验证 handle 的共享有界本地读取

**Files:** Create `src-tauri/src/project_file_read.rs`（含 cfg(test)）；Modify `src-tauri/src/lib.rs`；仅必要时 Modify `src-tauri/src/composer_files.rs` 的 normalization visibility。

**Interfaces:** Produces `ValidatedProjectFile::open(project:&Project,relative_path:&str)->Result<Self,FileReadError>`、`metadata(&self)->PublicFileMetadata`、`read_prefix(&mut self,max_bytes:usize)->Result<BoundedFileRead,FileReadError>`、`read_complete(&mut self,max_bytes:usize)->Result<BoundedFileRead,FileReadError>`。`PublicFileMetadata {size_bytes:u64,modified_at:Option<String>}`；`BoundedFileRead {bytes:Vec<u8>,size_bytes:u64,read_bytes:u64,eof:bool,sha256:String,hash_scope:FileHashScope}`；`FileHashScope::{Full,Prefix}`；`FileReadError::{InvalidPath,UnsafeFile,NotRegular,Unavailable,TooLarge,SourceChanged}`。这些仅为宿主内部类型，不是 DTO。

- [x] **RED:** 同 handle prefix/full、probe exclusion、超限 complete、所有非法路径、目录/内部文件、读取变化、final path containment 和 Windows reparse 判定测试。把 Win32 属性判定提为小纯函数用于注入 `FILE_ATTRIBUTE_REPARSE_POINT`；真实 symlink/junction 不成为测试运行前提。

```rust
#[test]
fn project_file_read_prefix_excludes_probe_byte() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("sample.csv"), b"abcd").unwrap();
    let project = Project::new(Uuid::new_v4(), "preview", root.path().to_str().unwrap(),
        ProjectTemplate::Blank, chrono::Utc::now());
    let mut source = ValidatedProjectFile::open(&project, "sample.csv").unwrap();
    let read = source.read_prefix(3).unwrap();
    assert_eq!(read.bytes, b"abc");
    assert_eq!(read.read_bytes, 3);
    assert!(!read.eof);
    assert_eq!(read.hash_scope, FileHashScope::Prefix);
    assert_eq!(read.sha256, hex::encode(sha2::Sha256::digest(b"abc")));
}
```

- [x] **Run RED:** `cargo test -p omicsops-desktop --lib project_file_read`；新模块注册且有非零测试，当前缺类型/方法应 FAIL。
- [x] **Implement:** 实现上述签名。复用 path normalization；逐 ancestor 检查 lstat/reparse，canonical root containment；Windows 以 `GetFinalPathNameByHandleW` 与 `GetFileInformationByHandle` 核对打开对象，统一拒绝非普通对象。metadata 为打开时安全观察值；read 前后同 handle 核对身份/长度/mtime。prefix 恰好预算且无 probe 是 EOF；complete 超预算固定 TooLarge。不要返回 raw OS error。
- [x] **Run GREEN:** 同命令 PASS；`cargo test -p omicsops-desktop --lib composer_files::tests` 保持 quote 原行为。确认 Windows cfg 编译及纯注入 reparse 分支有断言。
- [x] **Commit:** `feat: add bounded validated project file reads`；只暂存该任务文件。

### Task 2: DTO 与逻辑记录解析器

**Files:** Create `crates/omicsops-dto/src/project_table.rs`、`src-tauri/src/project_table.rs`（纯 parser/tests）；Modify DTO `src/lib.rs`、desktop `src/lib.rs`、`src-tauri/src/dto_contract_tests.rs`。

**Interfaces:** Request `ProjectTablePreviewRequest {project_id:Uuid,backend_id:String,relative_path:String}`；response `ProjectTablePreview {project_id:Uuid,backend_id:String,relative_path:String,size_bytes:Option<u64>,read_bytes:u64,parsed_bytes:u64,sha256:String,hash_scope:TablePreviewHashScope,headers:Vec<String>,rows:Vec<Vec<String>>,displayed_rows:u32,truncation_reasons:Vec<TablePreviewTruncation>,source_stability:TablePreviewStability}`。Enums `TablePreviewHashScope::{Full,Prefix}`、`TablePreviewStability::{Checked,Unknown}`、`TablePreviewTruncation::{ByteLimit,RowLimit,CellLimit,IncompleteRecord,IncompleteUtf8}`；`TablePreviewErrorCode::{InvalidPath,UnsupportedFormat,InvalidUtf8,MalformedTable,ColumnLimit,SourceChanged,ReadFailed,Timeout,UnavailableBackend}`。serde enums snake_case、requests deny_unknown_fields。

Host-only `TableDelimiter::{Csv,Tsv}`、`ParsedProjectTable {headers:Vec<String>,rows:Vec<Vec<String>>,parsed_bytes:u64,truncation_reasons:Vec<TablePreviewTruncation>}`；`parse_project_table(bytes:&[u8],delimiter:TableDelimiter,eof:bool)->Result<ParsedProjectTable,TablePreviewErrorCode>`。Raw parsed strings remain bounded by input；公开脱敏/4KiB limit 在 Task 3 projection。

- [x] **RED:** BOM/中文/CRLF、字段内换行/分隔符、escaped quote、empty/header-only、末行无换行、EOF未闭引号/宽度不符、prefix部分记录/半字符、中间非法UTF8、64/65列、200/201行、超过4KiB的原始字段保留测试（公开展示预算由Task3 projection验证）。提供 parser 已停止的 RowLimit 标记，不验证未观察尾部。

```rust
#[test]
fn table_preview_record_boundaries() {
    let parsed = parse_project_table(b"id,note\r\n1,\"a\nb\"\r\n2,unfinished", TableDelimiter::Csv, false).unwrap();
    assert_eq!(parsed.headers, vec!["id", "note"]);
    assert_eq!(parsed.rows, vec![vec!["1", "a\nb"]]);
    assert!(parsed.truncation_reasons.contains(&TablePreviewTruncation::IncompleteRecord));
    assert_eq!(parsed.parsed_bytes, 18);
    assert_eq!(parse_project_table(b"id\n\"unfinished", TableDelimiter::Csv, true).unwrap_err(), TablePreviewErrorCode::MalformedTable);
}
```

- [x] **Run RED:** `cargo test -p omicsops-desktop --lib project_table::tests`、`cargo test -p omicsops-desktop --lib dto_contract_tests::project_table`；缺 parser/DTO 应 FAIL。先核对 fixture byte count，再记录实际失败。
- [x] **Implement:** 实现逻辑字段状态机及固定预算，UTF8仅允许 prefix末尾缺失；不按 lines() 分割。返回 complete record 的 raw byte offset，EOF最后无换行仍完整，非EOF末尾记录丢弃。添加稳定字段名/nullable/unknown-field 合同，hash 不在 parser 重算或按显示文本计算。
- [x] **Run GREEN:** 同两个命令 PASS；`cargo test -p omicsops-dto` PASS。
- [x] **Commit:** `feat: parse bounded CSV and TSV preview records`。

### Task 3: 本地/SSH source 与统一宿主命令

**Files:** Create `src-tauri/src/project_table_commands.rs`（command/seam/tests）；Modify `crates/omicsops-adapters/src/ssh.rs`、`src-tauri/src/lib.rs`；按必要复用 `src-tauri/src/composer_files.rs` 和 `sync_commands.rs` 的现有 binding/canonical helpers，不更改其完整读取语义。

**Interfaces:** Consumes Tasks 1–2及现有 `FileBackend`。Adapter produces `RemoteFilePrefix {bytes:Vec<u8>,size_bytes:Option<u64>,eof:bool,metadata_stable:bool}` 和 `SshSession::read_file_prefix(&self,remote_path:&str,max_bytes:u64)->AdapterResult<RemoteFilePrefix>`，不修改 `read_file_limited`。Host-only `TableSourceRead {bytes:Vec<u8>,size_bytes:Option<u64>,eof:bool,stable:bool}`；async injectable `ProjectTableSource::read_prefix(&self,project:&Project,backend:&FileBackend,relative_path:&str,max_bytes:usize)->Result<TableSourceRead,TablePreviewErrorCode>`；`preview_project_table_with_source(store:&Store,source:&dyn ProjectTableSource,request:ProjectTablePreviewRequest)->Result<ProjectTablePreview,String>`；Tauri `preview_project_table(state:State<'_,AppState>,request:ProjectTablePreviewRequest)->Result<ProjectTablePreview,String>`。String errors 固定 enum code，不带 OS/SSH 输出。

- [ ] **RED:** fake source 捕获预算与绑定；foreign backend/path在调用前拒绝，257KiB伪结果拒绝；probe/full/prefix/unknown stability；大小变化；public_text后4KiB UTF-8展示预算与CellLimit；慢 connect/root 整条20秒 timeout；失败时 disconnect；固定 safe error不泄漏sentinel。adapter 注入 reader 测试 max+1 与前后metadata，不连接SSH。

```rust
assert_eq!(fake_source.last_budget(), Some(256 * 1024));
assert_eq!(preview.read_bytes, preview_raw_bytes.len() as u64);
assert_eq!(preview.sha256, hex::encode(sha2::Sha256::digest(&preview_raw_bytes)));
assert_eq!(unstable_preview.hash_scope, TablePreviewHashScope::Prefix);
assert_eq!(unstable_preview.source_stability, TablePreviewStability::Unknown);
assert_eq!(rejected_source.call_count(), 0);
```

- [ ] **Run RED:** `cargo test -p omicsops-desktop --lib project_table_commands`、`cargo test -p omicsops-adapters ssh::tests::file_prefix`；先创建测试与 fixture，当前缺方法/command 应 FAIL。
- [ ] **Implement:** Store加载project且验证backend/current profile；扩展名/path校验先于读。Local消费已验证handle；SSH source 内包含connect/trust/canonical root/path/lstat/read，用现有session cleanup规则并包整source async路径20秒。SFTP读取预算+1；同handle前后metadata不支持时unknown；不把source提供的摘要当可信。Host固定预算再验一次原bytes、原hash和parse结果，public_text之后utf8截断、标 CellLimit；注册命令。
- [ ] **Run GREEN:** 同两命令 PASS；`cargo test -p omicsops-desktop --lib composer_files::tests`、`cargo test -p omicsops-desktop --lib sync_commands::tests` 确认既有路径/quote语义。无真实远端验收。
- [ ] **Commit:** `feat: preview project tables from local and SSH sources`。

### Task 4: 只读文件树预览与有界来源展示

**Files:** Modify `src/types.ts`、`src/composer-file-api.ts`、`src/composer-file-api.test.ts`、`src/features/workspace/RemoteFileTree.tsx`、`RemoteFileTree.test.tsx`、`WorkspaceShell.tsx`、`ComposerIntegration.test.tsx`；Create 同workspace目录 `TablePreviewDialog.tsx`、`table-preview.css`、`TablePreviewDialog.test.tsx`。

**Interfaces:** Consumes DTO和现有 `WorkspaceFileReference`；`previewProjectTable(reference:WorkspaceFileReference):Promise<ProjectTablePreview>` invokes `{request:{project_id,backend_id,relative_path}}`；`TablePreviewDialog(props:{reference:WorkspaceFileReference;locale:Locale;onClose:()=>void}):ReactNode`。CSV/TSV routing只更改preview action；drag/reference/其他类型textQuote独立保持。

- [ ] **RED:** mock invoke核对payload；loading/error/retry、empty/header-only、来源size/read/parsed/hash scope、truncation中文/英文、XSS/formula作为文本、没有quote或模型调用；切文件/backend晚回包丢弃；打开立即Escape+父dialog只留父。

```tsx
fireEvent.keyDown(window, { key: "Escape" });
expect(onClose).toHaveBeenCalledTimes(1);
expect(closeParent).not.toHaveBeenCalled();
expect(createComposerQuote).not.toHaveBeenCalled();
expect(document.querySelector("script[data-cell]")).toBeNull();
```

- [ ] **Run RED:** `npx vitest run src/composer-file-api.test.ts src/features/workspace/TablePreviewDialog.test.tsx src/features/workspace/RemoteFileTree.test.tsx src/features/workspace/ComposerIntegration.test.tsx`；缺API/dialog/routing应 FAIL。
- [ ] **Implement:** 增 typed boundary 与只读React文本表格；按 tuple+generation guard旧响应，window escape stack及原focus trap；不缓存原始科学bytes。显示首行作为header、读取仅前缀、unknown stability和每个固定截断原因；安全错误可重试。
- [ ] **Run GREEN:** 同命令 PASS；`cargo fmt --all -- --check`、`cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`、`git diff --check`。锁变更先 npm ci；纯fmt另commit。手工 Windows小CSV/BOM/长字段和SSH fake展示，真实SSH未执行单独说明。
- [ ] **Commit:** `feat: show bounded table previews in workspace files`；根据实际验证更新总清单。

## 当前实施证据

2026-10-07：Task1提交7258149/7439276，Task2提交69886e4，均经独立审阅通过，无阻断发现。Task1最终reader16/composer10；Task2 parser12/DTO合同3/DTO crate31。Task2最终workspace1564通过、17 ignored；前端1138+22、Web/桌面构建和格式检查通过。Task2初始RED为71个总编译错误（包含缺接口与受限环境OUT_DIR错误），不是已执行断言失败；授权GREEN运行实际断言。真实模型/SSH及GUI手工验收未执行，安装包只构建未运行。Task3/4未实现，尚不代表表格功能交付。

## Plan Self-Review

Task 1供下游唯一handle接口；Task 2覆盖logical-record/byte预算与serde；Task 3覆盖binding/trust/whole-timeout/原始hash；Task 4覆盖实际入口/readonly/source/Escape。五项Review Focus各有具体断言；新模块仅围绕本次责任，未拆无关文件，未新增持久化表。以上命令待实施阶段实际执行，不能把计划里的 Expected 当运行结果。
