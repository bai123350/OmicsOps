# DeepSeek 模型设置

设置 → 模型提供方 → DeepSeek → Configure，填写 API key 后保存。
预填地址为 `https://api.deepseek.com/v1`，默认模型为 `deepseek-v4-flash`；
模型输入框提供 Flash、Pro 和 Flash Vision Exp 候选，也允许输入完整模型 ID。
保存后可使用“可用模型”查询服务端模型列表，或使用“测试”检查连接。
预设名称依据 [DeepSeek 官方文档](https://api-docs.deepseek.com/)；实际可用性以服务端为准。

DeepSeek 作为 UI 预设复用 `open_ai_compatible` 协议、现有保存命令及密钥管理，
没有新增 provider 枚举、DTO 或数据库迁移。模型能力仍由现有编译目录精确匹配，
预设不推断上下文、输出额度或工具能力。密钥沿用 Credential Manager/keyring，
模型调用会将请求发送到配置的服务端。

确定性 UI 测试覆盖默认值、自定义模型保存、已有配置识别、空密钥编辑和 Escape。
手工 smoke：在 Windows 设置中创建 DeepSeek 配置，保存后重新打开检查模型和地址，
确认密钥输入框为空；查询可用模型并测试连接。真实 API 验证需要用户自己的密钥，
模拟 UI 测试不代表真实模型或 Agent 端到端验收通过。
