//! MCP endpoint scenario tests.
//!
//! Drives `create_api_routes` with oneshot requests against the per-channel
//! public entries `POST /mcp/{channelId}`, covering the full externally
//! visible contract of the MCP server:
//! - Access: public — no credentials required, a Bearer header changes nothing
//! - Addressing: one entry per configured channel; unconfigured channels and
//!   the legacy channel-less `/mcp` are unmatched paths (404); the tool
//!   `channelId` parameter overrides the entry's channel
//! - Toggle: `[mcp]` section absent → 404 (entries not mounted)
//! - Protocol: initialize handshake, tools/list discovery, tools/call dispatch
//! - Tools: rwiki_qa / rwiki_search happy paths, business errors (isError),
//!   topK bounds, empty results, per-call independence (stateless server)

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

const MCP_TEST_API_TOKEN: &str = "mcp-test-api-token-12345";
const HIT_CHANNEL: &str = "help_center";
const NO_DOCS_CHANNEL: &str = "empty_channel";
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

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
/// Start an UNPOOLED mockito server and leak it for the test binary's
/// lifetime: callers return while background tasks still need the mock, so
/// the server must outlive them. Unpooled (`new_with_opts_async`) is
/// required — leaking a pooled `new_async` server would permanently hold
/// one of mockito's process-wide 50 pool licenses and hang later tests.
async fn leak_unpooled_mock_server() -> &'static mut mockito::Server {
    Box::leak(Box::new(
        mockito::Server::new_with_opts_async(mockito::ServerOpts::default()).await,
    ))
}

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
    let embed_server = leak_unpooled_mock_server().await;
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

    let embedding_model = rwiki_core::infrastructure::rig_client::connect_at(
        "sk-test-fake-key-for-mcp-scenarios-only",
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

    // Mockito LLM mock: serves the rewrite payload (JSON with queries) for the
    // rewrite stage and doubles as the non-streaming generation answer.
    let llm_server = leak_unpooled_mock_server().await;
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

    let llm_client =
        rwiki_core::infrastructure::rig_client::connect_at("sk-test-fake", llm_server.url());

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

/// Entry path of a configured channel, e.g. "/mcp/help_center".
fn entry_path(channel: &str) -> String {
    format!("/mcp/{channel}")
}

/// Build a JSON-RPC 2.0 POST request to an MCP channel entry with the
/// MCP-required Accept header. No Authorization header: the endpoint is
/// public; the token-carrying test adds the header explicitly.
///
/// The Host header is set explicitly because the Streamable HTTP service
/// rejects requests without one (oneshot-built requests carry no Host).
fn mcp_jsonrpc_request(
    path: &str,
    method: &str,
    params: serde_json::Value,
    id: i64,
) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
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

/// Perform the initialize handshake against a channel entry (real MCP clients
/// always initialize before listing or calling tools), then return the app
/// for reuse.
async fn initialize_first(app: &axum::Router, path: &str) {
    let (status, body) = send(
        app.clone(),
        mcp_jsonrpc_request(path, "initialize", initialize_params(), 1),
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

// User Story: As an Agent tool integrator (US-INTG-010), I want to connect with just the
// channel entry URL — no token or credentials — and complete the MCP
// initialize handshake, so my MCP client can use the service without any
// credential setup.
// Covers: the channel entry is public (no Authorization header) and initialize
// returns serverInfo (name rwiki) and the tools capability.

#[tokio::test]
async fn mcp_initialize_succeeds_without_credentials() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
            "initialize",
            initialize_params(),
            1,
        ),
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

// User Story: As an Agent tool integrator (US-INTG-010), I want requests that still carry
// an (even invalid) Authorization header to be processed unchanged, so old
// client configs with a token keep working without edits.
// Covers: a Bearer header is neither required nor inspected — the serverInfo
// response matches the credential-less one.

#[tokio::test]
async fn mcp_with_bearer_token_is_processed_unchanged() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let (status, bare_body) = send(
        app.clone(),
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
            "initialize",
            initialize_params(),
            1,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "bare initialize must return 200");

    let mut req = mcp_jsonrpc_request(
        &entry_path(HIT_CHANNEL),
        "initialize",
        initialize_params(),
        2,
    );
    req.headers_mut().insert(
        header::AUTHORIZATION,
        axum::http::HeaderValue::from_static("Bearer wrong-token"),
    );
    let (status, token_body) = send(app, req).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "initialize with an invalid Bearer token must still return 200: {token_body}"
    );

    let bare = parse_rpc_body(&bare_body);
    let with_token = parse_rpc_body(&token_body);
    assert_eq!(
        bare.get("result"),
        with_token.get("result"),
        "a carried token must not change the response"
    );
}

// User Story: As an Agent tool integrator (US-INTG-010), I want every configured channel
// to expose its own MCP entry, so I can organize connections per channel.
// Covers: each channel id configured in [channels] mounts its own
// POST /mcp/{channelId} endpoint.

#[tokio::test]
async fn mcp_each_configured_channel_has_its_own_entry() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    for channel in [HIT_CHANNEL, NO_DOCS_CHANNEL] {
        let (status, body) = send(
            app.clone(),
            mcp_jsonrpc_request(&entry_path(channel), "initialize", initialize_params(), 1),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "configured channel {channel} must have its own working entry: {body}"
        );
    }
}

// User Story: As an Agent tool integrator (US-INTG-010), I want the entry of a channel
// that is not configured to be unavailable, so no tools or knowledge content
// leak through made-up channel paths.
// Covers: POST /mcp/ghost_channel is an unmatched path → 404 with no
// serverInfo or tool list in the body.

#[tokio::test]
async fn mcp_unconfigured_channel_entry_returns_404() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path("ghost_channel"),
            "initialize",
            initialize_params(),
            1,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "unconfigured channel entry must be an unmatched path: {body}"
    );
    assert!(
        !body.contains("serverInfo") && !body.contains("rwiki_qa"),
        "no protocol or knowledge content may leak: {body}"
    );
}

// User Story: As an Agent tool integrator (US-INTG-010), I want the legacy channel-less
// entry to be gone, so addressing has exactly one shape.
// Covers: POST /mcp (legacy entry) is an unmatched path → 404.

#[tokio::test]
async fn mcp_legacy_entry_without_channel_returns_404() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let (status, _body) = send(
        app,
        mcp_jsonrpc_request("/mcp", "initialize", initialize_params(), 1),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "legacy channel-less entry must be an unmatched path"
    );
}

// User Story: As an Agent tool integrator (US-INTG-010), I want the tool's channelId
// parameter to decide which channel is searched regardless of the entry I
// connected to, so one connection can query any configured channel.
// Covers: entry channel and retrieval channel are independent — searching
// help_center content via the empty_channel entry returns it, and the
// reverse direction returns empty results (both directions matter).

#[tokio::test]
async fn mcp_tool_channel_param_overrides_entry_channel() {
    let state = mcp_app_state(true, true).await;
    let app = create_api_routes(state);
    initialize_first(&app, &entry_path(NO_DOCS_CHANNEL)).await;

    // Both directions matter: the channelId parameter must win over the
    // entry's segment — searching the seeded docs via the document-less
    // entry, and NOT falling back to the seeded entry's docs when the
    // parameter targets the document-less channel.
    for (entry, param, id, expect_empty) in [
        (NO_DOCS_CHANNEL, HIT_CHANNEL, 2, false),
        (HIT_CHANNEL, NO_DOCS_CHANNEL, 3, true),
    ] {
        let (status, body) = send(
            app.clone(),
            mcp_jsonrpc_request(
                &entry_path(entry),
                "tools/call",
                tools_call_params(
                    "rwiki_search",
                    serde_json::json!({"query": "what is rust", "channelId": param}),
                ),
                id,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "search via entry {entry}: {body}");
        let (is_error, text) = tool_result_text(&parse_rpc_body(&body));
        assert!(!is_error, "search must not be an error: {body}");
        let payload: serde_json::Value =
            serde_json::from_str(&text).expect("SearchToolResult JSON");
        let results = payload
            .get("results")
            .and_then(|r| r.as_array())
            .unwrap_or_else(|| panic!("results must be an array: {text}"));
        assert_eq!(
            results.is_empty(),
            expect_empty,
            "results must come from the param channel {param}, not the entry {entry}: {text}"
        );
    }
}

// User Story: As an Admin (US-INTG-013), I want MCP to stay off when the `[mcp]` config
// section is omitted, so deployments without Agent integration keep their
// existing behavior and attack surface.
// Covers: section-presence toggle — channel entries are not mounted when
// mcp_config is None.

#[tokio::test]
async fn mcp_disabled_returns_404_when_section_absent() {
    let state = mcp_app_state(false, false).await;
    let app = create_api_routes(state);

    let (status, _body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
            "initialize",
            initialize_params(),
            1,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "channel entry with [mcp] section omitted must return 404 (entries not mounted)"
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
            "tools/list",
            serde_json::json!({}),
            2,
        ),
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    // NO_DOCS_CHANNEL is configured but has no documents; the search tool does
    // not pre-check published docs, so the channel-scoped retrieval simply
    // returns zero hits.
    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
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
// Covers: GET on a channel entry returns 405 in stateless
// (NeverSessionManager) mode.

#[tokio::test]
async fn mcp_get_returns_405() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri(entry_path(HIT_CHANNEL))
        .header(header::HOST, "localhost")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::empty())
        .expect("build request");

    let (status, _body) = send(app, req).await;
    assert_eq!(
        status,
        StatusCode::METHOD_NOT_ALLOWED,
        "GET on a channel entry must return 405 (stateless server has no push stream)"
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
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    for (id, query) in [(2, "what is rust"), (3, "how does ownership work")] {
        let (status, body) = send(
            app.clone(),
            mcp_jsonrpc_request(
                &entry_path(HIT_CHANNEL),
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
// User Story: US-INTG-015 —— 不传 topK 时 MCP 缺省值与公开搜索接口同口径收敛
// 到 [1, MAX_TOP_K]（mcp.rs 共享常量），retrieval 配置越界不再让缺省调用报错。
// Covers: run_search 缺省 topK clamp；回到「缺省 = 原配置值」时本用例因
//         isError("topK 必须在 1 到 20 之间") 失败。
#[tokio::test]
async fn mcp_search_default_top_k_is_capped_by_limit() {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    conn.execute(
        "INSERT INTO documents (id, file_name, status, created_at, channel_id) \
         VALUES (?1, ?2, 'published', ?3, ?4)",
        rusqlite::params![
            "doc-mcp-cap",
            "cap-seed.md",
            "2026-01-01T00:00:00.000Z",
            HIT_CHANNEL
        ],
    )
    .expect("seed documents");
    // 25 independent single-chunk pages with strictly increasing distance to
    // the query vector e1, so the KNN top-20 is fixed and asserting the cap
    // is meaningful (the 21st..25th chunks are recallable but must be cut).
    for i in 0..25 {
        let theta = i as f64 * 0.01;
        conn.execute(
            "INSERT INTO chunk_metadata \
             (document_id, chunk_id, content, title, page_id, sub_index, chunk_count) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 1)",
            rusqlite::params![
                "doc-mcp-cap",
                format!("cap-sub-{i:02}"),
                format!("cap content {i:02}"),
                format!("Cap Page {i:02}"),
                format!("cap-page-{i:02}")
            ],
        )
        .expect("seed chunk_metadata");
        let rowid = conn.last_insert_rowid();
        let mut embedding = vec![0.0f32; 1536];
        embedding[0] = theta.cos() as f32;
        embedding[1] = theta.sin() as f32;
        let mut blob = Vec::with_capacity(1536 * 4);
        for &v in &embedding {
            blob.extend_from_slice(&v.to_le_bytes());
        }
        conn.execute(
            "INSERT INTO vec_chunks (rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![rowid, blob],
        )
        .expect("seed vec_chunks");
    }
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // Embeddings mock returns the unit vector e1 (axis 0) so query embedding
    // works offline and the KNN order matches the seeded angles.
    let embed_server = leak_unpooled_mock_server().await;
    let mut e1 = vec![0.0f64; 1536];
    e1[0] = 1.0;
    let embed_body = serde_json::json!({
        "object": "list",
        "data": [{"object": "embedding", "embedding": e1, "index": 0}],
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
        "sk-test-fake-key-for-mcp-cap-only",
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

    // LLM mock for the rewrite stage (same payload as mcp_app_state).
    let llm_server = leak_unpooled_mock_server().await;
    let _llm_mock = llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "id": "chatcmpl-mcp-cap",
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
    let llm_client =
        rwiki_core::infrastructure::rig_client::connect_at("sk-test-fake", llm_server.url());

    // max_context_chunks = 50 (> MAX_TOP_K): an unclamped default would
    // reject the call with a topK range error.
    let state = Arc::new(AppState {
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
        retrieval_config: rwiki_core::config::RetrievalConfig {
            max_context_chunks: 50,
            ..rwiki_core::config::RetrievalConfig::default()
        },
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config: None,
        mcp_config: Some(rwiki_core::config::McpConfig::default()),
        channels_config: mcp_test_channels_config(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    });
    let app = create_api_routes(state);
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
            "tools/call",
            tools_call_params(
                "rwiki_search",
                serde_json::json!({"query": "zzzzz-no-match-marker", "channelId": HIT_CHANNEL}),
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
    assert!(
        !is_error,
        "default topK must be clamped instead of rejected on config=50: {body}"
    );
    let payload: serde_json::Value = serde_json::from_str(&text).expect("SearchToolResult JSON");
    let results = payload
        .get("results")
        .and_then(|r| r.as_array())
        .expect("results must be an array");
    assert!(
        results.len() <= 20,
        "default topK is capped at MAX_TOP_K (20), got {}: {text}",
        results.len()
    );
}

// User Story: US-INTG-015 —— 全局空库（尚无任何已索引 chunk）时 MCP 检索返回
// 空结果而非内部错误，与「无文档与无命中统一返回空列表」的声明一致。
// Covers: run_search 全局空库短路；去掉短路时检索原语报错 →
//         isError("知识库服务内部错误")，本用例失败。
#[tokio::test]
async fn mcp_search_empty_knowledge_base_returns_empty_results() {
    let state = mcp_app_state(true, false).await;
    let app = create_api_routes(state);
    initialize_first(&app, &entry_path(HIT_CHANNEL)).await;

    let (status, body) = send(
        app,
        mcp_jsonrpc_request(
            &entry_path(HIT_CHANNEL),
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
    assert!(
        !is_error,
        "globally empty KB must be an empty result, not an internal error: {body}"
    );
    let payload: serde_json::Value = serde_json::from_str(&text).expect("SearchToolResult JSON");
    let results = payload
        .get("results")
        .and_then(|r| r.as_array())
        .expect("results must be an array");
    assert!(results.is_empty(), "empty KB must return []: {text}");
}
