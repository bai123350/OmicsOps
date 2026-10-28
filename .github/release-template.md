<!-- release-title: {{TAG}}: {{TITLE}} -->
# OmicsOps {{TAG}}

## 更新 / Changes

填写本版本的用户可见变化、修复和已知限制，并提供简短英文说明。
Describe the user-visible changes, fixes, and known limitations for this version.

## 下载 / Downloads

{{DOWNLOADS}}

Windows 日常安装选择 EXE，集中部署可选 MSI；Linux 根据环境选择 DEB 或 AppImage；
macOS 按 Apple Silicon 或 Intel 选择 DMG。Source code 压缩包不是桌面安装包。
Choose EXE/MSI for Windows, DEB/AppImage for Linux, and the DMG matching your Mac's CPU.
The automatically listed source archives are not desktop installers.

## 安装与隐私 / Setup and privacy

安装包不预置维护者的 API key、账户或项目数据。安装后自行配置模型；运行本地分析需自行准备
Python/R 及科研依赖。当前没有应用内自动更新，升级时下载新版本安装包。
Configure your own model credentials after installation. Local analysis requires Python/R
and scientific packages. Updates are manual; no in-app updater is configured.

## 签名与限制 / Signing and limitations

- Windows 未做发行者签名；macOS 为 ad-hoc 签名，未做 Developer ID 签名或公证。
- Windows is unsigned. macOS uses ad-hoc signing without Developer ID or notarization.
- Linux/macOS 是实验平台；Linux 凭据需要已解锁的桌面 Secret Service。
- Linux/macOS are experimental; Linux credentials require an unlocked desktop Secret Service.

## 验证 / Verification

发布前填写实际完成的构建、干净账户安装/升级、凭据持久化、资源加载检查，以及未执行项目。
Record actual native-build, fresh-user installation/upgrade, credential and resource checks,
and explicitly list unexecuted acceptance checks before publishing.

如存在上一版本，补充从上一标签到本标签的 GitHub compare 链接。
If a previous version exists, add its GitHub compare link.
