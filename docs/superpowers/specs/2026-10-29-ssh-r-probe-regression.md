# 项目 R 环境未识别：真实 shell 语法与候选入口回归

## 复现与原因

用户在新版运行时窗口中仍看到 `SSH · system`，Python 3.12.10 可用而 R 未找到，
尽管此前已在项目目录安装 R。独立提取当前 Rust 命令生成函数，再交本机已有
POSIX shell 执行 `sh -n`，发现 system 语法正常，项目和受管环境均以状态 2
报 unexpected token `;`。

原因是 prefix lookup 自带尾部 `; `，外层模板又追加分号，形成
`export PATH=...; ; case` 或 `fi; ; case`。解释器根本没有被执行。项目候选因此
被记录为探测错误，自动建议保持为空，UI 继续显示当前 system 中的解释器状态。
此前 Mock 测试直接返回正确帧，未在 Windows 上验证完整命令，漏掉该缺陷。

第二个问题是草稿 Runtime 未给环境观察组件传入选择回调：建议为空、曾手选
system 或列表截断时，其他已发现环境既不可见也不可选，刷新仍停留 system。

## 修复范围

- 统一命令语句分隔符；system、managed、单项目前缀与分语言绑定都校验完整
  shell 语法，而非只检查命令包含某段字符串。
- 增加离线真实 shell fixture，在临时目录内放置模拟 Python/Rscript，执行
  完整生成命令并校验版本帧，不要求真实 R、SSH、网络或 WSL。
- 草稿 Runtime 接入环境选择；冻结历史保持只读，不触发探测或修改当前配置。
  列表截断时的提示应指向实际可用的手动配置入口。
- 保留错误/超时与缺失的区别，不将其他候选的 R 可用冒充为 system 的 R 可用。

## 验证边界

本轮不读取真实会话、模型、API key、SSH 密码/私钥或科研文件，不安装或变更
远端环境。真实 shell 测试只验证受控命令构造和版本解析，不替代真实 SSH、
科研依赖或安装升级验收。修复后运行定向回归与默认完整检查及桌面打包，并按
后端、界面和验证文档分组提交。

## 本轮实际验证

- 修复前：真实 `sh -n` 校验 system 通过、managed/项目环境失败；新增语句
  分隔符测试也先失败。修复后两者均通过。
- 后端定向：11 通过、1 个 Windows 默认 ignored；显式指定本机已有 POSIX
  shell 运行该语法矩阵后，1 项手工入口通过，覆盖 12 个语言/环境组合。
- 独立真实命令复核：14 个语法组合通过；在带空格/单引号的临时项目中放置
  模拟解释器，12 个完整探测命令通过原 Rust parser。模拟 Python 3.11.18 和
  R 4.4.3 的 PATH、version、EXIT 0 均匹配各自目录；缺失语言不回退 system。
  这些版本是 fixture 输出，不是读取用户服务器取得的版本。
- 前端定向：RuntimeDialog/WorkspaceShell 共 188 项通过；TypeScript 通过。
- `npm run build` 通过；`npm test` 的 131 文件、1582 项通过，浏览器扩展 22、
  发布脚本 37 项通过。Vitest 使用最多 2 个 worker，未放宽超时或断言。
- `cargo test --workspace`：2078 通过，22 ignored。上面的本机 shell ignored
  入口另行显式验证；没有将真实 SSH 或模型 ignored 测试记成通过。
- `cargo fmt --all -- --check`、Git 差异空白检查通过。
- `npm run build:desktop` 通过，生成修复后的 Windows x64 EXE/MSI；资源、Web
  和原生二进制隐私检查通过。安装包仅在本机生成，未更新 GitHub Release。

Linux 原生 fake 解释器 fixture 本机 Windows 未运行；Windows 上已通过本机
POSIX shell 独立执行完整命令。未访问真实 SSH、R、数据库内容、模型或凭据；
真实服务器和原生安装/升级验收仍未执行。
