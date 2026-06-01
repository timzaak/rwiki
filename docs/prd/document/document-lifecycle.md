# 文档生命周期 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-30
**优先级**: P0
**权威范围**: document 状态、发布/取消发布、删除、可检索性

---

## 1. 相关用户故事

| ID | 标题 | 影响说明 |
|----|------|----------|
| US-CORE-001 | 上传 xlsx 文件到知识库 | 上传后进入文档生命周期 |
| US-CORE-004 | 查看已上传文档列表 | 列表展示 document 当前状态 |
| US-CORE-009 | 管理文档发布状态 | 发布和取消发布控制 RAG 可检索性 |
| US-CORE-014 | 上传 Markdown/MDX 文件到知识库 | Markdown/MDX 上传后使用同一生命周期 |

## 2. 范围界定

包含：

- 文件级 document 生命周期状态。
- 发布、取消发布、删除对检索可见性的影响。
- 文档列表和文档管理 API 的状态契约。

不包含：

- 文件格式解析规则，见 `document/document-ingestion.md`。
- chunk 分块和检索窗口扩展，见 `document/document-retrieval-and-citations.md`。
- API Token 鉴权，见 `infrastructure/api-token-auth.md`。

## 3. 业务规则

- document 状态枚举为 `processing`、`draft`、`published`、`failed`。
- 上传被接收后，document 进入 `processing`。
- 解析、分块、索引成功后，document 进入 `draft`，默认不参与 RAG 检索。
- 用户发布 draft document 后，状态变为 `published`，其 chunks 可参与 RAG 检索。
- 用户取消发布 published document 后，状态回到 `draft`，其 chunks 不再参与 RAG 检索。
- 解析、校验或索引失败后，document 进入 `failed`，不可发布、不可检索。
- 删除 document 时，同步删除该 document 下所有 page/chunk 数据。
- RAG 检索只返回 `published` document 下的 chunks。

## 4. 状态转换

| 当前状态 | 触发 | 目标状态 |
|----------|------|----------|
| `processing` | 解析和索引成功 | `draft` |
| `processing` | 解析、校验或索引失败 | `failed` |
| `draft` | 用户发布 | `published` |
| `published` | 用户取消发布 | `draft` |

无效转换返回冲突错误。`indexed` 仅是历史状态名称，不是当前权威状态。

## 5. API 约束

- 文档列表必须返回每个 document 的当前状态。
- 仅 `draft` document 可发布。
- 仅 `published` document 可取消发布。
- `processing` 和 `failed` document 的发布/取消发布请求返回冲突错误。
- 删除 document 后，该 document 不应再出现在列表或检索结果中。

## 6. 验收目标

- 新上传文档成功索引后为 `draft`，不会被聊天检索。
- 发布后状态为 `published`，内容可被聊天检索和引用。
- 取消发布后状态为 `draft`，内容不再被聊天检索。
- 对 `processing` 或 `failed` 文档执行发布/取消发布返回错误。
- 删除文档后，其所有 page/chunk 均不可检索。

## 7. 参考资料

- 领域模型：`/docs/prd/02-domain-model.md`
- 文档摄入：`/docs/prd/document/document-ingestion.md`
- 检索与引用：`/docs/prd/document/document-retrieval-and-citations.md`
