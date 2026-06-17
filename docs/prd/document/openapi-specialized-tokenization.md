# OpenAPI 专门分词方案 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-06-05
**优先级**: P1
**权威范围**: OpenAPI 文档 FTS 索引的格式感知分词策略、文档类型传播、向后兼容规则

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/01-user-user-stories.md`。

### 1.1 相关故事

| ID | 标题 | 优先级 | 角色 | 影响说明 |
|----|------|--------|------|----------|
| US-CORE-027 | 用 API 路径或方法名检索到对应端点 | P1 | User | 本 PRD 的核心用户价值 |
| US-CORE-023 | 精确关键词能命中包含该关键词的文档 | P1 | User | OpenAPI 专门分词提升该故事的检索质量 |
| US-CORE-018 | 上传 OpenAPI JSON 文件到知识库 | P0 | User | 本 PRD 优化上传后的索引分词质量 |

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 1 | US-CORE-018 |
| P1 | 2 | US-CORE-027、US-CORE-023 |

---

## 2. 范围界定

### 2.1 包含功能

- **OpenAPI 格式感知分词**：新增 OpenAPI 专用分词逻辑，从 Markdown 格式的端点内容中提取 API 结构化 token（路径分段、HTTP 方法、状态码、参数名、Content-Type）
- **文档类型传播**：解析管道中的分块结构新增文档类型和预计算分词结果两个可选字段，在解析器 → 分块 → 索引管道中传递文档类型信息
- **索引时路由**：FTS 索引写入时根据文档类型选择分词策略（OpenAPI → 专用分词，其他 → jieba 通用分词）
- **路径层级分词**：借鉴 Elasticsearch `path_hierarchy` 模式，API 路径按 `/` 分段并去掉 `{}`，每级路径段成为独立可检索单元

### 2.2 不包含功能 (Out of Scope)

- 查询时针对 OpenAPI 文档的专门分词增强（查询仍使用通用 jieba 分词）
- Swagger 2.0 文档的分词支持
- 前端或 API 接口变更
- FTS 索引自动重建或迁移脚本（已有文档需删除后重新上传）
- 数据库表结构变更
- 自定义分词词典或停用词配置
- SQLite FTS5 自定义 tokenizer 插件

### 2.3 依赖项

- 现有 FTS5 索引框架（`keyword-search-support.md`）
- 现有 OpenAPI 解析器（输出 Markdown 格式端点内容）
- 现有通用分词函数链（jieba 中文分词及降级逻辑）
- 现有解析分块 → 文档分块 → 索引管道

---

## 3. 需求概述

### 3.1 功能描述

当前 OpenAPI JSON 文档上传后，FTS 全文索引使用通用 jieba 分词器处理端点内容。通用 jieba 分词对 API 结构化信息（路径段、HTTP 方法、参数名、状态码）处理不佳，导致这些关键信息无法被关键词搜索精确命中。本需求为 OpenAPI 文档提供格式感知的专门分词方案，使 API 路径、方法、参数等结构化信息成为独立可检索 token。

### 3.2 关键特性

- **路径层级分词**：`/api/documents/{documentId}/publish` 拆分为 `api`、`documents`、`documentId`、`publish` 等独立 token，支持按任意路径段检索
- **结构化字段提取**：HTTP 方法、状态码、Content-Type 等作为独立 token 保留
- **参数名提取**：从端点 Markdown 内容中提取参数名和 schema 字段名作为可检索单元
- **自然语言兼容**：端点内容中的 summary/description 部分继续使用 jieba 中文分词
- **向后兼容**：新增字段为可选，非 OpenAPI 文档走原有 jieba 分词路径，无影响

---

## 4. 业务规则与状态

### 4.1 业务规则

- **分词策略路由**：索引时根据文档类型字段选择分词函数；OpenAPI 文档 → 专用分词逻辑，其他或未标记 → 现有 jieba 分词
- **可选字段向后兼容**：文档类型和预计算分词结果字段为可选，默认空；旧数据和现有解析器无需改动
- **自然语言部分不丢弃**：OpenAPI 专用分词在提取结构化 token 的同时，对自然语言部分（summary、description）仍做 jieba 分词，合并输出
- **查询时不区分文档类型**：查询分词仍使用通用 jieba 分词，用户查询为自然语言，API 关键词（方法名、路径等）已被 jieba 正确切分或作为整词匹配
- **已有数据需重新上传**：修改分词策略后，已索引的 OpenAPI 文档需删除后重新上传才能使用新分词；FTS5 token 在写入时确定，不支持按 token 更新

### 4.2 关键状态与异常

- **分词函数输入不一致**：OpenAPI 专用分词的输入为 Markdown 格式（由 OpenAPI 解析器的端点格式化功能生成），非原始 JSON；分词逻辑基于已知 Markdown 格式用固定模式提取，格式稳定可控
- **长文本分块后分词**：端点 Markdown 经过 `text_chunker` 分块后不改变内容格式，仅截断；分块不影响结构化 token 提取
- **非 OpenAPI 文档不受影响**：xlsx、Markdown/MDX 文档的 `content_type` 为 `None`，继续走原有 jieba 分词路径

---

## 5. 功能需求

### 5.1 核心需求

1. **OpenAPI 专门分词函数**
   - 新增 OpenAPI 专用分词逻辑，输入为 Markdown 格式的端点内容
   - 从 Markdown 中提取 API 路径分段（去掉 `{}`）、HTTP 方法、状态码、参数名、Content-Type 作为独立 token
   - 对自然语言部分（summary、description）继续使用 jieba 中文分词
   - 结构化 token 与自然语言 token 合并输出

2. **文档类型传播机制**
   - 解析分块结构新增文档类型字段和预计算分词结果字段，均为可选，默认空
   - 文档分块结构新增对应字段，从解析分块自动传播
   - OpenAPI 解析器创建分块时标记文档类型为 OpenAPI 并预计算分词结果

3. **索引时分词路由**
   - FTS 索引写入逻辑根据文档类型字段路由分词策略
   - 文档类型为 OpenAPI 且预计算分词结果有值 → 使用预计算 tokens
   - 其他情况 → 使用现有通用 jieba 分词

### 5.2 验收目标

- 查询 "publish" 能命中路径包含 `/publish` 的 OpenAPI 端点
- 查询 "POST" 或 "POST documents" 能命中对应 HTTP 方法的端点
- 查询 "documentId" 能命中使用该参数的端点
- 查询中文内容（如 "创建文档"）仍能基于 summary/description 匹配到相关端点
- xlsx/Markdown 文档的 FTS 检索行为和结果质量不受影响
- 已发布的 OpenAPI 文档删除后重新上传，新索引使用专用分词

---

## 6. API 相关约束

**适用性**: 不适用

本功能不涉及 API 接口变更。上传、检索、聊天等接口的请求格式和响应格式保持不变。分词策略变更为纯后端内部行为，对调用方完全透明。

---

## 7. 前端/交互约束

**适用性**: 不适用

本功能对前端完全透明。frontend 和 widget 均无需任何变更，检索质量提升由后端分词策略优化实现。

---

## 8. 已确认决策

- **分词策略**：OpenAPI 文档使用专用分词逻辑，其他文档继续使用 jieba 通用分词
- **路径分词模式**：借鉴 Elasticsearch `path_hierarchy` 模式，按 `/` 分段并去掉 `{}`
- **查询分词不变**：查询时仍使用通用 jieba 分词，不区分文档类型
- **向后兼容**：新增字段为可选，旧数据和新格式文档走不同路径互不干扰
- **数据迁移策略**：已有 OpenAPI 文档需删除后重新上传；不提供自动重建脚本
- **不引入新依赖**：使用现有 jieba-rs、pulldown-cmark、serde_json 实现
- **不注册 FTS5 自定义 tokenizer**：在应用层预计算 tokens 字符串，通过 FTS5 content 写入，比 C FFI 插件方案更简单
- **不支持 Swagger 2.0**：仅支持 OpenAPI 3.x，与现有 OpenAPI 导入 PRD 一致

---

## 9. 参考资料

- 用户故事：`docs/user-stories/01-user-user-stories.md`（US-CORE-027、US-CORE-023、US-CORE-018）
- 相关 PRD：`docs/prd/document/support-openapi.md`（OpenAPI 文档导入）
- 相关 PRD：`docs/prd/document/keyword-search-support.md`（FTS5 关键词搜索）
- 相关 PRD：`docs/prd/document/document-retrieval-and-citations.md`（检索规则基线）
- 学术参考：Pesl et al., "Analyzing OpenAPI Chunking for RAG", CAiSE 2025（format-specific 分词优于 naïve 方案）
- 业界参考：Elasticsearch `path_hierarchy` tokenizer（分层路径分词标准模式）
