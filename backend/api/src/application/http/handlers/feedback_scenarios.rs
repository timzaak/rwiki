//! Feedback submit/cancel and query list scenario tests.
//!
//! Verifies the two feedback endpoints:
//!
//! - POST `/api/chat/feedback` (unprotected): submit like/dislike, switch,
//!   cancel, and input validation
//! - GET `/api/chat/feedback` (Bearer Token): query with auth, type filter,
//!   pagination

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

/// Build a minimal `AppState` suitable for feedback tests.
async fn test_app_state() -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-feedback-tests-only");
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
    })
}

/// Helper to parse the JSON response body into a serde_json::Value.
async fn parse_json_body(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Build an authorized GET request for a given URI.
fn auth_request(method: Method, uri: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .body(Body::empty())
        .expect("build request")
}

/// Insert a feedback row directly into the database for pre-seeding query tests.
/// `created_at` overrides the default timestamp when provided.
async fn insert_test_feedback(
    state: &Arc<AppState>,
    session_id: &str,
    message_id: &str,
    feedback: &str,
    user_message: &str,
    assistant_message: &str,
    created_at: Option<&str>,
) {
    let sid = session_id.to_string();
    let mid = message_id.to_string();
    let fb = feedback.to_string();
    let um = user_message.to_string();
    let am = assistant_message.to_string();
    let ca = created_at.map(|s| s.to_string());

    state
        .sqlite
        .call(move |conn| -> Result<(), rusqlite::Error> {
            if let Some(ref ts) = ca {
                conn.execute(
                    "INSERT INTO chat_feedback (session_id, message_id, feedback, user_message, assistant_message, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![sid, mid, fb, um, am, ts],
                )?;
            } else {
                conn.execute(
                    "INSERT INTO chat_feedback (session_id, message_id, feedback, user_message, assistant_message) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![sid, mid, fb, um, am],
                )?;
            }
            Ok(())
        })
        .await
        .expect("insert test feedback");
}

/// Build a POST request with JSON body to `/api/chat/feedback`.
/// Sets Content-Type: application/json (required by axum's Json<T> extractor).
/// Does NOT include Authorization header (POST endpoint is unprotected).
fn feedback_post_request(body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/api/chat/feedback")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_string(&body).expect("serialize json"),
        ))
        .expect("build request")
}

// ---------------------------------------------------------------------------
// POST submit/cancel feedback scenarios (6 tests)
// ---------------------------------------------------------------------------

// User Story: US-CORE-029 -- As a user, I want to submit a like feedback for an
// assistant message so that I can indicate the response was helpful.
// Covers: Design 5.1 scenario 1; submit_feedback UPSERT with 'like' returns 204,
//          DB row has feedback='like' and correct session_id/message_id/content.

#[tokio::test]
async fn submit_like_feedback_returns_204_and_db_correct() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    let body = serde_json::json!({
        "sessionId": "sess-1",
        "messageId": "msg-1",
        "feedback": "like",
        "userMessage": "What is Rust?",
        "assistantMessage": "Rust is a systems programming language."
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "submit like must return 204"
    );

    // Verify DB row
    let row: (String, String, String, String, String) = state
        .sqlite
        .call(|conn| -> Result<_, rusqlite::Error> {
            conn.query_row(
                "SELECT session_id, message_id, feedback, user_message, assistant_message \
                 FROM chat_feedback WHERE session_id = 'sess-1' AND message_id = 'msg-1'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
        })
        .await
        .expect("query feedback row");

    assert_eq!(row.0, "sess-1", "session_id must match");
    assert_eq!(row.1, "msg-1", "message_id must match");
    assert_eq!(row.2, "like", "feedback must be 'like'");
    assert_eq!(row.3, "What is Rust?", "user_message must match");
    assert_eq!(
        row.4, "Rust is a systems programming language.",
        "assistant_message must match"
    );
}

// User Story: US-CORE-029 -- As a user, I want to submit a dislike feedback to
// indicate the response was unhelpful.
// Covers: Design 5.1 scenario 2; submit_feedback with 'dislike' returns 204.

#[tokio::test]
async fn submit_dislike_feedback_returns_204() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    let body = serde_json::json!({
        "sessionId": "sess-2",
        "messageId": "msg-2",
        "feedback": "dislike",
        "userMessage": "Explain async",
        "assistantMessage": "Async is complicated."
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "submit dislike must return 204"
    );

    // Verify DB row has feedback='dislike'
    let feedback: String = state
        .sqlite
        .call(|conn| -> Result<_, rusqlite::Error> {
            conn.query_row(
                "SELECT feedback FROM chat_feedback WHERE session_id = 'sess-2' AND message_id = 'msg-2'",
                [],
                |row| row.get::<_, String>(0),
            )
        })
        .await
        .expect("query feedback row");

    assert_eq!(feedback, "dislike", "feedback must be 'dislike'");
}

// User Story: US-CORE-029 -- As a user, I want to switch my feedback from like
// to dislike so my latest preference is recorded.
// Covers: Design 5.1 scenario 3; UPSERT updates existing record in-place.

#[tokio::test]
async fn switch_feedback_like_to_dislike_updates_db() {
    let state = test_app_state().await;

    // First submit like
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "sess-3",
        "messageId": "msg-3",
        "feedback": "like",
        "userMessage": "Q1",
        "assistantMessage": "A1"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "first submit must return 204"
    );

    // Then submit dislike with same session/message
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "sess-3",
        "messageId": "msg-3",
        "feedback": "dislike",
        "userMessage": "Q1",
        "assistantMessage": "A1"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "switch must return 204"
    );

    // Verify single row with feedback='dislike'
    let (feedback, count): (String, i64) = state
        .sqlite
        .call(|conn| -> Result<_, rusqlite::Error> {
            let feedback = conn.query_row(
                "SELECT feedback FROM chat_feedback WHERE session_id = 'sess-3' AND message_id = 'msg-3'",
                [],
                |row| row.get::<_, String>(0),
            )?;
            let count = conn.query_row(
                "SELECT COUNT(*) FROM chat_feedback WHERE session_id = 'sess-3' AND message_id = 'msg-3'",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            Ok((feedback, count))
        })
        .await
        .expect("query feedback");

    assert_eq!(
        feedback, "dislike",
        "feedback must be switched to 'dislike'"
    );
    assert_eq!(
        count, 1,
        "there must be exactly one row (UPSERT, not insert)"
    );
}

// User Story: US-CORE-029 -- As a user, I want to cancel my feedback so it is
// removed from the system.
// Covers: Design 5.1 scenario 4; feedback=null triggers DELETE, record removed.
//          Idempotent: canceling non-existent feedback also returns 204.

#[tokio::test]
async fn cancel_feedback_deletes_db_record() {
    let state = test_app_state().await;

    // First submit like
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "sess-4",
        "messageId": "msg-4",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "submit like must return 204"
    );

    // Cancel with feedback=null
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "sess-4",
        "messageId": "msg-4",
        "feedback": null,
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "cancel must return 204"
    );

    // Verify DB has 0 rows
    let count: i64 = state
        .sqlite
        .call(|conn| -> Result<_, rusqlite::Error> {
            conn.query_row(
                "SELECT COUNT(*) FROM chat_feedback WHERE session_id = 'sess-4' AND message_id = 'msg-4'",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .await
        .expect("count rows");
    assert_eq!(count, 0, "feedback record must be deleted after cancel");

    // Idempotent: cancel again on non-existent record still returns 204
    let app = create_api_routes(state);
    let body = serde_json::json!({
        "sessionId": "sess-4",
        "messageId": "msg-4",
        "feedback": null,
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "idempotent cancel must also return 204"
    );
}

// User Story: US-CORE-029 -- As a system, I want invalid input rejected with a
// clear error so callers know what is wrong.
// Covers: Design 5.1 scenario 5; empty sessionId/messageId returns 400.

#[tokio::test]
async fn missing_required_fields_returns_400() {
    let state = test_app_state().await;

    // Empty sessionId
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "",
        "messageId": "msg-5",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty sessionId must return 400"
    );

    // Empty messageId
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "sess-5",
        "messageId": "",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty messageId must return 400"
    );

    // Empty sessionId and messageId (whitespace-only also counts)
    let app = create_api_routes(state);
    let body = serde_json::json!({
        "sessionId": "   ",
        "messageId": "   ",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "whitespace-only sessionId/messageId must return 400"
    );
}

// User Story: US-CORE-029 -- As a system, I want invalid feedback values rejected
// so only 'like'/'dislike'/null are accepted.
// Covers: Design 5.1 scenario 6; feedback not in like/dislike/null returns 400.

#[tokio::test]
async fn invalid_feedback_value_returns_400() {
    let state = test_app_state().await;

    // feedback: "meh"
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "sess-6",
        "messageId": "msg-6",
        "feedback": "meh",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "feedback='meh' must return 400"
    );

    // feedback: "unknown"
    let app = create_api_routes(state);
    let body = serde_json::json!({
        "sessionId": "sess-6",
        "messageId": "msg-6",
        "feedback": "unknown",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "feedback='unknown' must return 400"
    );
}

// ---------------------------------------------------------------------------
// GET query feedback scenarios (4 tests)
// ---------------------------------------------------------------------------

// User Story: US-CORE-029 -- As an operator, I want to query feedback records
// to analyze knowledge base quality.
// Covers: Design 5.1 scenario 7; GET with valid token returns 200, items + total.

#[tokio::test]
async fn query_feedback_list_with_auth_returns_200() {
    let state = test_app_state().await;

    // Pre-seed 2 feedback records
    insert_test_feedback(
        &state,
        "sess-q1",
        "msg-q1",
        "like",
        "User Q1",
        "Assistant A1",
        None,
    )
    .await;
    insert_test_feedback(
        &state,
        "sess-q2",
        "msg-q2",
        "dislike",
        "User Q2",
        "Assistant A2",
        None,
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(Method::GET, "/api/chat/feedback".to_string());
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "authenticated GET must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2, "must return 2 items");
    assert_eq!(body["total"], 2, "total must be 2");

    // Verify each item has expected fields
    for item in items {
        assert!(item.get("id").is_some(), "item must have 'id'");
        assert!(
            item.get("sessionId").is_some(),
            "item must have 'sessionId'"
        );
        assert!(
            item.get("messageId").is_some(),
            "item must have 'messageId'"
        );
        assert!(item.get("feedback").is_some(), "item must have 'feedback'");
        assert!(
            item.get("userMessage").is_some(),
            "item must have 'userMessage'"
        );
        assert!(
            item.get("assistantMessage").is_some(),
            "item must have 'assistantMessage'"
        );
        assert!(
            item.get("createdAt").is_some(),
            "item must have 'createdAt'"
        );
    }
}

// User Story: US-CORE-029 -- As a system, I want unauthenticated queries rejected
// so only authorized operators can view feedback data.
// Covers: Design 5.1 scenario 8; GET without Bearer Token returns 401.

#[tokio::test]
async fn query_feedback_list_without_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    // GET without Authorization header
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/chat/feedback")
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET without token must return 401"
    );
}

// User Story: US-CORE-029 -- As an operator, I want to filter feedback by type
// to focus on positive or negative signals.
// Covers: Design 5.1 scenario 9; GET with ?feedback=like returns only like records.

#[tokio::test]
async fn query_feedback_with_type_filter_returns_matching() {
    let state = test_app_state().await;

    // Pre-seed 1 like + 1 dislike
    insert_test_feedback(
        &state,
        "sess-f1",
        "msg-f1",
        "like",
        "User Q",
        "Assistant A",
        None,
    )
    .await;
    insert_test_feedback(
        &state,
        "sess-f2",
        "msg-f2",
        "dislike",
        "User Q2",
        "Assistant A2",
        None,
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(Method::GET, "/api/chat/feedback?feedback=like".to_string());
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "filtered GET must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "must return 1 like item");
    assert_eq!(body["total"], 1, "total must be 1");
    assert_eq!(
        items[0]["feedback"], "like",
        "returned item must have feedback='like'"
    );
}

// User Story: US-CORE-029 -- As an operator, I want paginated feedback results
// to handle large datasets without overloading the UI.
// Covers: Design 5.1 scenario 10; GET with limit/offset returns correct page.

#[tokio::test]
async fn query_feedback_with_pagination_works() {
    let state = test_app_state().await;

    // Pre-seed 3 feedback records with explicit created_at for deterministic sort order
    insert_test_feedback(
        &state,
        "sess-p1",
        "msg-p1",
        "like",
        "User 1",
        "Assistant 1",
        Some("2025-01-01T00:00:00Z"),
    )
    .await;
    insert_test_feedback(
        &state,
        "sess-p2",
        "msg-p2",
        "dislike",
        "User 2",
        "Assistant 2",
        Some("2025-01-02T00:00:00Z"),
    )
    .await;
    insert_test_feedback(
        &state,
        "sess-p3",
        "msg-p3",
        "like",
        "User 3",
        "Assistant 3",
        Some("2025-01-03T00:00:00Z"),
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        "/api/chat/feedback?limit=2&offset=0".to_string(),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "paginated GET must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 2, "must return 2 items for limit=2");
    assert_eq!(body["total"], 3, "total must be 3 across all pages");

    // Verify items are sorted by createdAt DESC (newest first)
    assert_eq!(
        items[0]["sessionId"], "sess-p3",
        "first item must be newest (2025-01-03)"
    );
    assert_eq!(
        items[1]["sessionId"], "sess-p2",
        "second item must be middle (2025-01-02)"
    );
}
