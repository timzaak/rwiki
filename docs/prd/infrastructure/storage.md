# 存储与持久化 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-30
**优先级**: P1
**权威范围**: SQLite 存储形态、向量持久化、启动恢复、维度兼容性

---

## 1. 范围界定

包含：

- 单 SQLite 文件存储文档元数据和向量数据。
- sqlite-vec 向量持久化和 KNN 搜索。
- 应用启动时 migration、扩展注册、状态恢复和维度检查。
- Docker/本地部署的数据持久化边界。

不包含：

- embedding/LLM provider 配置，见 `infrastructure/model-providers.md`。
- API Token 鉴权，见 `infrastructure/api-token-auth.md`。
- tracing/observability，见 `infrastructure/observability.md`。
- 跨数据库生产迁移工具。

## 2. 当前存储模型

- 使用单个 SQLite 文件作为后端唯一持久化存储。
- 文档元数据、chunk metadata、向量数据由同一 SQLite 存储管理。
- 向量搜索使用 sqlite-vec。
- SQLite 文件路径通过配置指定，默认 `data/rwiki.db`。
- 文件不存在时自动创建目录和数据库。
- 启用 WAL 模式支持并发读写。

## 3. 业务规则

- 文档元数据和向量数据必须保持同一 document 生命周期。
- 删除 document 时，必须删除该 document 下所有 page/chunk 和向量数据。
- 向量维度由当前 embedding 模型配置决定。
- 应用启动时必须检查 sqlite-vec 表维度与当前 embedding 维度是否一致。
- 维度不匹配时拒绝启动，并提示重建索引或删除数据库。
- 启动时应处理遗留 `processing` 文档，避免永久卡住。

## 4. 异常与恢复

| 场景 | 行为 |
|------|------|
| SQLite 文件不存在 | 自动创建并执行 migration |
| SQLite 文件损坏 | 启动失败，提示修复或删除数据库 |
| 磁盘空间不足 | 写入失败并返回明确错误 |
| 向量维度不匹配 | 启动失败，要求重建向量数据 |
| 上次崩溃遗留 `processing` | 启动时标记为 `failed` |

## 5. 验收目标

- 无 PostgreSQL、Redis 等外部进程时，应用可完成上传、索引、检索和聊天。
- 上传文档后重启应用，无需重新上传即可检索。
- 删除 document 后，相关 metadata 和向量数据均被删除。
- 维度配置不匹配时应用拒绝启动，不静默返回错误检索结果。
- SQLite 文件通过 Docker Volume 挂载后，容器重启数据保留。

## 6. 参考资料

- 领域模型：`/docs/prd/02-domain-model.md`
- 文档生命周期：`/docs/prd/document/document-lifecycle.md`
- 模型 Provider：`/docs/prd/infrastructure/model-providers.md`
