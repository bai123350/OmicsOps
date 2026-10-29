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

## 找到 Rscript 路径但检测失败：版本参数回归

后续截图已经显示项目 `.envs/r443/bin/Rscript`，但状态为“检测失败”。代码使用
`Rscript --vanilla --version`；Rscript 的 `--version` 分支只在其为唯一参数时
执行。附带 `--vanilla` 后进入脚本参数解析，因未提供脚本而输出
`file name is missing`，退出码为 1。因此解释器存在时也会被记录为 Error，
可靠项目推荐随之保持为空。

证据来自 [R 4.4 分支源码](https://github.com/wch/r-source/blob/R-4-4-branch/src/unix/Rscript.c#L266-L319)
及本机现成 Rscript 4.3.3 的只读实测：旧命令退出 1，单独 `--version` 退出 0
并返回 `Rscript (R) version 4.3.3 (2024-02-29)`。这是本机命令参数复现，
没有访问用户 SSH 主机，也不代表远端 R 科研依赖验收。

修复将 Local 与 SSH 共用的 R 版本参数统一为单独 `--version`。该分支直接
打印编译版本并退出，不启动 R 会话、不读取用户启动文件，也无需 `--vanilla`。
正常执行 R 科研代码时的隔离参数不变。保持输出、路径、超时和退出码校验，
不把缺失、错误、超时伪装为可用。

此前模拟解释器忽略 argv，任意参数都返回版本，漏检了真实命令契约。回归将
要求 Rscript 只接受唯一 `--version`，覆盖本地参数、SSH system/managed/项目
单目录及按语言绑定；真实 shell fixture 也检查参数后才返回模拟版本。

参数回归先失败、修复后通过；`cargo test -p omicsops-desktop --lib
environment_probe_v4` 为 12 通过、1 默认 ignored。独立验证提取实际 Rust
生成器和原 parser，在本机 POSIX shell 执行严格 argv fake：旧帧为 `EXIT 1`
并解析为 Error，新帧为 `EXIT 0` 并解析为 Available 4.4.3。该版本为模拟输出。
复制 Windows 原生 Rscript 到临时项目的完整帧测试受安装布局影响未完成，
不算真实 R 完整管道验收；真实 R 证据限于原安装位置的参数对照。

`npm run build`、TypeScript、格式检查通过；`cargo test --workspace` 为
2085 通过、22 ignored、零失败。前端首轮全套有一项既有测试失败：切会话后
仅等待标题出现就检查输入框，尚未等 host snapshot 恢复结束。保持该 snapshot
pending 可确定性复现中间禁用态；测试补上 `conversationHydrating === false`
的等待，保留原断言和超时设置，不修改产品行为，定向用例及项目计算 16 项通过。
修正测试同步后重新执行 `npm test`：132 文件、1586 项通过，浏览器扩展 22 项、
发布脚本 37 项通过，零失败。Vitest 最多两个 worker，未增加等待超时。
`npm run build:desktop` 通过，Windows x64 EXE/MSI 已重新生成，配置资源、Web
及原生二进制隐私检查通过。修复、测试同步和文档分别提交。没有安装本轮包或
访问实际 SSH/R 远端，也未更新 GitHub Release；原生安装/升级验收仍未执行。
