# 桌面构建与 GitHub Releases

本流程生成可以安装的 OmicsOps 桌面程序。用户安装应用无需 Node.js、Rust 或源码；
执行本地科研计算仍需自行准备 `python` / `Rscript` 和所需科研包。SSH 计算使用远端环境。
应用内自动更新尚未实现。

发行包必须保持初始状态：不携带维护者的 API key、账户登录态、模型配置、SSH 连接、
聊天记录、项目数据或本机用户目录。应用的公开模型目录和随包 Skills 属于程序资源，
不包含可供他人使用的私人账户凭据。新用户应在自己的设备上配置模型与 API key。

## 纯净构建与隐私检查

正式分发使用 GitHub 托管 runner 上从指定提交干净检出的构建。不要上传日常开发目录中的
旧安装包、数据库或调试符号。`npm run build:desktop` 现在执行以下检查后才生成安装器：

- 只接受明确批准且受 Git 跟踪的随包资源；禁止额外文件、链接、环境文件、数据库、日志、
  私钥和证书。新增合法资源需要同步审查资源白名单。
- 前端不加载 `.env`，不向客户端暴露 `VITE_*` / `TAURI_*` 环境变量。构建子进程移除常见
  密钥、token 和密码环境变量；模型凭据只能在运行时由用户配置。
- Rust 发布构建重映射本机源码、Cargo 和用户目录，并关闭调试信息与开发资源目录回退。
- 扫描待打包资源、前端产物和未压缩主程序，发现已知密钥形状或当前构建机私人路径即停止。
  报告只显示文件位置、规则及有限的字节结构统计，不显示匹配内容或凭据哈希；额外结构
  诊断最多 20 条，不读取系统 keyring 或现有用户数据库。

自动扫描覆盖明确规则，并不是对所有未知格式私人信息的证明。发布前仍须审查新增资源，
在干净用户账户或虚拟机上检查首次启动。已有用户重装后可能继续看到该设备保存的配置；
这不表示安装包携带了维护者的数据，也不要为检查初始状态删除已有用户数据。

Windows 是主要支持平台。Linux/macOS 为实验平台；配置了构建工作流不代表已经完成
这些平台的安装、凭据持久化或真实科研流程验收。

## 构建矩阵与文件

| 目标 | GitHub runner | Rust target | 安装包 |
| --- | --- | --- | --- |
| windows-x64 | windows-latest | x86_64-pc-windows-msvc | `.exe` (NSIS)、`.msi` |
| linux-x64 | ubuntu-22.04 | x86_64-unknown-linux-gnu | `.deb`、`.AppImage` |
| macos-arm64 | macos-15 | aarch64-apple-darwin | `.dmg` |
| macos-x64 | macos-15-intel | x86_64-apple-darwin | `.dmg` |

四个任务均在对应操作系统和架构上构建、测试。Linux 构建基线是 Ubuntu 22.04，
实际支持的发行版和最低 macOS 版本仍需实机验证；不能根据构建机版本推断兼容性。
首版不包含 Windows/Linux ARM64、iOS、商店上架或自动更新。

安装包参考 wisp-science 的 Tauri 原生命名。以 `0.1.0` 为例：

| 用户平台 | 发布附件 | 选择方式 |
| --- | --- | --- |
| Windows x64 | `OmicsOps_0.1.0_x64-setup.exe` | 日常安装优先选择 |
| Windows x64 | `OmicsOps_0.1.0_x64_en-US.msi` | 使用 MSI 部署时选择 |
| Linux x64 | `OmicsOps_0.1.0_amd64.deb` | Debian/Ubuntu 包管理方式 |
| Linux x64 | `OmicsOps_0.1.0_amd64.AppImage` | 使用 AppImage 时选择 |
| macOS Apple Silicon | `OmicsOps_0.1.0_aarch64.dmg` | Apple 芯片 |
| macOS Intel | `OmicsOps_0.1.0_x64.dmg` | Intel 芯片 |

`scripts/release-assets.json` 是打包收集、上传校验和下载表共用的命名来源。内部构建目标仍为
`windows-x64` 等四个 ID；文件名中的 `amd64`、`aarch64`、`x64` 按平台惯例使用。
每次完整构建产生六个安装包，加上 `release-manifest.json` 和 `SHA256SUMS.txt`。
GitHub 自动提供的 Source code 压缩包是源码，不是可直接安装的桌面程序。
清单记录源码提交、版本、目标、文件
大小与 SHA-256；校验文件覆盖六个安装包和 JSON 清单。它们证明文件与构建清单一致，
不等同于发行者数字签名。

## 与 wisp-science 对齐的发布规则

参考提交为 `d783ce154e70905a783f7ed461f4869744c5836a`，并核对了其 v1.22.0 实际下载附件。

- 每个版本在 `.github/release-notes/vX.Y.Z.md` 中维护中英说明，记录用户可见变化、
  下载选择、签名状态、已知限制和实际验证情况；存在上一版本时补充 compare 链接。
- 可用首行 `<!-- release-title: vX.Y.Z: 本版主题 -->` 指定标题；无标记时标题为标签。
  文件缺失、内容为空、标题版本不符或模板占位符未处理时，带标签构建会提前失败。
- 安装包上传不覆盖人工编辑过的草稿标题和正文。说明文件跟随同一个固定源码提交。
- OmicsOps 保留四平台完成后创建草稿、人工公开的顺序。wisp-science 先创建公开 Release、
  再陆续补附件的顺序不用于本项目。
- 当前仅发布上述六种桌面安装包及两个校验文件。`latest.json`、`.app.tar.gz`、`.sig`
  属于上游更新机制；SwiftUI、WinUI、移动端和镜像也各有独立能力要求，本项目未实现这些产物。
  上游的证书、签名账号、更新地址和私有配置不复制到本项目。

## 首次设置

1. 将本流程、脚本、平台配置、锁文件和发布文档合并到仓库默认分支。`workflow_dispatch`
   的入口需要工作流先存在于默认分支。
2. 在仓库 **Settings → Actions → General** 允许 GitHub Actions 和工作流使用的 Actions。
   构建任务只有 `contents: read`；最后的草稿任务申请 `contents: write`。若组织策略禁止
   该权限，应由仓库管理员调整策略。无需为同仓库发布另存个人访问令牌。
3. 使用 GitHub 托管的 Windows、Linux、macOS runners。私有仓库应检查 Actions 配额。
4. 首先手动运行一次不带标签的构建，确认各个平台的检查与打包结果。

普通 CI 在 Windows、Linux x64、macOS ARM64 上运行确定性检查。Desktop release
工作流另外覆盖 macOS Intel，并在每个目标上运行 Python 回归、V4 场景、Web 构建、
Rust workspace 测试、前端/浏览器扩展/发布脚本测试和桌面打包。Web 构建先于桌面 Rust
编译，确保干净检出时存在 `dist`。Node 固定为 22，Python 固定为 3.12，npm 使用
`npm ci`，Cargo 使用 `--locked`。

## 先构建，不创建 Release

在 GitHub **Actions → Desktop release → Run workflow** 选择待验证分支，保持 `tag`
输入为空。工作流会将所选 ref 解析为固定提交，四个平台均检出这个提交。

构建成功后从运行详情的 **Artifacts** 下载 `desktop-complete`。其中包含六个安装包和
两个校验文件。单平台中间产物名为 `desktop-target-<目标>`；`v4-scenarios-<目标>` 仅为测试报告，
不会进入发布附件。Artifacts 默认保留 14 天；这一步不创建标签或 Release。

## 准备版本并生成草稿

当前脚本支持 `X.Y.Z` 数字版本（无前导零、预发布后缀或构建元数据），并检查 MSI 的
上限：X/Y 不超过 255，Z 不超过 65535。预览状态通过 GitHub 的 prerelease 标记表达。

1. 更新 `package.json`、`package-lock.json` 的根版本及 `packages[""].version`、根目录
   `Cargo.toml` 的 `workspace.package.version`、`src-tauri/tauri.conf.json` 的版本。
   更新 `Cargo.lock` 中继承 workspace 版本的包；桌面 crate 继续继承 workspace 版本。
   `npm version <版本> --no-git-tag-version` 可更新 npm 的两个文件，再更新 Rust/Tauri。
   运行 Cargo 检查刷新锁文件后审阅差异，不要批量升级不相关依赖。
   从 `.github/release-template.md` 复制为 `.github/release-notes/vX.Y.Z.md`，替换 `{{TAG}}`
   和 `{{TITLE}}` 并填写实际说明。`{{DOWNLOADS}}` 保留给脚本自动生成下载表，
   `{{DOWNLOAD_BASE}}` 和 `{{REPOSITORY_URL}}` 可用于同仓库链接；其他占位符必须替换。
   首版说明已准备在 `.github/release-notes/v0.1.0.md`，发布前需更新其中尚未完成的验收记录。
2. 从仓库根目录执行（Python 需要 3.12+，CI 使用 3.12）：

   先确认 `python --version`。如果它指向旧版本，请用 Python 3.12 可执行文件运行下列
   Python 命令；这不要求更改科研项目使用的 Python 环境。

   ```powershell
   npm ci
   python -B scripts/release.py check-version
   python -B -m unittest discover -s scripts -p "test_*.py"
   npm run build
   cargo test --locked --workspace
   npm test
   npm run build:desktop -- -- --locked
   ```

   再校验对应版本说明（将标签和 `owner/repo` 换为本次发布的实际值）：

   ```powershell
   node scripts/release_notes.cjs --tag v0.1.0 --repository owner/repo
   python -B scripts/check_release_privacy.py --document .github/release-notes/v0.1.0.md
   ```

   如需查看渲染后的正文，可添加 `--output target/release-preview.md`；输出文件必须尚不存在。
   本地预览不会访问 GitHub，也不会创建 Release。

3. 提交、评审并合并发布准备改动，确保目标提交已经推送到 GitHub。在该确定提交上
   创建新标签。以下只是 `0.1.0` 的例子，必须先确认版本和标签不存在：

   ```powershell
   git tag -a v0.1.0 -m "OmicsOps v0.1.0"
   git push origin v0.1.0
   ```

4. 推送 `v*` 标签会触发 Desktop release。版本不一致、缺少平台、测试失败或安装包
   校验失败时不会进入草稿任务。标签不会由发布脚本自动创建。
5. 全部成功后，在仓库 **Releases** 中查看 `draft + prerelease`。工作流上传六个安装包、
   JSON 清单和校验文件。标题和正文来自对应版本说明，下载链接自动指向当前仓库和标签。
   签名状态、平台限制和待补充的验收记录也来自这份说明。

也可以在手动运行时填写一个**已经存在**的 `vX.Y.Z` 标签，构建该标签对应的版本并
创建/恢复它的草稿。手动选择的分支不会改变填写标签所对应的源码提交。

## 失败、重跑和已公开版本

- 同一 ref 的发布工作流串行执行；不会取消正在运行的构建。
- 完整产物通过校验之前不创建新草稿。网络错误导致上传中断时，已有草稿可能只有部分
  附件，应保持草稿状态并重跑；不要在上传过程中手动发布。
- 草稿重跑要求相同标签、相同源码提交，并保留人工修改过的标题和正文。它可以替换
  同名构建附件；无法识别的额外附件会使流程停止，不会删除用户文件。
- 标签不存在、标签移动到另一个提交、草稿属于另一个提交，或版本已经公开时，发布
  脚本拒绝写入。已公开版本有问题时使用新版本和新标签，不要移动旧标签覆盖安装包。
- Actions 下载地址会过期；公开发布后使用 Release 附件链接分发。

## 安装验证与公开发布

从实际下载的安装包安装，再从桌面入口启动。记录平台版本、CPU 架构、安装包校验和、
操作和结果，至少检查：

- 安装、启动、卸载/升级；现有项目和用户配置不会因升级丢失。
- 捆绑 Skills、浏览器扩展和许可证能从安装资源目录加载。
- 保存测试用凭据后完全退出、重新启动仍可读取，再删除测试凭据；记录结果时不复制密钥。
- 从 GUI 启动时对 `python` / `Rscript` 的发现和一次简单运行。只有 `python3` 的环境并不
  自动满足当前 `python` 启动约定；macOS/Linux GUI 的 PATH 可能不同于终端。
- 如本版声称支持浏览器、SSH 或模型操作，分别验证并记录；真实模型、SSH、R、
  Micromamba/PBMC 验收遵循 `acceptance/README.md`，不把 ignored 测试算成通过。

Windows 校验示例：

```powershell
Get-FileHash -Algorithm SHA256 .\OmicsOps_0.1.0_x64-setup.exe
```

将结果与 `SHA256SUMS.txt` 对应行比较。Linux 可执行 `sha256sum -c SHA256SUMS.txt`，
macOS 可执行 `shasum -a 256 -c SHA256SUMS.txt`（需下载所有待校验文件到同一目录）。
Linux 的 AppImage 下载后需要先赋予执行权限，例如
`chmod +x OmicsOps_0.1.0_amd64.AppImage`；DEB 可通过发行版的包管理器安装。

完成后在 Releases 编辑草稿，填写变更、验收结果和已知限制，确认附件完整，再点击
**Publish release**。首次发布建议保留 **Set as a pre-release**。这一步由维护者执行；
工作流始终创建草稿，不会自动公开。

## 签名与平台功能边界

- **Windows**：当前安装器未做发行者代码签名。SmartScreen 或组织策略可能警告/阻止。
  正式签名需配置代码签名证书/服务；本流程不会收集或保存签名凭据。
- **macOS**：`tauri.macos.conf.json` 使用 `signingIdentity: "-"`，仅做 ad-hoc 签名，
  **没有 Apple Developer ID 签名或公证**。下载后仍可能需要在系统隐私与安全设置允许。
  若后续接入正式签名，应同时配置公证、更新发布模板并验证实际下载包。
- **Linux**：API key/SSH 凭据通过桌面 D-Bus Secret Service 保存（如 GNOME Keyring），
  必须有可用且已解锁的服务。DEB 建议安装 gnome-keyring；AppImage 不包含用户的
  keyring 服务。服务不可用时报错，不回退到 SQLite 或明文文件。
- **当前功能差异**：Claude Code 订阅子进程及离线合成示例创建仍限 Windows；
  非 Windows 订阅资源网页需要手动打开；Linux 浏览器需要设置可执行文件路径。
  本次未放宽进程隔离、文件安全或审批规则来移植这些功能。
- 本次不改变项目数据库、同步策略、SSH 后台计算的取消/恢复语义。模型/MCP/文献服务
  仍可能按授权接收对话、结果片段或元数据，不能声称所有数据永不离开本机。

## 本机打包

在目标操作系统安装 Tauri 的系统前置依赖、Rust 和 Node 后：

```text
npm ci
npm run build:desktop -- -- --locked
```

Tauri 自动合并 `tauri.windows.conf.json`、`tauri.linux.conf.json` 或
`tauri.macos.conf.json`。默认本机产物位于 workspace 的 `target/release/bundle/`；
显式传入 `--target <Rust target>` 时位于 `target/<Rust target>/release/bundle/`。
macOS 安装包在 macOS 上构建，Linux 安装包在 Linux 上构建。不要分发 debug 可执行文件。

隐私检查需要 Python 3.12+。若 `python` 指向旧版本，可将 `OMICSOPS_BUILD_PYTHON` 设置为
Python 3.12 可执行文件路径；无需改变科研项目的解释器。发布包装器只支持可选
`--target <本机 Rust target>` 和 `-- --locked`，不允许用调试模式或额外配置覆盖检查。
它会替换个人 Rust 编译 flags，因此自定义构建参数应先经过发布配置审查。

仅打包工具下载失败时，恢复网络后重新运行 `npm run build:desktop -- -- --locked`。
Cargo 会复用未改变的构建缓存，同时重新运行隐私检查。不要绕过包装器直接调用
`tauri bundle` 分发旧二进制；之前本机生成、尚未经过本轮隐私检查的安装包不可发布。

## 参考

- [wisp-science 的标签发布工作流](https://github.com/xuzhougeng/wisp-science/blob/d783ce154e70905a783f7ed461f4869744c5836a/.github/workflows/release-create.yml)
- [wisp-science 的版本说明与标题解析](https://github.com/xuzhougeng/wisp-science/blob/d783ce154e70905a783f7ed461f4869744c5836a/scripts/github_release_notes.sh)
- [wisp-science v1.22.0 的实际发布文件与下载选择](https://github.com/xuzhougeng/wisp-science/releases/tag/v1.22.0)
- [Tauri GitHub Actions](https://v2.tauri.app/distribute/pipelines/github/)
- [Tauri macOS 签名](https://v2.tauri.app/distribute/sign/macos/)
- [Tauri Windows 签名](https://v2.tauri.app/distribute/sign/windows/)
- [Tauri Linux/AppImage 限制](https://v2.tauri.app/distribute/appimage/)
