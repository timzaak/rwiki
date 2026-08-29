//! Document status transition scenario tests.
//!
//! Verifies that the publish/unpublish endpoints enforce the correct state
//! machine for document statuses:
//!
//! - draft -> published  (publish)
//! - published -> draft  (unpublish)
//! - all other transitions are rejected with 409
//! - nonexistent documents return 404
//! - unauthenticated requests return 401

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::post;
use rig::client::EmbeddingsClient;
use tower::ServiceExt;
use uuid::Uuid;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

const TEST_API_TOKEN: &str = "test-api-token-12345";
const CHANNEL_A: &str = "channel-a";
const CHANNEL_B: &str = "channel-b";
const EMBEDDING_DIMS: usize = 1536;

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

/// Start a local mock server that mimics the OpenAI embeddings API.
///
/// Returns the server's base URL (e.g. `http://127.0.0.1:{port}`).
/// The server stays alive as long as the returned `AbortHandle` is not
/// triggered (it is dropped when the test finishes).
async fn start_mock_embedding_server() -> (String, tokio::task::AbortHandle) {
    let app = axum::Router::new().route("/embeddings", post(mock_embeddings_handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server to random port");
    let addr = listener.local_addr().expect("get mock server addr");
    let base_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server error");
    });

    (base_url, handle.abort_handle())
}

/// Axum handler that returns a fake OpenAI embeddings response.
///
/// Returns a 1536-dimensional zero vector for each input text, matching the
/// dimension used by `text-embedding-3-small`.  The rig library validates
/// that the response data length matches the input count, so we read the
/// `input` array from the request body.
async fn mock_embeddings_handler(
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    let input_count = body
        .get("input")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(1);

    let embedding: Vec<f64> = vec![0.0; EMBEDDING_DIMS];

    let data: Vec<serde_json::Value> = (0..input_count)
        .map(|i| {
            serde_json::json!({
                "object": "embedding",
                "index": i,
                "embedding": embedding
            })
        })
        .collect();

    axum::Json(serde_json::json!({
        "object": "list",
        "data": data,
        "model": "text-embedding-3-small",
        "usage": {
            "prompt_tokens": 0,
            "total_tokens": 0
        }
    }))
}

/// Build a `ChannelsConfig` with two channels for cross-channel isolation tests.
fn channels_config_with_a_and_b() -> rwiki_core::config::ChannelsConfig {
    let mut channels = HashMap::new();
    channels.insert(
        CHANNEL_A.to_string(),
        rwiki_core::config::ChannelConfig {
            name: "Site A".to_string(),
            system_prompt: None,
            suggested_questions: None,
        },
    );
    channels.insert(
        CHANNEL_B.to_string(),
        rwiki_core::config::ChannelConfig {
            name: "Site B".to_string(),
            system_prompt: None,
            suggested_questions: None,
        },
    );
    rwiki_core::config::ChannelsConfig { channels }
}

/// Build a minimal `AppState` suitable for document status tests.
async fn test_app_state() -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let (mock_base_url, _abort_handle) = start_mock_embedding_server().await;

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-doc-status-tests-only")
        .base_url(&mock_base_url);
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
        mcp_config: None,
        channels_config: channels_config_with_a_and_b(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Insert a test document row owned by `channel_id` with the given status and return its UUID.
async fn insert_test_document_with_channel(
    state: &Arc<AppState>,
    status: &str,
    channel_id: &str,
) -> Uuid {
    let doc_id = Uuid::now_v7();
    let doc_id_str = doc_id.to_string();
    let file_name = "test.xlsx".to_string();
    let status_val = status.to_string();
    let channel_id_val = channel_id.to_string();
    state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "INSERT INTO documents (id, file_name, status, row_count, channel_id) VALUES (?, ?, ?, 0, ?)",
                rusqlite::params![doc_id_str, file_name, status_val, channel_id_val],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("insert test document");
    doc_id
}

/// Insert a test document row owned by channel-a with the given status and return its UUID.
async fn insert_test_document(state: &Arc<AppState>, status: &str) -> Uuid {
    insert_test_document_with_channel(state, status, CHANNEL_A).await
}

/// Query the current status and owning channel of a document from the database.
async fn document_status_and_channel(
    state: &Arc<AppState>,
    doc_id: Uuid,
) -> Option<(String, String)> {
    let doc_id_str = doc_id.to_string();
    state
        .sqlite
        .call(move |conn| {
            let result = conn
                .query_row(
                    "SELECT status, channel_id FROM documents WHERE id = ?",
                    rusqlite::params![doc_id_str],
                    |row| {
                        let status: String = row.get(0)?;
                        let channel_id: String = row.get(1)?;
                        Ok((status, channel_id))
                    },
                )
                .ok();
            Ok::<_, rusqlite::Error>(result)
        })
        .await
        .expect("query document status and channel")
}

/// Helper to parse the JSON response body into a serde_json::Value.
async fn parse_json_body(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Build an authorized request for a given method and URI.
fn auth_request(method: Method, uri: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .body(Body::empty())
        .expect("build request")
}

/// Build a multipart upload request with the given file name, content, and channelId.
fn upload_request_with_channel(
    file_name: &str,
    content: &[u8],
    boundary: &str,
    channel_id: &str,
) -> Request<Body> {
    let mut body_bytes = Vec::new();
    body_bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body_bytes.extend_from_slice(b"Content-Disposition: form-data; name=\"channelId\"\r\n\r\n");
    body_bytes.extend_from_slice(channel_id.as_bytes());
    body_bytes.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body_bytes.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
            .as_bytes(),
    );
    body_bytes.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body_bytes.extend_from_slice(content);
    body_bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    Request::builder()
        .method(Method::POST)
        .uri("/api/documents/upload")
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_bytes))
        .expect("build upload request")
}

/// Build a multipart upload request without a channelId field.
fn upload_request_without_channel(
    file_name: &str,
    content: &[u8],
    boundary: &str,
) -> Request<Body> {
    let mut body_bytes = Vec::new();
    body_bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body_bytes.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
            .as_bytes(),
    );
    body_bytes.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body_bytes.extend_from_slice(content);
    body_bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    Request::builder()
        .method(Method::POST)
        .uri("/api/documents/upload")
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_bytes))
        .expect("build upload request")
}

// ---------------------------------------------------------------------------
// 1. Happy path: valid transitions
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to publish a draft document
// so that its content becomes available in RAG search results.
// Covers: BE-D01 (DocumentStatus enum), BE-D03 (publish handler)

#[tokio::test]
async fn publish_draft_document_returns_200_published() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "draft").await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/publish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "publishing draft must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(body["id"], doc_id.to_string(), "response id must match");
    assert_eq!(
        body["status"], "published",
        "response status must be 'published'"
    );
}

// User Story: As a knowledge base editor, I want to unpublish a published
// document so that its content is removed from RAG search results.
// Covers: BE-D01 (DocumentStatus enum), BE-D03 (unpublish handler)

#[tokio::test]
async fn unpublish_published_document_returns_200_draft() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "published").await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/unpublish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "unpublishing must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(body["id"], doc_id.to_string(), "response id must match");
    assert_eq!(body["status"], "draft", "response status must be 'draft'");
}

// ---------------------------------------------------------------------------
// 2. Conflict: invalid transitions
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want a clear error when I attempt
// to publish a document that is already published, so that I know the current
// state and do not double-publish.
// Covers: BE-D03 (publish rejects non-draft with 409)

#[tokio::test]
async fn publish_already_published_returns_409() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "published").await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/publish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "publishing an already-published document must return 409"
    );
}

// User Story: As a knowledge base editor, I want a clear error when I attempt
// to publish a document that is still processing, so that I wait for it to
// finish indexing before publishing.
// Covers: BE-D03 (publish rejects non-draft with 409), BE-D01 (Processing status)

#[tokio::test]
async fn publish_processing_document_returns_409() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "processing").await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/publish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "publishing a processing document must return 409"
    );
}

// User Story: As a knowledge base editor, I want a clear error when I attempt
// to unpublish a draft document, because it is already in draft state.
// Covers: BE-D03 (unpublish rejects non-published with 409)

#[tokio::test]
async fn unpublish_draft_document_returns_409() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "draft").await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/unpublish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "unpublishing a draft document must return 409"
    );
}

// User Story: As a knowledge base editor, I want a clear error when I attempt
// to unpublish a failed document, because only published documents can be
// unpublished.
// Covers: BE-D03 (unpublish rejects non-published with 409), BE-D01 (Failed status)

#[tokio::test]
async fn unpublish_failed_document_returns_409() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "failed").await;
    let app = create_api_routes(state);

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/unpublish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "unpublishing a failed document must return 409"
    );
}

// ---------------------------------------------------------------------------
// 3. Not found: nonexistent documents
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want a 404 when I attempt to
// publish a document that does not exist, so that I know the ID is wrong.
// Covers: BE-D03 (publish returns 404 for unknown ID)

#[tokio::test]
async fn publish_nonexistent_document_returns_404() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let phantom_id = Uuid::now_v7();
    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{phantom_id}/publish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "publishing a nonexistent document must return 404"
    );
}

// User Story: As a knowledge base editor, I want a 404 when I attempt to
// unpublish a document that does not exist, so that I know the ID is wrong.
// Covers: BE-D03 (unpublish returns 404 for unknown ID)

#[tokio::test]
async fn unpublish_nonexistent_document_returns_404() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let phantom_id = Uuid::now_v7();
    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{phantom_id}/unpublish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unpublishing a nonexistent document must return 404"
    );
}

// ---------------------------------------------------------------------------
// 4. Authentication: missing token
// ---------------------------------------------------------------------------

// User Story: As an API operator, I want publish requests without a token to
// be rejected, so that only authorized clients can change document visibility.
// Covers: BE-D03 (publish route is behind auth middleware)

#[tokio::test]
async fn publish_without_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let phantom_id = Uuid::now_v7();
    let req = Request::builder()
        .method(Method::PATCH)
        .uri(format!(
            "/api/documents/{phantom_id}/publish?channelId={CHANNEL_A}"
        ))
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "publish without token must return 401"
    );
}

// User Story: As an API operator, I want unpublish requests without a token
// to be rejected, so that only authorized clients can revert document
// visibility.
// Covers: BE-D03 (unpublish route is behind auth middleware)

#[tokio::test]
async fn unpublish_without_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let phantom_id = Uuid::now_v7();
    let req = Request::builder()
        .method(Method::PATCH)
        .uri(format!(
            "/api/documents/{phantom_id}/unpublish?channelId={CHANNEL_A}"
        ))
        .body(Body::empty())
        .expect("build request");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "unpublish without token must return 401"
    );
}

// ---------------------------------------------------------------------------
// 5. Upload initial status
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want newly uploaded documents to
// start in "draft" status, so that content is not immediately visible in RAG
// search before I review and publish it.
// Covers: BE-D01 (upload sets initial status to "draft")

#[tokio::test]
async fn upload_sets_initial_status_to_draft() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "# Upload Test\n\nDraft status body.";
    let req = upload_request_with_channel(
        "draft.md",
        content.as_bytes(),
        "----BoundaryDraft",
        CHANNEL_A,
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload with channelId must succeed"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        body["status"], "draft",
        "uploaded document must have 'draft' status"
    );
}

// ---------------------------------------------------------------------------
// 6. End-to-end status visibility via list
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want published documents to show
// "published" status in the document list, so that I can verify the transition
// took effect.
// Covers: BE-D01, BE-D03 (upload -> publish -> list shows "published")

#[tokio::test]
async fn publish_then_list_shows_published_status() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "draft").await;
    let app = create_api_routes(state.clone());

    // Publish the draft document
    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/publish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK, "publish must succeed");

    // List documents and find our document
    let app = create_api_routes(state);
    let req = auth_request(Method::GET, format!("/api/documents?channelId={CHANNEL_A}"));
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK, "list must return 200");

    let body = parse_json_body(resp.into_body()).await;
    let documents = body["documents"].as_array().expect("documents array");
    let found = documents
        .iter()
        .find(|d| d["id"] == doc_id.to_string())
        .expect("find our document in list");
    assert_eq!(
        found["status"], "published",
        "listed document must show 'published' status after publish"
    );
}

// User Story: As a knowledge base editor, I want unpublished documents to show
// "draft" status in the document list, so that I can verify the revert took
// effect.
// Covers: BE-D01, BE-D03 (upload -> publish -> unpublish -> list shows "draft")

#[tokio::test]
async fn unpublish_then_list_shows_draft_status() {
    let state = test_app_state().await;
    let doc_id = insert_test_document(&state, "published").await;
    let app = create_api_routes(state.clone());

    // Unpublish the document
    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/unpublish?channelId={CHANNEL_A}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK, "unpublish must succeed");

    // List documents and find our document
    let app = create_api_routes(state);
    let req = auth_request(Method::GET, format!("/api/documents?channelId={CHANNEL_A}"));
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK, "list must return 200");

    let body = parse_json_body(resp.into_body()).await;
    let documents = body["documents"].as_array().expect("documents array");
    let found = documents
        .iter()
        .find(|d| d["id"] == doc_id.to_string())
        .expect("find our document in list");
    assert_eq!(
        found["status"], "draft",
        "listed document must show 'draft' status after unpublish"
    );
}

// ---------------------------------------------------------------------------
// 7. Channel-scoped upload and lifecycle isolation (BE-T02)
// ---------------------------------------------------------------------------

// User Story: support-multiple-website — As an API operator, I want upload
// requests without a channelId to be rejected so documents always belong to a
// configured channel.
// Covers: BE-D02 (upload requires channelId, returns 400 when missing).
#[tokio::test]
async fn upload_without_channel_id_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "# Missing Site\n\nBody.";
    let req = upload_request_without_channel(
        "missing-channel.md",
        content.as_bytes(),
        "----BoundaryNoSite",
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload without channelId must return 400"
    );
}

// User Story: support-multiple-website — As a knowledge base editor, I want
// uploaded documents to carry the channelId I provided so they can be managed and
// retrieved within that channel.
// Covers: BE-D02 (upload persists documents.channel_id and returns it in response).
#[tokio::test]
async fn upload_with_channel_id_persists_channel_id() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "# Site Scoped\n\nBody.";
    let req = upload_request_with_channel(
        "scoped.md",
        content.as_bytes(),
        "----BoundaryScoped",
        CHANNEL_A,
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload with channelId must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        body["channelId"], CHANNEL_A,
        "upload response must echo the provided channelId"
    );
    assert_eq!(body["status"], "draft", "uploaded document must be draft");
}

// User Story: support-multiple-website — As a knowledge base editor, I want the
// document list to show only documents that belong to the channel I am managing.
// Covers: BE-D02 (list filters by channelId, response items include channelId).
#[tokio::test]
async fn list_with_channel_id_returns_only_that_channels_documents() {
    let state = test_app_state().await;
    let channel_a_doc = insert_test_document_with_channel(&state, "draft", CHANNEL_A).await;
    let channel_b_doc = insert_test_document_with_channel(&state, "draft", CHANNEL_B).await;
    let app = create_api_routes(state);

    let req = auth_request(Method::GET, format!("/api/documents?channelId={CHANNEL_A}"));
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK, "list must return 200");

    let body = parse_json_body(resp.into_body()).await;
    let documents = body["documents"].as_array().expect("documents array");
    assert_eq!(
        documents.len(),
        1,
        "listing channel A must return exactly one document"
    );
    assert_eq!(documents[0]["id"], channel_a_doc.to_string());
    assert_eq!(documents[0]["channelId"], CHANNEL_A);
    assert!(
        documents.iter().all(|d| d["channelId"] == CHANNEL_A),
        "list must not include documents from other channels"
    );

    // The other-channel document must still exist but not be returned.
    assert!(
        !documents
            .iter()
            .any(|d| d["id"] == channel_b_doc.to_string()),
        "channel B document must not appear in channel A list"
    );
}

// User Story: support-multiple-website — As a knowledge base editor, I want a
// 404 when I try to publish another channel's document, and that document must not
// be mutated.
// Covers: BE-D02 (cross-channel publish returns 404 and leaves the target doc alone).
#[tokio::test]
async fn publish_with_mismatched_channel_id_returns_404_and_does_not_mutate() {
    let state = test_app_state().await;
    let doc_id = insert_test_document_with_channel(&state, "draft", CHANNEL_A).await;
    let app = create_api_routes(state.clone());

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/publish?channelId={CHANNEL_B}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "publishing with mismatched channelId must return 404"
    );

    let (status, channel_id) = document_status_and_channel(&state, doc_id)
        .await
        .expect("document should still exist");
    assert_eq!(
        status, "draft",
        "document must remain draft after failed publish"
    );
    assert_eq!(
        channel_id, CHANNEL_A,
        "document must remain owned by channel A"
    );
}

// User Story: support-multiple-website — As a knowledge base editor, I want a
// 404 when I try to unpublish another channel's document, and that document must
// not be mutated.
// Covers: BE-D02 (cross-channel unpublish returns 404 and leaves the target doc alone).
#[tokio::test]
async fn unpublish_with_mismatched_channel_id_returns_404_and_does_not_mutate() {
    let state = test_app_state().await;
    let doc_id = insert_test_document_with_channel(&state, "published", CHANNEL_A).await;
    let app = create_api_routes(state.clone());

    let req = auth_request(
        Method::PATCH,
        format!("/api/documents/{doc_id}/unpublish?channelId={CHANNEL_B}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unpublishing with mismatched channelId must return 404"
    );

    let (status, channel_id) = document_status_and_channel(&state, doc_id)
        .await
        .expect("document should still exist");
    assert_eq!(
        status, "published",
        "document must remain published after failed unpublish"
    );
    assert_eq!(
        channel_id, CHANNEL_A,
        "document must remain owned by channel A"
    );
}

// User Story: support-multiple-website — As a knowledge base editor, I want a
// 404 when I try to delete another channel's document, and that document must not
// be removed.
// Covers: BE-D02 (cross-channel delete returns 404 and leaves the target doc alone).
#[tokio::test]
async fn delete_with_mismatched_channel_id_returns_404_and_does_not_mutate() {
    let state = test_app_state().await;
    let doc_id = insert_test_document_with_channel(&state, "draft", CHANNEL_A).await;
    let app = create_api_routes(state.clone());

    let req = auth_request(
        Method::DELETE,
        format!("/api/documents/{doc_id}?channelId={CHANNEL_B}"),
    );
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "deleting with mismatched channelId must return 404"
    );

    let still_exists = document_status_and_channel(&state, doc_id).await.is_some();
    assert!(
        still_exists,
        "document must not be deleted by cross-channel request"
    );
}
