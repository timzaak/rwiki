# RWiki

**Knowledge base Q&A powered by RAG. Single binary, zero external databases.**

Upload Markdown, XLSX, or OpenAPI specs — RWiki chunks and vectorizes them, then serves streaming answers with structured citations. Hybrid search (keyword + vector), query rewrite, and pluggable embedding providers built in. Runs on SQLite, deploys with one command, works with any OpenAI-compatible LLM.

[中文文档](README.zh-CN.md)

![Knowledge Card](docs-web/public/knowledge-card-en.png)

## Quick Start

Copy `backend/config/config.example.toml` to `config.toml`, set your LLM key in `[llm].api_key`, and uncomment `static_dir = "/app/static"` (without it the web UI is not served), then:

```bash
docker run -d -p 8080:8080 \
  -v rwiki-data:/app/data \
  -v ./config.toml:/app/config.toml:ro \
  -e OPENAI_API_KEY=your-embedding-key \
  ghcr.io/timzaak/rwiki
```

`OPENAI_API_KEY` sets the embedding key. Open `http://localhost:8080`, upload a document, publish, start chatting.

Or try the demo (requires Docker, Rust, and Node.js; Python standard library only):

```bash
cp backend/config/demo-config-bailian.toml.example backend/config/demo.toml  # then set API keys
cd scripts && python demo-start.py
```

## Why RWiki

Most RAG setups need PostgreSQL + pgvector, Redis, a vector database, and a Docker Compose file with 5 services. For teams that just want "upload docs, ask questions," that's overkill.

RWiki does one thing — knowledge base Q&A — and keeps the infrastructure to a single binary with SQLite.

| | RWiki | Typical RAG Stack |
|---|---|---|
| Database | SQLite (built-in) | PostgreSQL + pgvector |
| Dependencies | None | Redis, vector DB, message queue |
| Deployment | Single binary | Docker Compose, 3–5 services |
| Setup | `docker pull` and run | Hours of configuration |

## Features

- **Streaming chat Q&A** — Ask questions, get answers with structured citations (title, section, link, tags) from your documents
- **Hybrid search** — FTS5 full-text + vector similarity with RRF fusion for better recall
- **Rerank (optional)** — Semantically re-scores retrieved chunks after hybrid search to sharpen precision. Supports OpenRouter, Zhipu BigModel, and Alibaba DashScope. Opt-in via a `[rerank]` section (zero overhead when omitted); auto-degrades to fusion results on failure.
- **Query rewrite & expansion** — Automatic query rewriting with multi-query expansion to handle ambiguous questions
- **Embeddable chat widget** — Single JS file, Shadow DOM, add to any site with two lines of HTML
- **MCP server (optional)** — Expose knowledge Q&A and retrieval as read-only MCP tools (`rwiki_qa`, `rwiki_search`) for Claude Code, Cursor, and other agent clients over Streamable HTTP. Opt-in via an `[mcp]` section (route not mounted when omitted)
- **Multi-format ingestion** — Markdown files, XLSX spreadsheets, OpenAPI specifications
- **API documentation assistant** — Upload OpenAPI specs, ask questions about your APIs
- **Provider-agnostic** — OpenAI, OpenRouter, BigModel, any OpenAI-compatible endpoint
- **Flexible embeddings** — OpenAI, BigModel, DashScope, Google Gemini, or any OpenAI-compatible endpoint (an embedding API key is required)
- **RAG evaluation** — Built-in eval endpoint returns retrieval traces and reference answers ready for evaluators such as Ragas / DeepEval / RAGChecker to score (HitRate, MRR, Recall, answer quality)
- **Observability** — OpenTelemetry / Jaeger tracing support for production monitoring
- **Configurable** — Custom system prompts, content language settings, and conversation memory tuning

## Deploy

### Docker (recommended)

```bash
docker pull ghcr.io/timzaak/rwiki
docker run -d -p 8080:8080 \
  -v rwiki-data:/app/data \
  -v ./config.toml:/app/config.toml:ro \
  -e OPENAI_API_KEY=your-embedding-key \
  ghcr.io/timzaak/rwiki
```

### From Source

Prerequisites: Rust (latest stable), Node.js 20+, an OpenAI-compatible embedding API key.

```bash
git clone https://github.com/timzaak/rwiki
cd rwiki/backend
cp config/config.example.toml config/config.toml
# Edit config.toml — set your API keys
cargo run
```

The backend serves the API on `http://localhost:8080`. For the web UI, start the frontend dev server in a second terminal:

```bash
cd rwiki/frontend
npm install
npm run dev   # http://localhost:3000, proxies /api to the backend
```

## Configuration

Copy `backend/config/config.example.toml` to `config.toml` and edit. All options with comments are documented there. For Docker deployments, uncomment `static_dir = "/app/static"` so the web UI and chat widget are served.

### MCP Server (optional)

Expose the knowledge base as read-only MCP tools (`rwiki_qa`, `rwiki_search`) for agent clients such as Claude Code and Cursor. Add an `[mcp]` section to `config.toml` to enable (omitting it keeps `/mcp` unmounted); auth reuses the `[api]` token and IP allowlist.

```bash
claude mcp add --transport http rwiki http://localhost:8080/mcp \
  --header "Authorization: Bearer <API_TOKEN>"
```

## AI-Built

This project is built entirely with [Claude Code](https://docs.anthropic.com/en/docs/claude-code) + [web-dev-skills](https://github.com/timzaak/web-dev-skills).

```bash
git clone https://github.com/timzaak/web-dev-skills
claude --plugin-dir /path/to/web-dev-skills
```

## License

[Apache License 2.0](LICENSE)
