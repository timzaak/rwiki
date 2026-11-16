//! Feedback submit/cancel and query list scenario tests.
//!
//! Verifies the two feedback endpoints:
//!
//! - POST `/api/chat/feedback` (unprotected): submit like/dislike, switch,
//!   cancel, input validation, and channel isolation
//! - GET `/api/chat/feedback` (Bearer Token): query with auth, type filter,
//!   pagination, and channel isolation

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;
use rwiki_core::config::{ChannelConfig, ChannelsConfig};

const TEST_API_TOKEN: &str = "test-api-token-12345";
const CHANNEL_A: &str = "help_center";
const CHANNEL_B: &str = "dev_docs";
const UNKNOWN_CHANNEL: &str = "unknown_channel";

/// Build a `ChannelsConfig` with two configured channels for isolation scenarios.
fn test_channels_config() -> ChannelsConfig {
    let mut channels = HashMap::new();
    channels.insert(
        CHANNEL_A.to_string(),
        ChannelConfig {
            name: "Help Center".to_string(),
            system_prompt: None,
            suggested_questions: None,
            content_language: None,
        },
    );
    channels.insert(
        CHANNEL_B.to_string(),
        ChannelConfig {
            name: "Developer Docs".to_string(),
            system_prompt: None,
            suggested_questions: None,
            content_language: None,
        },
    );
    ChannelsConfig { channels }
}

/// Build a minimal `AppState` suitable for feedback tests.
async fn test_app_state() -> Arc<AppState> {
    let storage = rwiki_core::infrastructure::storage_testing::make_storage("feedback").await;

    let embedding_model = rwiki_core::infrastructure::rig_client::connect(
        rig::providers::openai::OpenAIConfig::new("sk-test-fake-key-for-feedback-tests-only"),
    )
    .embedding("text-embedding-3-small", None);
    let app_embedding_model =
        rwiki_core::infrastructure::embedding_model::AppEmbeddingModel::new(embedding_model);

    let vector_store = Arc::new(
        rwiki_core::infrastructure::vector_store::VectorStoreManager::new(
            storage.clone(),
            app_embedding_model,
            "text-embedding-3-small".to_string(),
        ),
    );

    let llm_client =
        rwiki_core::infrastructure::rig_client::connect_at("sk-test-fake", "http://localhost:0");

    Arc::new(AppState {
        store: Arc::new(storage),
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
        mcp_config: None,
        channels_config: test_channels_config(),
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
#[allow(clippy::too_many_arguments)]
async fn insert_test_feedback(
    state: &Arc<AppState>,
    channel_id: &str,
    session_id: &str,
    message_id: &str,
    feedback: &str,
    user_message: &str,
    assistant_message: &str,
    created_at: Option<&str>,
) {
    rwiki_core::infrastructure::storage_testing::seed_feedback(
        &state.store,
        channel_id,
        session_id,
        message_id,
        feedback,
        user_message,
        assistant_message,
        created_at,
    )
    .await;
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

// User Story: US-CORE-029 -- As a user, I want to submit a like feedback for an
// assistant message so that I can indicate the response was helpful.
// Covers: Design 5.1 scenario 1; submit_feedback UPSERT with 'like' returns 204,
//          DB row has feedback='like' and correct session_id/message_id/content.

#[tokio::test]
async fn submit_like_feedback_returns_204_and_db_correct() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    let body = serde_json::json!({
        "channelId": [CHANNEL_A],
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
    let rows =
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_A)
            .await;
    let row = rows
        .iter()
        .find(|r| r.session_id == "sess-1" && r.message_id == "msg-1")
        .expect("feedback row must exist");

    assert_eq!(row.session_id, "sess-1", "session_id must match");
    assert_eq!(row.message_id, "msg-1", "message_id must match");
    assert_eq!(row.feedback, "like", "feedback must be 'like'");
    assert_eq!(row.user_message, "What is Rust?", "user_message must match");
    assert_eq!(
        row.assistant_message, "Rust is a systems programming language.",
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
        "channelId": [CHANNEL_A],
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
    let feedback =
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_A)
            .await
            .iter()
            .find(|r| r.session_id == "sess-2" && r.message_id == "msg-2")
            .expect("feedback row must exist")
            .feedback
            .clone();

    assert_eq!(feedback, "dislike", "feedback must be 'dislike'");
}

// User Story: support-multiple-website -- Feedback must be recorded under the
// requested channel so that operational data is isolated by channel.
// Covers: BE-D04; submit_feedback with a valid channelId persists chat_feedback.channel_id.

#[tokio::test]
async fn submit_feedback_with_channel_id_persists_channel_id() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    let body = serde_json::json!({
        "channelId": [CHANNEL_B],
        "sessionId": "sess-channel",
        "messageId": "msg-channel",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "submit with channelId must return 204"
    );

    // 反馈行落在请求的频道（按 sess/msg 定位；记录经 CHANNEL_B 频道读取）
    let channel_rows =
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_B)
            .await;
    assert!(
        channel_rows
            .iter()
            .any(|r| r.session_id == "sess-channel" && r.message_id == "msg-channel"),
        "chat_feedback.row must exist under requested channel {CHANNEL_B}"
    );
}

// User Story: support-multiple-website -- The system must reject feedback that
// does not identify a configured channel, so records cannot be written without a
// valid isolation scope.
// Covers: BE-D04; missing/empty/whitespace channelId returns 400.

#[tokio::test]
async fn missing_or_empty_channel_id_returns_400() {
    let state = test_app_state().await;

    // Missing channelId
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "sessionId": "sess-missing",
        "messageId": "msg-missing",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "missing channelId must return 400"
    );

    // Empty channelId (array with an empty-string element → Empty)
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "channelId": [""],
        "sessionId": "sess-empty",
        "messageId": "msg-empty",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty channelId must return 400"
    );

    // Whitespace-only channelId (array with a whitespace-only element → Empty)
    let app = create_api_routes(state);
    let body = serde_json::json!({
        "channelId": ["   "],
        "sessionId": "sess-ws",
        "messageId": "msg-ws",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "whitespace-only channelId must return 400"
    );
}

// User Story: support-multiple-website -- Only configured channels are accepted;
// an unknown channelId must be rejected before any DB write.
// Covers: BE-D04; unconfigured channelId returns 400.

#[tokio::test]
async fn unconfigured_channel_id_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    let body = serde_json::json!({
        "channelId": [UNKNOWN_CHANNEL],
        "sessionId": "sess-unknown",
        "messageId": "msg-unknown",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let req = feedback_post_request(body);
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "unconfigured channelId must return 400"
    );

    // Ensure no row was written（两个已配置频道均无该外部 id 的记录）
    let count = [
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_A)
            .await,
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_B)
            .await,
    ]
    .concat()
    .iter()
    .filter(|r| r.session_id == "sess-unknown" && r.message_id == "msg-unknown")
    .count() as i64;
    assert_eq!(
        count, 0,
        "no feedback row must be written for unconfigured channel"
    );
}

// User Story: support-multiple-website -- Identical session/message identifiers
// in different channels must not collide or overwrite each other.
// Covers: BE-D04; UNIQUE(channel_id, session_id, message_id) plus UPSERT by channel
// keeps records isolated.

#[tokio::test]
async fn identical_session_message_in_two_channels_does_not_overwrite() {
    let state = test_app_state().await;

    // Submit like for CHANNEL_A
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "channelId": [CHANNEL_A],
        "sessionId": "shared-sess",
        "messageId": "shared-msg",
        "feedback": "like",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let resp = app
        .oneshot(feedback_post_request(body))
        .await
        .expect("send request");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Submit dislike for CHANNEL_B with the same external ids
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "channelId": [CHANNEL_B],
        "sessionId": "shared-sess",
        "messageId": "shared-msg",
        "feedback": "dislike",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let resp = app
        .oneshot(feedback_post_request(body))
        .await
        .expect("send request");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Update CHANNEL_A to dislike
    let app = create_api_routes(state.clone());
    let body = serde_json::json!({
        "channelId": [CHANNEL_A],
        "sessionId": "shared-sess",
        "messageId": "shared-msg",
        "feedback": "dislike",
        "userMessage": "Q",
        "assistantMessage": "A"
    });
    let resp = app
        .oneshot(feedback_post_request(body))
        .await
        .expect("send request");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify both rows are isolated
    use rwiki_core::infrastructure::storage::FeedbackRecord;
    let find_fb = |rows: &[FeedbackRecord]| {
        rows.iter()
            .find(|r| r.session_id == "shared-sess" && r.message_id == "shared-msg")
            .expect("shared feedback row must exist in channel")
            .feedback
            .clone()
    };
    let channel_a_rows =
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_A)
            .await;
    let channel_b_rows =
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_B)
            .await;
    let channel_a_fb = find_fb(&channel_a_rows);
    let channel_b_fb = find_fb(&channel_b_rows);
    let total = channel_a_rows.len() + channel_b_rows.len();

    assert_eq!(
        channel_a_fb, "dislike",
        "CHANNEL_A record must be updated independently"
    );
    assert_eq!(
        channel_b_fb, "dislike",
        "CHANNEL_B record must keep its own value"
    );
    assert_eq!(
        total, 2,
        "two isolated records must exist, not one overwritten row"
    );
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
        "channelId": [CHANNEL_A],
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
        "channelId": [CHANNEL_A],
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
    let rows =
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_A)
            .await;
    let hits: Vec<_> = rows
        .iter()
        .filter(|r| r.session_id == "sess-3" && r.message_id == "msg-3")
        .collect();
    assert_eq!(hits.len(), 1);
    let feedback = hits[0].feedback.clone();
    let count = hits.len() as i64;

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
        "channelId": [CHANNEL_A],
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
        "channelId": [CHANNEL_A],
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

    // Verify DB has 0 rows for CHANNEL_A
    let count =
        rwiki_core::infrastructure::storage_testing::fetch_feedback_rows(&state.store, CHANNEL_A)
            .await
            .iter()
            .filter(|r| r.session_id == "sess-4" && r.message_id == "msg-4")
            .count() as i64;
    assert_eq!(count, 0, "feedback record must be deleted after cancel");

    // Idempotent: cancel again on non-existent record still returns 204
    let app = create_api_routes(state);
    let body = serde_json::json!({
        "channelId": [CHANNEL_A],
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
        "channelId": [CHANNEL_A],
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
        "channelId": [CHANNEL_A],
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
        "channelId": [CHANNEL_A],
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
        "channelId": [CHANNEL_A],
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
        "channelId": [CHANNEL_A],
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

// User Story: US-CORE-029 -- As an operator, I want to query feedback records
// to analyze knowledge base quality.
// Covers: Design 5.1 scenario 7; GET with valid token returns 200, items + total.

#[tokio::test]
async fn query_feedback_list_with_auth_returns_200() {
    let state = test_app_state().await;

    // Pre-seed 2 feedback records for CHANNEL_A
    insert_test_feedback(
        &state,
        CHANNEL_A,
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
        CHANNEL_A,
        "sess-q2",
        "msg-q2",
        "dislike",
        "User Q2",
        "Assistant A2",
        None,
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        format!("/api/chat/feedback?channelId={CHANNEL_A}"),
    );
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
            item.get("channelId").is_some(),
            "item must have 'channelId'"
        );
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
        .uri(format!("/api/chat/feedback?channelId={CHANNEL_A}"))
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "GET without token must return 401"
    );
}

// User Story: support-multiple-website -- Feedback queries must require a channelId
// and reject requests that do not specify one.
// Covers: BE-D04; GET /api/chat/feedback without channelId returns 400.

#[tokio::test]
async fn query_feedback_missing_channel_id_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = auth_request(Method::GET, "/api/chat/feedback".to_string());
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "GET without channelId must return 400"
    );
}

// User Story: support-multiple-website -- Only configured channels may be queried.
// Covers: BE-D04; GET /api/chat/feedback with unconfigured channelId returns 400.

#[tokio::test]
async fn query_feedback_unconfigured_channel_id_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::GET,
        format!("/api/chat/feedback?channelId={UNKNOWN_CHANNEL}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "GET with unconfigured channelId must return 400"
    );
}

// User Story: support-multiple-website -- Querying feedback for one channel must
// not return records belonging to another channel.
// Covers: BE-D04; GET with channelId returns only that channel's records.

#[tokio::test]
async fn query_feedback_returns_only_requested_channel_records() {
    let state = test_app_state().await;

    insert_test_feedback(
        &state,
        CHANNEL_A,
        "sess-iso-a1",
        "msg-iso-a1",
        "like",
        "A1",
        "A",
        None,
    )
    .await;
    insert_test_feedback(
        &state,
        CHANNEL_A,
        "sess-iso-a2",
        "msg-iso-a2",
        "dislike",
        "A2",
        "A",
        None,
    )
    .await;
    insert_test_feedback(
        &state,
        CHANNEL_B,
        "sess-iso-b1",
        "msg-iso-b1",
        "like",
        "B1",
        "B",
        None,
    )
    .await;

    let app = create_api_routes(state);

    // Query CHANNEL_A
    let req = auth_request(
        Method::GET,
        format!("/api/chat/feedback?channelId={CHANNEL_A}"),
    );
    let resp = app.clone().oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        2,
        "CHANNEL_A query must return only CHANNEL_A records"
    );
    assert_eq!(body["total"], 2, "total must count only CHANNEL_A records");
    for item in items {
        assert_eq!(
            item["channelId"], CHANNEL_A,
            "returned record must belong to CHANNEL_A"
        );
    }

    // Query CHANNEL_B
    let req = auth_request(
        Method::GET,
        format!("/api/chat/feedback?channelId={CHANNEL_B}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(
        items.len(),
        1,
        "CHANNEL_B query must return only CHANNEL_B records"
    );
    assert_eq!(body["total"], 1, "total must count only CHANNEL_B records");
    assert_eq!(
        items[0]["channelId"], CHANNEL_B,
        "returned record must belong to CHANNEL_B"
    );
}

// User Story: support-multiple-website -- The existing feedback-type filter must
// continue to work and must not leak records from other channels.
// Covers: BE-D04; ?feedback=like scoped by channelId returns only matching records
// for the requested channel.

#[tokio::test]
async fn query_feedback_type_filter_honors_channel_isolation() {
    let state = test_app_state().await;

    // CHANNEL_A: one like, one dislike
    insert_test_feedback(
        &state,
        CHANNEL_A,
        "sess-ft-a-like",
        "msg-ft-a-like",
        "like",
        "Q",
        "A",
        None,
    )
    .await;
    insert_test_feedback(
        &state,
        CHANNEL_A,
        "sess-ft-a-dislike",
        "msg-ft-a-dislike",
        "dislike",
        "Q",
        "A",
        None,
    )
    .await;
    // CHANNEL_B: one like
    insert_test_feedback(
        &state,
        CHANNEL_B,
        "sess-ft-b-like",
        "msg-ft-b-like",
        "like",
        "Q",
        "A",
        None,
    )
    .await;

    let app = create_api_routes(state);

    // CHANNEL_A filtered by like -> only its own like
    let req = auth_request(
        Method::GET,
        format!("/api/chat/feedback?channelId={CHANNEL_A}&feedback=like"),
    );
    let resp = app.clone().oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "CHANNEL_A like filter must return 1 record");
    assert_eq!(body["total"], 1, "total must be 1");
    assert_eq!(items[0]["channelId"], CHANNEL_A);
    assert_eq!(items[0]["feedback"], "like");

    // CHANNEL_B filtered by like -> its own like
    let req = auth_request(
        Method::GET,
        format!("/api/chat/feedback?channelId={CHANNEL_B}&feedback=like"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json_body(resp.into_body()).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "CHANNEL_B like filter must return 1 record");
    assert_eq!(body["total"], 1, "total must be 1");
    assert_eq!(items[0]["channelId"], CHANNEL_B);
}

// User Story: US-CORE-029 -- As an operator, I want to filter feedback by type
// to focus on positive or negative signals.
// Covers: Design 5.1 scenario 9; GET with ?feedback=like returns only like records.

#[tokio::test]
async fn query_feedback_with_type_filter_returns_matching() {
    let state = test_app_state().await;

    // Pre-seed 1 like + 1 dislike for CHANNEL_A
    insert_test_feedback(
        &state,
        CHANNEL_A,
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
        CHANNEL_A,
        "sess-f2",
        "msg-f2",
        "dislike",
        "User Q2",
        "Assistant A2",
        None,
    )
    .await;

    let app = create_api_routes(state);
    let req = auth_request(
        Method::GET,
        format!("/api/chat/feedback?channelId={CHANNEL_A}&feedback=like"),
    );
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
        CHANNEL_A,
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
        CHANNEL_A,
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
        CHANNEL_A,
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
        format!("/api/chat/feedback?channelId={CHANNEL_A}&limit=2&offset=0"),
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
