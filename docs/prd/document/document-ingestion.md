# 文档摄入 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-30
**优先级**: P0
**权威范围**: xlsx、Markdown/MDX、`.json`（OpenAPI）、`.jsonl`（FAQ）上传、解析、校验、page 生成、metadata 提取

---

## 1. 相关用户故事

| ID | 标题 | 影响说明 |
|----|------|----------|
| US-CORE-001 | 上传 xlsx 文件到知识库 | xlsx 上传和索引 |
| US-CORE-007 | 上传结构化 Wiki xlsx 文件 | xlsx 结构化字段解析 |
| US-CORE-008 | 聊天回答中查看来源引用 | 摄入阶段产生引用 metadata |
| US-CORE-014 | 上传 Markdown/MDX 文件到知识库 | Markdown/MDX 直接上传 |

## 2. 范围界定

包含：

- 单一上传端点支持 `.xlsx`、`.md`、`.mdx`、`.json`（OpenAPI）、`.jsonl`（FAQ）。扩展名即路由，详细规则见各自 PRD。
- xlsx 按结构化 Wiki 表格解析。
- Markdown/MDX 按 UTF-8 文本解析，不执行、不编译组件。
- `document_id`、`page_id`、metadata 的生成和透传。
- 格式校验和用户可理解的错误返回。

不包含：

- document 发布状态，见 `document/document-lifecycle.md`。
- 检索窗口扩展和上下文格式，见 `document/document-retrieval-and-citations.md`。
- 前端上传页面或 UI 调整。
- 批量压缩包导入、增量更新、版本管理。

## 3. 统一摄入模型

- 一次上传生成一个文件级 `document_id`。
- 一个 document 可生成一个或多个 page。
- xlsx 每个有效 Wiki 行生成一个 page。
- Markdown/MDX 每个文件生成一个 page。
- 每个 page 使用独立 `page_id`，用于 chunk 分组、去重和邻居扩展。
- Excel 行号只用于错误提示，不作为领域 ID。
- 写入去重（自包含）：上传时始终为新文档的每个 chunk 建立独立的检索条目，即使内容与已发布（`published`）文档完全相同也不跳过；相同 `content_hash`（MD5）仅复用已有向量、不重复向量化。这样取消发布老文档后，未改动内容仍挂在新文档下、可被检索，刷新批次不丢失内容。`refresh_embed` 控制是否强制重新向量化（默认复用）。

## 4. 支持格式

| 格式 | page 规则 | metadata 来源 | 关键限制 |
|------|-----------|---------------|----------|
| `.xlsx` | 每个有效 Wiki 行一个 page | `Title`、`Locale`、`Link`、`Tags`、`Markdown Content` 列 | 必须包含 `Title` 和 `Markdown Content` |
| `.md` | 单文件一个 page | frontmatter、H1、文件名 | UTF-8 文本，正文不能为空 |
| `.mdx` | 单文件一个 page | frontmatter、H1、文件名 | 按原始文本摄入，不执行 import/export/JSX |
| `.json` (OpenAPI) | 每个 path+method 组合一个 page | `operationId`、`summary`、path、method | 必须是合法 OpenAPI 3.x JSON，paths 非空；详见 `document/support-openapi.md` |
| `.jsonl` (FAQ) | 每条 Q&A 一个 page | question（title）、tags、locale | JSON Lines 格式（每行一个 JSON 对象），question/answer 必填非空；详见 `core/faq_format_support.md` |

## 5. xlsx 业务规则

- 首行为表头，按列名提取字段，列顺序无关。
- 必填列：`Title`、`Markdown Content`。
- 可选列：`Locale`、`Link`、`Tags`。
- 每个有效数据行必须有非空 title 和 markdown content。
- `Tags` 使用逗号分隔，空值规范化为空数组。
- 任一行必填字段缺失时，拒绝整个文件并返回所有错误行。

## 6. Markdown/MDX 业务规则

- 文件必须是 UTF-8 文本；UTF-8 BOM 在解析前移除。
- 仅当文件第一行是独立 `---` 时识别 frontmatter。
- frontmatter 仅支持 `title`、`locale`、`link`、`tags` 单行 `key: value`。
- 未知 frontmatter 字段忽略；重复字段、未闭合 frontmatter、非法字段行返回 400。
- title 降级链：frontmatter title > 正文第一个 H1 > 文件名去扩展名。
- frontmatter 后正文为空时返回 400。
- `.mdx` 不执行、不编译、不渲染 JSX，也不从 JS/TS 表达式中提取 metadata。

## 7. Metadata 规则

- 统一 metadata 字段为 `title`、`section`、`locale`、`link`、`tags`。
- 摄入阶段负责 `title`、`locale`、`link`、`tags`。
- `section` 由后续标题感知分块产生。
- metadata 不写入 embedding 正文，不参与向量化。

## 8. API 约束

- 复用现有 multipart 上传接口，不新增上传端点。
- multipart 字段为 `file` 和可选的 `refresh_embed`（布尔值，控制是否复用已有 embedding，默认 false）。
- 不支持格式返回 400，并明确提示支持 `xlsx/md/mdx/json/jsonl`。
- 上传成功后返回文件级 document 记录。

## 9. 验收目标

- xlsx 文件按结构化列解析，每个有效行生成独立 page。
- 含 frontmatter 的 `.md` 文件上传成功，metadata 正确提取。
- 无 frontmatter 的 `.md` 文件上传成功，title 按 H1 或文件名推导。
- `.mdx` 文件按文本摄入，不执行或编译组件语法。
- 空文件、非 UTF-8、frontmatter 错误、xlsx 必填列/字段缺失时返回 400。
- xlsx、Markdown/MDX 产出的 page/chunk 进入同一后续分块和检索管道。

## 10. 参考资料

- 领域模型：`/docs/prd/02-domain-model.md`
- 文档生命周期：`/docs/prd/document/document-lifecycle.md`
- 检索与引用：`/docs/prd/document/document-retrieval-and-citations.md`
- OpenAPI JSON 导入：`/docs/prd/document/support-openapi.md`
- FAQ JSON 格式支持：`/docs/prd/core/faq_format_support.md`
