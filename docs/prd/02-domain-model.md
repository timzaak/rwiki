# 领域模型 产品需求文档 (PRD)

**状态**: Draft
**创建时间**: 2026-05-30
**优先级**: P0
**权威范围**: document、page、chunk、status、metadata 的产品语义

---

## 1. 目的

本 PRD 统一知识库核心领域概念，避免同一概念在聊天、文档摄入、检索和基础设施 PRD 中重复定义。单个功能 PRD 只描述该能力对领域模型的使用和约束，不重新定义这些概念。

## 2. 核心概念

| 概念 | 权威定义 | 关键规则 |
|------|----------|----------|
| document | 一次上传形成的文件级记录 | 上传、列表、删除、发布、取消发布按 document 操作 |
| page | 可被检索的知识页面 | xlsx 每个有效 Wiki 行是一个 page；Markdown/MDX 每个文件是一个 page |
| chunk | page 的检索分块 | chunk 属于一个 page，并保留文件级 document 归属 |
| status | document 的生命周期状态 | `processing`、`draft`、`published`、`failed` |
| metadata | 辅助检索和引用的结构化信息 | `title`、`section`、`locale`、`link`、`tags` |

## 3. ID 语义

| 字段 | 层级 | 语义 | 用户可见性 |
|------|------|------|------------|
| `document_id` | 文件级 | 一次上传的 document ID | 文档 API 使用 |
| `page_id` | 知识页面级 | xlsx 行或 Markdown/MDX 文件对应的 page ID | 内部检索、分块、去重和邻居扩展使用 |
| `sub_index` | chunk 级 | 同一 page 内的 chunk 顺序 | 内部排序和窗口扩展使用 |
| `chunk_count` | page 级统计 | 同一 page 下 chunk 总数 | 内部窗口扩展使用 |

`row_index` 不再作为知识页面的权威领域标识。Excel 行号仅可用于错误提示或历史说明，不能作为 chunk 分组、去重、邻居扩展的当前规则。

## 4. Document 状态

| 状态 | 含义 | 可检索性 |
|------|------|----------|
| `processing` | 文件已接收，正在解析、分块或索引 | 不可检索 |
| `draft` | 索引完成但尚未发布 | 不可检索 |
| `published` | 已发布，可进入 RAG 检索 | 可检索 |
| `failed` | 解析、校验或索引失败 | 不可检索 |

状态转换规则由 `document/document-lifecycle.md` 维护。`indexed` 仅允许作为历史迁移背景出现，不是当前 document 状态。

## 5. Page 与 Chunk 规则

- 一个 document 可以包含一个或多个 page。
- xlsx document 中，每个有效 Wiki 行生成一个 page。
- Markdown/MDX document 中，一个文件生成一个 page。
- 一个 page 可以被切分为一个或多个 chunk。
- 邻居扩展、chunk_count 计算、去重和同页排序必须以 `page_id + sub_index` 为边界。
- 删除 document 时，必须删除该 document 下所有 page 的 chunks。
- 发布状态过滤按文件级 `document_id` 关联 document 状态，不按 `page_id` 判断发布状态。

## 6. Metadata 规则

| 字段 | 含义 | 来源 |
|------|------|------|
| `title` | 页面标题 | xlsx Title 列、Markdown frontmatter title、H1 或文件名 |
| `section` | chunk 所在章节 | Markdown 标题感知分块结果 |
| `locale` | 内容语言或地区标识 | xlsx Locale 列或 Markdown frontmatter locale |
| `link` | 原始页面或文档链接 | xlsx Link 列或 Markdown frontmatter link |
| `tags` | 主题标签 | xlsx Tags 列或 Markdown frontmatter tags |

metadata 不参与 embedding 正文计算，主要用于检索结果展示、LLM 上下文构建和来源引用。

## 7. 参考资料

- 产品总览：`docs/prd/01-product-overview.md`
- 文档生命周期：`docs/prd/document/document-lifecycle.md`
- 文档摄入：`docs/prd/document/document-ingestion.md`
- 文档检索与引用：`docs/prd/document/document-retrieval-and-citations.md`
