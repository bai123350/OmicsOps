# Skills 单层目录与设置列表

用户在 Windows 安装后发现 Skills 分为两个目录，要求所有技能直接放到
`skills` 文件夹下，设置中也不再出现分级目录。

## 行为

- 仓库和安装资源统一使用 `skills/<skill-name>/SKILL.md`，取消
  `single-cell`、`wisp-science` 两个来源父目录。每个技能的 scripts、references
  等相对文件树原样保留，避免破坏脚本和文档引用。
- 只用一个 `skills/BUNDLE.json` 和一次启动安装调用；合并默认启用、替换规则。
- 设置页平铺所有技能，继续提供导入、启停和详情；移除领域、分类折叠层。
- 来源与许可证放在 `docs/third-party/skills/<source>/`，安装包在 licenses 中
  保留这些归属文件。内部 category 保留，以继续应用既有上游兼容与审批提示，
  不再作为设置页分层依据。

## 兼容边界

此前运行时已把两个来源导入同一个用户数据 Skills 存储。其内容寻址路径
`skills/<name>/<sha256>`、记录 UUID、哈希、启用选择及安装收据继续使用，
无需迁移真实用户文件或修改数据库模式；历史冻结证据仍指向原内容。
本次消除的是安装资源的来源目录和设置分类层，而非技能包内部必要的文件树。

不修改对话、模型、API key、SSH 或运行授权。不覆盖公开 v0.1.0 的既有安装包，
不创建新版本或 Release。

## 验证

检查 35 个技能在统一根目录可安装，默认启用集合保持不变；重复安装保留 UUID、
哈希、路径和用户选择。继续验证 Wisp 源文件校验和及宿主兼容提示。前端测试直接
找到各来源技能，不需展开分类，验证启停、详情和导入仍可用。运行 Python
离线资源/隐私检查、Web 构建、Rust workspace 测试、前端测试和桌面打包。

Windows 手工检查：安装新构建后查看安装目录 skills 直属技能目录，在设置中
确认全部技能平铺；旧安装升级后确认已有技能启停选择保留。自动化构建不替代
真实安装、模型或 SSH 验收。

## 本次验证记录（2026-10-29，Windows）

- `npm run build`：通过；保留现有大 chunk 提示。
- `cargo test --workspace`：2057 通过、21 ignored。真实环境 ignored 测试未执行。
  首轮把 TEMP/TMP 设在仓库 target 下，导致既有剪贴板路径保护测试拒绝绝对路径；
  恢复系统临时目录后完整重跑通过，未改相关产品代码。
- `npm test`：Vitest 130 文件、1557 测试通过，浏览器扩展 22、发布脚本 37 通过。
  完整复测时将 VITEST_MAX_FORKS/VITEST_MAX_THREADS 设为 2，MIN 设为 1。
  此前两次默认并发各失败一项：插件预览即时 Escape、项目迁移预览 5 秒超时；
  插件测试单独 6 轮均通过。未修改这些功能或降低其断言要求。
- bundled Python `-B -m unittest discover -s scripts -p 'test_*.py'`：
  108 测试通过（其中 2 skipped）；Python 测试临时目录位于工作区内。
- `cargo fmt --all -- --check`、Git diff 空白检查：通过。
- 发布资源、Web 和原生二进制隐私检查：通过；206 个技能文件 Git blob
  与移动前完全一致。
- `npm run build:desktop`：通过，生成 Windows x64 EXE/MSI。检查实际生成的
  WiX/NSIS 清单：skills 下直接包含 35 个技能包，没有两个来源父目录。

仅完成本机自动化验证与打包。新安装、旧版升级、Linux/macOS 打包和真实模型/SSH
验收未执行；没有更新 GitHub Release。
