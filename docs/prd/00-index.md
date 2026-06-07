# PRD 索引

## 文档权威边界

- `docs/prd/**` 是正式产品需求源，描述用户价值、范围、业务规则、验收目标和用户可见契约。
- `docs/user-stories/**` 是用户故事和 Gherkin 验收标准源；PRD 只引用故事 ID，不复制完整 Gherkin。
- `.ai/design/**` 是技术设计源，只描述如何实现，不覆盖 PRD 中的产品规则。
- `.ai/future/**` 是候选方案或迁移方案；采纳后必须并入正式 PRD 或设计，并归档。

## 状态说明

| 状态 | 含义 |
|------|------|
| Draft | 需求未稳定，可继续调整 |
| Planned | 需求稳定，等待设计或实现 |
| Implemented | 已落地，PRD 作为行为基线维护 |
| Superseded | 已被其他 PRD 替代，必须写明替代文件 |
| Archived | 历史记录，不再作为需求源；本项目当前不保留归档 PRD 文件 |

## 正式 PRD

| 域 | 功能 | 文件 | 状态 | 权威范围 |
|----|------|------|------|----------|
| overview | 产品总览 | [01-product-overview.md](01-product-overview.md) | Draft | 产品定位、用户角色、能力地图、全局非目标 |
| overview | 领域模型 | [02-domain-model.md](02-domain-model.md) | Draft | document、page、chunk、status、metadata 语义 |
| chat | 聊天助手 | [chat/chat-assistant.md](chat/chat-assistant.md) | Implemented | 基础聊天体验、RAG 问答、流式输出、页面和弹窗入口 |
| document | 文档生命周期 | [document/document-lifecycle.md](document/document-lifecycle.md) | Implemented | document 状态、发布/取消发布、删除、可检索性 |
| document | 文档摄入 | [document/document-ingestion.md](document/document-ingestion.md) | Implemented | xlsx、Markdown/MDX、OpenAPI JSON 上传、解析、校验、page 生成、metadata 提取 |
| document | 文档检索与引用 | [document/document-retrieval-and-citations.md](document/document-retrieval-and-citations.md) | Implemented | 分块、窗口扩展、metadata 上下文、来源引用 |
| document | OpenAPI JSON 文档导入 | [document/support-openapi.md](document/support-openapi.md) | Implemented | OpenAPI 3.x JSON 上传、解析、端点级 page 生成、metadata 提取 |
| infrastructure | 存储与持久化 | [infrastructure/storage.md](infrastructure/storage.md) | Implemented | SQLite 存储形态、向量持久化、启动恢复、维度兼容性 |
| infrastructure | 模型 Provider | [infrastructure/model-providers.md](infrastructure/model-providers.md) | Implemented | LLM Provider、Embedding Provider、模型和维度配置 |
| infrastructure | 可观测性 | [infrastructure/observability.md](infrastructure/observability.md) | Implemented | tracing/span 输出、OTLP 配置、shutdown flush |
| infrastructure | API Token 鉴权 | [infrastructure/api-token-auth.md](infrastructure/api-token-auth.md) | Implemented | API token 安全边界和接口鉴权约束 |
| core | 可配置系统提示词 | [core/configurable-system-prompt.md](core/configurable-system-prompt.md) | Implemented | 系统提示词配置和聊天行为基线 |
| chat | 多轮对话 Hybrid 记忆管理 | [chat/multi-turn-conversation-hybrid-memory.md](chat/multi-turn-conversation-hybrid-memory.md) | Implemented | 多轮记忆策略和用户可见对话行为 |
| integration | 可嵌入 Chat Widget (JS) | [integration/chat-widget-embeddable-js.md](integration/chat-widget-embeddable-js.md) | Implemented | Widget 集成契约、加载生命周期和限制 |
| integration | API 调用编排 Agent | [integration/api-orchestrator-agent.md](integration/api-orchestrator-agent.md) | Draft | OpenAPI 规范管理、调用计划生成、审批执行、结果展示 |
| chat | 查询改写与扩展 | [chat/query-rewrite.md](chat/query-rewrite.md) | Implemented | 首轮改写、多查询扩展、RRF 融合、降级保障 |
| document | 关键词搜索支持 | [document/keyword-search-support.md](document/keyword-search-support.md) | Implemented | FTS5 全文搜索、jieba-rs 分词、混合检索、RRF 融合、降级保障 |
| chat | 查询语言感知改写 | [chat/query-language-aware-rewrite.md](chat/query-language-aware-rewrite.md) | Implemented | 知识库内容语言配置、语言感知查询改写、多轮持续语言改写、向后兼容 |
| chat | 上下文组装与 Prompt 格式优化 | [chat/chat-context-prompt-assembly.md](chat/chat-context-prompt-assembly.md) | Draft | XML 结构化上下文、来源编号、Preamble 英文化、XML 转义、引用指令增强 |
| document | OpenAPI 专门分词方案 | [document/openapi-specialized-tokenization.md](document/openapi-specialized-tokenization.md) | Draft | OpenAPI 文档 FTS 格式感知分词、路径层级分词、文档类型传播、向后兼容 |
| chat | Pre-Question 推荐问题按钮 | [chat/pre-question-suggested-buttons.md](chat/pre-question-suggested-buttons.md) | Draft | 空状态推荐问题按钮、后端配置 API、Widget 配置支持 |
