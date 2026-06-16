# Rerank（精排） 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-06-08
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/01-user-user-stories.md`。

### 1.1 相关故事

| ID | 标题 | 优先级 | 角色 |
|----|------|--------|------|
| US-CORE-030 | 开启 Rerank 后检索结果更精准 | P1 | User |
| US-CORE-031 | Rerank 失败时用户无感知 | P0 | User |

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 1 | Rerank 失败降级 |
| P1 | 1 | 精排提升检索准确性 |

---

## 2. 范围界定

### 2.1 包含功能

- 在现有检索管线（向量 + BM25 + RRF 融合）后增加 rerank 精排阶段
- 支持 **OpenRouter Rerank** provider（Cohere Rerank 系列模型）
- 支持 **智谱 BigModel Rerank** provider（BGE-Reranker 系列模型）
- 支持 **阿里云百炼 DashScope Rerank** provider（qwen3-rerank 模型）
- 通过配置文件切换 provider 和模型
- Rerank 默认关闭，管理员显式启用
- Rerank 调用失败时降级到无 rerank 的原有检索流程

### 2.2 不包含功能 (Out of Scope)

- 自托管 cross-encoder 模型（需 Python sidecar，不符合项目简洁原则）
- 前端任何改动（rerank 对用户透明）
- Rerank 结果日志持久化（仅 tracing 级别输出）
- 多 provider 并行或负载均衡
- Rerank 相关的运行时配置 API（仅支持配置文件）
- 对外暴露 rerank 得分或排序信息

### 2.3 依赖项

- 现有 VectorStoreManager 的混合搜索能力（向量 + BM25 + RRF）
- 现有 chat handler 的搜索调度流程
- 现有配置文件基础设施
- 模型 Provider PRD 的配置模式（`infrastructure/model-providers.md`）
- 查询改写 PRD 的检索管线集成点（`chat/query-rewrite.md`）

---

## 3. 需求概述

### 3.1 功能描述

在现有检索管线（召回 → RRF 融合 → 截断）中增加精排阶段，变为「召回 → 融合 → **精排** → 截断」。Rerank 通过 cross-encoder 模型在 token 级别评估查询-文档相关性，对 RRF 融合后的候选结果重新排序，提升送入 LLM 的 top-K 上下文质量。

### 3.2 关键特性

- **多 provider 支持**：OpenRouter Rerank（免费 Cohere 模型）、智谱 BigModel Rerank（BGE-Reranker 模型）和阿里云百炼 DashScope Rerank（qwen3-rerank 模型），通过配置切换
- **默认关闭**：rerank 会引入额外延迟（约 200-500ms），默认关闭，管理员按需启用
- **无感降级**：rerank API 调用失败时，使用 RRF 融合结果继续，用户不感知错误
- **管线位置固定**：rerank 在 RRF 融合后、上下文组装前执行，不影响召回阶段

---

## 4. 业务规则与状态

### 4.1 业务规则

- **默认关闭**：rerank 功能默认不启用，需要管理员在配置文件中显式开启
- **单 provider**：同一时间只能配置一个 rerank provider（OpenRouter、BigModel 或 DashScope）
- **候选数量控制**：送入 rerank 的候选文档数量应控制在 top-20 以内，平衡延迟与精度
- **rerank 用途边界**：rerank 结果仅影响送入 LLM 的上下文选择，不改变用户看到的引用来源展示逻辑
- **不改 API 契约**：聊天接口请求/响应/SSE 结构保持不变
- **不改数据库**：不引入新表、新字段、新迁移
- **不改前端**：widget 与 frontend 均无需变更

### 4.2 关键状态与异常

- **rerank API 调用失败**（网络错误、超时、API 错误）：降级使用 RRF 融合结果，记录 warn 日志，用户不感知
- **rerank API 超时**：超时上限可配置（建议默认 3 秒），超时后降级
- **rerank API Key 无效**：启动时验证不阻塞，首次调用失败后降级并记录错误
- **provider 配置缺失**：rerank 未配置或未启用时，检索流程完全不变

---

## 5. 功能需求

### 5.1 核心需求

1. **Reranker 模块**
   - 新增 reranker 模块，封装 rerank API 调用逻辑
   - 支持 OpenRouter Rerank、智谱 BigModel Rerank 和阿里云百炼 DashScope Rerank 三个 provider
   - 通过配置文件选择 provider、模型和参数

2. **OpenRouter Rerank Provider**
   - 支持 Cohere Rerank 系列模型（推荐 `cohere/rerank-v4-fast`）
   - 使用独立配置的 OpenRouter API Key 认证
   - 输入查询和候选文档，返回按相关性分数排序的结果

3. **智谱 BigModel Rerank Provider**
   - 支持 BGE-Reranker 系列模型（推荐 `rerank-pro`，底层模型为 `bge-reranker-v2-minicpm-layerwise`）
   - 使用独立配置的智谱 API Key 认证
   - 输入查询和候选文档，返回按相关性分数排序的结果

4. **阿里云百炼 DashScope Rerank Provider**
   - 支持 `qwen3-rerank` 模型（`gte-rerank` v1 已下线，`qwen3-rerank` 为官方推荐替代）
   - 接入百炼 OpenAI 兼容精排能力；未单独配置 API Key 时复用对话模型 Key
   - 输入查询和候选文档，返回按相关性分数排序的结果

5. **管线集成**
   - 在 RRF 融合后、上下文组装前调用 rerank
   - 将候选文档的 content 作为 documents 数组传入
   - 按 relevance_score 重排，取 top-N 送入 LLM

6. **降级保障**
   - rerank 调用失败或超时时，使用 RRF 融合结果继续
   - 所有降级记录日志，不影响用户主流程

### 5.2 验收目标

- 配置启用 rerank 后，对同一查询，rerank 重排后的 top-5 结果语义相关性高于未启用时
- OpenRouter Rerank provider 配置正确时，调用成功并返回排序结果
- 智谱 BigModel Rerank provider 配置正确时，调用成功并返回排序结果
- 阿里云百炼 DashScope Rerank provider 配置正确时，调用成功并返回排序结果
- rerank 未启用时，检索流程与现有行为完全一致
- rerank API 调用失败时，系统使用 RRF 融合结果正常回答，用户不感知错误
- rerank 阶段延迟 ≤ 1 秒（不含网络超时场景）
- widget 和 `/api/chat` 调用方无需任何变更

---

## 6. API 相关约束

**适用性**: 不适用

本功能不涉及 API 接口变更。聊天接口的请求格式、响应格式、SSE 事件结构保持不变。Rerank 为纯后端内部行为，对调用方完全透明。

---

## 7. 前端/交互约束

**适用性**: 不适用

本功能对前端完全透明。frontend 和 widget 均无需任何变更，检索质量的提升由后端 rerank 机制实现。

---

## 8. 已确认决策

- **默认关闭**：rerank 引入额外延迟，默认不启用，管理员按需开启
- **多 provider**：支持 OpenRouter Rerank、智谱 BigModel Rerank 和阿里云百炼 DashScope Rerank（默认 `qwen3-rerank`），通过配置切换
- **DashScope Key 复用**：DashScope rerank 未单独配置 API Key 时复用对话模型 Key（与 OpenRouter 行为一致）
- **DashScope rerank base_url 独立配置**：`dash_scope` 的 rerank `base_url` 不从 `[llm].base_url` 推导，默认使用中国大陆端点 `https://dashscope.aliyuncs.com/compatible-api/v1/reranks`；需要国际区（`dashscope-us`）或自定义网关时通过 `[rerank].base_url` 显式覆盖。chat/embedding/rerank 三面区域不再自动联动，由部署者按需统一配置。
- **单 provider**：同一时间只配置一个 rerank provider
- **管线位置**：rerank 在 RRF 融合后、上下文组装前执行
- **降级策略**：rerank 失败时使用 RRF 融合结果，用户无感知
- **不改 API 契约**：请求/响应/SSE 不变
- **不改前端**：widget 和 frontend 无变更
- **不改数据库**：不引入 schema 变更
- **仅配置文件**：rerank 参数通过配置文件管理，不暴露运行时配置 API

---

## 9. 参考资料

- 模型 Provider PRD：`docs/prd/infrastructure/model-providers.md`
- 文档检索与引用 PRD：`docs/prd/document/document-retrieval-and-citations.md`
- 查询改写 PRD：`docs/prd/chat/query-rewrite.md`
- 关键词搜索 PRD：`docs/prd/document/keyword-search-support.md`
- OpenRouter Rerank API：https://openrouter.ai/docs/api-reference/rerank
- 智谱 BigModel Rerank API：https://docs.bigmodel.cn/api-reference/%E6%A8%A1%E5%9E%8B-api/%E6%96%87%E6%9C%AC%E9%87%8D%E6%8E%92%E5%BA%8F
- 阿里云百炼 Rerank API：https://help.aliyun.com/zh/model-studio/text-rerank-api
