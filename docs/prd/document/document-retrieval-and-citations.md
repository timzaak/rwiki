# 文档检索与引用 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-30
**优先级**: P0
**权威范围**: 分块、窗口扩展、metadata 上下文、来源引用

---

## 1. 相关用户故事

| ID | 标题 | 影响说明 |
|----|------|----------|
| US-CORE-002 | 与知识库进行多轮对话 | 检索上下文质量影响回答质量 |
| US-CORE-008 | 聊天回答中查看来源引用 | 回答包含可追溯来源 |
| US-CORE-013 | AI 回答中包含 Link 和 Locale 元数据 | metadata 进入 LLM 上下文 |
| US-CORE-014 | 上传 Markdown/MDX 文件到知识库 | Markdown 内容按同一检索规则处理 |

## 2. 范围界定

包含：

- page 内容的标题感知分块。
- chunk 大小、`sub_index`、`chunk_count` 规则。
- 检索后的窗口扩展、去重、排序和 budget。
- title、section、locale、link、tags 在检索结果和 LLM 上下文中的使用。

不包含：

- 上传格式解析，见 `document/document-ingestion.md`。
- document 发布状态过滤规则，见 `document/document-lifecycle.md`。
- embedding provider 选择和向量维度，见 `infrastructure/model-providers.md`。

## 3. 分块规则

- 每个 page 按 Markdown 标题边界优先分块。
- 单个标题段落过长时，按段落或字符数二次分割。
- 默认分块字符上限为 1600。
- 每个 chunk 保留 `page_id`、`document_id`、`sub_index`、`chunk_count` 和 metadata。
- `section` 记录 chunk 所在的最近标题；无标题时为空。
- metadata 不参与 embedding 正文计算。

## 4. 检索规则

- 向量搜索只返回 `published` document 下的 chunks。
- 初始向量搜索返回 seed chunks。
- 对每个 seed chunk，在同一 `page_id` 内按 `sub_index` 做窗口扩展。
- 窗口扩展不得跨越 `page_id`。
- 多个 seed chunk 的重叠结果按 `(page_id, sub_index)` 去重。
- 最终上下文按同一 page 内 `sub_index` 顺序稳定排列；跨 page 顺序可按检索分数或稳定排序策略处理。
- 初始参数：`top_k = 5`、`window_size = 1`、`max_chunks_per_page = 3`、`max_total_context_chunks = 12`。

## 5. Metadata 与引用规则

- LLM 上下文必须包含每条检索结果的 `title`、`section`、`link`、`locale`。
- `link` 为空时不展示链接；`locale` 为空时不标注语言。
- `tags` 保留在 metadata 中；是否进入 LLM 上下文由具体回答策略决定，当前不作为检索过滤维度。
- 回答应优先引用 title、section 和 link，帮助用户追溯来源。
- 无 metadata 或旧数据缺字段时，检索和回答不能失败，只降级展示可用字段。

## 6. API 约束

- 本能力不新增对外 API 端点。
- Chat API 的 SSE 契约保持不变。
- 来源引用以内嵌回答文本体现，前端无需特殊渲染。

## 7. 验收目标

- 长文 page 被拆成多个 chunk 时，检索命中任一 chunk 后可扩展相邻上下文。
- 窗口扩展不跨 page，不把其他 xlsx 行或其他 Markdown 文件内容混入同一上下文。
- 多个 seed chunk 命中同一 page 时，扩展结果不重复。
- 扩展后上下文不超过 `max_total_context_chunks`。
- 回答上下文包含 title、section、link、locale；缺失字段时正常降级。
- 发布状态过滤生效，draft/processing/failed 文档不可被检索。

## 8. 参考资料

- 领域模型：`/docs/prd/02-domain-model.md`
- 文档摄入：`/docs/prd/document/document-ingestion.md`
- 文档生命周期：`/docs/prd/document/document-lifecycle.md`
- 模型 Provider：`/docs/prd/infrastructure/model-providers.md`
