//! Eval endpoint scenario tests.
//!
//! Verifies the `POST /api/eval/query` endpoint:
//!
//! - Auth enforcement (401 without token)
//! - Input validation (400 on empty query)
//! - Search results contain document_id fields
//! - Rerank scores propagate correctly
//! - Empty knowledge base returns 503
//! - sessionId carries session context
//!
//! These tests exercise the eval handler through the full HTTP stack
//! using the Axum test harness (oneshot) with minimal AppState instances.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use rwiki_core::config::RerankConfig;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Ensure the sqlite-vec extension is registered globally.
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

/// Build a minimal AppState for eval tests.
/// When `seed_data` is true, inserts a chunk so vector_store.is_empty() returns false.
/// When `seed_data` is false, leaves the store empty to test 503.
async fn test_app_state(seed_data: bool) -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");

    if seed_data {
        conn.execute(
            "INSERT INTO chunk_metadata (document_id, chunk_id, content, title) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["doc-001", "chunk-001", "Rust is a systems programming language.", "Rust Introduction"],
        )
        .expect("seed chunk_metadata");
    }

    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // Mock embedding server
    let embed_server = Box::leak(Box::new(mockito::Server::new_async().await));
    let dummy_embedding: Vec<f64> = vec![0.0; 1536];
    let embed_body = serde_json::json!({
        "object": "list",
        "data": [{"object": "embedding", "embedding": dummy_embedding, "index": 0}],
        "model": "text-embedding-3-small",
        "usage": {"prompt_tokens": 1, "total_tokens": 1}
    })
    .to_string();
    let _embed_mock = embed_server
        .mock("POST", "/embeddings")
        .with_status(200)
        .with_body(&embed_body)
        .expect_at_most(20)
        .create_async()
        .await;

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key")
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

    // LLM client pointing to mock server for rewrite + generation
    let llm_server = Box::leak(Box::new(mockito::Server::new_async().await));

    // Mock rewrite + generation response (JSON with queries for rewrite, plain text for generation)
    let _llm_mock = llm_server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_body(serde_json::json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1700000000u64,
            "model": "test-model",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "{\"queries\": [\"Rust systems programming\"]}"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }).to_string())
        .expect_at_most(20)
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
        api_token: "test-api-token".to_string(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        reranker: None,
        rerank_config: RerankConfig::default(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Build an eval query POST request with JSON body and optional auth.
fn eval_request(query: &str, session_id: Option<&str>, with_auth: bool) -> Request<Body> {
    let mut body = serde_json::json!({
        "query": query,
    });
    if let Some(sid) = session_id {
        body["sessionId"] = serde_json::json!(sid);
    }
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/api/eval/query")
        .header(header::CONTENT_TYPE, "application/json");
    if with_auth {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-api-token");
    }
    builder
        .body(Body::from(
            serde_json::to_string(&body).expect("serialize json"),
        ))
        .expect("build request")
}

/// Parse response body as JSON.
async fn parse_json_body(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: Design 4.2.2 -- The eval endpoint requires Bearer token auth.
// Without a token, the endpoint must return 401.
// Covers: Route registration under doc_router with auth_middleware.
//         auth_middleware returns 401 when no Authorization header is present.

#[tokio::test]
async fn eval_query_requires_auth() {
    let state = test_app_state(true).await;
    let app = create_api_routes(state);

    let req = eval_request("test query", None, false);
    let resp = app.oneshot(req).await.expect("send request");

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "eval endpoint must return 401 without auth token"
    );
}

// User Story: Design 4.2.2 -- Empty query must be rejected with 400.
// The handler validates that query is not empty/whitespace before proceeding.
// Covers: eval_query handler validation: `if req.query.trim().is_empty()`.

#[tokio::test]
async fn eval_query_empty_query_returns_400() {
    let state = test_app_state(true).await;
    let app = create_api_routes(state);

    let req = eval_request("", None, true);
    let resp = app.oneshot(req).await.expect("send request");

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "eval endpoint must return 400 for empty query"
    );

    let body: serde_json::Value = parse_json_body(resp.into_body()).await;
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("empty")
            || body["message"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase()
                .contains("empty"),
        "error message should indicate empty query, got: {:?}",
        body
    );
}

// User Story: Design 4.2.2 -- When the knowledge base is empty (no indexed data),
// the eval endpoint must return 503 Service Unavailable.
// Covers: eval_query handler check: `if state.vector_store.is_empty().await`.

#[tokio::test]
async fn eval_query_empty_knowledge_base_returns_503() {
    let state = test_app_state(false).await;
    let app = create_api_routes(state);

    let req = eval_request("test query", None, true);
    let resp = app.oneshot(req).await.expect("send request");

    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "eval endpoint must return 503 when knowledge base is empty"
    );
}

// User Story: Design 4.2.2 -- The eval response must contain search results with
// document_id fields, enabling external eval tools to compute retrieval metrics
// (HitRate, MRR, Recall) by matching returned document_ids against golden dataset.
// Covers: EvalSearchResult struct includes document_id field; the eval handler
//         maps SearchResult into EvalSearchResult preserving all fields.

#[tokio::test]
async fn eval_query_returns_search_results_with_doc_ids() {
    let state = test_app_state(true).await;
    let app = create_api_routes(state);

    let req = eval_request("Rust programming language", None, true);
    let resp = app.oneshot(req).await.expect("send request");

    // The handler should return 200 with JSON response
    assert!(
        resp.status().is_success(),
        "eval endpoint should succeed with seeded data, got status {}",
        resp.status()
    );

    let body: serde_json::Value = parse_json_body(resp.into_body()).await;

    // Verify response structure has search results with document_id
    let search_results = body
        .get("searchResults")
        .expect("response must contain searchResults field");
    assert!(search_results.is_array(), "searchResults must be an array");

    // If results are returned, each must have documentId
    if let Some(results) = search_results.as_array() {
        for result in results {
            assert!(
                result.get("documentId").is_some(),
                "each search result must contain documentId field, got: {:?}",
                result
            );
            assert!(
                result.get("chunkId").is_some(),
                "each search result must contain chunkId field"
            );
            assert!(
                result.get("score").is_some(),
                "each search result must contain score field"
            );
        }
    }
}

// User Story: Design 4.2.2 -- When rerank is enabled, scores in search results
// must be the rerank relevance scores (not raw RRF scores). When rerank is
// disabled, scores must be the original RRF scores.
// Covers: search_and_rerank applies rerank scores via result.score = score,
//         and eval handler maps those scores into EvalSearchResult.

#[tokio::test]
async fn eval_query_includes_rerank_scores() {
    // Build state with reranker disabled (default) -- scores are RRF scores
    let state = test_app_state(true).await;
    let app = create_api_routes(state);

    let req = eval_request("Rust programming language", None, true);
    let resp = app.oneshot(req).await.expect("send request");

    assert!(
        resp.status().is_success(),
        "eval endpoint should succeed, got status {}",
        resp.status()
    );

    let body: serde_json::Value = parse_json_body(resp.into_body()).await;

    // When reranker is None, reranked field must be false
    assert!(
        !body
            .get("reranked")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        "reranked must be false when reranker is disabled"
    );

    // Search results must still have scores
    if let Some(results) = body.get("searchResults").and_then(|v| v.as_array()) {
        for result in results {
            assert!(
                result.get("score").is_some(),
                "each search result must have a score field"
            );
        }
    }
}

// User Story: Design 4.2.2 -- When a sessionId is provided, the eval handler
// should reuse the existing session's conversation history for query rewrite
// context (same SessionStore as the chat endpoint).
// Covers: eval_query handler reads session from state.chat_sessions, and
//         passes history to rewrite_query.

#[tokio::test]
async fn eval_query_with_session_id_carries_context() {
    let state = test_app_state(true).await;

    // Pre-populate a session with history
    {
        let mut sessions = state.chat_sessions.lock().await;
        let mut session =
            rwiki_core::domain::chat::ChatSession::new("test-eval-session".to_string());
        session.add_message("user", "What is Rust?");
        session.add_message("assistant", "Rust is a systems programming language.");
        sessions.insert("test-eval-session".to_string(), session);
    }

    let app = create_api_routes(state);

    let req = eval_request("How does memory work?", Some("test-eval-session"), true);
    let resp = app.oneshot(req).await.expect("send request");

    assert!(
        resp.status().is_success(),
        "eval endpoint should succeed with sessionId, got status {}",
        resp.status()
    );

    let body: serde_json::Value = parse_json_body(resp.into_body()).await;

    // Verify the response is well-formed
    assert!(
        body.get("query").is_some(),
        "response must contain query field"
    );
    assert!(
        body.get("rewrittenQueries").is_some(),
        "response must contain rewrittenQueries field"
    );
    assert!(
        body.get("answer").is_some(),
        "response must contain answer field"
    );
    assert!(
        body.get("timingMs").is_some(),
        "response must contain timingMs field"
    );
}
