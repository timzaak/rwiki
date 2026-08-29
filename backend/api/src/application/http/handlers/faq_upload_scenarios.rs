//! FAQ JSONL upload scenario tests.
//!
//! Verifies the `.jsonl` upload pipeline routes correctly to the FAQ parser
//! (one JSON object per line), and that FAQ error variants surface as 400
//! responses matching the `FaqParseError` Display contract. Also verifies
//! OpenAPI `.json` uploads are unaffected by the FAQ support split.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::post;
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

const TEST_API_TOKEN: &str = "test-faq-upload-token";
const EMBEDDING_DIMS: usize = 1536;
const CHANNEL_ID: &str = "channel-a";

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
/// dimension used by `text-embedding-3-small`.
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

/// Build a minimal `AppState` suitable for FAQ upload tests.
///
/// Starts a local mock embedding server so the upload pipeline can
/// complete without calling the real OpenAI API.
async fn test_app_state() -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let (mock_base_url, _abort_handle) = start_mock_embedding_server().await;

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-faq-upload-tests-only")
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
        channels_config: {
            let mut channels = HashMap::new();
            channels.insert(
                CHANNEL_ID.to_string(),
                rwiki_core::config::ChannelConfig {
                    name: "Site A".to_string(),
                    system_prompt: None,
                    suggested_questions: None,
                },
            );
            rwiki_core::config::ChannelsConfig { channels }
        },
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

/// Helper to read the full response body as a String.
async fn read_body_string(body: Body) -> String {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("body is utf-8")
}

/// Build a multipart upload request with the given file name, content, and channelId.
fn upload_request(file_name: &str, content: &[u8], boundary: &str) -> Request<Body> {
    let mut body_bytes = Vec::new();
    body_bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body_bytes.extend_from_slice(b"Content-Disposition: form-data; name=\"channelId\"\r\n\r\n");
    body_bytes.extend_from_slice(CHANNEL_ID.as_bytes());
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
// Test fixtures
// ---------------------------------------------------------------------------

/// Valid FAQ JSON array with 2 Q&A pairs.
/// Valid FAQ JSONL with 2 Q&A pairs (one JSON object per line).
fn valid_faq_jsonl() -> Vec<u8> {
    b"{\"question\": \"Q1\", \"answer\": \"A1\"}\n{\"question\": \"Q2\", \"answer\": \"A2\"}\n"
        .to_vec()
}

/// FAQ JSONL whose only entry is missing the required `answer` field.
fn faq_missing_answer_jsonl() -> Vec<u8> {
    b"{\"question\": \"Q\"}\n".to_vec()
}

/// Empty file (no bytes) — must be rejected as "no usable Q&A data".
fn empty_faq_jsonl() -> Vec<u8> {
    Vec::new()
}

/// Plain JSON array of objects lacking FAQ fields, uploaded as `.json`.
/// Must be rejected by the OpenAPI parser (not FAQ-routed) since `.json`
/// now exclusively means OpenAPI.
fn non_openapi_array_json() -> Vec<u8> {
    r#"[{"title": "Q", "content": "A"}]"#.as_bytes().to_vec()
}

/// Minimal valid OpenAPI 3.0 JSON with one GET endpoint.
fn valid_openapi_json() -> Vec<u8> {
    r#"{
  "openapi": "3.0.0",
  "info": { "title": "Petstore", "version": "1.0.0" },
  "paths": {
    "/pets": {
      "get": {
        "operationId": "listPets",
        "summary": "List all pets",
        "responses": { "200": { "description": "A list of pets" } }
      }
    }
  }
}"#
    .as_bytes()
    .to_vec()
}

// ---------------------------------------------------------------------------
// 1. Upload valid FAQ JSONL -> 200, status=draft, rowCount=2
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to upload a FAQ JSONL file
// so that each Q&A becomes an independent knowledge page in draft status,
// ready for review before publishing. (US-CORE-032 scenario 1)
// Covers: US-CORE-032, BE-D01, BE-T01

#[tokio::test]
async fn upload_valid_faq_jsonl_returns_200_draft() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request("faq.jsonl", &valid_faq_jsonl(), "----BoundaryFaqValid");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload valid FAQ JSONL must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        body["status"], "draft",
        "uploaded document must have 'draft' status"
    );
    assert_eq!(body["rowCount"], 2, "rowCount must be 2 for 2 Q&A pairs");
    assert!(
        body["id"].is_string() && !body["id"].as_str().unwrap().is_empty(),
        "response must have a non-empty id"
    );
    assert_eq!(body["fileName"], "faq.jsonl", "fileName must be faq.jsonl");
}

// ---------------------------------------------------------------------------
// 2. Upload FAQ missing required field -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want FAQ files with missing
// required fields to be rejected with a 400 that pinpoints the failing entry
// and field, so I can fix the source file at the exact location.
// (US-CORE-032 scenario 2)
// Covers: US-CORE-032, BE-D01, BE-T01

#[tokio::test]
async fn upload_faq_missing_answer_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request(
        "faq-missing-answer.jsonl",
        &faq_missing_answer_jsonl(),
        "----BoundaryFaqMissingAnswer",
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload FAQ missing required field must return 400"
    );

    let body_text = read_body_string(resp.into_body()).await;
    // Q&A item 0 is missing required field(s)
    assert!(
        body_text.contains("Q&A item 0 is missing required field(s)"),
        "error message must contain 'Q&A item 0 is missing required field(s)', got: {body_text}"
    );
    // answer
    assert!(
        body_text.contains("answer"),
        "error message must contain the missing field name 'answer', got: {body_text}"
    );
}

// ---------------------------------------------------------------------------
// 3. Upload empty FAQ file -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want an empty FAQ file upload to
// be rejected with a clear "no usable Q&A data" message, so I notice the file
// is empty instead of accidentally creating an empty draft document.
// (US-CORE-032 scenario 3)
// Covers: US-CORE-032, BE-D01, BE-T01

#[tokio::test]
async fn upload_empty_faq_file_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request("empty.jsonl", &empty_faq_jsonl(), "----BoundaryEmptyFaq");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload empty FAQ array must return 400"
    );

    let body_text = read_body_string(resp.into_body()).await;
    // No usable Q&A records found in the file
    assert!(
        body_text.contains("No usable Q&A records found in the file"),
        "error message must contain 'No usable Q&A records found in the file', got: {body_text}"
    );
}

// ---------------------------------------------------------------------------
// 4. Upload `.json` non-OpenAPI -> 400 (OpenAPI parser rejects it)
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want a `.json` upload that is not
// a valid OpenAPI document to be rejected by the OpenAPI parser, so that
// `.json` exclusively routes to OpenAPI and FAQ arrays cannot be uploaded as
// `.json` anymore. (US-CORE-032 scenario 4)
// Covers: US-CORE-032, BE-D01, BE-T01

#[tokio::test]
async fn upload_non_openapi_json_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request(
        "weird.json",
        &non_openapi_array_json(),
        "----BoundaryNonOpenApi",
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload non-OpenAPI `.json` must return 400"
    );
}

// ---------------------------------------------------------------------------
// 5. Upload valid OpenAPI JSON -> 200, draft (FAQ support does not regress)
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want existing OpenAPI `.json`
// uploads to keep working after FAQ support is added, so that the new format
// detection does not break the previously supported upload path.
// (US-CORE-032 scenario 6)
// Covers: US-CORE-032, BE-D01, BE-T01

#[tokio::test]
async fn upload_openapi_json_unaffected_by_faq_support() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request(
        "petstore.json",
        &valid_openapi_json(),
        "----BoundaryOpenApiStillWorks",
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload valid OpenAPI JSON must still return 200 after FAQ support"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        body["status"], "draft",
        "uploaded OpenAPI document must have 'draft' status"
    );
    assert!(
        body["rowCount"].as_i64().map(|n| n >= 1).unwrap_or(false),
        "OpenAPI upload must produce at least 1 chunk (rowCount >= 1), got: {}",
        body["rowCount"]
    );
    assert!(
        body["id"].is_string() && !body["id"].as_str().unwrap().is_empty(),
        "response must have a non-empty id"
    );
}

// ---------------------------------------------------------------------------
// 6. Channel-scoped upload assertions (BE-T02)
// ---------------------------------------------------------------------------

// User Story: support-multiple-website — As a knowledge base editor, I want the
// FAQ JSONL upload path to persist the channelId I provide and echo it back.
// Covers: BE-D02 (FAQ JSONL upload writes documents.channel_id and returns channelId).
#[tokio::test]
async fn upload_faq_with_channel_id_persists_channel_id() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    let req = upload_request("faq.jsonl", &valid_faq_jsonl(), "----BoundaryFaqSite");
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload FAQ JSONL with channelId must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    let doc_id = body["id"].as_str().expect("document id").to_string();
    assert_eq!(
        body["channelId"], CHANNEL_ID,
        "upload response must include the provided channelId"
    );

    let stored_channel_id: String = state
        .sqlite
        .call(move |conn| {
            conn.query_row(
                "SELECT channel_id FROM documents WHERE id = ?",
                rusqlite::params![doc_id],
                |row| row.get::<_, String>(0),
            )
        })
        .await
        .expect("query document channel_id");
    assert_eq!(
        stored_channel_id, CHANNEL_ID,
        "documents.channel_id must match the uploaded channelId"
    );
}

// User Story: support-multiple-website — As an API operator, I want FAQ
// uploads without a channelId to be rejected with 400.
// Covers: BE-D02 (upload endpoint requires channelId before format routing).
#[tokio::test]
async fn upload_faq_without_channel_id_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req =
        upload_request_without_channel("faq.jsonl", &valid_faq_jsonl(), "----BoundaryFaqNoSite");
    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload without channelId must return 400"
    );
}
