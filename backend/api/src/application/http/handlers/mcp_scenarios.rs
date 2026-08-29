//! MCP endpoint scenario tests.
//!
//! Drives `create_api_routes` with oneshot requests against `POST /mcp`,
//! covering the full externally visible contract of the MCP server:
//! - Auth: 401 without/with invalid Bearer token (same boundary as doc routes)
//! - Toggle: `[mcp]` section absent → 404 (route not mounted)
//! - Protocol: initialize handshake, tools/list discovery, tools/call dispatch
//! - Tools: rwiki_qa / rwiki_search happy paths, business errors (isError),
//!   topK bounds, empty results, per-call independence (stateless server)

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

const MCP_TEST_API_TOKEN: &str = "mcp-test-api-token-12345";
const HIT_CHANNEL: &str = "help_center";
const NO_DOCS_CHANNEL: &str = "empty_channel";
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

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

fn mcp_test_channels_config() -> rwiki_core::config::ChannelsConfig {
    let mut channels = HashMap::new();
    for id in [HIT_CHANNEL, NO_DOCS_CHANNEL] {
        channels.insert(
            id.to_string(),
            rwiki_core::config::ChannelConfig {
                name: format!("Channel {id}"),
                system_prompt: None,
                suggested_questions: None,
            },
        );
    }
    rwiki_core::config::ChannelsConfig { channels }
}

/// Build an AppState for MCP scenario tests.
///
/// - `mcp_enabled` controls the `[mcp]` section-presence toggle.
/// - `seed_published` inserts a published document with a retrievable chunk
///   (chunk_metadata + vec_chunks dummy embedding) into `HIT_CHANNEL`, so the
///   vector KNN path returns it offline; `NO_DOCS_CHANNEL` stays document-less.
///
/// Both the embeddings endpoint and the LLM completions endpoint are mocked
/// with mockito so the full pipeline (rewrite → retrieval → generation) runs
/// offline: embeddings return a 1536-dim zero vector; completions return a
/// rewrite-JSON payload that doubles as the generated answer text.
async fn mcp_app_state(mcp_enabled: bool, seed_published: bool) -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");

    if seed_published {
        conn.execute(
            "INSERT INTO documents (id, file_name, status, created_at, channel_id) \
             VALUES (?1, ?2, 'published', ?3, ?4)",
            rusqlite::params![
                "doc-mcp-001",
                "mcp-seed.md",
                "2026-01-01T00:00:00.000Z",
                HIT_CHANNEL
            ],
        )
        .expect("seed documents");
        conn.execute(
            "INSERT INTO chunk_metadata (document_id, chunk_id, content, title) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                "doc-mcp-001",
                "chunk-mcp-001",
                "Rust is a systems programming language.",
                "Rust Introduction"
            ],
        )
        .expect("seed chunk_metadata");
        let rowid = conn.last_insert_rowid();
        // 1536-dim zero embedding (blob), matching text-embedding-3-small.
        let dummy_embedding = vec![0u8; 1536 * 4];
        conn.execute(
            "INSERT INTO vec_chunks (rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![rowid, dummy_embedding],
        )
        .expect("seed vec_chunks");
    }
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // Mockito embeddings mock so query embedding works offline.
    let embed_server = Box::leak(Box::new(mockito::Server::new_async().await));
    let embed_body = serde_json::json!({
        "object": "list",
        "data": [{"object": "embedding", "embedding": vec![0.0f64; 1536], "index": 0}],
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

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-mcp-scenarios-only")
        .base_url(embed_server.url());
    let embedding_model = openai_client
        .build()
        .expect("build openai client")
        .embedding_model("text-embedding-3-small");
    let app_embedding_model =
        rwiki_core::infrastructure::embedding_model::AppEmbeddingModel::new(embedding_model);

    let vector_store = Arc::new(
        rwiki_core::infrastructure::vector_store::VectorStoreManager::new(
            sqlite.clone(),
            app_embedding_model,
            "text-embedding-3-small".to_string(),
        ),
    );

    // Mockito LLM mock: serves the rewrite payload (JSON with queries) for the
    // rewrite stage and doubles as the non-streaming generation answer.
    let llm_server = Box::leak(Box::new(mockito::Server::new_async().await));
    let _llm_mock = llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "id": "chatcmpl-mcp-test",
                "object": "chat.completion",
                "created": 1700000000u64,
                "model": "test-model",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "{\"queries\": [\"Rust systems programming\"]}"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
            })
            .to_string(),
        )
        .expect_at_most(50)
        .create_async()
        .await;

    let llm_client = rig::providers::openai::CompletionsClient::builder()
        .api_key("sk-test-fake")
        .base_url(llm_server.url())
        .build()
        .expect("build LLM client");

    Arc::new(AppState {
        sqlite,
        enable_openapi: false,
        vector_store,
        chat_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        llm_client,
        llm_model: "test-model".to_string(),
        api_token: MCP_TEST_API_TOKEN.to_string(),
        api_allowed_ip_ranges: Vec::new(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        allowed_origins: vec![],
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config: None,
        mcp_config: mcp_enabled.then(rwiki_core::config::McpConfig::default),
        channels_config: mcp_test_channels_config(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Build a JSON-RPC 2.0 POST /mcp request with the MCP-required Accept header.
///
/// The Host header is set explicitly because the Streamable HTTP service
/// rejects requests without one (oneshot-built requests carry no Host).
fn mcp_jsonrpc_request(method: &str, params: serde_json::Value, id: i64) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header(header::HOST, "localhost")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {MCP_TEST_API_TOKEN}"),
        )
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
}

fn initialize_params() -> serde_json::Value {
    serde_json::json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {},
        "clientInfo": {"name": "mcp-scenario-test", "version": "0.0.0"},
    })
}

fn tools_call_params(tool: &str, arguments: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"name": tool, "arguments": arguments})
}

/// Send a request through the app and read the full body as text.
async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, String) {
    let resp = app.oneshot(req).await.expect("send request");
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// Parse a JSON-RPC response body into a serde Value.
///
/// The Streamable HTTP server may answer with a plain `application/json`
/// body or an SSE `text/event-stream` body; both carry the same JSON-RPC
/// envelope, so unwrap whichever shape arrives.
fn parse_rpc_body(body: &str) -> serde_json::Value {
    for line in body.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                if v.get("id").is_some() || v.get("result").is_some() || v.get("error").is_some() {
                    return v;
                }
            }
        }
    }
    serde_json::from_str(body).expect("parse JSON-RPC body")
}

/// Perform the initialize handshake (real MCP clients always initialize
/// before listing or calling tools), then return the app for reuse.
async fn initialize_first(app: &axum::Router) {
    let (status, body) = send(
        app.clone(),
        mcp_jsonrpc_request("initialize", initialize_params(), 1),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "initialize must succeed: {body}");
}

/// Extract the tool result from a tools/call JSON-RPC response:
/// returns (is_error, text) of content[0].
fn tool_result_text(rpc: &serde_json::Value) -> (bool, String) {
    let result = rpc
        .get("result")
        .expect("tools/call must return a result envelope");
    let is_error = result
        .get("isError")
        .and_then(|v| v.as_bool())
        .expect("isError must be a bool");
    let text = result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .expect("content[0].text must be a string")
        .to_string();
    (is_error, text)
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: As an Agent tool integrator (US-INTG-010), I want MCP requests without a
// valid API token to be rejected, so knowledge base content is never exposed
// to unauthorized callers.
// Covers: /mcp sits behind the same auth_middleware boundary as doc routes.

#[tokio::test]
async fn mcp_without_token_returns_401() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let mut req = mcp_jsonrpc_request("initialize", initialize_params(), 1);
    req.headers_mut().remove(header::AUTHORIZATION);

    let (status, _body) = send(app, req).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "POST /mcp without Authorization must return 401"
    );
}

// User Story: As an Agent tool integrator (US-INTG-010), I want an invalid Bearer token to
// be rejected without distinguishing the failure reason, so token guessing
// yields no useful signal.
// Covers: auth_middleware 401 on invalid token, before any MCP protocol work.

#[tokio::test]
async fn mcp_with_invalid_token_returns_401() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let mut req = mcp_jsonrpc_request("initialize", initialize_params(), 1);
    req.headers_mut().insert(
        header::AUTHORIZATION,
        axum::http::HeaderValue::from_static("Bearer wrong-token"),
    );

    let (status, _body) = send(app, req).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "POST /mcp with a wrong Bearer token must return 401"
    );
}

// User Story: As an Admin (US-INTG-013), I want MCP to stay off when the `[mcp]` config
// section is omitted, so deployments without Agent integration keep their
// existing behavior and attack surface.
// Covers: section-presence toggle — /mcp is not mounted when mcp_config is None.

#[tokio::test]
async fn mcp_disabled_returns_404_when_section_absent() {
    let state = mcp_app_state(false, false).await;
    let app = create_api_routes(state);

    let (status, _body) = send(
        app,
        mcp_jsonrpc_request("initialize", initialize_params(), 1),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "POST /mcp with [mcp] section omitted must return 404 (route not mounted)"
    );
}

// User Story: As an Agent tool integrator (US-INTG-010), I want to connect with a valid
// token and complete the MCP initialize handshake, so my MCP client can use
// the service.
// Covers: initialize returns serverInfo (name rwiki) and tools capability.

#[tokio::test]
async fn mcp_initialize_returns_server_info() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let (status, body) = send(
        app,
        mcp_jsonrpc_request("initialize", initialize_params(), 1),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "initialize must return 200: {body}");

    let rpc = parse_rpc_body(&body);
    assert!(
        rpc.get("error").is_none(),
        "initialize must not return a JSON-RPC error: {body}"
    );
    let result = rpc.get("result").expect("initialize result");
    assert_eq!(
        result
            .get("serverInfo")
            .and_then(|s| s.get("name"))
            .and_then(|n| n.as_str()),
        Some("rwiki"),
        "serverInfo.name must be rwiki: {body}"
    );
    assert!(
        result
            .get("capabilities")
            .and_then(|c| c.get("tools"))
            .is_some(),
        "capabilities must advertise tools: {body}"
    );
}

// User Story: As an Agent tool integrator (US-INTG-010), I want to discover the exact tool
// list (knowledge QA + knowledge search) after connecting, so my agent knows
// what it can call.
// Covers: tools/list exposes exactly rwiki_qa and rwiki_search with schemas.

#[tokio::test]
async fn mcp_tools_list_returns_both_tools() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);
    initialize_first(&app).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request("tools/list", serde_json::json!({}), 2),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tools/list must return 200: {body}");

    let rpc = parse_rpc_body(&body);
    let tools = rpc
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
        .unwrap_or_else(|| panic!("tools/list must return a tools array: {body}"));

    let mut names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["rwiki_qa", "rwiki_search"],
        "tools/list must expose exactly the two read-only tools: {body}"
    );

    for tool in tools {
        assert!(
            tool.get("inputSchema").is_some(),
            "every tool must carry an inputSchema: {body}"
        );
    }
}

// User Story: As an Agent tool integrator (US-INTG-011), I want tool calls with an
// undefined channelId to be rejected with a clear tool error, so I can correct
// the channel identifier myself.
// Covers: business validation errors surface via isError (HTTP 200), matching
// the public chat wording.

#[tokio::test]
async fn mcp_call_with_unconfigured_channel_returns_tool_error() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);
    initialize_first(&app).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            "tools/call",
            tools_call_params(
                "rwiki_qa",
                serde_json::json!({"query": "what is rust", "channelId": "ghost_channel"}),
            ),
            2,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "tool-level business errors are carried by isError, not HTTP status: {body}"
    );

    let (is_error, text) = tool_result_text(&parse_rpc_body(&body));
    assert!(is_error, "unconfigured channel must set isError: {body}");
    assert!(
        text.contains("未配置"),
        "error text must match the public chat wording: {text}"
    );
}

// User Story: As an Agent tool integrator (US-INTG-011), I want the QA tool to refuse
// answering on a channel without published documents instead of inventing an
// answer, so agents do not consume fabricated content.
// Covers: has-published-documents precheck with the public chat wording.

#[tokio::test]
async fn mcp_qa_channel_without_published_docs_returns_tool_error() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);
    initialize_first(&app).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            "tools/call",
            tools_call_params(
                "rwiki_qa",
                serde_json::json!({"query": "what is rust", "channelId": NO_DOCS_CHANNEL}),
            ),
            2,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "tool error must still be HTTP 200: {body}"
    );

    let (is_error, text) = tool_result_text(&parse_rpc_body(&body));
    assert!(
        is_error,
        "channel without published docs must set isError: {body}"
    );
    assert_eq!(
        text, "当前频道没有可用文档",
        "error text must match the public chat wording exactly: {text}"
    );
}

// User Story: As an Agent tool integrator (US-INTG-011), I want the QA tool to return a
// complete answer with source references limited to the channel's published
// documents, so agents can cite provenance.
// Covers: full RAG pipeline over MCP with 1-based contiguous reference indexes.

#[tokio::test]
async fn mcp_qa_returns_answer_with_references() {
    let state = mcp_app_state(true, true).await;
    let app = create_api_routes(state);
    initialize_first(&app).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            "tools/call",
            tools_call_params(
                "rwiki_qa",
                serde_json::json!({"query": "what is rust", "channelId": HIT_CHANNEL}),
            ),
            2,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "qa call must return 200: {body}");

    let (is_error, text) = tool_result_text(&parse_rpc_body(&body));
    assert!(!is_error, "qa on a seeded channel must succeed: {body}");

    let payload: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("content text must be the QaToolResult JSON ({e}): {text}"));
    let answer = payload
        .get("answer")
        .and_then(|a| a.as_str())
        .expect("answer must be a string");
    assert!(!answer.is_empty(), "answer must not be empty: {text}");

    let references = payload
        .get("references")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("references must be an array: {text}"));
    assert!(
        !references.is_empty(),
        "references must cite the retrieved context: {text}"
    );
    for (i, reference) in references.iter().enumerate() {
        assert_eq!(
            reference.get("index").and_then(|v| v.as_u64()),
            Some(i as u64 + 1),
            "reference indexes must be 1-based and contiguous: {text}"
        );
        assert!(
            reference.get("title").and_then(|t| t.as_str()).is_some(),
            "every reference must carry a title: {text}"
        );
    }
}

// User Story: As an Agent tool integrator (US-INTG-012), I want the search tool to return
// raw relevance-sorted chunks with source info, so my agent can read and quote
// the knowledge base directly.
// Covers: search happy path — non-empty results with title/content fields.

#[tokio::test]
async fn mcp_search_returns_sorted_chunks_with_source() {
    let state = mcp_app_state(true, true).await;
    let app = create_api_routes(state);
    initialize_first(&app).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            "tools/call",
            tools_call_params(
                "rwiki_search",
                serde_json::json!({"query": "what is rust", "channelId": HIT_CHANNEL}),
            ),
            2,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "search call must return 200: {body}"
    );

    let (is_error, text) = tool_result_text(&parse_rpc_body(&body));
    assert!(!is_error, "search on a seeded channel must succeed: {body}");

    let payload: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("content text must be the SearchToolResult JSON ({e}): {text}"));
    let results = payload
        .get("results")
        .and_then(|r| r.as_array())
        .unwrap_or_else(|| panic!("results must be an array: {text}"));
    assert!(
        !results.is_empty(),
        "seeded channel must yield results: {text}"
    );

    let mut prev_score = f64::INFINITY;
    for result in results {
        assert!(
            result.get("title").and_then(|t| t.as_str()).is_some(),
            "every chunk must carry a title: {text}"
        );
        assert!(
            result
                .get("content")
                .and_then(|c| c.as_str())
                .is_some_and(|c| !c.is_empty()),
            "every chunk must carry non-empty content: {text}"
        );
        let score = result
            .get("score")
            .and_then(|s| s.as_f64())
            .expect("every chunk must carry a numeric score");
        assert!(
            score <= prev_score,
            "results must be ordered by descending relevance: {text}"
        );
        prev_score = score;
    }
}

// User Story: As an Agent tool integrator (US-INTG-012), I want "no relevant content" to be
// a normal empty result rather than an error, so my agent can branch cleanly.
// Covers: empty results are isError=false with results == [].

#[tokio::test]
async fn mcp_search_returns_empty_results_without_error() {
    let state = mcp_app_state(true, true).await;
    let app = create_api_routes(state);
    initialize_first(&app).await;

    // NO_DOCS_CHANNEL is configured but has no documents; the search tool does
    // not pre-check published docs, so the channel-scoped retrieval simply
    // returns zero hits.
    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            "tools/call",
            tools_call_params(
                "rwiki_search",
                serde_json::json!({"query": "what is rust", "channelId": NO_DOCS_CHANNEL}),
            ),
            2,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "search call must return 200: {body}"
    );

    let (is_error, text) = tool_result_text(&parse_rpc_body(&body));
    assert!(!is_error, "no relevant content is not an error: {body}");
    let payload: serde_json::Value = serde_json::from_str(&text).expect("SearchToolResult JSON");
    let results = payload
        .get("results")
        .and_then(|r| r.as_array())
        .expect("results must be an array");
    assert!(results.is_empty(), "results must be empty: {text}");
}

// User Story: As an Agent tool integrator (US-INTG-012), I want an out-of-range topK to be
// rejected with a clear tool error, so a typo cannot request unbounded data.
// Covers: topK validation (1..=20) via isError.

#[tokio::test]
async fn mcp_search_rejects_out_of_range_top_k() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);
    initialize_first(&app).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            "tools/call",
            tools_call_params(
                "rwiki_search",
                serde_json::json!({"query": "what is rust", "channelId": HIT_CHANNEL, "topK": 21}),
            ),
            2,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "tool error must still be HTTP 200: {body}"
    );

    let (is_error, text) = tool_result_text(&parse_rpc_body(&body));
    assert!(is_error, "topK=21 must set isError: {body}");
    assert!(
        text.contains("topK"),
        "error text must name the offending parameter: {text}"
    );
}

// User Story: As an Agent tool integrator (US-INTG-010), I want the stateless server to
// reject the SSE push-stream channel, because no server-side session exists.
// Covers: GET /mcp returns 405 in stateless (NeverSessionManager) mode.

#[tokio::test]
async fn mcp_get_returns_405() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/mcp")
        .header(header::HOST, "localhost")
        .header(header::ACCEPT, "text/event-stream")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {MCP_TEST_API_TOKEN}"),
        )
        .body(Body::empty())
        .expect("build request");

    let (status, _body) = send(app, req).await;
    assert_eq!(
        status,
        StatusCode::METHOD_NOT_ALLOWED,
        "GET /mcp must return 405 (stateless server has no push stream)"
    );
}

// User Story: As an Agent tool integrator (US-INTG-011), I want each tool call to be fully
// independent with no server-side conversation state, so my client owns the
// conversation flow (single-turn semantics).
// Covers: repeated calls succeed and the server-side chat session store stays
// empty — MCP never writes conversation history.

#[tokio::test]
async fn mcp_repeated_calls_are_independent() {
    let state = mcp_app_state(true, true).await;
    let app = create_api_routes(state.clone());
    initialize_first(&app).await;

    for (id, query) in [(2, "what is rust"), (3, "how does ownership work")] {
        let (status, body) = send(
            app.clone(),
            mcp_jsonrpc_request(
                "tools/call",
                tools_call_params(
                    "rwiki_qa",
                    serde_json::json!({"query": query, "channelId": HIT_CHANNEL}),
                ),
                id,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "call {id} must return 200: {body}");
        let (is_error, _text) = tool_result_text(&parse_rpc_body(&body));
        assert!(!is_error, "call {id} must succeed independently: {body}");
    }

    let sessions_empty = state.chat_sessions.lock().await.is_empty();
    assert!(
        sessions_empty,
        "MCP calls must not create or retain server-side chat sessions"
    );
}
