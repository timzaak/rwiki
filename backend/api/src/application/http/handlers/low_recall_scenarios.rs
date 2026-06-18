//! Low-recall records list/filter/pagination/auth/empty/invalid-params scenarios.
//!
//! Verifies the single read endpoint:
//!
//! - GET `/api/low-recall/records` (Bearer Token): list, score filter, time range
//!   filter, pagination, auth, empty state, and invalid-params rejection
//!
//! Mirrors `feedback_scenarios.rs` structure (own `Once`, own helpers; no
//! cross-file shared statics).

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

/// Build a minimal `AppState` suitable for low-recall tests.
/// `low_recall_config: None` — these scenarios only exercise the read endpoint,
/// the bypass write path is covered in `chat_scenarios.rs`.
async fn test_app_state() -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-low-recall-tests-only");
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
        static_dir: None,
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config: None,
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Helper to parse the JSON response body into a serde_json::Value.
async fn parse_json_body(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Build an authorized request for a given URI (Bearer Token injected).
fn auth_request(method: Method, uri: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .body(Body::empty())
        .expect("build request")
}

/// Insert a `low_recall_records` row directly into the database for pre-seeding.
/// `top_score = None` writes SQL NULL (complete miss case).
/// `created_at` overrides the default timestamp when provided, making time-range
/// filter and pagination scenarios deterministic.
#[allow(clippy::too_many_arguments)]
async fn insert_test_record(
    state: &Arc<AppState>,
    session_id: Option<&str>,
    query: &str,
    top_score: Option<f64>,
    result_count: i64,
    sources_json: &str,
    created_at: Option<&str>,
) {
    let sid = session_id.map(|s| s.to_string());
    let q = query.to_string();
    let rc = result_count;
    let sj = sources_json.to_string();
    let ca = created_at.map(|s| s.to_string());

    state
        .sqlite
        .call(move |conn| -> Result<(), rusqlite::Error> {
            if let Some(ref ts) = ca {
                conn.execute(
                    "INSERT INTO low_recall_records \
                     (session_id, query, top_score, result_count, sources, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![sid, q, top_score, rc, sj, ts],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO low_recall_records \
                     (session_id, query, top_score, result_count, sources) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![sid, q, top_score, rc, sj],
                )?;
            }
            Ok(())
        })
        .await
        .expect("insert test low_recall_records row");
}

// ---------------------------------------------------------------------------
// GET /api/low-recall/records scenarios (6 tests)
// ---------------------------------------------------------------------------

// User Story: US-CORE-038 -- As an operator, I want to view low-recall records
// filtered by score so I can identify knowledge-base blind spots.
// Covers: Design §6.1 scenario 1 / §4.2 maxScore filter; records with topScore
//          0.1/0.5/0.9 + ?maxScore=0.4 -> only the 0.1 record returned.

#[tokio::test]
async fn list_returns_only_low_score_records() {
    let state = test_app_state().await;

    // Three records spanning low / mid / high scores; newest timestamp so that
    // ordering does not matter for this single-result assertion.
    insert_test_record(
        &state,
        Some("sess-low"),
        "low-score query",
        Some(0.1),
        3,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        Some("sess-mid"),
        "mid-score query",
        Some(0.5),
        4,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        Some("sess-high"),
        "high-score query",
        Some(0.9),
        5,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        "/api/low-recall/records?maxScore=0.4".to_string(),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "authenticated GET with maxScore must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        1,
        "maxScore=0.4 must return only the 0.1 record"
    );
    assert_eq!(body["total"], 1, "total must reflect the filtered count");

    let top_score = items[0]["topScore"].as_f64().expect("topScore number");
    assert!(
        (top_score - 0.1).abs() < f64::EPSILON,
        "returned record must be the 0.1 one, got {top_score}"
    );
}

// User Story: US-CORE-038 -- As an operator, I want to filter records by time
// range so I can investigate incidents within a specific window.
// Covers: Design §6.1 scenario 2 / §4.2 from/to closed interval; records on
//          2025-01-01/02/03 + from/to bracketing only 01-02 -> 1 record.

#[tokio::test]
async fn list_filters_by_time_range() {
    let state = test_app_state().await;

    insert_test_record(
        &state,
        Some("sess-d1"),
        "day 1 query",
        Some(0.2),
        2,
        "[]",
        Some("2025-01-01T12:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        Some("sess-d2"),
        "day 2 query",
        Some(0.2),
        2,
        "[]",
        Some("2025-01-02T12:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        Some("sess-d3"),
        "day 3 query",
        Some(0.2),
        2,
        "[]",
        Some("2025-01-03T12:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        "/api/low-recall/records?from=2025-01-02T00:00:00Z&to=2025-01-02T23:59:59Z".to_string(),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "time-range filtered GET must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "only the 01-02 record must be returned");
    assert_eq!(body["total"], 1, "total must be 1");
    assert_eq!(
        items[0]["sessionId"], "sess-d2",
        "the returned record must be the day-2 one"
    );
}

// User Story: US-CORE-038 -- As an operator, I want paginated records with a
// correct total so I can page through large datasets reliably.
// Covers: Design §6.1 scenario 3 / §4.2 limit/offset + total; 3 records with
//          explicit created_at + ?limit=2&offset=0 -> 2 items, total=3, newest
//          first (createdAt DESC).

#[tokio::test]
async fn list_pagination_total() {
    let state = test_app_state().await;

    insert_test_record(
        &state,
        Some("sess-pg1"),
        "page-1 query",
        Some(0.15),
        1,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        Some("sess-pg2"),
        "page-2 query",
        Some(0.15),
        1,
        "[]",
        Some("2025-01-02T00:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        Some("sess-pg3"),
        "page-3 query",
        Some(0.15),
        1,
        "[]",
        Some("2025-01-03T00:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        "/api/low-recall/records?limit=2&offset=0".to_string(),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "paginated GET must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2, "limit=2 must return 2 items");
    assert_eq!(body["total"], 3, "total must reflect all matching records");

    // Order: createdAt DESC (newest first)
    assert_eq!(
        items[0]["sessionId"], "sess-pg3",
        "first item must be newest (2025-01-03)"
    );
    assert_eq!(
        items[1]["sessionId"], "sess-pg2",
        "second item must be middle (2025-01-02)"
    );
}

// User Story: US-CORE-038 -- As an operator, I want unauthorized access rejected
// so that sensitive user queries are protected.
// Covers: Design §6.1 scenario 4 / §4.2 401 on missing or invalid token; the
//          endpoint is mounted under doc_router (auth_middleware), so both the
//          no-token and wrong-token cases must yield 401 Unauthorized.

#[tokio::test]
async fn list_requires_auth() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    // No Authorization header at all
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/low-recall/records")
        .body(Body::empty())
        .expect("build request");
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET without token must return 401"
    );

    // Wrong token
    let app = create_api_routes(state);
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/low-recall/records")
        .header(header::AUTHORIZATION, "Bearer wrong")
        .body(Body::empty())
        .expect("build request");
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET with wrong token must return 401"
    );
}

// User Story: US-CORE-038 -- As an operator, when there are no low-recall
// records I want an empty list (not an error) so the UI can show the empty
// state (US-CORE-038 scenario 4).
// Covers: Design §6.1 scenario 4 empty state / §4.2 200 with items=[] total=0
//          when the table has no rows.

#[tokio::test]
async fn list_empty_returns_empty() {
    let state = test_app_state().await;

    // No records inserted
    let app = create_api_routes(state);
    let req = auth_request(Method::GET, "/api/low-recall/records".to_string());
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "authenticated GET on empty table must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert!(items.is_empty(), "items must be an empty array");
    assert_eq!(
        body["total"], 0,
        "total must be 0 when there are no records"
    );
}

// User Story: US-CORE-038 -- As a system, I want invalid filter parameters
// rejected with 400 so callers know the query was malformed.
// Covers: Design §6.1 scenario 6 / §4.2 400 on minScore > maxScore and on
//          non-ISO8601 from/to.

#[tokio::test]
async fn list_invalid_params_400() {
    let state = test_app_state().await;

    // minScore > maxScore
    let app = create_api_routes(state.clone());
    let req = auth_request(
        Method::GET,
        "/api/low-recall/records?minScore=0.8&maxScore=0.2".to_string(),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "minScore > maxScore must return 400"
    );

    // Non-ISO8601 `from`
    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        "/api/low-recall/records?from=not-a-date".to_string(),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "non-ISO8601 `from` must return 400"
    );
}
