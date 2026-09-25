# Wisp Science Agent 与运行时源码对照

日期：2026-09-26。范围：通过 GitHub 插件只读核对实现及相关测试，补充已确认设计的源码依据。

固定提交：`b242fcbd1867551889643bfb7ee734dc48f7f5f1`。下文链接均固定到该提交和实际源码行；提案、README 不作为已实现能力证据。

对应 [运行边界设计](2026-09-26-wisp-agent-runtime-sandbox-design.md) 与 [三项实施计划](../plans/2026-09-26-wisp-agent-runtime-boundaries.md)。设计已确认，实施计划仍待审；本轮没有实施产品代码。

## 1. 对现方案的结论

源码支持“先让用户和模型获得一致运行边界，再独立演进服务端隔离”的方向。可借鉴的是明确身份、宿主授权、只读观察、执行与等待分离，而不是把上游所有管理器或状态机移植过来。

OmicsOps 已有冻结计算选择、按 run 隔离的 Python/R 内核、宿主审批、完整模型请求预算、Linux SSH 独立后台作业及重连查询。首期仍只补宿主统一投影、Agent 上下文和界面解释；三个实施任务无需扩张。

发现一个必须保留差异的行为：Wisp 第二次取消可强制收口本地状态；OmicsOps 不得据此把未确认停止的远端计算标为已取消。下面区分可借鉴的远端身份核验和不照搬的本地状态收口。

## 2. 已实现的 Agent 思路

### 工具预算与一次溢出恢复

**源码：** [agent.rs:367–435](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-core/src/agent.rs#L367-L435) 在模型调用边界计入工具 schema 估算，先归档再压缩；发生上下文溢出后最多强制压缩并重试一次。

**思路：** 长任务保留可追溯原始记录，压缩服务于下一次请求，不将失败无限重试。这里核对的是上游估算/压缩路径，不能由此推断它与 OmicsOps 的最终 provider JSON 预算完全等价。

**OmicsOps：已具备，首期复用。** core 的 candidate/compacted 两条路径已通过 `execution_request` 组装 system，再由 desktop provider 适配器校验完整请求。新增边界摘要沿用这条路径；计划任务 2 验证压缩前后均保留摘要且计入预算，不再新增压缩器或放宽额度。

### 工具结果推动循环与用户控制

**源码：** [agent.rs:535–676](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-core/src/agent.rs#L535-L676) 经工具注册表执行调用；遇到用户控制结果时跳过同批后续调用并记录对应结果，无进展观察包含实际工具结果。

**思路：** 用户暂停/控制是循环中的有效状态；没有执行的后续工具不能当作成功结果。Agent 循环负责调度与观察，具体执行工具和宿主承担外部副作用边界。

**OmicsOps：已具备，保持边界。** 继续使用现有工具结果、暂停/指导和证据契约。首期视图不产生科研执行结果，也不替代审批。此段 Agent 源码本身不能证明代码运行于安全沙盒。

### 宿主解析权限与派发前再次授权

**源码：** [execution.rs:143–187](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-core/src/execution.rs#L143-L187) 要求 capability registry 和 host policy，并校验计划；[execution.rs:378–405](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-core/src/execution.rs#L378-L405) 在派发前授权子请求。

**源码：** [delegation_policy.rs:334–383](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-core/src/delegation_policy.rs#L334-L383)、[517–544](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-core/src/delegation_policy.rs#L517-L544)、[1097–1135](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-core/src/delegation_policy.rs#L1097-L1135) 由宿主解析 capability IDs，受已启用能力、权限、上下文和预算上限约束；Manual/Automatic 的确认由宿主规则决定。

**思路：** 模型表达意图，宿主确定实际权限；计划校验不能替代派发前的校验。

**OmicsOps：已具备，首期解释但不改授权。** 继续沿用冻结能力、调用 hash/scope 和宿主风险审批。`RuntimeBoundaryViewV4` 只说明策略，既不是授权令牌，也不能让子 Agent、Skill 或 MCP 扩权。上游委派权限系统不整体移植。

## 3. 已实现的 Runtime 思路

### 身份隔离与陈旧状态检查

**源码：** [manager.rs:43–54](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runtime/src/manager.rs#L43-L54) 的 RuntimeKey 包含 project/scope/session/context/language；[424–497](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runtime/src/manager.rs#L424-L497) 对带 expected_generation/required_objects 的执行要求既有会话且 generation 匹配。

**思路：** 项目相同不意味着可共享解释器状态；runtime 重建后，旧观察不能继续当作当前事实。

**OmicsOps：身份隔离已具备，generation 后置。** 现有键为 project/run/backend/language/environment，保留 run 内复用语义。首期没有对象检查或 live runtime 状态，不需要引入 generation。草稿、冻结 run 和查询响应的身份匹配则直接采用这一原则。

### 观察不隐式创建执行环境

**源码：** [manager.rs:500–517](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runtime/src/manager.rs#L500-L517) 的 inspect 查找既有 runtime，不调用 prepare 启动一个新 runtime。

**思路：** 用户查看状态不应产生执行资源或安装环境；“尚无状态”是有效答案。

**OmicsOps：首期采用更窄的纯描述。** 新只读查询只读取配置/冻结记录，不调用 initialize、SSH、镜像 inspect 或解释器。缺 image ID 的草稿显示配置未完成，历史无选择显示未记录；不借机运行 Wisp 式对象 inspect。

### SSH 启动与写入范围不等于强隔离

**源码：** [runtime_launcher.rs:441–518](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/src-tauri/src/runtime_launcher.rs#L441-L518) 构造 attached 命令，其中 SSH 分支为远端 `cd` 后 `exec` 解释器和 worker；[293–315](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/src-tauri/src/runtime_launcher.rs#L293-L315) 的 kernel write scope 适用于 Python local/WSL，SSH 返回 None。

**源码：** [exploration_isolation.rs:61–77](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runs/src/exploration_isolation.rs#L61-L77) 的 local source 检查采用路径规范化与包含关系，host-local context 限于 local/WSL。

**思路：** 必须按具体执行路径描述保证。上述 SSH launch 路径没有建立容器或 OS 强隔离；路径范围规则也不足以证明恶意代码被 OS 阻断。不能据这些局部文件推断整个上游项目完全没有其他沙盒机制。

**OmicsOps：首期采用准确文案，服务器沙盒后置。** Local/SSH 是对应账号权限的进程，Micromamba 管理依赖而非安全隔离。本机 Docker/Podman 已有受限启动参数，但项目目录整体可写且只有 PID 上限。首期如实展示，不把 SSH descriptor 改成 container。

## 4. 远端长作业与取消的取舍

**源码：** [remote.rs:649–720](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runs/src/run_context/remote.rs#L649-L720) 发现已确认 handle 后直接复用；启动确认丢失后通过 prepare 检查既有 handle，避免直接重复执行启动命令。

**采用原则：** 先核对原任务身份，再讨论恢复。OmicsOps 已通过 job_id、request hash、主机指纹、remote root 和持久化回执实现同类防重派边界；首期只解释“提交成功不等于完成、未知不自动重跑”，不重写远端 controller。

**源码：** [remote.rs:894–956](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runs/src/run_context/remote.rs#L894-L956) 取消前校验 token/PGID/starttime，发 TERM 后确认进程组是否仍存活；存活则返回可重试状态。

**后置借鉴：** 若未来增加远端取消，借鉴身份匹配及停止后确认，明确请求已送达与进程已终止的区别。本期不新增后台取消命令，也不把停止 Agent 解释成取消后台作业。

**不照搬：** [run_context.rs:1772–1786](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runs/src/run_context.rs#L1772-L1786) 的第二次 cancel 会本地 abort 并调用 `force_finish_cancelling_run` 标记 Cancelled。该路径不能作为远端停止的充分证据；OmicsOps 保留“远端终止未确认”的语义，不因放弃本地等待就宣称取消成功。

## 5. 测试证据与验证限度

已读上游测试源码包括：[generation 变化拒绝旧请求，manager.rs:1589–1627](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runtime/src/manager.rs#L1589-L1627)、[缺失 runtime 的 inspect 不启动 worker，1651–1661](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runtime/src/manager.rs#L1651-L1661)、[丢弃调用方不取消/破坏 runtime 协议，1665–1679](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runtime/src/manager.rs#L1665-L1679)。

远端测试源码还覆盖 [第二次取消强制收口](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runs/src/run_context/tests.rs#L880)、[启动超时后接回已确认 supervisor](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runs/src/run_context/tests.rs#L1210)、[远端进程组未确认停止则保持 cancelling](https://github.com/xuzhougeng/wisp-science/blob/b242fcbd1867551889643bfb7ee734dc48f7f5f1/crates/wisp-runs/src/run_context/tests.rs#L1382)。它们支持对代码意图的理解，也揭示本地取消收口与远端确认需要分开看。

**本轮未执行上游测试。** 源码与测试阅读不等于上游运行验收，更不等于 OmicsOps 真实模型、SSH、R/Micromamba 或 PBMC 验收。

首期实施后仍执行原计划中的 DTO/只读查询、candidate/compacted 全请求预算、草稿/历史身份和 Escape 测试；沿用仓库完整检查与必要桌面构建，不增加仅验证上游实现的测试负担。

## 6. 对设计与计划的实际影响

本轮补齐了实际源码依据：只读观察、宿主授权、会话身份与远端防重派均有明确落点，不再仅依赖架构提案理解它们。

保留原有三项首期任务及接口。新增的源码差异只要求继续坚持已有取消边界，不需要改协议、加表、引入 generation 或部署服务端沙盒。

本轮只新增此审计及原设计/计划的交叉链接；实施计划待审状态保持不变，没有新增审批阶段，也没有产品代码提交。
