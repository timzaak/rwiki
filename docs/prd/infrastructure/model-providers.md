# 模型 Provider 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-30
**优先级**: P1
**权威范围**: LLM Provider、Embedding Provider、base_url/api_key/model/dimensions 配置

---

## 1. 范围界定

包含：

- LLM 通过 OpenAI-compatible Chat Completions 协议接入。
- Embedding 通过 OpenAI-compatible Embeddings 协议接入。
- provider 由 `base_url`、`api_key`、`model` 和可选 `dimensions` 配置决定。
- provider 切换后的兼容性和重建索引要求。

不包含：

- 多 provider 负载均衡、故障切换或自动降级。
- 前端模型选择 UI。
- 模型质量基准测试工具。
- 向量存储实现，见 `infrastructure/storage.md`。

## 2. LLM Provider 规则

- LLM 使用 OpenAI-compatible Chat Completions 协议。
- 通过 `[llm].base_url` 指向目标 provider。
- 通过 `[llm].api_key` 配置对应 provider 的 API Key。
- 通过 `[llm].model` 配置模型名称。
- OpenAI、OpenRouter、GLM、阿里云百炼（DashScope）、Google Gemini（OpenAI 兼容层）等兼容 provider 均通过同一配置模型接入。
- LLM provider 与 embedding provider 独立配置，互不影响。
- SSE 流式输出必须兼容现有 Chat API 契约。
- 单一 provider 全栈：部分兼容 provider（如阿里云百炼 DashScope）可同时提供对话与向量化能力；结合 rerank provider 选项（见 `core/rerank-investigation.md`），部署者可用单一 provider 与单一 API Key 驱动完整 RAG 链路。

## 3. Embedding Provider 规则

- Embedding 使用 OpenAI-compatible Embeddings 协议。
- 通过 embedding 配置的 `base_url`、`api_key`、`model` 选择 provider 和模型。
- 可选 `dimensions` 用于指定向量维度。
- 不配置 `dimensions` 时使用模型默认维度。
- Google Gemini 通过 OpenAI 兼容层接入（`base_url = "https://generativelanguage.googleapis.com/v1beta/openai"`），模型 `gemini-embedding-001`。
- Gemini 必须显式配置 `dimensions`（768/1536/3072）：向量存储按该值确定列宽，省略时按 0 维建表，向量无法写入。
- Gemini 单条输入上限 2048 token；默认分块 1600 字符，中文长文本可能超限，需要调小分块上限。
- 切换 provider、model 或 dimensions 后，已有向量数据可能不兼容，必须重建索引或清空数据库。
- 当前向量存储维度检查由 `infrastructure/storage.md` 规定。

## 4. 配置约束

| 配置 | 含义 | 约束 |
|------|------|------|
| `base_url` | Provider API 根地址 | 必须匹配 OpenAI-compatible 协议 |
| `api_key` | Provider API Key | 必须属于目标 provider |
| `model` | 模型名称 | 必须为目标 provider 支持的模型 |
| `dimensions` | embedding 向量维度 | 可选；变更后需重建索引 |

## 5. 异常规则

- API Key 无效时，调用失败并返回可诊断错误。
- base_url 错误时，调用失败并提示连接或端点错误。
- 模型名称不支持时，返回模型不存在或 provider 错误。
- embedding 维度与存储维度不一致时，应用不得继续提供检索能力。
- provider SSE 格式不兼容时，Chat 流式响应应失败并暴露服务端可诊断日志。

## 6. 验收目标

- 配置 OpenAI-compatible LLM 后，聊天 SSE 正常输出。
- 配置 OpenAI-compatible embedding 后，文档可上传、索引、检索。
- 配置 dimensions 后，生成向量维度与配置一致。
- 切换 embedding dimensions 后，应用能识别不兼容存储并阻止错误检索。
- LLM provider 切换不影响 embedding provider 配置。
- 配置阿里云百炼（DashScope）对话与向量化后，聊天 SSE 正常输出、文档可上传索引检索。
- 配置 Gemini（`gemini-embedding-001`，显式 `dimensions`）向量化后，文档可上传、索引、检索。

## 7. 参考资料

- 存储与持久化：`/docs/prd/infrastructure/storage.md`
- 聊天助手：`/docs/prd/chat/chat-assistant.md`
