//! Auth middleware scenario tests.
//!
//! Verifies that the Bearer Token authentication middleware:
//! - Rejects document routes without a token (401)
//! - Rejects document routes with an invalid token (401)
//! - Allows document routes with a valid token (200)
//! - Leaves health and chat routes accessible without authentication

use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

const TEST_API_TOKEN: &str = "test-api-token-12345";

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

/// Build a minimal `AppState` suitable for auth middleware tests.
///
/// The tests only exercise the middleware layer. When a request is rejected
/// by the middleware the handler never runs, so the state fields the
/// handlers depend on (vector_store, llm_client, etc.) only need to satisfy
/// type constraints. For the "valid token" tests the handlers do run, so we
/// provide a real in-memory SQLite with migrations applied.
async fn test_app_state() -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // Build a minimal embedding model so VectorStoreManager compiles.
    // We use a dummy OpenAI client with a fake key; embeddings are never called
    // for these auth tests.
    let openai_client =
        rig::providers::openai::Client::builder().api_key("sk-test-fake-key-for-auth-tests-only");
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
        api_token: TEST_API_TOKEN.to_string(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: As an API operator, I want unauthenticated requests to document
// endpoints to be rejected, so that only authorized clients can manage
// documents.
// Covers: BE-D02 (auth middleware rejects missing token on protected routes)

#[tokio::test]
async fn document_list_without_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/documents")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET /api/documents without Authorization header must return 401"
    );
}

// User Story: As an API operator, I want requests with invalid Bearer tokens
// to be rejected, so that guessing tokens does not grant access.
// Covers: BE-D02 (auth middleware rejects invalid token)

#[tokio::test]
async fn document_list_with_invalid_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/documents")
        .header(header::AUTHORIZATION, "Bearer wrong-token")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET /api/documents with wrong Bearer token must return 401"
    );
}

// User Story: As an authorized client, I want to access document endpoints
// with a valid Bearer token, so that I can manage documents.
// Covers: BE-D02 (auth middleware passes valid token through)

#[tokio::test]
async fn document_list_with_valid_token_returns_200() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/documents")
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /api/documents with correct token must return 200"
    );
}

// User Story: As a monitoring system, I want the health check endpoint to be
// accessible without authentication, so that I can probe service health
// without managing tokens.
// Covers: BE-D03 (health route is unprotected)

#[tokio::test]
async fn health_check_without_token_returns_200() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /health without token must return 200"
    );
}

// User Story: As a user, I want to chat with the knowledge base without an
// API token, so that the chat endpoint remains open for interactive use.
// Covers: BE-D03 (chat route is unprotected)

#[tokio::test]
async fn chat_without_token_does_not_return_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    // POST an empty JSON body; the handler will reject with 400 (empty
    // message) or 503 (empty knowledge base), but never 401.
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/chat")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"message":""}"#))
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "POST /api/chat without token must NOT return 401"
    );
}

// User Story: As an API operator, I want file upload to require
// authentication, so that only authorized users can ingest documents.
// Covers: BE-D02, BE-D03 (upload route is behind auth middleware)

#[tokio::test]
async fn upload_without_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/documents/upload")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "POST /api/documents/upload without token must return 401"
    );
}

// User Story: As an API operator, I want document deletion to require
// authentication, so that only authorized users can remove documents.
// Covers: BE-D02, BE-D03 (delete route is behind auth middleware)

#[tokio::test]
async fn delete_without_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/api/documents/01920000-0000-0000-0000-000000000000")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "DELETE /api/documents/{{id}} without token must return 401"
    );
}
