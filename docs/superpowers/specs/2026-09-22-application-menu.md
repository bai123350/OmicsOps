# 应用顶栏菜单

用户要求参考 wisp-science 截图，为 OmicsOps 添加 File、Edit、View、Help 四个菜单。布局参考其公开 main 分支的 [window_titlebar.rs](https://github.com/xuzhougeng/wisp-science/blob/main/ui/src/window_titlebar.rs)，菜单内容按 OmicsOps 已有能力实现。

## 行为

- 启动页、项目首页、工作区共用 38px 顶栏；品牌和版本左侧显示，四个一级菜单使用截图中的英文名称，下拉内容跟随当前语言。
- File 提供新建项目、新会话、项目列表与设置；新建项目复用现有创建对话框，新会话遵守当前运行、计划锁定和会话恢复状态。
- Edit 提供工作区搜索、模型、Skills 和 MCP 设置；View 提供项目文件、外观设置与持久化主题选择；Help 提供本地快速入门和关于信息。
- 没有项目时禁用项目专属操作。菜单点击可展开、切换和关闭，支持方向键、Home/End、Tab 退出和窗口级 Escape；一次 Escape 只关闭视觉最顶层。
- 菜单操作复用现有回调与错误处理，不改变 Agent、SSH 作业、审批、凭据、数据传输或持久化模型。返回项目列表不等同于停止计算。

## 窗口与布局

Windows 主窗口关闭系统装饰，由顶栏品牌/空白区域提供拖动，右侧提供最小化、最大化/还原和关闭。窗口动作使用 Tauri 当前窗口 API，仅增加对应窗口操作权限；最大化状态监听在卸载时注销。加载期间也保留窗口控制。

macOS/Linux 保留系统装饰，页面仍显示四个菜单；浏览器预览不显示原生窗口按钮。它们的原生交互与安装行为未经本次 Windows 验证保证。

内容区及对话框背景从可用高度中扣除顶栏高度，跟随现有外观缩放。对话框限制在扣除顶栏和内边距后的高度中滚动；窄窗口的浮动侧栏同步向下偏移。顶栏和帮助层高于现有工作区弹层，全局 Ctrl+K 会清理菜单/帮助状态以维持视觉层与 Escape 栈顺序。

## 验证

自动化覆盖菜单导航、功能入口、禁用状态、主题切换、叠加设置页时即时 Escape、新建项目请求消费及窗口动作/监听清理。交付前执行 `cargo test --workspace`、`npm test`、`npm run build`、`npm run build:desktop`。

手工 smoke：Windows 1100×680 与 900×560 下检查菜单、短窗口滚动、设置、新项目、拖动、双击顶栏、最小化、最大化/还原和关闭；检查系统 125%/150% DPI 与外观缩放。浏览器检查不能替代原生窗口和 DPI 验收。实际执行结果在本次任务交付中记录。

本次确定性检查：`cargo test --workspace` 1255 项通过、12 项 ignored；`npm test` 832 项前端测试和 22 项浏览器扩展测试通过；`npm run build` 与最终 `npm run build:desktop` 均通过；`cargo fmt --all -- --check` 和 `git diff --check` 通过。构建保留已有的大 chunk 提示与 MSVC 链接器信息警告。本地构建产生 Windows EXE 和 NSIS 产物，没有安装、发布或分发。

浏览器检查已执行：默认 1280×720 与 900×560 的菜单/新建项目布局、浅色和系统深色主题、帮助到 Ctrl+K 搜索的切换，以及即时 Escape。真实 Windows 原生窗口操作、安装、125%/150% DPI、macOS/Linux、真实模型与 SSH 验收未执行。
