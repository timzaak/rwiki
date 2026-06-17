# OpenAPI JSON 文档导入 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-31
**优先级**: P0
**权威范围**: OpenAPI 3.x JSON 文件上传、解析、page 生成、metadata 提取

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/01-user-user-stories.md`。

### 1.1 相关故事
- `[US-CORE-018]` 上传 OpenAPI JSON 文件到知识库，优先级 P0，来源 `docs/user-stories/01-user-user-stories.md`
- 角色：User
- 摘要：上传 OpenAPI 3.x JSON 作为知识库文档，每个 API 端点生成独立知识页

### 1.2 关联故事（非本 PRD 直接覆盖，但影响检索体验）
- `[US-CORE-002]` 与知识库进行多轮对话 — OpenAPI 文档上传后进入同一检索管道
- `[US-CORE-008]` 聊天回答中查看来源引用 — 端点 page 产生的 metadata 参与引用展示

### 1.3 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 1 | US-CORE-018 |

---

## 2. 范围界定

### 2.1 包含功能
- 在现有上传接口中新增 `.json` 文件格式支持
- 验证 JSON 是否为合法 OpenAPI 3.x 格式（检查 `openapi` 字段和 `paths` 字段）
- 解析 OpenAPI JSON，每个端点（path + method）生成一个 page
- 端点内容以 Markdown 格式输出，包含路径、方法、描述、参数、请求体、响应描述
- 将 `$ref` 引用的 `components/schemas` 定义以摘要形式内联到端点描述中
- page/chunk 进入现有分块和检索管道

### 2.2 不包含功能 (Out of Scope)
- Swagger 2.0 格式支持
- OpenAPI 规范完整性校验（不验证是否符合 OpenAPI 规范的每一个字段要求）
- 跨文件 `$ref` 引用解析（仅处理 `#/components/schemas/` 本地引用）
- 前端上传页面或 UI 调整
- 批量导入、增量更新、版本管理
- OpenAPI 规范中的 `security`、`servers`、`externalDocs` 等非端点内容的索引

### 2.3 依赖项
- 现有文档上传接口和 multipart 处理流程
- 现有 `ParsedChunk` 结构和分块/嵌入管道
- 现有 `serde_json` 依赖（无新增依赖）

---

## 3. 需求概述

### 3.1 功能描述
支持将 OpenAPI 3.x JSON 文件作为知识库文档导入，使 API 文档内容可通过 RAG 检索和问答。每个 API 端点（path + method 组合）生成一个独立的知识页，用户可针对 API 文档进行精确查询。

### 3.2 关键特性
- 端点级粒度：每个 API 端点独立成页，确保检索精度到单个操作
- Markdown 输出：解析结果以 Markdown 格式化，与现有分块策略兼容
- Schema 内联：`$ref` 引用的 schema 定义以摘要形式内联，确保每个端点内容自包含
- 格式验证：基本验证 OpenAPI 3.x 格式，对非 OpenAPI JSON 和空 paths 给出明确错误提示

---

## 4. 业务规则与状态

### 4.1 业务规则
- 上传的 JSON 文件必须包含 `openapi` 字段且版本以 `3.` 开头，否则拒绝并返回格式错误
- `paths` 字段必须存在且非空，否则拒绝并提示无可解析端点
- 每个 path + method 组合生成一个独立 page
- page 的 title 取自 `operationId` > `summary` > path（按优先级降级）
- page 的 section 取自端点所属的 `tags` 数组第一个元素，无 tags 时为空
- 文档级 title 取自 OpenAPI `info.title` 字段
- `$ref` 引用仅展开 `#/components/schemas/` 本地引用，跨文件引用忽略
- 上传文件大小沿用现有 50MB 上限

### 4.2 关键状态与异常
- 非 OpenAPI JSON（缺少 `openapi` 字段）→ 400，提示格式不正确
- OpenAPI 版本非 3.x → 400，提示仅支持 OpenAPI 3.x
- `paths` 为空或不存在 → 400，提示无可解析端点
- JSON 解析失败（非法 JSON 语法）→ 400，提示 JSON 格式无效
- 上传成功后文档状态为"草稿"，遵循现有发布流程

---

## 5. 功能需求

### 5.1 核心需求
- 上传接口支持 `.json` 扩展名，在现有扩展名路由中添加 `.json` 分支
- OpenAPI JSON 解析器将每个端点转换为 `ParsedChunk`，以 Markdown 格式输出
- 端点 Markdown 内容包含：HTTP 方法、路径、摘要/描述、参数列表（路径/查询/请求头/请求体）、响应状态码和描述
- schema 引用在端点描述中以摘要形式内联，不单独生成 page
- 解析后的 page/chunk 进入现有分块和嵌入流水线

### 5.2 验收目标
- 合法 OpenAPI 3.x JSON 上传成功，每个端点生成独立 page
- 端点 page 内容包含路径、方法、参数和响应信息，可被正常检索
- 非 OpenAPI JSON 上传返回 400，提示格式不正确
- `paths` 为空的 OpenAPI JSON 返回 400，提示无可解析端点
- 上传并发布后，用户可通过聊天查询 API 文档并获得基于文档内容的回答
- OpenAPI JSON 文档与 xlsx、Markdown/MDX 文档共享同一检索和引用管道

---

## 6. API 相关约束

**适用性**: 适用

- 复用现有 multipart 文档上传接口，不新增端点
- `.json` 作为新增支持的文件扩展名
- 不支持格式返回 400，错误提示中需包含新支持的 `.json` 格式
- 访问控制遵循现有 API Token 鉴权规则

---

## 7. 前端/交互约束

**适用性**: 不适用

此功能为纯后端文档格式扩展，前端无需改动。用户通过现有 API 上传 OpenAPI JSON 文件。

---

## 8. 已确认决策
- 仅支持 OpenAPI 3.x，不支持 Swagger 2.0
- 每个 API 端点（path + method）生成一个独立 page
- `$ref` 仅展开本地 `#/components/schemas/` 引用，不处理跨文件引用
- 解析输出为 Markdown 格式，与现有分块策略兼容
- 无新增外部依赖，使用现有 `serde_json` 通用解析
- 不涉及前端改动、数据库 schema 变更或新增 API 端点

---

## 9. 参考资料
- 用户故事：`docs/user-stories/01-user-user-stories.md`（US-CORE-018）
- 技术预研：`.ai/tech-research/support-openapi.md`
- 文档摄入 PRD：`docs/prd/document/document-ingestion.md`
- 领域模型：`docs/prd/02-domain-model.md`
