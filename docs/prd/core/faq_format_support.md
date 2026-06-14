# FAQ JSONL 格式支持 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-06-13
**优先级**: P1
**权威范围**: FAQ 问答对 JSONL 文件上传、解析、page 生成、metadata 映射

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/01-user-user-stories.md`。

### 1.1 相关故事
- `[US-CORE-032]` 上传 FAQ JSONL 文件到知识库，优先级 P1，来源 `docs/user-stories/01-user-user-stories.md`
- 角色：User
- 摘要：上传 FAQ 问答对 JSONL 文件作为知识库文档，每条 Q&A 生成独立知识页

### 1.2 关联故事（非本 PRD 直接覆盖，但影响检索体验）
- `[US-CORE-002]` 与知识库进行多轮对话 — FAQ 文档上传后进入同一检索管道
- `[US-CORE-008]` 聊天回答中查看来源引用 — FAQ page 产生的 metadata 参与引用展示
- `[US-CORE-018]` 上传 OpenAPI JSON 文件到知识库 — 独占 `.json` 扩展名，与 FAQ `.jsonl` 路由互不影响

### 1.3 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P1 | 1 | US-CORE-032 |

---

## 2. 范围界定

### 2.1 包含功能
- 新增 `.jsonl` 扩展名路由支持 FAQ JSON Lines 格式（每行一个 JSON 对象）
- 每条 FAQ 问答对生成一个独立 page，question 和 answer 均参与 embedding 和全文搜索
- FAQ 可选字段 `tags` 和 `locale` 映射到现有 metadata 字段
- FAQ page/chunk 进入现有分块和检索管道

### 2.2 不包含功能 (Out of Scope)
- FAQ 数据的编辑、更新或增量导入
- FAQ 专用检索策略（如 BM25 问题匹配加权）
- CSV、YAML 等其他 FAQ 数据格式
- 前端上传页面或 UI 调整
- FAQ 问答对的语义去重或合并（如按问题聚类、合并近似问答）。注：内容级写入去重（相同内容已上线则跳过）由摄入管道统一处理，见 `document/document-ingestion.md`
- `category` 字段的存储或处理（忽略该字段，用户可将分类信息纳入 `tags` 或 question/answer 文本）

### 2.3 依赖项
- 现有文档上传接口和 multipart 处理流程
- 现有分块、embedding 和检索管道
- 现有 JSON 解析能力（无新增依赖）

---

## 3. 需求概述

### 3.1 功能描述
支持将 FAQ 问答对 JSONL（JSON Lines）文件作为知识库文档导入，使 FAQ 数据可通过 RAG 检索和问答。每条问答对生成一个独立知识页，question 以 Markdown H2 标题形式进入 content，确保同时参与 embedding 向量化和 section 元数据生成。`.jsonl` 扩展名独占 FAQ 路由，与 `.json`（OpenAPI）扩展名路由互不影响。

### 3.2 关键特性
- 问答对粒度：每条 Q&A 独立成页，确保检索精度到单个问答
- 问题感知 embedding：question 以 H2 标题形式进入 content，与 answer 一起参与向量化，使"用户提问"与"已有 FAQ 问题"在向量空间中语义接近
- 扩展名直接路由：`.jsonl` → FAQ 解析，`.json` → OpenAPI 解析，无需内容结构检测
- 行级错误隔离：单行 JSON 语法错误或字段错误只影响该行，错误信息精确到物理行号或 Q&A 序号
- 全管线复用：FAQ page/chunk 进入现有分块、embedding、FTS5、向量搜索、窗口扩展和 rerank

---

## 4. 业务规则与状态

### 4.1 业务规则
- 上传的文件扩展名必须是 `.jsonl`，内容为 JSON Lines 格式（每行一个独立 JSON 对象）
- 文件编码必须为 UTF-8（可含可选 BOM）
- 行与行之间允许空白行，解析时跳过
- 每条 Q&A 对象必须包含 `question`（字符串，非空白）和 `answer`（字符串，非空白）字段
- `tags`（可选，逗号分隔字符串或字符串数组）映射到 page metadata `tags`
- `locale`（可选，字符串）映射到 page metadata `locale`
- `category`（可选）忽略，不存储
- 其他未知字段忽略
- page 的 title 取自 FAQ `question` 字段
- page 的 content 格式化为 `"## {question}\n\n{answer}"`
- 每条 FAQ 问答对生成一个独立 page
- 文档级 title 取文件名去扩展名
- 上传文件大小沿用现有 50MB 上限

### 4.2 关键状态与异常
- 非 UTF-8 编码 → 400，提示 `文件编码不支持，仅支持 UTF-8`
- 某 JSONL 行 JSON 语法错或不是 JSON 对象 → 400，提示 `第 N 行 JSON 格式无效: {细节}`（N 为物理行号，1-based）
- 某条 Q&A 缺 `question` 或 `answer` → 400，提示 `第 N 条问答缺少必填字段: question/answer`（N 为 Q&A 序号，0-based，跳过空白行后编号）
- 某条 Q&A 的 `question` 或 `answer` 为空白 → 400，提示 `第 N 条问答的 question 不能为空`
- 文件没有任何 Q&A 数据（空文件或只有空白行）→ 400，提示 `文件中没有可用的问答数据`
- 上传成功后文档状态为"草稿"，遵循现有发布流程

---

## 5. 功能需求

### 5.1 核心需求
- `.jsonl` 文件按行解析，每行为独立 JSON 对象，跳过空白行
- FAQ 解析将每条问答对转换为统一知识页结构，content 格式化为 `"## {question}\n\n{answer}"`
- FAQ 的可选 `tags` 和 `locale` 映射到对应 metadata 字段
- 解析后的 page/chunk 进入现有分块和嵌入流水线
- `.json` 文件由 OpenAPI 解析器处理；非 OpenAPI 的 `.json` 由其自身报错
- 不支持格式返回 400，错误提示更新为支持 `xlsx/md/mdx/json/jsonl`

### 5.2 验收目标
- 合法 FAQ JSONL 上传成功，每条问答对生成独立 page
- page 的 title 为 FAQ question，content 包含 question（H2）和 answer
- question 和 answer 均参与 embedding 向量化和 FTS5 全文搜索
- 缺少 `question` 或 `answer` 的 FAQ 条目返回 400，提示中含 Q&A 序号
- `question` 或 `answer` 为空白返回 400
- 空文件或仅含空白行的文件返回 400
- 某行 JSON 语法错误返回 400，提示中含物理行号
- 上传并发布后，用户通过聊天提出与 FAQ question 语义相近的问题时，能检索到对应答案
- `.json` 上传由 OpenAPI 解析器处理，非 OpenAPI 的 `.json` 由其报错
- FAQ、OpenAPI、xlsx、Markdown/MDX 文档共享同一检索和引用管道

---

## 6. API 相关约束

**适用性**: 适用

- 复用现有 multipart 文档上传接口，不新增端点
- `.jsonl` 文件路由到 FAQ 解析器；`.json` 文件直接路由到 OpenAPI 解析器
- 不支持格式返回 400，错误提示更新为支持 `xlsx/md/mdx/json/jsonl`
- 访问控制遵循现有 API Token 鉴权规则

---

## 7. 前端/交互约束

**适用性**: 不适用

此功能为纯后端文档格式扩展，前端无需改动。用户通过现有 API 上传 FAQ JSONL 文件，上传体验与现有格式一致。

---

## 8. 已确认决策
- FAQ 采用 JSONL（JSON Lines）格式，文件扩展名 `.jsonl`，每行一个 JSON 对象
- `.jsonl` 扩展名独占 FAQ 路由，`.json` 独占 OpenAPI 路由，扩展名即路由，不再做内容结构检测
- `question` 和 `answer` 为必填字段，不可为空白
- content 格式化为 `"## {question}\n\n{answer}"`，确保 question 参与 embedding 和 section 追踪
- JSONL 解析容忍记录间的空白行
- 单行 JSON 语法错误信息含物理行号（1-based），必填字段错误含 Q&A 序号（0-based，跳过空白行后编号）
- `tags` 和 `locale` 映射到现有 metadata 字段，`category` 忽略
- 无新增外部依赖
- 不涉及前端改动、数据库 schema 变更或新增 API 端点

---

## 9. 参考资料
- 用户故事：`docs/user-stories/01-user-user-stories.md`（US-CORE-032）
- 文档摄入 PRD：`docs/prd/document/document-ingestion.md`
- OpenAPI 文档导入 PRD：`docs/prd/document/support-openapi.md`
- 领域模型：`docs/prd/02-domain-model.md`
