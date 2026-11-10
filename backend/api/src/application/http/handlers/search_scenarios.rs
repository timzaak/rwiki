//! Public search endpoint scenario tests (`POST /api/search`).
//!
//! Drives `create_api_routes` with oneshot requests against the public
//! channel-scoped search endpoint, covering its externally visible contract:
//! - Access: public — no credentials required, a Bearer header changes nothing
//! - Validation: empty query / missing-or-empty channelId / unconfigured
//!   channel / topK bounds (1..=20), exact public-chat wording
//! - Results: pure FTS keyword recall (no rewrite, no embedding, no rerank),
//!   page-granularity ordering (pages by best chunk score, chunks within a
//!   page in document order), zero-hit empty list (incl. lexically disjoint
//!   queries — no semantic recall by design), channel isolation, default
//!   topK cap, markdown syntax stripped from content for display (MCP keeps
//!   the raw chunk)
//! - Failure: FTS-internal failures degrade to empty results (never 500);
//!   no LLM/embedding provider is on this path
//! - Mode switch: everything above is the default `fts` mode;
//!   `[retrieval].public_search_mode = "hybrid"` routes the endpoint through
//!   the rewrite + hybrid pipeline instead — semantic recall for lexically
//!   disjoint queries, global empty-KB short-circuit, retrieval errors
//!   collapsed to a fixed generic 500
//!
//! Ordering determinism: the default mode is pure FTS (BM25, negated so
//! higher = better) + title-match bonus. Seeded FTS rows carry the raw English
//! content; queries use only terms present in every intended hit (FTS5
//! MATCH terms are implicit AND), and BM25 length normalization (equal tf,
//! shorter doc wins) fixes the cross-page order while stable page grouping
//! fixes the within-page order (sub_index). Hybrid-mode tests query with a
//! lexically disjoint marker so the keyword leg stays empty; the embeddings
//! mock always returns the unit vector e1 and seeded chunks carry hand-placed
//! embeddings, so the KNN order is fixed: b-sub1 (e1) > b-sub0 (mid) >
//! a-sub0 (e2).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

const SEARCH_TEST_API_TOKEN: &str = "search-test-api-token-12345";
const HIT_CHANNEL: &str = "help_center";
const NO_DOCS_CHANNEL: &str = "empty_channel";
const SECOND_CHANNEL: &str = "dev_docs";
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
/// Marker query with no lexical overlap to any seeded title/content; under
/// the default `fts` mode it recalls nothing (empty results by design). The
/// hybrid paths (the endpoint's `hybrid` mode and the MCP side of the
/// markdown test) recall via the embeddings mock (always e1).
const NO_MATCH_MARKER_QUERY: &str = "zzzzz-no-match-marker";
/// Markdown-syntax chunk seed (heading/emphasis/link/fenced code) for the
/// display-stripping scenario.
const MARKDOWN_CHUNK_SEED: &str = "# Setup Guide\n\nInstall via **npm** and see the [docs](https://example.com/docs).\n\n```bash\nnpm install rwiki\n```";

/// Ensure the sqlite-vec extension is registered globally so that the
/// `vec0` virtual table module is available for in-memory connections.
static SQLITE_VEC_INIT: Once = Once::new();
fn ensure_sqlite_vec_loaded() {
    SQLITE_VEC_INIT.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });
}

#[derive(Clone, Copy, PartialEq)]
enum SeedKind {
    /// No chunks at all — global empty knowledge base.
    Empty,
    /// Lexical seed: page-a (doc-a, "Alpha part") and page-b (doc-b,
    /// sub0 "Beta leading part", sub1 "Beta part"), all in HIT_CHANNEL.
    /// Query "part" hits all three; BM25 length normalization (equal tf,
    /// shorter doc wins) makes the 2-term chunks outrank the 3-term
    /// b-sub0, so page grouping must keep b-sub0 before the
    /// higher-scoring b-sub1 (document order within a page).
    DesignVector,
    /// Lexical seed in HIT_CHANNEL plus one separate page ("Gamma only
    /// part") in SECOND_CHANNEL, for channel isolation via query "part".
    DualChannel,
    /// 25 single-chunk pages ("Angle content {i}") for the default-topK
    /// cap: query "angle" hits all 25, FTS LIMIT keeps the first 20.
    TwentyFivePages,
    /// One markdown-syntax chunk (heading/emphasis/link/fenced code), for
    /// the display-stripping contract (HTTP hits lexically via "npm
    /// install"; MCP hybrid recalls via its e1 embedding).
    MarkdownChunk,
}

fn embedding_blob(values: &[f64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for &v in values {
        bytes.extend_from_slice(&(v as f32).to_le_bytes());
    }
    bytes
}

/// 1536-dim unit vector along axis 0 — what the embeddings mock returns for
/// every query.
fn e1() -> Vec<f64> {
    let mut v = vec![0.0f64; 1536];
    v[0] = 1.0;
    v
}

/// 1536-dim unit vector along axis 1 — orthogonal to e1 (least relevant).
fn e2() -> Vec<f64> {
    let mut v = vec![0.0f64; 1536];
    v[1] = 1.0;
    v
}

/// Unit vector midway between e1 and e2 (second most relevant to e1).
fn mid() -> Vec<f64> {
    let mut v = vec![0.0f64; 1536];
    let c = 1.0f64 / 2.0f64.sqrt();
    v[0] = c;
    v[1] = c;
    v
}

fn search_channels_config() -> rwiki_core::config::ChannelsConfig {
    let mut channels = HashMap::new();
    for id in [HIT_CHANNEL, NO_DOCS_CHANNEL, SECOND_CHANNEL] {
        channels.insert(
            id.to_string(),
            rwiki_core::config::ChannelConfig {
                name: format!("Channel {id}"),
                system_prompt: None,
                suggested_questions: None,
                content_language: None,
            },
        );
    }
    rwiki_core::config::ChannelsConfig { channels }
}

fn seed_document(conn: &mut rusqlite::Connection, doc_id: &str, file_name: &str, channel_id: &str) {
    conn.execute(
        "INSERT INTO documents (id, file_name, status, created_at, channel_id) \
         VALUES (?1, ?2, 'published', ?3, ?4)",
        rusqlite::params![doc_id, file_name, "2026-01-01T00:00:00.000Z", channel_id],
    )
    .expect("seed documents");
}

struct SeedChunkSpec<'a> {
    doc_id: &'a str,
    chunk_id: &'a str,
    page_id: &'a str,
    sub_index: i64,
    chunk_count: i64,
    title: &'a str,
    section: &'a str,
    content: &'a str,
    embedding: &'a [f64],
}

fn seed_chunk(conn: &mut rusqlite::Connection, spec: SeedChunkSpec<'_>) {
    let SeedChunkSpec {
        doc_id,
        chunk_id,
        page_id,
        sub_index,
        chunk_count,
        title,
        section,
        content,
        embedding,
    } = spec;
    conn.execute(
        "INSERT INTO chunk_metadata \
         (document_id, chunk_id, content, title, section, locale, link, page_id, sub_index, chunk_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            doc_id,
            chunk_id,
            content,
            title,
            section,
            "en",
            format!("https://example.com/{chunk_id}"),
            page_id,
            sub_index,
            chunk_count
        ],
    )
    .expect("seed chunk_metadata");
    let rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO vec_chunks (rowid, embedding) VALUES (?1, ?2)",
        rusqlite::params![rowid, embedding_blob(embedding)],
    )
    .expect("seed vec_chunks");
    // FTS 行：生产侧写入 jieba 分词结果，英文种子词间本以空格分隔，
    // unicode61 建索引时与原始文本等价，直接种 content 即可。
    conn.execute(
        "INSERT INTO fts_chunks (rowid, tokens) VALUES (?1, ?2)",
        rusqlite::params![rowid, content],
    )
    .expect("seed fts_chunks");
}

fn seed_design_vector_set(conn: &mut rusqlite::Connection, channel: &str) {
    seed_document(conn, "doc-a", "alpha.md", channel);
    seed_chunk(
        conn,
        SeedChunkSpec {
            doc_id: "doc-a",
            chunk_id: "a-sub0",
            page_id: "page-a",
            sub_index: 0,
            chunk_count: 1,
            title: "Alpha Page",
            section: "Alpha Section",
            content: "Alpha part",
            embedding: &e2(),
        },
    );
    seed_document(conn, "doc-b", "beta.md", channel);
    seed_chunk(
        conn,
        SeedChunkSpec {
            doc_id: "doc-b",
            chunk_id: "b-sub0",
            page_id: "page-b",
            sub_index: 0,
            chunk_count: 2,
            title: "Beta Page",
            section: "Beta Section",
            content: "Beta leading part",
            embedding: &mid(),
        },
    );
    seed_chunk(
        conn,
        SeedChunkSpec {
            doc_id: "doc-b",
            chunk_id: "b-sub1",
            page_id: "page-b",
            sub_index: 1,
            chunk_count: 2,
            title: "Beta Page",
            section: "Beta Section",
            content: "Beta part",
            embedding: &e1(),
        },
    );
}

/// Start an UNPOOLED mockito server and leak it for the test binary's
/// lifetime (same rationale as mcp_scenarios: pooled servers would hold one
/// of mockito's process-wide licenses and hang later tests).
async fn leak_unpooled_mock_server() -> &'static mut mockito::Server {
    Box::leak(Box::new(
        mockito::Server::new_with_opts_async(mockito::ServerOpts::default()).await,
    ))
}

/// Build the AppState for search scenario tests in the default (`fts`)
/// public search mode.
///
/// `max_context_chunks` overrides `retrieval.max_context_chunks` (default 12)
/// for the default-topK cap scenario. The embeddings and LLM completions
/// mocks exist only for the hybrid paths (the endpoint's `hybrid` mode, and
/// the markdown-stripping test mounting MCP `rwiki_search` as the
/// raw-markdown control); the default `fts` mode never calls either provider.
async fn search_app_state(seed: SeedKind, max_context_chunks: usize) -> Arc<AppState> {
    search_app_state_with_mode(
        seed,
        max_context_chunks,
        rwiki_core::config::PublicSearchMode::default(),
    )
    .await
}

/// Variant of `search_app_state` with an explicit
/// `retrieval.public_search_mode`, used to drive the endpoint's hybrid arm.
async fn search_app_state_with_mode(
    seed: SeedKind,
    max_context_chunks: usize,
    public_search_mode: rwiki_core::config::PublicSearchMode,
) -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");

    match seed {
        SeedKind::Empty => {}
        SeedKind::DesignVector => {
            seed_design_vector_set(&mut conn, HIT_CHANNEL);
        }
        SeedKind::DualChannel => {
            seed_design_vector_set(&mut conn, HIT_CHANNEL);
            seed_document(&mut conn, "doc-c", "gamma.md", SECOND_CHANNEL);
            seed_chunk(
                &mut conn,
                SeedChunkSpec {
                    doc_id: "doc-c",
                    chunk_id: "c-sub0",
                    page_id: "page-c",
                    sub_index: 0,
                    chunk_count: 1,
                    title: "Gamma Page",
                    section: "Gamma Section",
                    content: "Gamma only part",
                    embedding: &e1(),
                },
            );
        }
        SeedKind::TwentyFivePages => {
            seed_document(&mut conn, "doc-many", "many-pages.md", HIT_CHANNEL);
            // 25 independent pages; query "angle" hits all of them, the FTS
            // LIMIT (default topK cap) keeps 20.
            for i in 0..25 {
                let theta = i as f64 * 0.01;
                let mut embedding = vec![0.0f64; 1536];
                embedding[0] = theta.cos();
                embedding[1] = theta.sin();
                seed_chunk(
                    &mut conn,
                    SeedChunkSpec {
                        doc_id: "doc-many",
                        chunk_id: &format!("many-sub-{i:02}"),
                        page_id: &format!("page-{i:02}"),
                        sub_index: 0,
                        chunk_count: 1,
                        title: &format!("Angle Page {i:02}"),
                        section: "Angle Section",
                        content: &format!("Angle content {i:02}"),
                        embedding: &embedding,
                    },
                );
            }
        }
        SeedKind::MarkdownChunk => {
            seed_document(&mut conn, "doc-md", "setup-guide.md", HIT_CHANNEL);
            seed_chunk(
                &mut conn,
                SeedChunkSpec {
                    doc_id: "doc-md",
                    chunk_id: "md-sub0",
                    page_id: "page-md",
                    sub_index: 0,
                    chunk_count: 1,
                    title: "Setup Guide Page",
                    section: "Setup Section",
                    content: MARKDOWN_CHUNK_SEED,
                    embedding: &e1(),
                },
            );
        }
    }
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let embed_server = leak_unpooled_mock_server().await;
    let embed_body = serde_json::json!({
        "object": "list",
        "data": [{"object": "embedding", "embedding": e1(), "index": 0}],
        "model": "text-embedding-3-small",
        "usage": {"prompt_tokens": 1, "total_tokens": 1}
    })
    .to_string();
    let _embed_mock = embed_server
        .mock("POST", "/embeddings")
        .with_status(200)
        .with_body(&embed_body)
        .expect_at_most(50)
        .create_async()
        .await;

    let embedding_model = rwiki_core::infrastructure::rig_client::connect_at(
        "sk-test-fake-key-for-search-scenarios-only",
        embed_server.url(),
    )
    .embedding("text-embedding-3-small", None);
    let app_embedding_model =
        rwiki_core::infrastructure::embedding_model::AppEmbeddingModel::new(embedding_model);

    let vector_store = Arc::new(
        rwiki_core::infrastructure::vector_store::VectorStoreManager::new(
            sqlite.clone(),
            app_embedding_model,
            "text-embedding-3-small".to_string(),
        ),
    );

    // LLM mock serves the rewrite payload for the hybrid paths; the default
    // `fts` mode of /api/search has no LLM stage.
    let llm_server = leak_unpooled_mock_server().await;
    let _llm_mock = llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_body(chat_completion_body(
            "chatcmpl-search-test",
            "{\"queries\": [\"Rust systems programming\"]}",
        ))
        .expect_at_most(50)
        .create_async()
        .await;

    let llm_client =
        rwiki_core::infrastructure::rig_client::connect_at("sk-test-fake", llm_server.url());

    let retrieval_config = rwiki_core::config::RetrievalConfig {
        max_context_chunks,
        public_search_mode,
        ..rwiki_core::config::RetrievalConfig::default()
    };

    Arc::new(AppState {
        sqlite,
        enable_openapi: false,
        vector_store,
        chat_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        llm_client,
        llm_model: "test-model".to_string(),
        api_token: SEARCH_TEST_API_TOKEN.to_string(),
        api_allowed_ip_ranges: Vec::new(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        allowed_origins: vec![],
        retrieval_config,
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config: None,
        mcp_config: Some(rwiki_core::config::McpConfig::default()),
        channels_config: search_channels_config(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Build a public `POST /api/search` request. No Authorization header: the
/// endpoint is public; the token-carrying test adds the header explicitly.
fn search_request(body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/api/search")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build request")
}

fn search_body(query: &str, channel_id: &str) -> serde_json::Value {
    serde_json::json!({"query": query, "channelId": channel_id})
}

/// OpenAI chat.completion 响应体（rewrite mock 用）：两处 mock 共用同一 wire 格式。
fn chat_completion_body(id: &str, content: &str) -> String {
    serde_json::json!({
        "id": id,
        "object": "chat.completion",
        "created": 1700000000u64,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    })
    .to_string()
}

async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, String) {
    let resp = app.oneshot(req).await.expect("send request");
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn post_search(
    app: axum::Router,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let (status, text) = send(app, search_request(body)).await;
    let json = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("response must be JSON ({e}): {text}"));
    (status, json)
}

fn message_of(body: &serde_json::Value) -> &str {
    body.get("message").and_then(|m| m.as_str()).unwrap_or("")
}

// User Story: US-INTG-015 场景1 —— 无凭据检索成功，返回按相关性排序的片段，
// 排序粒度为 page：page 之间按页内最高分降序，页内保持文档顺序。
// Covers: 最终排序双向锁定——BM25 长度归一化（等词频下 2 词 chunk 分高于
//         3 词的 b-sub0）使 chunk 级平铺输出 [a-sub0, b-sub1, b-sub0]（页内
//         乱序、b-sub0 垫底）；无最终排序时输出 FTS 位置序同上。page 级
//         排序必须输出页内 sub_index 序。两种退化都让本用例失败。
#[tokio::test]
async fn search_orders_pages_by_relevance_keeping_sub_index_within_page() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    let (status, body) = post_search(app, search_body("part", HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "public search must return 200: {body}"
    );

    let results = body
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {body}"));
    assert_eq!(results.len(), 3, "all three seeded chunks return: {body}");

    let contents: Vec<&str> = results
        .iter()
        .filter_map(|r| r.get("content").and_then(|c| c.as_str()))
        .collect();
    assert_eq!(
        contents,
        vec!["Alpha part", "Beta leading part", "Beta part"],
        "page-a precedes page-b; within page-b sub0 precedes sub1 even though \
         sub1 scores higher (shorter doc, higher BM25): {body}"
    );

    for result in results {
        for field in [
            "title",
            "section",
            "content",
            "link",
            "locale",
            "score",
            "documentId",
        ] {
            assert!(
                result.get(field).is_some(),
                "every result must carry {field}: {body}"
            );
        }
    }
}

// User Story: US-INTG-015 场景1 / PRD §5.2 验收目标1 —— 携带任意凭据与不携带
// 凭据的响应逐字节一致。
// Covers: 公开端点不要求、不解析、不拒绝凭据；有人给该路由加鉴权中间件时失败。
#[tokio::test]
async fn search_with_bearer_token_returns_same_results() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    let (bare_status, bare_text) = send(
        app.clone(),
        search_request(search_body("alpha part", HIT_CHANNEL)),
    )
    .await;
    assert_eq!(bare_status, StatusCode::OK, "bare request must return 200");

    let mut req = search_request(search_body("alpha part", HIT_CHANNEL));
    req.headers_mut().insert(
        header::AUTHORIZATION,
        axum::http::HeaderValue::from_static("Bearer some-arbitrary-token"),
    );
    let (token_status, token_text) = send(app, req).await;
    assert_eq!(
        token_status,
        StatusCode::OK,
        "token request must return 200"
    );

    assert_eq!(
        bare_text, token_text,
        "a carried credential must not change the response byte for byte"
    );
}

// User Story: US-INTG-015 —— 查询缺失或为空白串时被拒绝，文案与 MCP 工具一致。
// Covers: 400 + 逐字 `查询不能为空`；DTO serde default 使字段缺失同样进入
//         handler 校验而非 422。
#[tokio::test]
async fn search_rejects_empty_query() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    for (label, body) in [
        (
            "empty string",
            serde_json::json!({"query": "", "channelId": HIT_CHANNEL}),
        ),
        (
            "whitespace only",
            serde_json::json!({"query": "   ", "channelId": HIT_CHANNEL}),
        ),
        (
            "field omitted",
            serde_json::json!({"channelId": HIT_CHANNEL}),
        ),
    ] {
        let (status, json) = post_search(app.clone(), body).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "empty query ({label}) must return 400: {json}"
        );
        assert_eq!(
            message_of(&json),
            "查询不能为空",
            "exact public wording required ({label}): {json}"
        );
    }
}

// User Story: US-INTG-015 场景4 / PRD §5.2 验收目标2 —— channelId 缺失或为空白
// 串时返回明确的 400 提示而非 422，不回退任何默认频道。
// Covers: DTO serde default + handler 复核；去掉 serde default 时失败。
#[tokio::test]
async fn search_rejects_missing_channel() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    for (label, body) in [
        ("field omitted", serde_json::json!({"query": "rust"})),
        (
            "whitespace only",
            serde_json::json!({"query": "rust", "channelId": "   "}),
        ),
    ] {
        let (status, json) = post_search(app.clone(), body).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "missing channelId ({label}) must return a clear 400, not 422: {json}"
        );
        assert_eq!(
            message_of(&json),
            "channelId 不能为空",
            "exact public wording required ({label}): {json}"
        );
    }
}

// User Story: US-INTG-015 场景4 —— 未配置频道被明确拒绝，不返回任何知识库内容。
// Covers: 400 + 逐字 `频道 ghost 未配置`；文案漂移或内容泄漏时失败。
#[tokio::test]
async fn search_rejects_unconfigured_channel() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    let (status, json) = post_search(app, search_body("rust", "ghost")).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unconfigured channel: {json}"
    );
    assert_eq!(
        message_of(&json),
        "频道 ghost 未配置",
        "exact public wording required: {json}"
    );
    assert!(
        !json.to_string().contains("Beta") && !json.to_string().contains("Alpha"),
        "no knowledge content may leak in the rejection: {json}"
    );
}

// User Story: US-INTG-015 场景2 —— 频道数据隔离：单次请求结构上不可见其他
// 频道内容。
// Covers: 双频道种子下，各频道结果的 documentId 只属于对应频道。
#[tokio::test]
async fn search_channel_isolation() {
    let state = search_app_state(SeedKind::DualChannel, 12).await;
    let app = create_api_routes(state);

    for (channel, allowed) in [
        (HIT_CHANNEL, ["doc-a", "doc-b"].as_slice()),
        (SECOND_CHANNEL, ["doc-c"].as_slice()),
    ] {
        let (status, json) = post_search(app.clone(), search_body("part", channel)).await;
        assert_eq!(status, StatusCode::OK, "search on {channel}: {json}");
        let results = json
            .get("results")
            .and_then(|r| r.as_array())
            .unwrap_or_else(|| panic!("results must be an array: {json}"));
        assert!(!results.is_empty(), "{channel} has seeded content: {json}");
        for result in results {
            let doc = result
                .get("documentId")
                .and_then(|d| d.as_str())
                .unwrap_or_default();
            assert!(
                allowed.contains(&doc),
                "results for {channel} must only contain that channel's documents {allowed:?}, got {doc}"
            );
        }
    }
}

// User Story: US-INTG-015 场景3 / US-CORE-041 场景2 —— 频道无已发布文档与检索
// 零命中统一返回空列表，不视为错误。
// Covers: 有人加「无可用文档」报错型预检查（503/400）时失败。
#[tokio::test]
async fn search_zero_hit_channel_returns_empty_results() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    let (status, json) = post_search(app, search_body("part", NO_DOCS_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "zero-hit channel must be 200, not 503/400: {json}"
    );
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {json}"));
    assert!(
        results.is_empty(),
        "zero-hit channel must return []: {json}"
    );
}

// User Story: US-CORE-041 场景2 —— 纯 FTS 口径下，
// 与文档词面不相交的查询（同义词、跨语言等语义查询）召回为零，返回空
// 列表而非错误，也不伪装出低相关结果。
// Covers: FTS-only 召回特性——有人在该端点加回 embedding/混合召回（词面
//         不相交却返回非空）或把零命中当错误（4xx/5xx）时失败。
#[tokio::test]
async fn search_lexically_disjoint_query_returns_empty_results() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    let (status, json) = post_search(app, search_body(NO_MATCH_MARKER_QUERY, HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "lexically disjoint query is a zero-hit 200, not an error: {json}"
    );
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {json}"));
    assert!(
        results.is_empty(),
        "no lexical overlap means no recall under the FTS-only contract: {json}"
    );
}

// User Story: US-INTG-015 —— 全局空库（尚无任何已索引 chunk）时返回空结果
// 而非 500。
// Covers: 空 FTS 索引优雅降级为零命中；有人把空库当错误（500/预检查报错）
//         时失败。
#[tokio::test]
async fn search_empty_knowledge_base_returns_empty_results() {
    let state = search_app_state(SeedKind::Empty, 12).await;
    let app = create_api_routes(state);

    let (status, json) = post_search(app, search_body("rust", HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "globally empty KB must short-circuit to 200 []: {json}"
    );
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {json}"));
    assert!(results.is_empty(), "empty KB returns no results: {json}");
}

// User Story: US-INTG-015 —— 显式越界 topK 被拒绝并指明参数；边界值 1 与 20
// 正常返回。负数与超出 u32 的整数同属越界（DTO 以 i64 承接，走 400 统一
// 文案而非框架 422）。
// Covers: topK 校验（1..=20），文案与 MCP 工具同构。
#[tokio::test]
async fn search_rejects_out_of_range_top_k() {
    let state = search_app_state(SeedKind::DesignVector, 12).await;
    let app = create_api_routes(state);

    for bad in [0i64, 21, -1, 4294967296] {
        let body = serde_json::json!({"query": "rust", "channelId": HIT_CHANNEL, "topK": bad});
        let (status, json) = post_search(app.clone(), body).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "topK={bad} must be 400: {json}"
        );
        assert!(
            message_of(&json).contains("topK"),
            "error must name the offending parameter: {json}"
        );
    }

    for good in [1i64, 20] {
        let body =
            serde_json::json!({"query": "alpha part", "channelId": HIT_CHANNEL, "topK": good});
        let (status, json) = post_search(app.clone(), body).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "boundary topK={good} must be 200: {json}"
        );
    }
}

// User Story: US-INTG-015 —— 不传 topK 时缺省值收敛到 [1, 20]，运维把
// max_context_chunks 调到越界值（>20 或 0）不会让缺省请求被拒。
// Covers: 缺省 = clamp(retrieval.max_context_chunks, 1, 20)；回到「缺省 =
//         原配置值」时两种配置都会 400，本用例失败。
#[tokio::test]
async fn search_default_top_k_is_capped_by_limit() {
    // max_context_chunks = 50 (> 20): 25 lexically matching pages seeded,
    // the default request must return exactly the capped 20 chunks.
    let state = search_app_state(SeedKind::TwentyFivePages, 50).await;
    let app = create_api_routes(state);
    let (status, json) = post_search(app, search_body("angle", HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "default topK must never 400 on config=50: {json}"
    );
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {json}"));
    assert_eq!(
        results.len(),
        20,
        "25 FTS hits, default topK capped at 20 even when max_context_chunks=50: {json}"
    );

    // max_context_chunks = 0: the clamp keeps the default request at 200.
    let state = search_app_state(SeedKind::DesignVector, 0).await;
    let app = create_api_routes(state);
    let (status, json) = post_search(app, search_body("alpha part", HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "default topK must never 400 on config=0: {json}"
    );
}

// 搜索摘要直读：HTTP 响应的 content 剥离 markdown 语法供前端直读；
// MCP rwiki_search 的消费者是 LLM，保留原始 markdown 结构。
// Covers: From<SearchResult> for SearchResultItem 漏掉剥离（裸 #/**/```
//         回归摘要）时 HTTP 断言失败；剥离被上移到共享检索编排（连 MCP
//         一起剥）时 MCP 断言失败。
#[tokio::test]
async fn search_strips_markdown_for_display_but_mcp_keeps_raw() {
    let state = search_app_state(SeedKind::MarkdownChunk, 12).await;
    let app = create_api_routes(state);

    // HTTP side: lexical query hits the chunk; content is plain text.
    let (status, http_json) =
        post_search(app.clone(), search_body("npm install", HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "HTTP search must succeed: {http_json}"
    );
    let http_content = http_json
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or_else(|| panic!("first result content: {http_json}"));
    assert_eq!(
        http_content, "Setup Guide Install via npm and see the docs. npm install rwiki",
        "HTTP content must be the stripped plain text: {http_json}"
    );

    // MCP side: initialize then rwiki_search keeps the raw markdown chunk.
    let entry = format!("/mcp/{HIT_CHANNEL}");
    let mcp_request = |method: &str, params: serde_json::Value, id: i64| {
        Request::builder()
            .method(Method::POST)
            .uri(&entry)
            .header(header::HOST, "localhost")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                }))
                .expect("serialize json-rpc"),
            ))
            .expect("build request")
    };

    let (status, body) = send(
        app.clone(),
        mcp_request(
            "initialize",
            serde_json::json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "search-strip-test", "version": "0.0.0"},
            }),
            1,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "MCP initialize must succeed: {body}"
    );

    let (status, body) = send(
        app,
        mcp_request(
            "tools/call",
            serde_json::json!({"name": "rwiki_search", "arguments": {
                "query": NO_MATCH_MARKER_QUERY, "channelId": HIT_CHANNEL
            }}),
            2,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "MCP search must succeed: {body}");

    let rpc: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|_| {
        // Streamable HTTP may answer SSE-style; take the first data line.
        body.lines()
            .find_map(|l| l.strip_prefix("data:"))
            .and_then(|d| serde_json::from_str(d.trim()).ok())
            .expect("parse JSON-RPC body")
    });
    let is_error = rpc
        .get("result")
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    assert!(!is_error, "MCP search must not error: {body}");
    let tool_text = rpc
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .expect("content[0].text");
    let payload: serde_json::Value =
        serde_json::from_str(tool_text).expect("SearchToolResult JSON");
    let mcp_content = payload
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .and_then(|r| r.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or_else(|| panic!("first MCP result content: {tool_text}"));
    assert_eq!(
        mcp_content, MARKDOWN_CHUNK_SEED,
        "MCP rwiki_search must keep the raw markdown chunk for LLM consumers"
    );
}

// `[retrieval].public_search_mode = "hybrid"` —— 运维切回混合检索后，与文档
// 词面不相交的语义查询重新可召回（向量路），这是选择 hybrid 的全部意义。
// Covers: 配置开关真正切换检索链路——handler 忽略配置（始终走 FTS）时该查询
//         零命中，本用例失败；与默认模式下同一查询断言空结果的
//         `search_lexically_disjoint_query_returns_empty_results` 成对锁定两种
//         模式的召回差异。排序断言同时锁定混合链路的 page 级最终排序：KNN 序
//         为 b-sub1 (e1) > b-sub0 (mid) > a-sub0 (e2)，退化为 chunk 级平铺会
//         输出 [b-sub1, b-sub0, a-sub0]（页内乱序）。
#[tokio::test]
async fn search_hybrid_mode_recalls_lexically_disjoint_query() {
    let state = search_app_state_with_mode(
        SeedKind::DesignVector,
        12,
        rwiki_core::config::PublicSearchMode::Hybrid,
    )
    .await;
    let app = create_api_routes(state);

    let (status, body) = post_search(app, search_body(NO_MATCH_MARKER_QUERY, HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "hybrid-mode search must return 200: {body}"
    );

    let contents: Vec<&str> = body
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {body}"))
        .iter()
        .filter_map(|r| r.get("content").and_then(|c| c.as_str()))
        .collect();
    assert_eq!(
        contents,
        vec!["Beta leading part", "Beta part", "Alpha part"],
        "hybrid mode recalls via the vector leg despite zero lexical overlap: page-b \
         (best chunk score) precedes page-a, within page-b sub0 precedes sub1: {body}"
    );
}

// hybrid 模式下全局空库（尚无任何已索引 chunk）返回空结果而非 500。
// Covers: 混合臂的全局空库短路——去掉短路时检索原语报错 → 500，本用例失败。
#[tokio::test]
async fn search_hybrid_mode_empty_knowledge_base_returns_empty_results() {
    let state = search_app_state_with_mode(
        SeedKind::Empty,
        12,
        rwiki_core::config::PublicSearchMode::Hybrid,
    )
    .await;
    let app = create_api_routes(state);

    let (status, json) = post_search(app, search_body("rust", HIT_CHANNEL)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "globally empty KB must short-circuit to 200 [] in hybrid mode: {json}"
    );
    let results = json
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {json}"));
    assert!(results.is_empty(), "empty KB returns no results: {json}");
}

// hybrid 模式下检索内部失败收敛为固定 500 文案，不外露底层错误细节——公开
// 端点的信息不泄漏口径在 hybrid 模式重新可达（fts 模式下检索失败降级为空
// 结果，不出 500）。
// Covers: 查询向量化失败路径；改回 e.to_string() 直出（body 出现
//         「查询向量化失败」或 mock 错误正文）时失败。
#[tokio::test]
async fn search_hybrid_mode_retrieval_failure_returns_generic_500() {
    let base = search_app_state_with_mode(
        SeedKind::DesignVector,
        12,
        rwiki_core::config::PublicSearchMode::Hybrid,
    )
    .await;

    let embed_server = leak_unpooled_mock_server().await;
    let _embed_mock = embed_server
        .mock("POST", "/embeddings")
        .with_status(500)
        .with_body("embedding provider exploded")
        .expect_at_most(50)
        .create_async()
        .await;
    let embedding_model = rwiki_core::infrastructure::rig_client::connect_at(
        "sk-test-fake-key-for-search-scenarios-only",
        embed_server.url(),
    )
    .embedding("text-embedding-3-small", None);
    let mut state = (*base).clone();
    state.vector_store = Arc::new(
        rwiki_core::infrastructure::vector_store::VectorStoreManager::new(
            state.sqlite.clone(),
            rwiki_core::infrastructure::embedding_model::AppEmbeddingModel::new(embedding_model),
            "text-embedding-3-small".to_string(),
        ),
    );
    let app = create_api_routes(Arc::new(state));

    let (status, text) = send(
        app,
        search_request(search_body(NO_MATCH_MARKER_QUERY, HIT_CHANNEL)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "retrieval failure is a 500: {text}"
    );
    let json: serde_json::Value = serde_json::from_str(&text).expect("error body is JSON");
    assert_eq!(
        message_of(&json),
        "知识库服务内部错误",
        "500 message must be the fixed generic wording: {text}"
    );
    assert!(
        !text.contains("查询向量化失败"),
        "internal error wording must not leak: {text}"
    );
    assert!(
        !text.contains("embedding provider exploded"),
        "upstream provider error text must not leak: {text}"
    );
}
