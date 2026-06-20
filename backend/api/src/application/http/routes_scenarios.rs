//! Static serving scenario tests.
//!
//! Verifies the backend serves the Web SPA (index.html + assets) at root with a
//! SPA fallback for client-side routes, while:
//! - Keeping `/widget/*` backward-compatible for third-party embeds
//! - Leaving `/health` and protected `/api/*` routes unaffected by the fallback
//!
//! The SPA fallback returning 200 (not 404) for unknown paths is the crux: it is
//! what lets TanStack Router client-side routes (e.g. `/admin`, `/auth/login`)
//! survive full page loads and deep links.

use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use rig::client::EmbeddingsClient;
use tempfile::TempDir;
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

const TEST_API_TOKEN: &str = "test-api-token-12345";
/// Marker written into the fake index.html so SPA-fallback assertions can prove
/// the response is the SPA shell (not a 404 body, not an asset file).
const SPA_MARKER: &str = "RWIKI_SPA_MARKER";

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

/// Build a minimal `AppState` that points `static_dir` at the given directory.
///
/// Mirrors `middleware_scenarios::test_app_state` (handlers' dependencies only
/// need to satisfy type constraints; they are never exercised by these routing
/// tests), but sets `static_dir` so `create_api_routes` wires up static serving.
async fn test_app_state_with_static_dir(static_dir: String) -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let openai_client =
        rig::providers::openai::Client::builder().api_key("sk-test-fake-key-for-static-tests-only");
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
        api_allowed_ip_ranges: Vec::new(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: Some(static_dir),
        allowed_origins: vec![],
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config: None,
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Create a temp dir populated like a real frontend build:
/// `index.html` (with marker), `assets/app.js`, `rwiki-chat.js`.
/// Returns the `TempDir` (caller must hold it to keep files alive) and the
/// wired-up router.
async fn setup() -> (TempDir, axum::Router) {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();

    std::fs::write(
        root.join("index.html"),
        format!("<html><body>{SPA_MARKER}</body></html>"),
    )
    .expect("write index.html");
    std::fs::create_dir_all(root.join("assets")).expect("create assets dir");
    std::fs::write(root.join("assets").join("app.js"), "// app bundle")
        .expect("write assets/app.js");
    std::fs::write(root.join("rwiki-chat.js"), "// widget bundle").expect("write rwiki-chat.js");

    let state = test_app_state_with_static_dir(root.to_string_lossy().into_owned()).await;
    let app = create_api_routes(state);
    (dir, app)
}

/// Collect a response body into a String for content assertions.
async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: As a visitor, I want the site root to load the web app, so I can
// use the admin console / landing page from a single deployed server.
#[tokio::test]
async fn root_serves_index_html() {
    let (_dir, app) = setup().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET / must serve the SPA entrypoint"
    );
    let body = body_string(resp).await;
    assert!(
        body.contains(SPA_MARKER),
        "GET / body must be index.html (SPA shell)"
    );
}

// User Story: As a user with a deep link (or after a refresh) to a client-side
// route like /admin, I want the page to load — not 404 — so the client router
// can take over. THIS is the assertion that proves the fallback returns 200.
#[tokio::test]
async fn client_route_falls_back_to_index_html() {
    let (_dir, app) = setup().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/admin")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /admin (client route) must fall back to index.html with 200, not 404"
    );
    let body = body_string(resp).await;
    assert!(
        body.contains(SPA_MARKER),
        "GET /admin body must be index.html so TanStack Router can boot"
    );
}

// User Story: As a browser, I want the JS chunks referenced by index.html to
// resolve to real files, so the SPA actually executes.
#[tokio::test]
async fn assets_file_served_directly() {
    let (_dir, app) = setup().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/assets/app.js")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /assets/app.js must serve the real asset file"
    );
    let body = body_string(resp).await;
    assert_eq!(
        body, "// app bundle",
        "/assets/app.js must return its file contents, not the index.html fallback"
    );
}

// User Story: As a third-party site embedding the widget, I need the widget JS
// URL to remain stable, so existing <script src="/widget/rwiki-chat.js"> tags
// keep working after this change.
#[tokio::test]
async fn widget_js_served_at_widget_prefix() {
    let (_dir, app) = setup().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/widget/rwiki-chat.js")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /widget/rwiki-chat.js must stay reachable (backward compat for embeds)"
    );
    let body = body_string(resp).await;
    assert_eq!(
        body, "// widget bundle",
        "/widget/rwiki-chat.js must serve the widget JS"
    );
}

// User Story: As a monitoring system, I want /health to keep working, so the
// SPA fallback does not shadow API / operational routes.
#[tokio::test]
async fn health_route_not_swallowed_by_spa_fallback() {
    let (_dir, app) = setup().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/health")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /health must hit the health handler, not the SPA fallback"
    );
}

// User Story: As an API operator, I want protected routes to still require
// authentication, so that wiring up static serving does not bypass route-level
// middleware.
#[tokio::test]
async fn protected_api_route_still_requires_auth() {
    let (_dir, app) = setup().await;

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/documents")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET /api/documents without token must still return 401 (auth middleware intact)"
    );
}
