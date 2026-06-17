# Chat 上下文组装与 Prompt 格式优化 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-06-05
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/01-user-user-stories.md`。

### 1.1 相关故事

| ID | 标题 | 优先级 | 角色 | 影响说明 |
|----|------|--------|------|----------|
| US-CORE-008 | 聊天回答中查看来源引用 | P1 | User | 结构化上下文改善引用准确率和稳定性 |
| US-CORE-013 | AI 回答中包含 Link 和 Locale 元数据 | P1 | User | XML 子标签明确传递元数据，减少 LLM 遗漏 |

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P1 | 2 | 来源引用、元数据传递 |

---

## 2. 范围界定

### 2.1 包含功能

- **上下文块 XML 格式化**：`format_context_block()` 输出从纯文本改为 XML 标签格式，每个文档有明确的结构化边界
- **来源编号**：为每个上下文文档分配稳定编号（从 1 开始），便于 LLM 精确引用
- **Preamble 标签英文化**：`build_preamble()` 中的中文结构标签（"对话摘要"、"上下文"）改为英文
- **XML 转义**：对 title、section、link、locale、content 中的 XML 特殊字符进行确定性转义
- **默认系统提示词更新**：`DEFAULT_SYSTEM_PROMPT` 增加明确的引用格式指令
- **配置示例同步**：`config.example.toml` 中的系统提示词示例同步更新

### 2.2 不包含功能 (Out of Scope)

- 检索结果排序策略调整（如 sandwich 模式）
- Anthropic Citations API 集成
- 前端引用渲染方式变更
- 内部 prompt 英文化（`build_rewrite_prompt`、`build_compact_prompt`、`build_first_turn_rewrite_prompt`）— 可作为后续一致性改进
- API 接口变更
- 数据库 schema 变更

### 2.3 依赖项

- 现有 `format_context_block()` 和 `build_preamble()` 函数
- 现有 `SearchResult` 结构体（title、section、link、locale、tags、content）
- 现有 `ChatConfig::DEFAULT_SYSTEM_PROMPT` 和 `config.example.toml`
- 无新依赖需求

---

## 3. 需求概述

### 3.1 功能描述

优化 RAG 聊天上下文的组装方式。当前上下文块使用纯文本格式，缺乏明确的文档边界、来源编号和特殊字符处理，导致 LLM 引用准确率受限且存在结构被文档内容中特殊字符破坏的风险。本功能将上下文块改为 XML 标签格式并增加稳定编号，将 preamble 结构标签改为英文，更新默认系统提示词中的引用格式指令，并确保所有元数据和内容在进入 XML 前完成转义。

### 3.2 关键特性

- **XML 结构化上下文**：每个检索结果用 `<document>` 标签包裹，元数据用子标签承载，正文放入 `<content>` 边界内
- **稳定来源编号**：每个文档分配从 1 递增的 index，支持 `[Source N]` 格式引用
- **英文结构标签**：Preamble 使用 "Conversation Summary" 和 "Context" 替代中文标签
- **确定性转义**：所有进入 XML 的文本内容经过 `& < > " '` 转义，防止文档内容破坏 prompt 结构
- **引用指令增强**：默认系统提示词明确指定引用格式、链接处理和冲突来源的处理方式

---

## 4. 业务规则与状态

### 4.1 业务规则

- **上下文格式**：RAG 上下文使用 XML 标签格式（`<documents>` 包裹，`<document index="N">` 标识每个来源），格式变更只影响 LLM 输入，不影响前端展示和 API 契约
- **编号稳定性**：来源编号按检索结果顺序从 1 递增，与 `<document index="N">` 和系统提示词中的 `[Source N]` 引用格式保持一致
- **空元数据省略**：link 为空、locale 为空、section 为空时省略对应 XML 子标签，保持现有降级语义
- **转义为强制要求**：所有 title、section、link、locale、content 在拼入 XML 前必须经过转义，不可跳过
- **标签语言**：Preamble 中的结构标签统一使用英文（"Conversation Summary"、"Context"、"documents"、"document"、"content" 等）
- **系统提示词同步**：`DEFAULT_SYSTEM_PROMPT` 和 `config.example.toml` 中的引用指令保持一致

### 4.2 关键状态与异常

- **文档内容含特殊字符**：文档 title、section 或 content 包含 `&`、`<`、`>` 等字符时，转义函数确保不破坏 XML 结构
- **上下文 token 增加**：XML 标签格式比纯文本增加数百 token 开销；在 `token_budget = 8000` 下通常可接受，上线后通过 `context_chars` 观测确认

---

## 5. 功能需求

### 5.1 核心需求

1. **XML 格式化上下文块**
   - `format_context_block()` 改为输出 XML 标签格式
   - 每个 `<document>` 包含 `index` 属性和子标签：`<title>`、`<section>`（可选）、`<link>`（可选）、`<locale>`（可选）、`<content>`
   - 多个 `<document>` 包裹在 `<documents>` 根标签内
   - 空 link、空 locale、空 section 省略对应子标签

2. **来源编号**
   - 调用处从 `.map(format_context_block)` 改为 `.enumerate().map()`，传入从 1 开始的编号
   - 编号作为 `<document index="N">` 属性

3. **Preamble 标签英文化**
   - `build_preamble()` 中 "对话摘要" 改为 "Conversation Summary"
   - "上下文" 改为 "Context"

4. **XML 转义**
   - 新增确定性 XML escaping 函数，处理 `& < > " '`
   - 所有 title、section、link、locale、content 进入 XML 前经过转义

5. **默认系统提示词更新**
   - `DEFAULT_SYSTEM_PROMPT` 增加明确的引用格式指令
   - 引用格式使用 `[Source N]`，与 `<document index="N">` 编号一致
   - 包含链接引用规则和多来源冲突处理说明

6. **配置示例同步**
   - `config.example.toml` 中的系统提示词示例同步更新

### 5.2 验收目标

- 上下文块输出为合法 XML 结构，每个 `<document>` 有明确边界和编号
- 来源编号从 1 开始，顺序与检索结果一致
- title、section、link、locale、content 包含 `& < > " '` 时被正确转义
- 空 link、空 locale、空 section 省略对应 XML 子标签
- `build_preamble()` 不包含中文结构标签
- `DEFAULT_SYSTEM_PROMPT` 包含明确的引用格式指令
- `config.example.toml` 与 `DEFAULT_SYSTEM_PROMPT` 引用格式一致
- 所有现有单元测试和场景测试更新并通过

---

## 6. API 相关约束

**适用性**: 不适用

本功能不涉及 API 接口变更。聊天接口的请求格式、响应格式、SSE 事件结构保持不变。上下文组装格式优化为纯后端内部行为，对调用方完全透明。

---

## 7. 前端/交互约束

**适用性**: 不适用

本功能对前端完全透明。frontend 和 widget 均无需任何变更。引用准确率的提升由后端 prompt 格式优化实现。

---

## 8. 已确认决策

- **上下文格式**: XML 标签（`<documents>/<document index="N">` 结构），不使用 Markdown 或纯文本分隔符
- **标签语言**: Preamble 结构标签统一使用英文
- **来源编号**: 从 1 递增，引用格式 `[Source N]`
- **转义规则**: 确定性 XML escaping（`& < > " '`），不引入新依赖
- **默认提示词**: 增加引用格式指令和链接处理规则
- **排序策略**: 维持当前排序，不引入 sandwich 模式
- **内部 prompt**: rewrite/compact/first-turn-rewrite 的中文标签不在本次范围内
- **不引入新依赖**: 所有改动为字符串格式化逻辑变更
- **不改 API 契约**: 请求/响应/SSE 不变
- **不改前端**: widget 和 frontend 无变更
- **不改数据库**: 不引入 schema 变更
- **向后兼容**: 产品未上线，无需考虑兼容性

---

## 9. 参考资料

- 用户故事：`docs/user-stories/01-user-user-stories.md`（US-CORE-008、US-CORE-013）
- 上游 PRD：`docs/prd/chat/chat-assistant.md`（聊天助手基线）
- 上游 PRD：`docs/prd/core/configurable-system-prompt.md`（系统提示词配置）
- 关联 PRD：`docs/prd/document/document-retrieval-and-citations.md`（检索与引用规则）
- 关联 PRD：`docs/prd/chat/multi-turn-conversation-hybrid-memory.md`（对话记忆策略）
- 技术预研：`.ai/tech-research/chat-context-prompt-assembly.md`
- Anthropic Prompting Best Practices: https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/claude-prompting-best-practices
- RAG Prompt Engineering: https://mbrenndoerfer.com/writing/rag-prompt-engineering-context-citations
