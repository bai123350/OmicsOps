# Skills Settings Management Implementation Plan

## 2026-09-21 implementation record

S1 ownership receipts, S2 detail/preview and S3 guarded removal are implemented and connected. Final UI is in `9fc6824`, native lifecycle in `343e06a`, and format-only changes in `0a0f48f`. Library-only removal preserves files; owned-file removal records a durable journal, quarantines the installation, commits catalog removal transactionally, and cleans only files matching recorded proofs. Retry restores verified precommit quarantine or finishes postcommit cleanup, including partially deleted inventories. Unknown/changed files remain protected.

Active or resumable runs and effective transitive dependencies block removal. Bundled and plugin-owned records cannot be deleted as ordinary imports. Plugin ownership resolves through its parent receipt. Pending removal cannot be reset by enable/import, and startup reconciliation precedes bundled installation. Native Skills tests 24/24, Store removal tests 2/2 and independent lifecycle review passed. Combined checks and native/manual limitations are in [settings verification](../specs/2026-09-21-settings-verification.md). Detailed tasks below retain the original plan.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐Skills详情、受限文件读取、安全移除和启停失败反馈，保留来源所有权与运行证据。

**Architecture:** 宿主按skill UUID解析真实安装记录；独立来源receipt证明所有权，不能从名称/分类猜测。目录文件为不可变安装材料；已冻结文本保留，删除受live使用与恢复保护。

**Tech Stack:** Rust/Tauri 2、现有Store JSON、React/TypeScript、临时目录/SQLite测试。

**Spec:** `docs/superpowers/specs/2026-09-15-settings-baseline-review.md`；与 `2026-09-15-plugins-general-settings.md` 的包所有权边界对齐，仅实现技能管理本切片。

## Global Constraints

- 用户已授权此基础缺口；不新增Skill编辑器、商店、远程安装器或整个Plugins系统。
- Windows-first；所有路径由宿主记录解析，拒绝任意绝对读取、父路径、设备路径、ADS、Windows保留名和链接/reparse逃逸。
- source_path不是“原始导入目录”。不删除外部来源，不因停用技能改冻结运行、撤销审批或停止远端作业。
- 新DTO在omicsops-dto，TS/native注册与当前owner串行接线；所有行为变化有自动化测试。默认全检查及desktop build遵守AGENTS。
- 返回UI的内容经过现有公开文本脱敏；不调用vault枚举或get以“补充详情”。不要把预览文字发送模型、存Store或写日志。

## 已核对的安装与冻结语义

`crates/omicsops-core/src/workspace.rs::SkillPackage`含id/name/version/source_path/sha256/enabled/capabilities/category，没有origin字段。

`omicsops-adapters/src/skills.rs::install_skill_directory`把导入内容复制到 `skills_root/{name}/{sha256}`；sha256是排序后的相对路径长度、路径、文件大小和文件字节组成的包摘要，**不是SKILL.md单文件hash**。包最多4096文件/100MiB；既有文件内符号链接可能被解析并复制为普通文件。新的读取/删除不应因import曾允许包内文件链接就跟随安装目录后来出现的链接。

`skill_commands.rs::install_bundled_skills`与手动import使用同一安装器、同一skills_root；`Store::upsert_skill_package_by_sha`还会对相同摘要返回已有record，并可能补category。因此category、目录位置、名称或“用户再次导入”都不能证明文件属于独立手动导入，bundled和import可能指向同一个ID/内容。

`freeze_skill_package`校验全包摘要后返回 `FrozenSkillUseV4 { skill_id, name, version, package_sha256, sections, frozen_sha256 }`，sections保存文本和摘要，无source_path。`agent_v4.rs::use_skill`把冻结对象作为工具结果保存；composer引用也冻结有界文本。历史结果可独立阅读。**但** `agent_skill_packages`、`skill_documents`、`use_skill`和composer新引用仍从live Store+目录读取；依赖解析还会把显式disabled但被启用工作流依赖的技能加入有效目录。不能宣称启停只影响未来运行，或冻结后即可删安装目录。

## S1：明确来源receipt，不迁移或猜测旧所有权

**Files:** 新增 `src-tauri/src/skill_settings.rs`；窄修改 `skill_commands.rs::persist_installed/import_skill_directory/install_bundled_skills` 与适配安装器返回的创建事实；Store复用JSON，必要查询/事务写在Store；共享DTO和contract tests。

```rust
enum SkillOrigin { Bundled, ManagedImport, PluginOwned, LegacyUnknown, External }
struct SkillInstallationReceipt {
    skill_id: Uuid, package_sha256: String, installed_root: String,
    origin: SkillOrigin, owns_files: bool,
    plugin_installation_id: Option<Uuid>, phase: String,
}
// Store kind: skill_installation_v1; id=actual returned SkillPackage.id
```

安装器新增内部 `created_new_directory`事实（或新内部receipt返回结构），由实际成功rename产生；不能由调用前exists推断。manual import只有当实际新建受管目录、Store返回的新记录对应同路径/摘要、未与bundled/plugin共享时才能写ManagedImport+owns_files=true。命令返回前receipt失败必须返回明确错误，文件和目录保留为unknown，禁止后来把这次失败当拥有证明。

bundled安装无论首次/去重都为实际返回ID登记Bundled保护；任何已有ManagedImport同摘要后来被bundle引用，来源升级为Bundled且owns_files=false，绝不反向降级。当前bundle启动登记可识别当次实际内置包；旧版本未再出现的条目没有证明时仍LegacyUnknown，不猜是普通导入。已有记录再次手工导入且目录早已存在只能保留原owner/unknown，不能凭这次操作取得删除权。

Plugins安装未来在创建独立SkillPackage时登记PluginOwned+plugin_installation_id；技能命令立即拒绝此类物理删除/库移除，返回Plugins入口。即使Plugins尚未实现，若包安装记录已声明该skill ID也以PluginOwned保护，不能回退unknown。插件安装不要用全局SHA upsert夺取独立导入record；该约束沿用插件计划。

receipt missing：source_path在skills_root内归LegacyUnknown，仅允许库移除；在外部归External，同样只允许库移除、不通过设置读取任意外部路径。bundled保护、plugin保护优先于receipt较弱标记；receipt路径/摘要与record不一致立即冲突，不能执行文件删除。

- [ ] 临时Store/目录测试：bundled与manual相同SHA只一个ID且永远Bundled保护；category为None不代表import；新manual目录产生own receipt；同目录再次import不提升unknown所有权；receipt写失败不返回成功且不能删除目录；plugin ID无法经manual覆盖。
- [ ] 运行 `cargo test -p omicsops-desktop skill_settings`及adapter安装器测试确认新增断言失败；实现receipt并保持旧SkillPackage JSON向后兼容，重复运行通过后独立提交。

## S2：详情与有界文本文件预览

**Files:** `skill_settings.rs`，共享DTO/native注册；新增 `src/skill-settings-api.ts`、`src/features/settings/SkillDetails.tsx`及测试；SettingsPanel传skill ID打开详情。

```rust
struct SkillSettingsDetail {
    skill: SkillPackage, origin: SkillOrigin, integrity: String,
    files: Vec<SkillSettingsFile>, inventory_complete: bool,
    dependent_skills: Vec<Uuid>, can_remove_from_library: bool,
    can_delete_files: bool, blocking_reasons: Vec<String>,
}
struct SkillSettingsFile { relative_path: String, size_bytes: u64, previewable: bool }
struct SkillFilePreview { relative_path: String, content: String, redacted: bool, package_sha256: String }
// settings_skill_detail(skill_id: Uuid) -> SkillSettingsDetail
// settings_read_skill_file(skill_id: Uuid, relative_path: String,
//                          expected_package_sha256: String) -> SkillFilePreview
```

详情先查UUID，再从宿主skills_root和record解析根；只允许受管skills_root内目录或确有Plugin安装receipt证明的受管包子目录。外部/unknown外部只返回脱敏元数据及“请从原来源检查”，不能拿合法UUID绕开任意本地读取限制。返回SkillPackage元数据前对label/source_path/capabilities做公开文本脱敏；内部比较仍使用原record值。

列文件只允许普通文件，递归深度<=16、文件<=4096、包总<=100MiB；超过上限返回inventory_complete=false，详情仍可查看但禁物理删除。检查root本身和每个component的symlink_metadata及WindowsFILE_ATTRIBUTE_REPARSE_POINT，不能先canonicalize再丢失链接证据。每项相对路径normalize为`/`；禁止`.`/`..`、`:`、前导separator、UNC/device、尾点/尾空格和保留名。二进制仅列安全名称/大小，不读正文。

预览仅md/py/r/sh/ps1/yml/yaml/toml/json/txt扩展，单文件<=256KiB，UTF-8、拒绝NUL/不允许控制字符；read采用take(limit+1)防metadata检查后增长。只读已列出的精确相对路径；每次重读record/expected包摘要及路径约束，完整包摘要变化返回“安装内容已变化，请重新导入”，不显示来自旧摘要的内容。文件句柄打开后再验证实际类型/位置/长度，Windows使用不跟随reparse的打开策略；不得自动执行preview脚本或启动shell。

复用/窄提取 `composer_quotes.rs::is_credential_filename` 防止 `.env*`、pem/key、password/secret/credentials等文件进入preview（目录component同样检查）。采用 `composer_references.rs::public_text` 的API key/password/token/authorization/PEM脱敏，对文件内容、名称、错误统一处理；不要只用core当前两个regex就声称覆盖密码/私钥。被阻止项返回通用原因，不回显可包含secret的原路径；脱敏preview不返回原始单文件摘要、base64原文或原内容长度差分。package_sha256是原安装身份而不是可执行信任证明。

- [ ] native测试UUID不存在、record跨root、绝对/父路径/ADS/Windows保留名、root和中间reparse、二进制/过大/读取时增长拒绝；普通SKILL.md和script可显示但零执行；文件变化expected摘要拒绝；未知外部记录不能读外部测试secret。
- [ ] secret-sentinel测试覆盖路径含token、JSON password/API key、authorization和PEM，DTO/错误不含原secret，Store中不新增preview正文。数据被脱敏时redacted=true，UI明确展示脱敏预览。
- [ ] UI测试打开详情调用真实API、选择文件只读一项、错误保留详情、重复点击受busy保护、立即Escape先关文件预览/详情顶层，父设置保持打开；运行定向native/前端测试后提交S2。

## S3：运行保护与安全库移除/物理删除

**Files:** `skill_settings.rs`；Store新增受限阻塞检查和receipt事务操作；native AppState新增共享短期skills读写锁（也可tauri managed state），窄接现有读/import/enable/use_skill/composer边界。不实现运行全局冻结技能目录的新系统。

```rust
enum SkillRemovalMode { LibraryOnly, OwnedFiles }
struct RemoveSkillRequest { skill_id: Uuid, expected_package_sha256: String, mode: SkillRemovalMode }
struct SkillRemovalResult {
    removed_from_library: bool, files_removed: bool,
    preserved_files: bool, status: String, message: String,
}
// settings_remove_skill(request) -> SkillRemovalResult
// settings_list_skill_removals() -> Vec<SkillRemovalOperation>
// settings_retry_skill_removal(operation_id: Uuid) -> SkillRemovalResult
// SkillRemovalOperation contains operation_id, skill_id, name (sanitized),
// package_sha256, phase, and preserved_files; never file contents.
```

**库移除**：删当前catalog record并保留receipt tombstone，原安装目录完全保留，UI文案固定“从库移除（保留安装文件）”，不能声称释放空间。LegacyUnknown/External允许此动作；Bundled拒绝并提供停用；PluginOwned转Plugins。恢复通过显式重新导入，缺所有权证明仍unknown。

**物理删除**：仅ManagedImport+owns_files、receipt/record/expected摘要一致、root精确为宿主skills_root/{name}/{sha256}且无重叠/共享引用时允许。遍历所有SkillPackage和plugin绑定验证无相同/嵌套root持有者；只看sha不同不够。全包重新inspect必须与原包SHA一致，文件清单、大小、位置仍满足上限；有新文件、改内容、链接或未知条目即拒绝删除且保留库记录。Bundled即使disabled也不能删。

**运行与依赖阻塞**：当前能力从全局live技能集合查询，不能可靠只按历史use_skill事件判断未来是否还会用。基础实现保守阻止库移除/物理删除，只要任何项目存在active_runs条目，或持久化planning/awaiting_approval/running/waiting_for_input/waiting_for_approval/needs_attention等可继续状态；未知状态也阻止。Store查询不得只看当前项目；明确终态Completed/Failed/Cancelled按现有恢复守卫判定，若有独立待恢复runtime/approval工作仍视为阻塞，不能仅用终态字符串忽略恢复记录。UI显示阻塞运行数量与回运行入口，不自动停止运行。另阻止仍被有效启用技能传递依赖的条目，复用现有depends_on解析和suffix匹配语义，不做另一套精确名称假设；用户先停用依赖方。

已历史冻结sections和composer存储正文不删除、不重写其hash、不把历史引用清成空。删除后尚未发送的草稿skill UUID引用会在既有发送时校验为不存在，UI刷新目录并显示引用失效，不偷偷删用户草稿或自动转用同名新版本。设置启停同样不修改过去的sections，未来live use_skill可能受可用性变化影响；依赖仍有效时显示“作为X依赖仍可用”，不能仅把enabled=false渲染成绝对不可用。

**并发边界**：详情/预览、agent_skill_packages+渲染/freeze、composer选择到freeze这一完整短读过程获取shared read guard；import/enable/remove获取write guard。不能仅锁HTTP命令，agent_v4直接读取也必须接入。删除在write guard内重新做run/dependency/ownership检查；Store `BEGIN IMMEDIATE`内认领removing阶段并撤销catalog可发现性，新run注册/恢复读技能必须在同一宿主边界后重读有效record，不能拿锁外旧SkillPackage继续读路径。锁不跨模型、SSH、网络、审批等待或整个运行；不存在每个run永久持锁。

**可恢复文件操作**：将已验证的包根原子rename到skills_root/.removing/{operationUuid}（同volume），receipt持久化original_root/quarantine_root/阶段和清单摘要。rename前后都校验范围、reparse和清单；rename失败保持catalog与文件并报告失败。隔离完成且校验通过后删除catalog并记removed_from_library；逐个删除清单内已验证且摘要未变的普通文件，再删空目录，绝不对用户未知树直接remove_dir_all。Windows验证/删除使用不跟随reparse的句柄及阻止并发写/重命名的共享模式；路径在检查后被替换或无法保持证明则保留文件，不赌TOCTOU。

失败若发生在catalog移除前，尝试原子恢复原根并保留原记录；原根已被占据则保留隔离目录和needs_attention状态，不覆盖新目录。catalog移除后的部分清理失败返回removed_from_library=true/files_removed=false/preserved_files=true；不伪称全失败诱导重新删除其他路径。启动恢复只检查已记录隔离目录，文件修改或新文件始终保留；通过settings_list_skill_removals列出未完operation（最多100，超过则拒绝新物理删除直到处理已有项），页面“重试已验证清理”按operation UUID解析已有journal，不能接受新路径，不自动扫全skills根。已经移除的skill再次调用remove通过匹配的receipt返回幂等结果而非定位其他同名skill。未知receipt或超限只能LibraryOnly。物理删除逻辑不能被Plugins直接复用来删共享包记录，未来插件owner自行证明拥有关系。

- [ ] 测试LibraryOnly保留全部文件；bundled/plugin拒绝；legacy不能强制OwnedFiles；新import能完成真实删除且不碰外部源；共享/嵌套根拒绝；依赖方有效时拒绝；expected摘要冲突零更改。
- [ ] 测试其他项目active/waiting/needs_attention和待恢复记录阻塞，完成且无恢复工作后允许；删除不变更历史FrozenSkillUseV4/事件hash；未发送草稿引用删除后明确失效。
- [ ] 受控并发测试读freeze与delete互斥，reader取得的是完整文本或明确不存在，不能半读；检查后新增run/依赖/包绑定不能绕过guard；用户替换链接/修改文件时清理保留而非越界删除。注入rename/Store/删除阶段失败验证返回状态、恢复与幂等，不执行真实Agent/SSH。
- [ ] 定向 `cargo test -p omicsops-desktop skill_settings`及Store测试通过后提交S3；不能只用metadata断言替代真实临时文件删除结果。

## S4：启停反馈、页面整合与交付

**Files:** `SettingsPanel.tsx`中的SkillCard/SkillsAndMcpSettings、`SkillDetails.tsx`和对应tests；根API接线。启停沿用现有set_skill_enabled及Store同名版本原子切换，不复制持久化逻辑。

- [ ] 所有启停通过捕获错误的handler并设置item/global busy，失败保持原卡片与错误，成功重新读取完整目录（同名其他版本可能被停用）。不能直接`void onSetSkillEnabled`丢异常。新增测试失败/重试/重复点击/同名替换后刷新一致。
- [ ] 卡片“详情”打开实际详情；显示source/origin/全包SHA/完整性/有效依赖状态。危险操作只根据host can_*提供，命令仍重验；库移除和物理删除使用不同按钮、不同确认文字和返回状态，partial清理明确保留文件。
- [ ] 确认层立即Escape只关顶层，父详情/设置保持；失败不乐观移除，成功刷新Skills及composer目录，保留现有用户草稿；PluginOwned按钮导航Plugins，未提供该页时显示由包管理且禁操作，不能降级通用删除。
- [ ] 跑Skills组件、SettingsPanel、composer reference定向测试，再 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`；Windows一次性本地skill smoke详情/文本/启停/库移除/owned删除及保留修改文件。真实模型/SSH不执行须记录。
- [ ] 独立提交并更新master Skills缺口完成情况；只有native来源记录、真实预览/删除和根导航均接通才标该基础页完成。
