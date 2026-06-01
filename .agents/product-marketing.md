# Product Marketing Context

*Last updated: 2026-06-01*

## Product Overview

**One-liner:** RWiki is a lightweight, self-hosted knowledge base Q&A platform powered by RAG and Agent technology.

**What it does:** Upload documents (Markdown, XLSX, OpenAPI), RWiki automatically chunks and vectorizes them, then users ask questions via streaming chat. Single binary + SQLite, zero external dependencies.

**Product category:** Knowledge base / RAG platform / Self-hosted AI assistant

**Product type:** Open-source self-hosted software (Apache 2.0)

**Business model:** Open source, no pricing tiers currently.

## Target Audience

**Target companies:** Small-to-mid tech teams, startups, indie developers who need internal knowledge Q&A without infrastructure overhead.

**Decision-makers:** Developers, tech leads, engineering managers.

**Primary use case:** Turn existing documentation into an AI-powered Q&A system that teams can chat with — without setting up PostgreSQL, Redis, or any external database.

**Jobs to be done:**
- "I have docs scattered across files and wikis; I want one place to ask questions and get sourced answers."
- "I want to add an AI chat assistant to my product docs site without building RAG from scratch."
- "I need a knowledge base that runs on a single server with minimal ops."

**Use cases:**
- Internal team knowledge Q&A
- Product documentation chatbot (embeddable widget)
- API documentation assistant (OpenAPI upload support)
- Customer support knowledge base

## Personas

| Persona | Cares about | Challenge | Value we promise |
|---------|-------------|-----------|------------------|
| Developer (user) | Easy setup, clean API, no ops burden | RAG systems are complex to build and deploy | Single binary, SQLite, REST API, 5-min deploy |
| Tech Lead (champion) | Team adoption, low maintenance | Doesn't want another Postgres/Redis dependency to manage | Zero external DB — just run it |
| Engineering Manager (decision maker) | ROI, time-to-value | Knowledge silos slow the team down | Upload docs → publish → ask questions, same day |

## Problems & Pain Points

**Core problem:** Setting up a RAG-based knowledge Q&A system typically requires PostgreSQL + pgvector, Redis, vector DB, and significant infrastructure work. For small teams, this is overkill.

**Why alternatives fall short:**
- 第三方和官方平台的 AI 方案（Notion AI、飞书、语雀）都是云托管，数据不在自己手里，也没法嵌入自己的站点。
- 开源方案（MaxKB、RagFlow、DocsGPT）虽然能自部署，但一上来就要 PostgreSQL + Redis + 向量数据库，部署成本高。
- 工作流平台（Dify、FastGPT）功能很强，但对"只想给文档加个 AI 问答"的需求来说太重了。

**What it costs them:** Hours of infrastructure setup, ongoing DB maintenance, vendor lock-in with cloud RAG services.

**Emotional tension:** "第三方平台上有不少 AI 功能，但我想做得更简单，数据在自己手里，部署轻量。"

## Competitive Landscape

RWiki 不和工作流平台竞争。它只做一件事：文档 AI 问答，做到最轻量。

**同方向（文档/知识 AI 问答）：**
- **MaxKB / RagFlow** — 功能更全，但需要 PostgreSQL + Redis + Elasticsearch 等外部依赖，部署重。
- **DocsGPT** — 开源文档问答，架构相对简单，但需要 MongoDB +向量数据库，非单二进制。
- **各平台内置 AI（Notion AI、飞书智能问答、语雀 AI）** — 云端托管，数据不在自己手里，不支持自定义 LLM 或嵌入到自己的站点。

**不同方向（工作流/Agent 编排）：**
- Dify / FastGPT / Coze — 这些是工作流平台，目标是让用户编排复杂的 AI Agent 流程。RWiki 不做这个。如果用户需要工作流，应该用这些工具。

## Differentiation

**Key differentiators:**
- SQLite-only — no external database required. Single binary deployment.
- Embeddable chat widget — single JS file, Shadow DOM, drop into any site.
- AI-built — the entire project is developed with AI (Claude Code + web-dev-skills), demonstrating the capabilities it provides.
- OpenAPI support — upload API specs directly for API documentation Q&A.
- Provider-agnostic — works with any OpenAI-compatible LLM/embedding provider.

**How we do it differently:** Ship as a single Rust binary with embedded SQLite. No Docker Compose with 5 services. No managed cloud requirement. Upload docs, publish, chat.

**Why that's better:** 5-minute setup vs. 5-hour infrastructure wrangling. One `docker run` command. Zero database administration.

**Why customers choose us:** They want a knowledge Q&A system that "just works" on a single server without becoming an infrastructure project.

## Objections

| Objection | Response |
|-----------|----------|
| "SQLite 能不能扛住？" | SQLite 并发读性能足够。适合中小团队（~50 并发以内）。如果需要更大规模，那是另一个产品类别了。 |
| "我需要工作流/Agent 编排" | RWiki 只做文档问答，不做工作流。需要工作流用 Dify/FastGPT。 |
| "AI 全自动开发的项目靠谱吗？" | Apache 2.0 开源，有 E2E 测试、健康检查、Docker 支持。已在文档站点上生产使用。 |

**Anti-persona:** Enterprise teams needing multi-tenant, RBAC, audit logs, or 1000+ concurrent users. Teams that need workflow/agent orchestration beyond Q&A. Teams that require managed cloud hosting.

## Switching Dynamics

**Push:** 第三方平台的 AI 方案不够自主（数据在云端），开源方案又太重（一堆外部依赖）。"我只想简单本地部署一个文档问答。"

**Pull:** Single binary, SQLite, one command to deploy. Clean API with streaming chat. Embeddable widget for any site.

**Habit:** Teams already invested in Confluence/Notion AI or their own RAG pipeline may not see the need to switch.

**Anxiety:** "Can SQLite really handle my workload?" "Is an AI-built project reliable?" "What if I outgrow it?"

## Customer Language

**How they describe the problem:**
- "现在第三方或者官方平台上有很多 AI 方案，我想做得更简单一些"
- "我只想给文档加个 AI 问答，不想搭一整套基础设施"
- "想尽量实现轻量的本地化部署"

**How they describe us:**
- "A lightweight RAG knowledge base that just uses SQLite"
- "Drop-in chat widget for documentation"
- "Self-hosted, single binary"

**Words to use:** 轻量, 本地化部署, 单二进制, SQLite, 零外部依赖, 嵌入式, 自托管, provider-agnostic

**Words to avoid:** 工作流, 编排, 企业级, 平台 (暗示重量级), framework

**Glossary:**

| Term | Meaning |
|------|---------|
| RAG | Retrieval-Augmented Generation — search docs first, then let LLM answer with context |
| Agent | LLM-powered assistant that can call tools/APIs |
| Widget | Embeddable chat component (rwiki-chat.js) |
| Publish | Make a document's chunks available for search and Q&A |

## Brand Voice

**Tone:** Technical but approachable. Direct, no fluff.

**Style:** Concise, developer-to-developer. Show, don't tell. Code examples over descriptions.

**Personality:** Lightweight, pragmatic, honest, developer-friendly.

## Proof Points

**Metrics:** Single binary ~20MB. Docker image ~50MB. Setup time: 5 minutes. Zero external databases.

**Customers:** Self-dogfooding — docs-web (the documentation site) embeds the RWiki chat widget.

**Testimonials:** *(None yet — collect from early users)*

**Value themes:**

| Theme | Proof |
|-------|-------|
| Lightweight | SQLite-only, single binary, Alpine Docker image |
| Easy integration | Single JS file widget, Shadow DOM, 2-line HTML integration |
| AI-built | Entire codebase developed with Claude Code + web-dev-skills |
| Provider freedom | Works with OpenAI, OpenRouter, BigModel, any OpenAI-compatible API |

## Goals

**Business goal:** 让需要"文档 AI 问答"的团队知道：不需要复杂基础设施，RWiki 一个二进制 + SQLite 就够了。

**Conversion action:** Star on GitHub, deploy with Docker, try the demo.

**Current metrics:** *(Not tracked yet — add when available)*
