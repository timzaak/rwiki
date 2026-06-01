# 产品总览 产品需求文档 (PRD)

**状态**: Draft
**创建时间**: 2026-05-30
**优先级**: P0
**权威范围**: 产品定位、用户角色、能力地图、全局非目标

---

## 1. 产品定位

本项目是集成 RAG + Agent 的知识库问答和语言调用服务 API。系统允许用户上传知识文档，将内容解析、分块、向量化并持久化；用户通过聊天接口提问时，系统检索相关知识片段并调用 OpenAI-compatible LLM 生成回答。

## 2. 用户角色

| 角色 | 说明 | 用户故事来源 |
|------|------|--------------|
| User | 直接使用知识库上传、管理和问答能力的用户 | [User 用户故事](/docs/user-stories/01-user-user-stories.md) |
| Website Integrator | 将聊天 Widget 嵌入外部网站的集成方 | [Website Integrator 用户故事](/docs/user-stories/02-website-integrator-user-stories.md) |

## 3. 核心能力地图

| 能力域 | 范围 | 主要 PRD |
|--------|------|----------|
| 文档摄入 | xlsx、Markdown/MDX 上传、解析、校验、metadata 提取 | `document/document-ingestion.md` |
| 文档生命周期 | processing、draft、published、failed 状态和可检索性 | `document/document-lifecycle.md` |
| 检索与引用 | 分块、窗口扩展、metadata 和来源引用 | `document/document-retrieval-and-citations.md` |
| 聊天体验 | RAG 问答、多轮对话、流式回答、聊天页面和弹窗 | `chat/chat-assistant.md`、`chat/multi-turn-conversation-hybrid-memory.md` |
| 外部集成 | 可嵌入 Chat Widget 的加载、生命周期和集成契约 | `integration/chat-widget-embeddable-js.md` |
| 基础设施 | SQLite 存储、Embedding Provider、LLM Provider、鉴权、可观测性 | `infrastructure/**` |
| 核心配置 | 系统提示词等跨能力行为配置 | `core/configurable-system-prompt.md` |

## 4. 全局非目标

- 多租户和多知识库隔离。
- 复杂角色权限、审批流和团队协作。
- 生产级跨数据库数据迁移工具。
- 文件版本管理、增量更新和内容 diff。
- 面向通用文件格式的全类型解析能力。
- 由 PRD 直接规定代码文件、模块结构或具体依赖实现。

## 5. 文档权威关系

- `docs/prd/**` 是正式产品需求源，描述用户价值、范围、业务规则、验收目标和用户可见契约。
- `docs/user-stories/**` 是用户故事和 Gherkin 验收标准源。
- `.ai/design/**` 是技术设计源，只描述如何实现，不覆盖 PRD 中的产品规则。
- `.ai/future/**` 是候选方案或迁移方案；采纳后必须并入正式 PRD 或设计，并归档。

## 6. 参考资料

- PRD 索引：[00-index.md](00-index.md)
- 领域模型：[02-domain-model.md](02-domain-model.md)
- 用户故事索引：[00-index.md](/docs/user-stories/00-index.md)
- 角色定义：[_roles.md](/docs/user-stories/_roles.md)
