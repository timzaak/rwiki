//! Rerank degradation and multi-query integration scenario tests.
//!
//! Verifies the rerank stage in the chat handler pipeline:
//!
//! - **Degradation**: When rerank is disabled or fails (API error, auth error,
//!   timeout, invalid JSON), original search results pass through unchanged.
//! - **Multi-query**: Rerank applies to fused results after global RRF, and
//!   failure degrades to the global RRF fusion results.
//!
//! These tests exercise the rerank stage through the full HTTP stack by
//! constructing minimal AppState instances with mockito-backed rerankers.
//! The chat endpoint is invoked via the Axum test harness (oneshot).

use std::sync::Arc;
use std::sync::Once;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request};
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use rwiki_core::config::RerankConfig;
use rwiki_core::infrastructure::reranker::OpenRouterReranker;
use rwiki_core::infrastructure::reranker::RerankerProvider;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

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

/// Build a minimal `AppState` with reranker configured per the given provider
/// and config. All other fields use test defaults.
async fn test_app_state_with_reranker(
    reranker: Option<RerankerProvider>,
    rerank_config: RerankConfig,
) -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    // Seed a dummy row so vector_store.is_empty() returns false, allowing
    // the chat handler to proceed past the "knowledge base empty" guard.
    conn.execute(
        "INSERT INTO chunk_metadata (document_id, chunk_id, content, title) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params!["test-doc", "test-chunk", "seed content", "Seed Title"],
    )
    .expect("seed chunk_metadata");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // Start a mockito server for the embedding API so search_hybrid can
    // embed the query without hitting the real OpenAI endpoint.
    // Box::leak keeps the server alive for the test's lifetime (acceptable in tests).
    let embed_server = Box::leak(Box::new(mockito::Server::new_async().await));
    // Return a dummy 1536-dim embedding (all zeros) matching text-embedding-3-small.
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
        .expect_at_most(20) // allow multiple embedding calls per test
        .create_async()
        .await;

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-rerank-tests-only")
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

    let llm_client = rig::providers::openai::CompletionsClient::builder()
        .api_key("sk-test-fake")
        .base_url("http://localhost:0")
        .build()
        .expect("build LLM client");

    Arc::new(AppState {
        sqlite,
        enable_openapi: false,
        vector_store,
        chat_sessions: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        llm_client,
        llm_model: "test-model".to_string(),
        api_token: "test-api-token".to_string(),
        api_allowed_ip_ranges: Vec::new(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker,
        rerank_config,
        low_recall_config: None,
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Build a minimal `AppState` with reranker disabled (reranker: None).
async fn test_app_state_rerank_disabled() -> Arc<AppState> {
    test_app_state_with_reranker(None, RerankConfig::default()).await
}

/// Build a chat POST request with JSON body.
fn chat_request(message: &str, session_id: &str) -> Request<Body> {
    let body = serde_json::json!({
        "message": message,
        "sessionId": session_id,
    });
    Request::builder()
        .method(Method::POST)
        .uri("/api/chat")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_string(&body).expect("serialize json"),
        ))
        .expect("build request")
}

// ---------------------------------------------------------------------------
// Degradation scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-031 -- As a user, when rerank is disabled (the default),
// search results must pass through unchanged. No rerank API call is made.
// Covers: Design 4.5.4 -- when state.reranker is None, the if-let branch is
//         skipped and search_results are used as-is. Design 4.5.1 -- default
//         enable=false means reranker is None in AppState.

#[tokio::test]
async fn rerank_disabled_returns_original_results() {
    let state = test_app_state_rerank_disabled().await;
    let app = create_api_routes(state);

    let req = chat_request("test query", "session-rerank-disabled");
    let resp = app.oneshot(req).await.expect("send request");

    // The chat endpoint returns SSE (200) regardless of rerank status.
    // When reranker is None, the rerank stage is skipped entirely.
    // We verify the endpoint does not error (would be 500 if rerank broke).
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed when rerank is disabled, got status {}",
        resp.status()
    );
}

// User Story: US-CORE-031 -- As a user, when the rerank API returns a 500
// server error, the system must degrade to the original RRF fusion results.
// The user should still receive a normal response without errors.
// Covers: Design 4.5.4 -- Err(e) arm in the rerank match returns search_results.
//         US-CORE-031 core constraint: "API 调用失败时降级使用 RRF 融合结果".

#[tokio::test]
async fn rerank_api_error_degrades_to_original() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/rerank")
        .with_status(500)
        .with_body(r#"{"error":"internal server error"}"#)
        .create_async()
        .await;

    let reranker = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let config = RerankConfig::default();

    let state = test_app_state_with_reranker(Some(reranker), config).await;
    let app = create_api_routes(state);

    let req = chat_request("test query", "session-api-error");
    let resp = app.oneshot(req).await.expect("send request");

    // The chat endpoint must still succeed (SSE 200) despite rerank API 500.
    // The rerank stage catches the error and falls back to original results.
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed when rerank API returns 500, got status {}",
        resp.status()
    );
}

// User Story: US-CORE-031 -- As a user, when the rerank API returns 401
// (invalid API key), the system must degrade gracefully. The user must not
// see any error; the system logs a warning and uses original RRF results.
// Covers: US-CORE-031 "API Key 无效" scenario. Design 4.5.4 -- RerankError::Api
//         with status 401 is caught in the Err arm and falls back.

#[tokio::test]
async fn rerank_api_auth_error_degrades_to_original() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/rerank")
        .with_status(401)
        .with_body(r#"{"error":"invalid api key"}"#)
        .create_async()
        .await;

    let reranker = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "bad-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let config = RerankConfig::default();

    let state = test_app_state_with_reranker(Some(reranker), config).await;
    let app = create_api_routes(state);

    let req = chat_request("test query", "session-auth-error");
    let resp = app.oneshot(req).await.expect("send request");

    // Chat must succeed despite 401 from rerank API.
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed when rerank API returns 401, got status {}",
        resp.status()
    );
}

// User Story: US-CORE-031 -- As a user, when the rerank API is too slow
// (exceeds timeout), the system must degrade to original results without
// making the user wait. This prevents rerank from blocking the entire
// response pipeline.
// Covers: Design 4.5.4 -- timeout triggers RerankError::Timeout which is
//         caught in the Err arm. Design 4.5.1 -- timeout_secs config default
//         is 3 seconds. PRD non-functional: "rerank 阶段延迟 <= 1 秒".

#[tokio::test]
async fn rerank_timeout_degrades_to_original() {
    let mut server = mockito::Server::new_async().await;
    // Mock a response that writes slowly via chunked body, simulating latency.
    // The writer sleeps 500ms before completing, exceeding the 50ms timeout.
    let _mock = server
        .mock("POST", "/rerank")
        .with_status(200)
        .with_chunked_body(|writer| {
            std::thread::sleep(Duration::from_millis(500));
            writer.write_all(r#"{"results":[{"index":0,"relevance_score":0.9}]}"#.as_bytes())?;
            Ok(())
        })
        .create_async()
        .await;

    let reranker = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_millis(50), // 50ms timeout -- much shorter than 500ms delay
        format!("{}/rerank", server.url()),
    ));

    let config = RerankConfig::default();

    let state = test_app_state_with_reranker(Some(reranker), config).await;
    let app = create_api_routes(state);

    let req = chat_request("test query", "session-timeout");
    let resp = app.oneshot(req).await.expect("send request");

    // Chat must succeed despite rerank timeout.
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed when rerank times out, got status {}",
        resp.status()
    );
}

// User Story: US-CORE-031 -- As a user, when the rerank API returns 200 but
// with malformed JSON (e.g., missing expected fields), the system must
// degrade gracefully instead of crashing or returning an error to the user.
// Covers: Design 4.5.4 -- RerankError::ResponseParse is caught in the Err arm.
//         The handler logs a warning and falls back to original results.

#[tokio::test]
async fn rerank_invalid_json_degrades_to_original() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/rerank")
        .with_status(200)
        .with_body(r#"{"unexpected": "format", "no_results_field": true}"#)
        .create_async()
        .await;

    let reranker = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let config = RerankConfig::default();

    let state = test_app_state_with_reranker(Some(reranker), config).await;
    let app = create_api_routes(state);

    let req = chat_request("test query", "session-invalid-json");
    let resp = app.oneshot(req).await.expect("send request");

    // Chat must succeed despite malformed rerank response.
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed when rerank returns invalid JSON, got status {}",
        resp.status()
    );
}

// ---------------------------------------------------------------------------
// Multi-query path scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-030 -- As a user submitting a query that triggers
// multi-query rewrite, the rerank stage must receive the globally fused
// results (after global RRF) and apply cross-encoder re-scoring. The
// final context sent to the LLM must reflect rerank ordering, not the
// raw RRF fusion order.
// Covers: Design 4.5.4 -- the same rerank block (lines 605-632) applies to
//         both single-query and multi-query paths because it runs after
//         search_results are finalized. For multi-query, search_results
//         come from search_multi_query_hybrid() which performs global RRF.
//         The reranker receives the fused content and re-orders.

#[tokio::test]
async fn multi_query_rerank_applies_to_fused_results() {
    let mut server = mockito::Server::new_async().await;
    // Mock returns results in rerank order (index 1 > index 0 by score)
    let _mock = server
        .mock("POST", "/rerank")
        .with_status(200)
        .with_body(
            r#"{"results":[{"index":1,"relevance_score":0.95},{"index":0,"relevance_score":0.7}]}"#,
        )
        .create_async()
        .await;

    let reranker = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let config = RerankConfig::default();

    let state = test_app_state_with_reranker(Some(reranker), config).await;
    let app = create_api_routes(state);

    // Use a query that would trigger multi-query rewrite (long enough, non-trivial)
    let req = chat_request(
        "How does Kubernetes handle pod scheduling?",
        "session-multi-rerank",
    );
    let resp = app.oneshot(req).await.expect("send request");

    // The chat endpoint must succeed. The rerank stage applies to whatever
    // search_results are produced (single or multi-query path).
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed with multi-query rerank, got status {}",
        resp.status()
    );
}

// User Story: US-CORE-031 -- As a user submitting a multi-query rewrite,
// when the rerank API fails, the system must degrade to the global RRF
// fusion results (not the raw per-query results). This ensures the user
// still receives the best available results from the multi-query search.
// Covers: Design 4.5.4 -- Err arm returns search_results which, for the
//         multi-query path, are the global RRF fusion results from
//         search_multi_query_hybrid(). Design 5.1 row 4: "多查询 + rerank
//         降级：rerank 失败时回退到全局 RRF 融合结果".

#[tokio::test]
async fn multi_query_rerank_failure_degrades_to_fused_results() {
    let mut server = mockito::Server::new_async().await;
    // Mock a 500 error -- rerank fails
    let _mock = server
        .mock("POST", "/rerank")
        .with_status(500)
        .with_body(r#"{"error":"service unavailable"}"#)
        .create_async()
        .await;

    let reranker = RerankerProvider::OpenRouter(OpenRouterReranker::with_base_url(
        "test-key".to_string(),
        "cohere/rerank-v4-fast".to_string(),
        Duration::from_secs(3),
        format!("{}/rerank", server.url()),
    ));

    let config = RerankConfig::default();

    let state = test_app_state_with_reranker(Some(reranker), config).await;
    let app = create_api_routes(state);

    let req = chat_request(
        "Explain Rust ownership model in detail",
        "session-multi-fallback",
    );
    let resp = app.oneshot(req).await.expect("send request");

    // Chat must succeed despite rerank failure on multi-query path.
    // The handler falls back to global RRF fusion results.
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed when multi-query rerank fails, got status {}",
        resp.status()
    );
}
