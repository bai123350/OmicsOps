# Plugins and General Remaining Slices Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 交付声明式本地插件包生命周期，以及General尚缺的真实目录偏好、选择动作、通知和原生状态入口。

**Architecture:** 插件是受管文件+清单+所有权，不产生新授权主体；General偏好逐个连接真实消费者，不增加无执行端开关。

**Tech Stack:** Tauri 2、Store JSON、Rust filesystem、现有Skills/MCP、React/TypeScript。

**Spec:** `docs/superpowers/specs/2026-09-15-wisp-settings-source-audit.md`；延续master中Plugins与General已有mini-slices。

## Global Constraints

- 用户已授权全部19页按页实现；插件本地声明式包是本产品基础适配，ZIP/HTTPS市场、任意代码执行不是本页前提。
- Windows-first；所有路径拒绝目录穿越、设备路径、ADS、链接/reparse以及用户目录外删除。数据库与已有项目根不迁移。
- 不伪造通知权限、更新源或网络代理生效范围；设置不取消远端作业，不改变冻结审批。
- 默认完整检查 `cargo test --workspace`、`npm test`、`npm run build`；native组合/资源/配置变化另跑 `npm run build:desktop`。锁变化跑 `npm ci`。
- 修改共享DTO/lib/types/SettingsPanel/DesktopApp前和owner协调。此文档不表示已实现或通过。

## P1：本地声明包检查、安装及所有权

**Files:** 新增 `src-tauri/src/integration_packages.rs`、`src/integration-packages-api.ts`、`src/features/settings/PluginsSettings.tsx`及测试；共享DTO/native注册；skills owner协作少量宿主级启停边界。

```json
{
  "schema_version": 1,
  "id": "lab.qc", "version": "1.0.0", "name": "QC helpers",
  "skills": [{"path": "skills/qc/SKILL.md", "sha256": "<64 lowercase hex>"}],
  "mcp_presets": ["<exact compiled preset id>"]
}
```

文件名固定 `omicsops-plugin.json`，serde deny_unknown_fields；id正则 `[a-z0-9][a-z0-9.-]{0,63}`且禁止`.`/`..`结尾、Windows保留名；version为受限semver，不能成为路径拼接组件（安装路径用宿主生成UUID）。只接受相对SKILL.md；每个skill拥有自己的根且不能嵌套/重叠，只允许明确列出的markdown文件，未列附件不能悄悄复制。manifest<=64KiB，最多20skills、20presets、单SKILL<=256KiB、总文件<=5MiB。包内路径逐component验证并检查reparse；SHA256从实际字节计算，源目录不执行任何脚本。缺少附件的技能说明不能伪装完整安装：检查skill链接，包内相对引用未在格式支持范围内则拒绝并说明本格式仅支持独立SKILL.md。manifest/skill验证继续经过既有技能安全解析，不声称内容可信。

```rust
struct PluginInspection {
    manifest_digest: String, source_path: String, id: String, version: String,
    name: String, files: Vec<PluginFilePreview>, bindings: Vec<PluginBindingPreview>,
    existing_installation_id: Option<Uuid>, changes: Vec<String>,
}
struct PluginInstallRequest { source_path: String, expected_digest: String }
struct InstalledPlugin {
    installation_id: Uuid, package_id: String, version: String,
    digest: String, enabled: bool, phase: String,
    skills: Vec<PluginOwnedSkill>, mcp_bindings: Vec<PluginPresetBinding>,
}
// settings_inspect_plugin(source_path) -> PluginInspection
// settings_install_plugin(request) -> InstalledPlugin
// settings_list_plugins() -> Vec<InstalledPlugin>
// settings_set_plugin_enabled(installation_id, enabled) -> InstalledPlugin
// settings_remove_plugin(installation_id, expected_digest) -> PluginRemovalResult
```

`PluginOwnedSkill`含skill_id、受管相对路径及安装摘要；`PluginPresetBinding`含preset id、实际server_id、ownership=`shared_reference`，不能把早已安装的内置MCP记录认成本包所有。返回UI的source_path是用户选择的本地来源信息，不上传模型。包的信任状态始终`local_unverified`；hash只证明检查与安装内容一致，不代表作者受信。

安装分阶段：重新检查全部文件/hash与expected_digest→独立app_data/plugins/staging UUID目录复制→在同盘原子rename为installs/UUID→Store写 `integration_package_v1` staging记录→建立包独有SkillPackage记录（source_path位于包安装根、默认disabled）→记录共享MCP引用→将phase置installed并enabled=false。只有最后成功后UI宣称已安装。source变更或复制摘要不一致则停在失败阶段并拒绝发布可用skill。写失败只清理由此安装创建且摘要相同的staging文件，不清其他目录。

直接复用 `import_skill_directory` 会去重或移动到全局skills根，不能用它丢失所有权；可复用其inspect/安全校验与SkillPackage构造，包skill_id必须独立生成并记录来源。内置MCP安装已由启动注册，插件绑定只是package→existing preset引用；安装、启用都不调用inspect/start/approve，不自动启用全局连接。UI显示共享连接的真实全局状态并链接Connections，说明禁用包不关闭该共享连接。

同package_id+digest安装幂等返回existing；不同digest/version先检查差异并由明确“更新”动作传expected_old_digest。更新先完整stage新版本，所有新技能disabled；成功后只切本包记录引用，新版需用户显式启用；旧技能停止新选择，但已冻结snapshot不变。失败旧版保持可用。可先将“更新”作为同页第二提交，不能静默覆盖或永久只报不支持更新。

- [ ] 写pure临时目录测试：manifest未知字段/脚本/URL/绝对路径/`..`/ADS/Windows保留名/相对链接遗漏/hash错拒绝；reparse/symlink不跟随；源在inspect后变化拒绝；正常独立SKILL复制字节一致。
- [ ] 写临时Store+模拟MCP注册器测试：重复安装幂等，失败阶段可重试且不会重复创建skill；preset已存在只创建shared_reference、零进程启动零审批写；新旧包更新失败旧版可用。
- [ ] 运行 `cargo test -p omicsops-desktop integration_packages` 确认失败，再实施并通过；补DTO serialized keys测试。独立提交native安装边界。

## P2：启停、移除、恢复与设置页

启用包仅让包拥有且用户允许的skills进入未来Agent技能目录；所有实际入口（agent_skill_packages、composer目录、直接引用解析）必须在宿主检查installed+enabled父包，不能只在UI隐藏。Skills页单独启用某包技能不得绕过父包disabled；显示来源并引导Plugins。包停用不改其他导入技能，不撤销冻结运行内容，不停止全局共享MCP。优先将父包启用检查集中在skill owner现有可用性查询，避免按多个字符串路径到处猜包归属。

移除先phase=removing并停止新发现；对本包skill逐一验证owner ID/源路径/摘要，用户改过的文件保留并返回preserved_files。包的shared MCP链接只删本包绑定，不改server profile、credentials、approval。删除范围严格为已验证的installs/UUID，不递归跟随reparse；目录若有新未知文件则保留并报告。更新/移除互斥（per installation mutex），禁止并发把新版本当旧版删掉。保留小型tombstone phase=removed与未删除文件提示，不让崩溃导致“已移除”假状态。

启动reconcile只处理本app_data内已记录staging/removing记录；不自动执行安装命令或联网，识别已完成rename和登记；无法证明摘要/所有权时标needs_attention，由UI重试。完整生命周期状态是`staging|installed|updating|removing|needs_attention|removed`，不能只有installed布尔掩盖半成功。

- [ ] 写测试：disable阻止实际技能发现/新引用，原有独立技能仍可用；移除共享MCP仍存在且approval未变；改动文件保留；未知文件/链接目录拒绝删除；重试remove幂等；模拟每个阶段崩溃reconcile不重跑脚本且真实状态可见。
- [ ] UI提供选择本地包→只读预览文件/绑定/版本差异→安装、启停、更新、详情与移除；所有动作调用命令，busy禁重复，失败保留可重试阶段；没有来源时提供可复制上述manifest模板，不填已安装样例。
- [ ] `PluginsSettings.test.tsx`覆盖真实API参数、检查后安装expected_digest、操作失败/迟到响应、立即Escape只关预览/删除确认且父设置仍开；集成验证包技能可在实际composer目录选择但仍走既有审批。
- [ ] 默认全检查与桌面构建；Windows临时本地包smoke安装/启停/改文件/更新/移除；真实MCP服务及模型未执行须记录。完成后逐页提交并更新master。

## G1：真实默认项目目录与状态入口

**Files:** 新增 `src-tauri/src/general_settings.rs`、`src/general-settings-api.ts`、`src/features/settings/GeneralAdvancedSettings.tsx`及测试；修改 `ProjectLibrary.tsx`、`src/tauri-api.ts`目录选择和父级偏好传递；共享DTO/handler。

当前ProjectLibrary创建时必需显式localRoot，`create_project`据此创建项目；因此General字段名称固定“新项目目录选择的起始位置”，不是数据库目录、自动项目根或现有项目搬家。Store JSON `general_native_preferences_v1` 存 `project_directory_start: Option<String>`；setter校验绝对存在目录，拒绝设备路径/ADS/reparse，保存不创建或迁移任何目录。选择项目目录时给现有dialog.open的defaultPath传此值，用户仍明确选择最终项目目录；失效路径回退系统选择器并显示提示，不阻止手动选择。清除偏好真实恢复默认起始位置。

```rust
struct GeneralNativePreferences { project_directory_start: Option<String> }
struct GeneralSystemStatus {
    app_version: String, app_data_directory: String,
    update_status: String, update_source_configured: bool,
}
// settings_general_preferences() -> GeneralNativePreferences
// settings_save_general_preferences(preferences) -> GeneralNativePreferences
// settings_general_system_status() -> GeneralSystemStatus
// settings_probe_system_interpreters() -> SystemInterpreterDiagnostics
```

版本来自 `app.package_info().version`，路径来自app.path().app_data_dir。仓库无正式发布源，真实update_status固定由配置检测结果产生`unconfigured`，页面显示“未配置更新源”与当前版本，并提供“重新读取更新配置”按钮；不能放可点击的虚假“检查更新”或auto-update开关。此适配不要求创建release/tag或假远程版本。已有可信源将来落库才新增HTTP检查协议。

General环境卡提供system探测动作，但当前没有可直接调用的settings system diagnostics命令。`agent_v4_compute_backends`及`configured_process_backends`只检查配置，python_status/r_status固定unverified，不能当解释器探测。新增只读命令 `settings_probe_system_interpreters() -> SystemInterpreterDiagnostics`，其中结果包含python/Rscript各自的found/missing/error状态和checked_at；复用 `agent_v4.rs::LocalEnvironmentPortV4::check_interpreter` → `program_available` 的实际检查，可提取窄的共享probe helper并注入测试runner。固定仅检查system PATH的python/Rscript，不接受用户输入任意program或自定义路径，不隐式准备/安装环境；超时及执行失败必须与未找到区分。General和Environments共用此诊断命令，显示“找到可执行文件”且依赖未验证；SSH管理跳Environments。网络卡列明模型HTTP与MCP stdio真实配置入口：Models可实际编辑base URL并probe，Connections可实际编辑stdio命令/env并inspect；点击导航保留原设置工作区。此批没有统一代理设置，不显示system/direct/custom假选项、不读取可能带密码的环境代理值。不把SSH称HTTP代理受控；HTTP传输和子进程继承环境的范围明确。

- [ ] 临时Store测试偏好保存/清除重载，路径无效拒绝、没有mkdir/移动DB；选择器mock断言defaultPath传递正确、取消保留创建表单；偏好变更不改已选localRoot。
- [ ] 状态DTO测试真实version、unconfigured不是latest；UI测试显示版本与缺源原因且无假开关；环境/网络按钮指向对应真实功能。新增system probe用模拟runner验证只查python/Rscript、无安装/SSH调用，found/missing/error及超时分别返回；UI点击真实命令后渲染结果，不能复用backend目录的unverified当成功。
- [ ] 实现后运行general/ProjectLibrary/SettingsPanel定向测试，再完整检查/desktop构建，提交G1。目录选择Windows人工验证，macOS未验证明确记录。

## G2：消息正文选择浮窗与共享开关

**Files:** 新增 `src/features/workspace/MessageSelectionActions.tsx`及测试，`src/use-general-preferences.ts`扩 `selectionActionsEnabled`持久化，WorkspaceShell消息正文接入与General开关。

仅对同一消息正文容器内非空、<=16KiB的DOM文本选区提供复制/“引用到草稿”。选择范围不得覆盖输入框、password、tool隐藏详情、两个消息或两个项目；失焦/scroll/切会话/清选区立即关闭。选择只触发浮窗不自动复制或发请求；复制只写用户选择文本到clipboard；引用将可见文本（含消息来源标签）追加用户草稿，保留原草稿、不发送、不冒充文件quote snapshot。现有 `composer_quotes.rs`专用于经后端验证的文件引用，不用其ID包装任意消息DOM文本。

开关和浮窗共用根偏好状态，默认开启；关后立即不再显示，重载保持。浮窗进入窗口Escape作用域，打开后无需焦点先移入；selectionchange监听卸载清理；点击动作时保存选区快照，避免按钮mousedown清选区后失效。引用内容仍走现有发送/敏感内容验证，不能成为系统提示或授权。

- [ ] 测试正文选择→复制准确文本；引用保留草稿只追加一次且未调用send；跨消息/输入框/超限选区不出现；关闭偏好立即消失并重载保持；剪贴板失败显示可重试错误。
- [ ] 测试立即Escape关闭顶部选择浮窗而其他覆盖层未关闭；再次Escape按既有堆栈；切项目迟到selectionchange不能带旧正文到新草稿。
- [ ] 实现组件/接线，运行composer、general、overlay定向测试及全检查；Windows鼠标与键盘选择smoke，独立提交G2。

## G3：真实系统通知桥接与事件去重

**Files:** 新增 `src-tauri/src/notification_settings.rs`、`src/notification-settings-api.ts`、`src/use-run-notifications.ts`与测试；Tauri通知插件依赖/初始化/capabilities根据实际Rust API最小授权；General UI与DesktopApp新到达事件接线。

新增 `NotificationPreferences { enabled: bool }` Store JSON `notification_preferences_v1`，默认false避免安装即打扰；UI明确默认关闭可启用，与上游默认不同有记录。`NotificationStatus { preference_enabled, permission: granted|denied|prompt|unsupported, platform }`来自实际原生插件；状态读取不得请求权限，用户启用/测试动作才请求。系统拒绝时保留准确拒绝状态，不能把偏好true当通知可送达。

命令 `settings_notification_status`、`settings_set_notifications_enabled`、`settings_send_test_notification` 只接受布尔或无参数；生产 `notify_run_event(run_id,event_hash)`重新读取持久化事件并验证只允许运行完成/失败/需要审批的真实新事件，客户端不能给任意标题正文。宿主还检查偏好和当前主窗口focus（前台不通知）。标题固定“OmicsOps”，正文仅“任务已完成/任务失败/任务需要处理”，不带模型输出、项目路径或样本名；测试通知注明测试。

前端仅从live event subscription送候选，不从历史水合/恢复数组重放；后端用Store `notification_receipt_v1` 的event_hash幂等认领（必须事务插入唯一键），重复订阅/StrictMode只派一次。认领与OS发送不能原子，采用at-most-once：认领后OS错误记failed并显示最近失败、不自动重发相同事件；测试按钮可验证当前系统状态。主窗口前台跳过不创建将来补发队列。停止本地等待/远端作业仍运行不当成完成通知。点击导航只有插件实际提供并经平台验证时才接入，基础页不承诺未支持的激活参数。

- [ ] 用NotificationSink trait fake和临时Store测试关偏好/前台/权限拒绝均零发送；真实允许类型与event_hash校验；同事件并发两次仅一次sink；发送失败状态可见且不重复；通知文本不包含secret-sentinel或事件正文。
- [ ] hook测试历史水合零请求、新终态事件一次、StrictMode/重复event不重复、切项目后旧订阅清理；General测试状态/启用/权限拒绝/测试成功失败，关闭偏好立即生效。
- [ ] 实现后运行native/前端定向测试、默认完整检查、desktop build，依赖锁变化npm ci。Windows真实测试系统通知开启/关闭/前台抑制；开发可执行程序与安装版权限行为分开记录，macOS未测不作保证。G3单独提交，G1–G3都接入后才将General登记为适配范围完成。
