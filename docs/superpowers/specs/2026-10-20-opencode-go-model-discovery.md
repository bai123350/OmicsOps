# OpenCode Go 公共模型列表查询

日期：2026-10-20

## 问题与证据

已保存的 OpenCode Go 配置点击“可用模型”或打开对话中的模型选择器时，可能出现
模型列表读取失败。两个入口最终调用同一个桌面模型发现命令。

此前，桌面端在查询模型列表前先构建完整的推理客户端。这个过程读取凭据并校验
推理强度和 Fast 模式，随后查询模型列表时复用推理请求的认证方式。针对本次故障
的只读诊断确认：同一个 `https://opencode.ai/zen/go/v1/models` 端点，携带
已保存凭据的 Bearer 请求返回 HTTP 401，不携带认证信息的请求返回 HTTP 200
和有效模型列表。诊断不保存或输出凭据、完整请求头或响应中的敏感信息。

核对 [OpenCode Go 官方说明](https://opencode.ai/v2/docs/console/go/#models)：
当前文档给出的公共列表地址为 `/inference/go/v1/models`，无认证 curl 示例。
2026-10-20 对新旧两个地址的无认证检查均返回 HTTP 200；因此不能把旧地址失效
当作本次根因。本次保留已验证可用的旧地址，不迁移推理端点或已保存配置。

该模型列表是公共信息。发现模型不应依赖推理凭据是否有效，也不应被当前模型的
推理参数校验阻断。模型列表查询成功只代表取得公开列表，不能作为凭据有效、
账户具有模型权限或推理能够成功的证据。

## 设计与边界

在 `omicsops-adapters` 中提供独立的 `ModelDiscoveryClient`：

- `from_profile(&ModelProfile, Arc<ModelClientServices>)` 选择发现路径。
- 精确匹配受支持的 OpenCode Go HTTPS 端点，以及 Chat Completions、
  Anthropic Messages、Responses 三种 provider 后，使用独立公共 GET。
- 公共查询不读取 keyring，不发送 `Authorization` 或 `x-api-key`，不构建
  推理客户端，也不校验推理强度、Fast 模式或当前模型的推理能力。
- 其他 provider 和非精确匹配的端点继续使用已有模型发现行为；不能根据相似
  host、路径、协议、端口或模型前缀推断公共查询能力。
- 公共查询保留超时、响应大小限制、严格模型 ID 解析、排序和去重；失败不能
  被转换为静态目录或伪造的成功结果。

桌面端 `list_model_profile_models` 与
`list_model_profile_model_discovery` 共用一个 helper：按 ID 读取已有配置，
创建 discovery client 并取得结果。旧的列表命令仅从相同结果中提取 `models`，
结构化命令继续返回已有 `ModelDiscoveryResult`。

`probe_model_profile` 和正常推理继续使用原来的推理客户端与凭据校验。
前端继续区分查询失败、空列表和正常列表，保留已有脱敏提示与重试行为。
本次不改变 provider 的推理协议选择和模型目录能力匹配规则。

## 兼容性、数据与权限

已有 OpenCode Go 配置无需更换 Base URL、重新保存或刷新目录能力。
此修复不更改配置 ID、模型 ID、目录快照、凭据引用或 SQLite 模式，不新增迁移。
公开列表返回的模型 ID 不代表当前账户已获授权，选择后的保存与推理仍执行原有
校验。

凭据继续仅存于现有 Credential Manager/keyring 路径。公共列表查询不读取、
传输或持久化凭据，也不向模型提供方发送科研提示词、工具结果或项目数据。
提供方仍会收到普通网络请求信息。正常推理的数据传输边界保持不变。

此修复不改变宿主审批、工具授权、项目隔离、运行证据、审计、远端作业或恢复
语义；不将一次模型发现结果记为已验证的科研结果或成功的推理运行。

## 确定性验证方案

测试使用模拟 transport 和内存凭据存储，不依赖 API key、真实账户或网络：

1. 对三种 Go provider 验证公共 GET 的准确端点和无认证请求头，确认没有
   keyring 读取或推理 POST。
2. 验证缺少凭据、凭据读取失败和当前推理参数不可用时，公共发现仍可成功。
3. 验证推理客户端的凭据与推理参数校验继续生效。
4. 验证相似 host、非 HTTPS、不同端口、额外路径、userinfo、query 和 fragment
   不会进入 Go 公共发现路径。
5. 验证模型数组解析、排序去重、空列表、无效响应、非成功状态、重定向和
   大小限制；失败保持为错误，并复用现有 15 秒超时。
6. 运行桌面端编译和既有模型设置、模型选择器、DTO 契约测试，确认两个命令使用
   同一发现结果且没有 UI 或序列化契约变化。

已执行的回归红态：

- `cargo test -p omicsops-adapters go_model_discovery_omits_credentials_but_inference_keeps_them --lib`
  因实际带有 `Authorization` 而失败。
- `cargo test -p omicsops-adapters --test model_client_contracts model_client_discovery_reports_only_valid_provider_data`
  因 Responses 列表携带认证头而失败。
- `cargo test -p omicsops-adapters --test model_client_contracts public_go_discovery`
  中公共查询在构建推理客户端时因缺少凭据失败。

修复后的确定性检查：

- `cargo test -p omicsops-adapters --lib --test model_client_contracts --test responses_contracts`：
  67 通过、0 失败、2 ignored。首次在沙箱中运行时，本地 HTTP 重定向测试和
  Skills 原子安装测试失败，多个 loopback probe 等待未结束，已中止该次运行；
  以允许本地 HTTP/临时目录操作的权限完整重跑上述相同命令后全部通过。
- `npm run build`：通过；仍有既有 bundle 大小提示。
- `npm test`：1,441 项前端测试和 22 项浏览器扩展测试通过。
- `cargo test --workspace`：1,942 通过、0 失败、19 ignored，退出码 0。
  ignored 数量包含新增公共列表验收，并不代表这些验收已经通过。
- `cargo fmt --all -- --check`：通过。

确定性检查之后，按 `acceptance/README.md` 设置
`OMICSOPS_LIVE_GO_DISCOVERY=1` 并显式执行：

```text
cargo test -p omicsops-adapters --test model_client_contracts live_public_go_discovery_without_credentials -- --ignored --exact --nocapture
```

结果：1 通过、0 失败、退出码 0；生产 discovery 客户端分别以 Chat、Messages、
Responses 三种内存配置获得 37 个公共模型，测试耗时 1.21 秒。该测试的凭据
vault 在任何读取时都会失败，未访问用户数据库、调用推理或修改配置。
自动化替身、公开模型列表获取成功或构建成功不等于真实模型端到端验收。

`npm run build:desktop`：通过、退出码 0，Windows release 优化编译用时
5 分 20 秒，完成本地 NSIS 构建。保留既有 Web bundle 大小提示及 Windows
链接器信息提示。未安装、发布或分发构建产物。

## 手工 smoke 与未执行验收

在 Windows 桌面应用中使用已有 OpenCode Go 配置：

1. 点击设置中的“可用模型”，确认返回列表且可以再次刷新。
2. 打开对话中的模型选择器，确认可以获取、筛选并选择服务端返回的模型。
3. 切换 Chat Completions、Messages、Responses 配置，确认列表查询行为一致。
4. 在一次性配置中使用缺失或无效凭据，确认公共列表仍可读取；“测试”及推理
   仍通过原有认证路径报告其实际结果。
5. 断网后查询并恢复网络重试，确认失败提示与重试行为正常。

本次未执行原生 UI 手工 smoke；真实公共接口验收结果见上文。真实模型推理、Agent Runtime
V4、SSH、R、Micromamba 和 PBMC 验收未因公开模型列表诊断而执行；如需这些验收，
仍须遵循 `acceptance/README.md` 使用一次性环境并显式运行对应 ignored 测试。
