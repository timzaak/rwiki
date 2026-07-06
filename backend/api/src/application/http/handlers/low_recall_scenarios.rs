//! Low-recall records list/filter/pagination/auth/empty/invalid-params scenarios.
//!
//! Verifies the single read endpoint:
//!
//! - GET `/api/low-recall/records` (Bearer Token): list, score filter, time range
//!   filter, pagination, auth, empty state, invalid-params rejection, and site
//!   isolation.
//!
//! Mirrors `feedback_scenarios.rs` structure (own `Once`, own helpers; no
//! cross-file shared statics).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;
use rwiki_core::config::{SiteConfig, SitesConfig};

// ---------------------------------------------------------------------------
// Test constants / helpers
// ---------------------------------------------------------------------------

const TEST_API_TOKEN: &str = "test-api-token-12345";
const SITE_A: &str = "help_center";
const SITE_B: &str = "dev_docs";
const UNKNOWN_SITE: &str = "unknown_site";

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

/// Build a `SitesConfig` with two configured sites for isolation scenarios.
fn test_sites_config() -> SitesConfig {
    let mut sites = HashMap::new();
    sites.insert(
        SITE_A.to_string(),
        SiteConfig {
            name: "Help Center".to_string(),
            system_prompt: None,
            suggested_questions: None,
        },
    );
    sites.insert(
        SITE_B.to_string(),
        SiteConfig {
            name: "Developer Docs".to_string(),
            system_prompt: None,
            suggested_questions: None,
        },
    );
    SitesConfig { sites }
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
        allowed_origins: vec![],
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config: None,
        sites_config: test_sites_config(),
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
    site_id: &str,
    session_id: Option<&str>,
    query: &str,
    top_score: Option<f64>,
    result_count: i64,
    sources_json: &str,
    created_at: Option<&str>,
) {
    let sid = site_id.to_string();
    let sess = session_id.map(|s| s.to_string());
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
                     (site_id, session_id, query, top_score, result_count, sources, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![sid, sess, q, top_score, rc, sj, ts],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO low_recall_records \
                     (site_id, session_id, query, top_score, result_count, sources) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![sid, sess, q, top_score, rc, sj],
                )?;
            }
            Ok(())
        })
        .await
        .expect("insert test low_recall_records row");
}

// ---------------------------------------------------------------------------
// GET /api/low-recall/records scenarios
// ---------------------------------------------------------------------------

// User Story: US-CORE-038 -- As an operator, I want to view low-recall records
// filtered by score so I can identify knowledge-base blind spots.
// Covers: Design §6.1 scenario 1 / §4.2 maxScore filter; records with topScore
//          0.1/0.5/0.9 + ?maxScore=0.4 -> only the 0.1 record returned.

#[tokio::test]
async fn list_returns_only_low_score_records() {
    let state = test_app_state().await;

    // SITE_A: three records spanning low / mid / high scores.
    insert_test_record(
        &state,
        SITE_A,
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
        SITE_A,
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
        SITE_A,
        Some("sess-high"),
        "high-score query",
        Some(0.9),
        5,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    // SITE_B: a low-score record that must be excluded when querying SITE_A.
    insert_test_record(
        &state,
        SITE_B,
        Some("sess-b-low"),
        "site-b low-score query",
        Some(0.1),
        2,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_A}&maxScore=0.4"),
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
        "maxScore=0.4 must return only the SITE_A 0.1 record"
    );
    assert_eq!(body["total"], 1, "total must reflect the filtered count");

    let top_score = items[0]["topScore"].as_f64().expect("topScore number");
    assert!(
        (top_score - 0.1).abs() < f64::EPSILON,
        "returned record must be the 0.1 one, got {top_score}"
    );
    assert_eq!(
        items[0]["siteId"], SITE_A,
        "returned record must belong to SITE_A"
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
        SITE_A,
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
        SITE_A,
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
        SITE_A,
        Some("sess-d3"),
        "day 3 query",
        Some(0.2),
        2,
        "[]",
        Some("2025-01-03T12:00:00Z"),
    )
    .await;
    // Same-day record for SITE_B must not be returned for SITE_A.
    insert_test_record(
        &state,
        SITE_B,
        Some("sess-b-d2"),
        "site-b day 2 query",
        Some(0.2),
        2,
        "[]",
        Some("2025-01-02T12:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_A}&from=2025-01-02T00:00:00Z&to=2025-01-02T23:59:59Z"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "time-range filtered GET must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        1,
        "only the SITE_A 01-02 record must be returned"
    );
    assert_eq!(body["total"], 1, "total must be 1");
    assert_eq!(
        items[0]["sessionId"], "sess-d2",
        "the returned record must be the SITE_A day-2 one"
    );
    assert_eq!(
        items[0]["siteId"], SITE_A,
        "returned record must belong to SITE_A"
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
        SITE_A,
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
        SITE_A,
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
        SITE_A,
        Some("sess-pg3"),
        "page-3 query",
        Some(0.15),
        1,
        "[]",
        Some("2025-01-03T00:00:00Z"),
    )
    .await;
    // SITE_B records must not affect SITE_A pagination totals.
    insert_test_record(
        &state,
        SITE_B,
        Some("sess-b-pg"),
        "site-b query",
        Some(0.15),
        1,
        "[]",
        Some("2025-01-02T00:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_A}&limit=2&offset=0"),
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
    assert_eq!(body["total"], 3, "total must reflect only SITE_A records");

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

// User Story: support-multiple-website -- Low-recall queries must require a
// siteId and reject requests without one.
// Covers: BE-D04; GET /api/low-recall/records without siteId returns 400.

#[tokio::test]
async fn list_missing_site_id_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = auth_request(Method::GET, "/api/low-recall/records".to_string());
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "GET without siteId must return 400"
    );
}

// User Story: support-multiple-website -- Only configured sites may be queried.
// Covers: BE-D04; GET /api/low-recall/records with unconfigured siteId returns 400.

#[tokio::test]
async fn list_unconfigured_site_id_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={UNKNOWN_SITE}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "GET with unconfigured siteId must return 400"
    );
}

// User Story: support-multiple-website -- Querying low-recall records for one
// site must not return records belonging to another site.
// Covers: BE-D04; GET with siteId returns only that site's records while
// preserving default sort order.

#[tokio::test]
async fn list_returns_only_requested_site_records() {
    let state = test_app_state().await;

    insert_test_record(
        &state,
        SITE_A,
        Some("sess-iso-a1"),
        "site-a query 1",
        Some(0.1),
        1,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        SITE_A,
        Some("sess-iso-a2"),
        "site-a query 2",
        Some(0.2),
        1,
        "[]",
        Some("2025-01-02T00:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        SITE_B,
        Some("sess-iso-b1"),
        "site-b query 1",
        Some(0.1),
        1,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);

    // Query SITE_A
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_A}"),
    );
    let resp = app.clone().oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        2,
        "SITE_A query must return only SITE_A records"
    );
    assert_eq!(body["total"], 2, "total must count only SITE_A records");
    for item in items {
        assert_eq!(
            item["siteId"], SITE_A,
            "returned record must belong to SITE_A"
        );
    }

    // Query SITE_B
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_B}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        1,
        "SITE_B query must return only SITE_B records"
    );
    assert_eq!(body["total"], 1, "total must count only SITE_B records");
    assert_eq!(
        items[0]["siteId"], SITE_B,
        "returned record must belong to SITE_B"
    );
}

// User Story: support-multiple-website -- Score filtering must be scoped by
// siteId; a filter should not include low-score records from other sites.
// Covers: BE-D04; ?maxScore combined with siteId returns only matching records
// for the requested site.

#[tokio::test]
async fn list_site_isolation_preserves_score_filter() {
    let state = test_app_state().await;

    // SITE_A: one low, one high.
    insert_test_record(
        &state,
        SITE_A,
        Some("sess-filter-a-low"),
        "site-a low",
        Some(0.1),
        1,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    insert_test_record(
        &state,
        SITE_A,
        Some("sess-filter-a-high"),
        "site-a high",
        Some(0.9),
        1,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    // SITE_B: one low that must not leak into SITE_A results.
    insert_test_record(
        &state,
        SITE_B,
        Some("sess-filter-b-low"),
        "site-b low",
        Some(0.1),
        1,
        "[]",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);

    // SITE_A maxScore=0.4 -> only SITE_A low record
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_A}&maxScore=0.4"),
    );
    let resp = app.clone().oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        1,
        "SITE_A maxScore filter must return 1 record"
    );
    assert_eq!(body["total"], 1);
    assert_eq!(items[0]["sessionId"], "sess-filter-a-low");
    assert_eq!(items[0]["siteId"], SITE_A);

    // SITE_B maxScore=0.4 -> only SITE_B low record
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_B}&maxScore=0.4"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        1,
        "SITE_B maxScore filter must return 1 record"
    );
    assert_eq!(body["total"], 1);
    assert_eq!(items[0]["sessionId"], "sess-filter-b-low");
    assert_eq!(items[0]["siteId"], SITE_B);
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
        .uri(format!("/api/low-recall/records?siteId={SITE_A}"))
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
        .uri(format!("/api/low-recall/records?siteId={SITE_A}"))
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
//          when the table has no rows for the requested site.

#[tokio::test]
async fn list_empty_returns_empty() {
    let state = test_app_state().await;

    // No records inserted for SITE_A
    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        format!("/api/low-recall/records?siteId={SITE_A}"),
    );
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
        format!("/api/low-recall/records?siteId={SITE_A}&minScore=0.8&maxScore=0.2"),
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
        format!("/api/low-recall/records?siteId={SITE_A}&from=not-a-date"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "non-ISO8601 `from` must return 400"
    );
}
